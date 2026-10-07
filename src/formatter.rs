//! The formatters that omni can run: how to detect them, find them and run them.

use std::collections::HashSet;
use std::fmt;
use std::io::Write;

use anyhow::{Context, Result, bail};
use camino::{Utf8Path, Utf8PathBuf};

use crate::language::Language;
use crate::pyproject::PyProject;

/// A formatter that omni can run.
///
/// The order of the variants is the priority. When two formatters have signals of the same
/// strength in the same directory, omni selects the formatter that is first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Formatter {
    /// [Vite+](https://viteplus.dev/guide/fmt). It formats with oxfmt and the `fmt` block of
    /// `vite.config.*`, so it has a higher priority than oxfmt.
    ///
    /// omni does not run `vp fmt`, because it is approximately 7 times slower for one file. It runs
    /// `node_modules/.bin/oxfmt`, which `vite-plus` installs for editors. This wrapper sets
    /// `VP_VERSION`, so that oxfmt reads the `fmt` block in `vite.config.*`.
    VitePlus,
    /// [oxfmt](https://oxc.rs/docs/guide/usage/formatter.html).
    Oxfmt,
    /// `rs fmt` from [Rstack](https://rstack.rs/guide/cli/fmt).
    Rstack,
    /// [Prettier](https://prettier.io).
    Prettier,
    /// [Ruff](https://docs.astral.sh/ruff/formatter/). omni sorts the imports and then formats
    /// the file, the same as `ruff check --select I --fix` and then `ruff format`.
    Ruff,
    /// [rustfmt](https://github.com/rust-lang/rustfmt).
    Rustfmt,
}

/// How strongly a project signal shows that the project uses a formatter.
///
/// The order of the variants is important. A stronger signal has a larger value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Strength {
    /// The formatter is a dependency in `package.json` or `pyproject.toml`, or the project is a
    /// Cargo package.
    Dependency,
    /// `package.json` or `pyproject.toml` has a key for the formatter configuration.
    ManifestKey,
    /// A configuration file for the formatter exists.
    ConfigFile,
}

/// One piece of evidence that a directory uses a formatter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signal {
    /// The formatter that the signal is for.
    pub formatter: Formatter,
    /// How strongly the signal shows that the project uses the formatter.
    pub strength: Strength,
    /// A description of the signal for the user, for example "found `.prettierrc`".
    pub reason: String,
}

const PRETTIER_CONFIGS: &[&str] = &[
    ".prettierrc",
    ".prettierrc.json",
    ".prettierrc.json5",
    ".prettierrc.yaml",
    ".prettierrc.yml",
    ".prettierrc.toml",
    ".prettierrc.js",
    ".prettierrc.mjs",
    ".prettierrc.cjs",
    ".prettierrc.ts",
    ".prettierrc.mts",
    ".prettierrc.cts",
    "prettier.config.js",
    "prettier.config.mjs",
    "prettier.config.cjs",
    "prettier.config.ts",
    "prettier.config.mts",
    "prettier.config.cts",
];

const OXFMT_CONFIGS: &[&str] = &[
    ".oxfmtrc.json",
    ".oxfmtrc.jsonc",
    "oxfmt.config.ts",
    "oxfmt.config.mts",
];

const RSTACK_CONFIGS: &[&str] = &[
    "rstack.config.ts",
    "rstack.config.js",
    "rstack.config.mts",
    "rstack.config.mjs",
];

/// Vite configuration files. A Vite+ project has one of these files and the `vite-plus` package.
const VITE_CONFIGS: &[&str] = &[
    "vite.config.ts",
    "vite.config.mts",
    "vite.config.cts",
    "vite.config.js",
    "vite.config.mjs",
    "vite.config.cjs",
];

const RUSTFMT_CONFIGS: &[&str] = &["rustfmt.toml", ".rustfmt.toml"];

const RUFF_CONFIGS: &[&str] = &["ruff.toml", ".ruff.toml"];

impl Formatter {
    /// The name of the formatter for messages.
    pub fn name(self) -> &'static str {
        match self {
            Formatter::VitePlus => "vite+ (oxfmt)",
            Formatter::Oxfmt => "oxfmt",
            Formatter::Rstack => "rs fmt",
            Formatter::Prettier => "prettier",
            Formatter::Ruff => "ruff",
            Formatter::Rustfmt => "rustfmt",
        }
    }

    /// Returns `true` if the formatter can format files of `language`.
    pub fn supports(self, language: Language) -> bool {
        use Language::*;
        match self {
            Formatter::VitePlus | Formatter::Oxfmt => !matches!(language, Rust | Python),
            // rs fmt supports the same built-in languages as Prettier.
            Formatter::Rstack | Formatter::Prettier => {
                !matches!(language, Rust | Python | Toml | Svelte)
            }
            Formatter::Ruff => matches!(language, Python),
            Formatter::Rustfmt => matches!(language, Rust),
        }
    }

    /// The formatters to try, in order, for a file that is not in a project.
    pub fn defaults(language: Language) -> &'static [Formatter] {
        match language {
            Language::Rust => &[Formatter::Rustfmt],
            Language::Python => &[Formatter::Ruff],
            // Prettier does not support all languages. oxfmt formats the others.
            _ => &[Formatter::Prettier, Formatter::Oxfmt],
        }
    }

    /// The path of the binary that a project installs, relative to the project directory. For
    /// example, `node_modules/.bin/prettier`. `None` if projects do not install the formatter.
    pub fn local_bin(self) -> Option<Utf8PathBuf> {
        let npm = |name: &str| {
            // On Windows, npm installs a `.cmd` wrapper. Only the wrapper can run.
            let name = if cfg!(windows) {
                format!("{name}.cmd")
            } else {
                name.to_owned()
            };
            Some(Utf8PathBuf::from("node_modules/.bin").join(name))
        };
        match self {
            Formatter::VitePlus | Formatter::Oxfmt => npm("oxfmt"),
            Formatter::Rstack => npm("rs"),
            Formatter::Prettier => npm("prettier"),
            // The virtual environment that uv and most other tools make.
            Formatter::Ruff if cfg!(windows) => Some(".venv/Scripts/ruff.exe".into()),
            Formatter::Ruff => Some(".venv/bin/ruff".into()),
            Formatter::Rustfmt => None,
        }
    }

    /// The name of the binary to find on `PATH`. `None` if omni must use the binary of the
    /// project.
    pub fn path_bin(self) -> Option<&'static str> {
        match self {
            // A binary on `PATH` does not know the `vite-plus` version of the project. Also, an
            // `oxfmt` on `PATH` is usually plain oxfmt, which ignores `vite.config.*`.
            Formatter::VitePlus => None,
            Formatter::Oxfmt => Some("oxfmt"),
            // Do not use `rs` from `PATH`. On BSD and macOS, `/usr/bin/rs` is a different program.
            // The `rstack` package also installs the `rstack` alias.
            Formatter::Rstack => Some("rstack"),
            Formatter::Prettier => Some("prettier"),
            Formatter::Ruff => Some("ruff"),
            Formatter::Rustfmt => Some("rustfmt"),
        }
    }

    /// Find the signals for each formatter in one directory.
    ///
    /// `entries` contains the names of the files in the directory. `package_json` and `pyproject`
    /// are the parsed `package.json` and `pyproject.toml` of the directory, if they exist.
    pub fn detect(
        entries: &HashSet<String>,
        package_json: Option<&serde_json::Value>,
        pyproject: Option<&PyProject>,
    ) -> Vec<Signal> {
        let mut signals = Vec::new();

        let mut config_files = |formatter: Formatter, names: &[&str]| {
            for name in names {
                if entries.contains(*name) {
                    signals.push(Signal {
                        formatter,
                        strength: Strength::ConfigFile,
                        reason: format!("found `{name}`"),
                    });
                }
            }
        };
        config_files(Formatter::Oxfmt, OXFMT_CONFIGS);
        config_files(Formatter::Rstack, RSTACK_CONFIGS);
        config_files(Formatter::Prettier, PRETTIER_CONFIGS);
        config_files(Formatter::Rustfmt, RUSTFMT_CONFIGS);
        config_files(Formatter::Ruff, RUFF_CONFIGS);

        if entries.contains("Cargo.toml") {
            signals.push(Signal {
                formatter: Formatter::Rustfmt,
                strength: Strength::Dependency,
                reason: "found `Cargo.toml`".to_owned(),
            });
        }

        if let Some(package_json) = package_json {
            // A Vite config alone is not a signal, because plain Vite projects have one too. omni
            // does not read the config to find a `vite-plus` import. The `vite-plus` package
            // gives the same information.
            if let Some(section) = dependency_section(package_json, "vite-plus")
                && let Some(config) = VITE_CONFIGS.iter().find(|n| entries.contains(**n))
            {
                signals.push(Signal {
                    formatter: Formatter::VitePlus,
                    strength: Strength::ConfigFile,
                    reason: format!(
                        "found `{config}`, and `package.json` has `vite-plus` in `{section}`"
                    ),
                });
            }

            if package_json.get("prettier").is_some() {
                signals.push(Signal {
                    formatter: Formatter::Prettier,
                    strength: Strength::ManifestKey,
                    reason: "`package.json` has a `prettier` key".to_owned(),
                });
            }

            for (formatter, package) in [
                (Formatter::VitePlus, "vite-plus"),
                (Formatter::Oxfmt, "oxfmt"),
                (Formatter::Rstack, "rstack"),
                (Formatter::Prettier, "prettier"),
            ] {
                if let Some(section) = dependency_section(package_json, package) {
                    signals.push(Signal {
                        formatter,
                        strength: Strength::Dependency,
                        reason: format!("`package.json` has `{package}` in `{section}`"),
                    });
                }
            }
        }

        if let Some(pyproject) = pyproject {
            if pyproject.has_ruff_config() {
                signals.push(Signal {
                    formatter: Formatter::Ruff,
                    strength: Strength::ManifestKey,
                    reason: "`pyproject.toml` has a `[tool.ruff]` table".to_owned(),
                });
            }
            if let Some(section) = pyproject.dependency_section("ruff") {
                signals.push(Signal {
                    formatter: Formatter::Ruff,
                    strength: Strength::Dependency,
                    reason: format!("`pyproject.toml` has `ruff` in `{section}`"),
                });
            }
        }

        signals
    }

    /// The commands to format one file from stdin to stdout. omni runs each command with the
    /// same binary, and gives the output of a command to the next command.
    ///
    /// `filepath` is the name of the file. The formatter uses it to select a parser and to apply
    /// its configuration. `edition` is the Rust edition. omni uses it only for rustfmt. `options`
    /// are the arguments from [`Formatter::option_args`].
    pub fn stdin_commands(
        self,
        filepath: &Utf8Path,
        edition: Option<&str>,
        options: &[String],
    ) -> Vec<Vec<String>> {
        let filepath = filepath.to_string();
        let command = |parts: &[&[&str]]| -> Vec<String> {
            parts
                .iter()
                .flat_map(|part| part.iter().map(|s| s.to_string()))
                .collect()
        };
        let options: Vec<&str> = options.iter().map(String::as_str).collect();

        match self {
            Formatter::VitePlus | Formatter::Oxfmt | Formatter::Prettier => {
                vec![command(&[&options, &["--stdin-filepath", &filepath]])]
            }
            Formatter::Rstack => vec![command(&[
                &["fmt"],
                &options,
                &["--stdin-filepath", &filepath],
            ])],
            Formatter::Ruff => {
                // `--force-exclude` obeys the `exclude` setting of the project, also for a file
                // from stdin. ruff then gives the source back without changes.
                let stdin = ["--force-exclude", "--stdin-filename", &filepath, "-"];
                vec![
                    // Sort the imports. Only I001 (unsorted imports) is a formatting rule.
                    // `--exit-zero`: a syntax error does not stop the chain here. `ruff format`
                    // finds it and fails.
                    command(&[
                        &["check", "--select=I001", "--fix", "--exit-zero", "--quiet"],
                        &options,
                        &stdin,
                    ]),
                    command(&[&["format"], &options, &stdin]),
                ]
            }
            Formatter::Rustfmt => {
                let edition = edition.map(|e| format!("--edition={e}"));
                // Without a file argument, rustfmt reads stdin. It does not follow `mod`
                // declarations into other files.
                vec![command(&[
                    &options,
                    &edition.as_deref().into_iter().collect::<Vec<_>>(),
                    &["--emit=stdout"],
                ])]
            }
        }
    }
}

/// Arguments that give the default options to a formatter.
///
/// Keep this value until the formatter exits. It can own a temporary configuration file, and it
/// deletes the file when it is dropped.
pub struct OptionArgs {
    /// The arguments to add to the command.
    pub args: Vec<String>,
    _file: Option<tempfile::NamedTempFile>,
}

impl Formatter {
    /// Make the arguments that give `options` to the formatter.
    ///
    /// Prettier and oxfmt get a temporary JSON configuration file. rustfmt gets
    /// `--config key=value,...`. ruff gets one `--config "key = value"` for each option.
    pub fn option_args(self, options: &toml::Table) -> Result<OptionArgs> {
        match self {
            Formatter::Prettier | Formatter::Oxfmt => {
                let json = serde_json::to_value(options)?;
                // Both formatters select the configuration format from the extension.
                let mut file = tempfile::Builder::new()
                    .prefix("omni-")
                    .suffix(".json")
                    .tempfile()
                    .context("cannot make a temporary configuration file")?;
                serde_json::to_writer(&mut file, &json)?;
                file.flush()?;
                let path = Utf8Path::from_path(file.path())
                    .context("the temporary directory is not UTF-8")?;
                Ok(OptionArgs {
                    args: vec![format!("--config={path}")],
                    _file: Some(file),
                })
            }
            Formatter::Rustfmt => {
                let mut pairs = Vec::new();
                for (key, value) in options {
                    let value = match value {
                        toml::Value::String(s) => s.clone(),
                        toml::Value::Array(_) | toml::Value::Table(_) => {
                            bail!("the rustfmt option `{key}` must be a string, number or boolean")
                        }
                        other => other.to_string(),
                    };
                    // rustfmt separates the options with commas.
                    if value.contains(',') {
                        bail!("the rustfmt option `{key}` must not contain a comma");
                    }
                    pairs.push(format!("{key}={value}"));
                }
                Ok(OptionArgs {
                    args: vec!["--config".to_owned(), pairs.join(",")],
                    _file: None,
                })
            }
            Formatter::Ruff => {
                let mut args = Vec::new();
                for (key, value) in flatten(options) {
                    args.push("--config".to_owned());
                    args.push(format!("{key} = {value}"));
                }
                Ok(OptionArgs { args, _file: None })
            }
            Formatter::VitePlus | Formatter::Rstack => {
                bail!("{self} cannot be a default formatter, so it has no default options")
            }
        }
    }
}

/// Flatten nested tables to dotted keys. For example, `[format] quote-style = "single"` becomes
/// `("format.quote-style", "single")`. ruff takes one option for each `--config`.
fn flatten(table: &toml::Table) -> Vec<(String, &toml::Value)> {
    let mut pairs = Vec::new();
    for (key, value) in table {
        match value {
            toml::Value::Table(inner) => {
                for (inner_key, inner_value) in flatten(inner) {
                    pairs.push((format!("{key}.{inner_key}"), inner_value));
                }
            }
            _ => pairs.push((key.clone(), value)),
        }
    }
    pairs
}

impl fmt::Display for Formatter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

fn dependency_section(package_json: &serde_json::Value, package: &str) -> Option<&'static str> {
    ["devDependencies", "dependencies"]
        .into_iter()
        .find(|section| {
            package_json
                .get(*section)
                .and_then(|deps| deps.get(package))
                .is_some()
        })
}

/// Find the Rust edition for a file in `dir`, the same as `cargo fmt` does. omni uses the nearest
/// `Cargo.toml`.
///
/// Returns `None` if there is no `Cargo.toml`, or if it does not set an edition.
pub fn rust_edition(dir: &Utf8Path) -> Option<String> {
    let dir = dir.ancestors().find(|d| d.join("Cargo.toml").is_file())?;
    let manifest = read_toml(&dir.join("Cargo.toml"))?;
    let edition = manifest.get("package")?.get("edition")?;

    if let Some(edition) = edition.as_str() {
        return Some(edition.to_owned());
    }

    // The crate inherits the edition from the workspace (`edition.workspace = true`). The
    // workspace root can be the same manifest as the crate, so start at `dir`.
    if edition.get("workspace").and_then(|w| w.as_bool()) == Some(true) {
        for ancestor in dir.ancestors() {
            let Some(root) = read_toml(&ancestor.join("Cargo.toml")) else {
                continue;
            };
            if let Some(workspace) = root.get("workspace") {
                return workspace
                    .get("package")?
                    .get("edition")?
                    .as_str()
                    .map(str::to_owned);
            }
        }
    }

    None
}

fn read_toml(path: &Utf8Path) -> Option<toml::Table> {
    let text = std::fs::read_to_string(path).ok()?;
    match text.parse() {
        Ok(table) => Some(table),
        Err(err) => {
            log::warn!("cannot parse `{path}`: {err}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(names: &[&str]) -> HashSet<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn detects_config_files() {
        let signals = Formatter::detect(&entries(&[".prettierrc", "README.md"]), None, None);
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].formatter, Formatter::Prettier);
        assert_eq!(signals[0].strength, Strength::ConfigFile);
    }

    #[test]
    fn detects_package_json() {
        let package_json = serde_json::json!({
            "prettier": {},
            "devDependencies": { "oxfmt": "1.0.0" },
        });
        let signals = Formatter::detect(&entries(&["package.json"]), Some(&package_json), None);
        assert!(
            signals
                .iter()
                .any(|s| s.formatter == Formatter::Prettier && s.strength == Strength::ManifestKey)
        );
        assert!(
            signals
                .iter()
                .any(|s| s.formatter == Formatter::Oxfmt && s.strength == Strength::Dependency)
        );
    }

    #[test]
    fn detects_vite_plus() {
        let vite_plus = serde_json::json!({ "devDependencies": { "vite-plus": "1.1.0" } });
        let vite = serde_json::json!({ "devDependencies": { "vite": "7.0.0" } });

        // A Vite config and the `vite-plus` package: a strong signal.
        let signals = Formatter::detect(
            &entries(&["vite.config.ts", "package.json"]),
            Some(&vite_plus),
            None,
        );
        assert!(
            signals
                .iter()
                .any(|s| s.formatter == Formatter::VitePlus && s.strength == Strength::ConfigFile)
        );

        // A plain Vite project: no signal.
        let signals = Formatter::detect(
            &entries(&["vite.config.ts", "package.json"]),
            Some(&vite),
            None,
        );
        assert!(signals.is_empty());

        // The `vite-plus` package without a Vite config: only a dependency signal.
        let signals = Formatter::detect(&entries(&["package.json"]), Some(&vite_plus), None);
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].strength, Strength::Dependency);
    }

    #[test]
    fn option_args_for_prettier_write_a_json_file() {
        let options: toml::Table = "printWidth = 100\nproseWrap = \"always\"\n"
            .parse()
            .unwrap();
        let option_args = Formatter::Prettier.option_args(&options).unwrap();
        let path = option_args.args[0].strip_prefix("--config=").unwrap();
        assert!(path.ends_with(".json"));
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "printWidth": 100, "proseWrap": "always" })
        );

        // The file stays until omni drops the arguments.
        let path = path.to_owned();
        drop(option_args);
        assert!(!Utf8Path::new(&path).exists());
    }

    #[test]
    fn option_args_for_rustfmt() {
        let options: toml::Table = "max_width = 100\nnewline_style = \"Unix\"\n"
            .parse()
            .unwrap();
        let option_args = Formatter::Rustfmt.option_args(&options).unwrap();
        assert_eq!(
            option_args.args,
            ["--config", "max_width=100,newline_style=Unix"]
        );
    }

    #[test]
    fn rstack_never_uses_rs_from_path() {
        assert_eq!(Formatter::Rstack.path_bin(), Some("rstack"));
        assert_eq!(
            Formatter::Rstack.local_bin().unwrap().file_name(),
            Some(if cfg!(windows) { "rs.cmd" } else { "rs" })
        );
    }

    #[test]
    fn stdin_commands() {
        let path = Utf8Path::new("a/b.md");
        assert_eq!(
            Formatter::VitePlus.stdin_commands(path, None, &[]),
            [["--stdin-filepath", "a/b.md"]]
        );
        assert_eq!(
            Formatter::Rstack.stdin_commands(path, None, &[]),
            [["fmt", "--stdin-filepath", "a/b.md"]]
        );
        assert_eq!(
            Formatter::Rustfmt.stdin_commands(path, Some("2024"), &[]),
            [["--edition=2024", "--emit=stdout"]]
        );
    }

    #[test]
    fn ruff_sorts_imports_then_formats() {
        let path = Utf8Path::new("a/b.py");
        let options = ["--config".to_owned(), "line-length = 100".to_owned()];
        let commands = Formatter::Ruff.stdin_commands(path, None, &options);
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0][0], "check");
        assert!(commands[0].contains(&"--select=I001".to_owned()));
        assert_eq!(commands[1][0], "format");
        for command in &commands {
            assert!(command.contains(&"line-length = 100".to_owned()));
            assert_eq!(
                command[command.len() - 3..],
                ["--stdin-filename", "a/b.py", "-"]
            );
        }
    }

    #[test]
    fn option_args_for_ruff_flatten_tables() {
        let options: toml::Table = "line-length = 100\n[format]\nquote-style = \"single\"\n"
            .parse()
            .unwrap();
        let option_args = Formatter::Ruff.option_args(&options).unwrap();
        assert_eq!(
            option_args.args,
            [
                "--config",
                "format.quote-style = \"single\"",
                "--config",
                "line-length = 100",
            ]
        );
    }

    #[test]
    fn detects_ruff() {
        let signals = Formatter::detect(&entries(&["ruff.toml"]), None, None);
        assert_eq!(signals[0].formatter, Formatter::Ruff);
        assert_eq!(signals[0].strength, Strength::ConfigFile);

        let pyproject = PyProject::parse(
            "[tool.ruff]\nline-length = 100\n[dependency-groups]\ndev = [\"ruff\"]\n",
        )
        .unwrap();
        let signals = Formatter::detect(&entries(&["pyproject.toml"]), None, Some(&pyproject));
        let strengths: Vec<Strength> = signals.iter().map(|s| s.strength).collect();
        assert_eq!(strengths, [Strength::ManifestKey, Strength::Dependency]);
    }

    #[test]
    fn edition_from_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"a\"]\n[workspace.package]\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::create_dir(root.join("a")).unwrap();
        std::fs::write(
            root.join("a/Cargo.toml"),
            "[package]\nname = \"a\"\nedition.workspace = true\n",
        )
        .unwrap();
        assert_eq!(rust_edition(&root.join("a")).as_deref(), Some("2021"));
        std::fs::create_dir(root.join("a/src")).unwrap();
        assert_eq!(rust_edition(&root.join("a/src")).as_deref(), Some("2021"));
    }

    #[test]
    fn edition_from_own_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"a\"\nedition.workspace = true\n\
             [workspace]\n[workspace.package]\nedition = \"2024\"\n",
        )
        .unwrap();
        assert_eq!(rust_edition(root).as_deref(), Some("2024"));
    }
}
