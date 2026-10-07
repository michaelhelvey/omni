//! Read the parts of `pyproject.toml` that show if a Python project uses ruff.

use std::collections::BTreeMap;

use serde::Deserialize;

/// The parts of `pyproject.toml` that omni uses. serde ignores all other keys.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PyProject {
    #[serde(default)]
    project: Project,
    /// PEP 735 dependency groups. An entry is a requirement string or an `include-group` table,
    /// so keep the raw values.
    #[serde(default)]
    dependency_groups: BTreeMap<String, Vec<toml::Value>>,
    #[serde(default)]
    tool: Tool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Project {
    #[serde(default)]
    dependencies: Vec<String>,
    #[serde(default)]
    optional_dependencies: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
struct Tool {
    ruff: Option<toml::Value>,
    #[serde(default)]
    uv: Uv,
    #[serde(default)]
    poetry: Poetry,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Uv {
    #[serde(default)]
    dev_dependencies: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Poetry {
    /// Poetry 1.2 and later: `[tool.poetry.group.<name>.dependencies]`.
    #[serde(default)]
    group: BTreeMap<String, PoetryGroup>,
    /// Earlier Poetry versions: `[tool.poetry.dev-dependencies]`.
    #[serde(default)]
    dev_dependencies: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Default, Deserialize)]
struct PoetryGroup {
    #[serde(default)]
    dependencies: BTreeMap<String, toml::Value>,
}

impl PyProject {
    /// Parse the text of a `pyproject.toml` file.
    pub fn parse(text: &str) -> Result<PyProject, toml::de::Error> {
        toml::from_str(text)
    }

    /// Returns `true` if the file has a `[tool.ruff]` table.
    pub fn has_ruff_config(&self) -> bool {
        self.tool.ruff.is_some()
    }

    /// The name of the section that lists `package` as a dependency, if one does.
    pub fn dependency_section(&self, package: &str) -> Option<String> {
        // Each section, with the requirements in it. Poetry lists package names as keys.
        let mut sections: Vec<(String, Vec<&str>)> = Vec::new();

        sections.push((
            "project.dependencies".to_owned(),
            strings(&self.project.dependencies),
        ));
        for (name, list) in &self.project.optional_dependencies {
            sections.push((
                format!("project.optional-dependencies.{name}"),
                strings(list),
            ));
        }
        for (name, entries) in &self.dependency_groups {
            // Skip the `{ include-group = "..." }` tables.
            let list = entries.iter().filter_map(toml::Value::as_str).collect();
            sections.push((format!("dependency-groups.{name}"), list));
        }
        sections.push((
            "tool.uv.dev-dependencies".to_owned(),
            strings(&self.tool.uv.dev_dependencies),
        ));
        for (name, group) in &self.tool.poetry.group {
            let list = group.dependencies.keys().map(String::as_str).collect();
            sections.push((format!("tool.poetry.group.{name}.dependencies"), list));
        }
        let list = self
            .tool
            .poetry
            .dev_dependencies
            .keys()
            .map(String::as_str)
            .collect();
        sections.push(("tool.poetry.dev-dependencies".to_owned(), list));

        let package = normalize(package);
        sections
            .into_iter()
            .find(|(_, list)| {
                list.iter()
                    .any(|r| normalize(requirement_name(r)) == package)
            })
            .map(|(section, _)| section)
    }
}

/// The package name at the start of a requirement, for example `ruff` in `ruff>=0.5; python_version
/// > "3.9"`.
fn requirement_name(requirement: &str) -> &str {
    let requirement = requirement.trim_start();
    let end = requirement
        .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
        .unwrap_or(requirement.len());
    &requirement[..end]
}

fn strings(list: &[String]) -> Vec<&str> {
    list.iter().map(String::as_str).collect()
}

/// Normalize a package name as PEP 503 does: lowercase, and `-`, `_` and `.` are the same.
fn normalize(name: &str) -> String {
    name.to_ascii_lowercase().replace(['_', '.'], "-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_ruff_config() {
        let pyproject = PyProject::parse("[tool.ruff]\nline-length = 100\n").unwrap();
        assert!(pyproject.has_ruff_config());
        assert!(
            !PyProject::parse("[project]\nname = \"x\"\n")
                .unwrap()
                .has_ruff_config()
        );
    }

    #[test]
    fn finds_ruff_in_each_dependency_list() {
        let cases = [
            (
                "[project]\ndependencies = [\"Ruff>=0.5\"]",
                "project.dependencies",
            ),
            (
                "[project.optional-dependencies]\ndev = [\"ruff ; python_version > '3.9'\"]",
                "project.optional-dependencies.dev",
            ),
            (
                "[dependency-groups]\ndev = [{ include-group = \"lint\" }, \"ruff==0.15.0\"]",
                "dependency-groups.dev",
            ),
            (
                "[tool.uv]\ndev-dependencies = [\"ruff\"]",
                "tool.uv.dev-dependencies",
            ),
            (
                "[tool.poetry.group.dev.dependencies]\nruff = \"^0.5\"",
                "tool.poetry.group.dev.dependencies",
            ),
            (
                "[tool.poetry.dev-dependencies]\nruff = \"*\"",
                "tool.poetry.dev-dependencies",
            ),
        ];
        for (text, section) in cases {
            let pyproject = PyProject::parse(text).unwrap();
            assert_eq!(
                pyproject.dependency_section("ruff").as_deref(),
                Some(section),
                "{text}"
            );
        }
    }

    #[test]
    fn does_not_match_other_packages() {
        let pyproject =
            PyProject::parse("[project]\ndependencies = [\"ruff-lsp\", \"pyruff\"]").unwrap();
        assert_eq!(pyproject.dependency_section("ruff"), None);
    }

    #[test]
    fn ignores_unknown_keys() {
        let pyproject = PyProject::parse(
            "[build-system]\nrequires = [\"hatchling\"]\n[tool.black]\nline-length = 88\n",
        )
        .unwrap();
        assert!(!pyproject.has_ruff_config());
    }
}
