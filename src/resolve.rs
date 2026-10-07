//! Select the formatter for a language in a directory.

use std::collections::HashSet;

use camino::{Utf8Path, Utf8PathBuf};

use crate::formatter::{Formatter, Signal};
use crate::language::Language;

/// Files that show that a directory is the root of a project.
const PROJECT_MARKERS: &[&str] = &[".git", "package.json", "Cargo.toml"];

/// The result of resolution for one language in one directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// What omni does with the file.
    pub outcome: Outcome,
    /// Why omni made this decision. `doctor` shows these to the user.
    pub reasons: Vec<String>,
}

/// What omni does with a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Format the file.
    Format {
        /// The formatter to run.
        formatter: Formatter,
        /// The directory where omni found the formatter. omni runs the formatter from here.
        root: Utf8PathBuf,
        /// The binary to run. `None` if the project uses the formatter but it is not installed.
        binary: Option<Utf8PathBuf>,
        /// `true` if the file is not in a project and omni selected a default formatter. Only
        /// then does omni apply the default options from the user configuration.
        default: bool,
    },
    /// Do not format the file. The reasons in the decision tell why.
    Skip,
}

/// What omni knows about one directory.
#[derive(Debug, Default)]
struct DirInfo {
    signals: Vec<Signal>,
    project_marker: Option<&'static str>,
    git_root: bool,
}

type PathLookup = Box<dyn Fn(&str) -> Option<Utf8PathBuf>>;

/// Selects the formatter for a language in a directory.
pub struct Resolver {
    path_lookup: PathLookup,
}

impl Resolver {
    /// Make a resolver that finds binaries on the real `PATH`.
    pub fn new() -> Resolver {
        Resolver::with_path_lookup(Box::new(|name| {
            which::which(name)
                .ok()
                .and_then(|p| Utf8PathBuf::from_path_buf(p).ok())
        }))
    }

    /// Make a resolver that finds binaries on `PATH` with `path_lookup`. Tests use this.
    pub fn with_path_lookup(path_lookup: PathLookup) -> Resolver {
        Resolver { path_lookup }
    }

    /// Select the formatter for files of `language` in `dir`. `dir` must be an absolute path.
    pub fn resolve(&self, dir: &Utf8Path, language: Language) -> Decision {
        let mut project: Option<(Utf8PathBuf, &'static str)> = None;

        for ancestor in dir.ancestors() {
            let info = read_dir_info(ancestor);

            let candidates: Vec<&Signal> = info
                .signals
                .iter()
                .filter(|s| s.formatter.supports(language))
                .collect();
            if !candidates.is_empty() {
                return self.pick(ancestor, &candidates);
            }

            if project.is_none()
                && let Some(marker) = info.project_marker
            {
                project = Some((ancestor.to_owned(), marker));
            }
            // Do not look for configuration outside the repository.
            if info.git_root {
                break;
            }
        }

        if let Some((root, marker)) = project {
            return Decision {
                outcome: Outcome::Skip,
                reasons: vec![format!(
                    "`{root}` is a project (found `{marker}`), but it has no formatter \
                     configured for this language"
                )],
            };
        }

        self.default_for(dir, language)
    }

    /// Select one formatter from the signals in one directory.
    fn pick(&self, dir: &Utf8Path, candidates: &[&Signal]) -> Decision {
        // The strongest signal wins. If two signals have the same strength, the formatter that
        // is first in the `Formatter` enum wins.
        let best = candidates
            .iter()
            .min_by_key(|s| (std::cmp::Reverse(s.strength), s.formatter))
            .expect("candidates is not empty");

        let mut reasons = vec![format!("{} in `{dir}`", best.reason)];
        for other in candidates {
            if other.formatter == best.formatter {
                continue;
            }
            if other.strength == best.strength {
                reasons.push(format!(
                    "warning: {} also has a signal of the same strength ({}). omni selected {} \
                     because it has a higher priority",
                    other.formatter, other.reason, best.formatter
                ));
            } else {
                reasons.push(format!(
                    "ignored {} because its signal is weaker ({})",
                    other.formatter, other.reason
                ));
            }
        }

        let binary = self.find_binary(best.formatter, dir);
        if binary.is_none() {
            reasons.push(format!("{} is not installed", best.formatter));
        }

        Decision {
            outcome: Outcome::Format {
                formatter: best.formatter,
                root: dir.to_owned(),
                binary,
                default: false,
            },
            reasons,
        }
    }

    /// Select the first default formatter that is installed. omni uses defaults only for files
    /// that are not in a project.
    fn default_for(&self, dir: &Utf8Path, language: Language) -> Decision {
        // Do not try a default formatter that cannot format the language.
        let defaults: Vec<Formatter> = Formatter::defaults(language)
            .iter()
            .copied()
            .filter(|f| f.supports(language))
            .collect();
        for &formatter in &defaults {
            if let Some(binary) = (self.path_lookup)(formatter.path_bin()) {
                return Decision {
                    outcome: Outcome::Format {
                        formatter,
                        root: dir.to_owned(),
                        binary: Some(binary),
                        default: true,
                    },
                    reasons: vec![format!(
                        "not in a project; {formatter} is the first default formatter on PATH"
                    )],
                };
            }
        }

        let tried: Vec<&str> = defaults.iter().map(|f| f.name()).collect();
        Decision {
            outcome: Outcome::Skip,
            reasons: vec![format!(
                "not in a project, and no default formatter is installed (tried: {})",
                tried.join(", ")
            )],
        }
    }

    /// Find the binary for a formatter. omni prefers the binary in `node_modules/.bin`, so that
    /// the project gets the version that it installed.
    fn find_binary(&self, formatter: Formatter, dir: &Utf8Path) -> Option<Utf8PathBuf> {
        if let Some(name) = formatter.npm_bin() {
            // On Windows, npm installs a `.cmd` wrapper. Only the wrapper can run.
            let file_name = if cfg!(windows) {
                format!("{name}.cmd")
            } else {
                name.to_owned()
            };
            for ancestor in dir.ancestors() {
                let candidate = ancestor.join("node_modules").join(".bin").join(&file_name);
                if candidate.is_file() {
                    return Some(candidate);
                }
                // Do not use a binary from outside the repository.
                if ancestor.join(".git").exists() {
                    break;
                }
            }
        }
        (self.path_lookup)(formatter.path_bin())
    }
}

fn read_dir_info(dir: &Utf8Path) -> DirInfo {
    let Ok(read_dir) = dir.read_dir_utf8() else {
        return DirInfo::default();
    };
    let entries: HashSet<String> = read_dir
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_owned())
        .collect();

    let package_json = if entries.contains("package.json") {
        let path = dir.join("package.json");
        match std::fs::read_to_string(&path).map(|text| serde_json::from_str(&text)) {
            Ok(Ok(value)) => Some(value),
            Ok(Err(err)) => {
                log::warn!("cannot parse `{path}`: {err}");
                None
            }
            Err(err) => {
                log::warn!("cannot read `{path}`: {err}");
                None
            }
        }
    } else {
        None
    };

    DirInfo {
        signals: Formatter::detect(&entries, package_json.as_ref()),
        project_marker: PROJECT_MARKERS
            .iter()
            .copied()
            .find(|m| entries.contains(*m)),
        git_root: entries.contains(".git"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        _dir: tempfile::TempDir,
        root: Utf8PathBuf,
    }

    impl Fixture {
        fn new() -> Fixture {
            let dir = tempfile::tempdir().unwrap();
            // Canonicalize, because on macOS the temporary directory is behind a symlink.
            let root = Utf8PathBuf::from_path_buf(dir.path().canonicalize().unwrap()).unwrap();
            Fixture { _dir: dir, root }
        }

        fn file(&self, path: &str, contents: &str) -> &Fixture {
            let path = self.root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
            self
        }

        fn dir(&self, path: &str) -> Utf8PathBuf {
            let path = self.root.join(path);
            std::fs::create_dir_all(&path).unwrap();
            path
        }
    }

    /// A resolver that finds only the given binaries on `PATH`.
    fn resolver(installed: &[&'static str]) -> Resolver {
        let installed = installed.to_vec();
        Resolver::with_path_lookup(Box::new(move |name| {
            installed
                .contains(&name)
                .then(|| Utf8PathBuf::from(format!("/bin/{name}")))
        }))
    }

    fn formatter(decision: &Decision) -> Option<Formatter> {
        match decision.outcome {
            Outcome::Format { formatter, .. } => Some(formatter),
            Outcome::Skip => None,
        }
    }

    #[test]
    fn nearest_config_wins() {
        let fx = Fixture::new();
        fx.file(".git/HEAD", "")
            .file(".oxfmtrc.json", "{}")
            .file("packages/web/.prettierrc", "{}");

        let r = resolver(&[]);
        let web = r.resolve(&fx.dir("packages/web/src"), Language::Markdown);
        assert_eq!(formatter(&web), Some(Formatter::Prettier));
        let api = r.resolve(&fx.dir("packages/api"), Language::Markdown);
        assert_eq!(formatter(&api), Some(Formatter::Oxfmt));
    }

    #[test]
    fn stronger_signal_wins_in_same_dir() {
        let fx = Fixture::new();
        fx.file(".git/HEAD", "")
            .file(".prettierrc", "{}")
            .file("package.json", r#"{ "devDependencies": { "oxfmt": "1" } }"#);

        let decision = resolver(&[]).resolve(&fx.root, Language::TypeScript);
        assert_eq!(formatter(&decision), Some(Formatter::Prettier));
    }

    #[test]
    fn tie_uses_priority_and_warns() {
        let fx = Fixture::new();
        fx.file(".git/HEAD", "")
            .file(".prettierrc", "{}")
            .file(".oxfmtrc.json", "{}");

        let decision = resolver(&[]).resolve(&fx.root, Language::Css);
        assert_eq!(formatter(&decision), Some(Formatter::Oxfmt));
        assert!(decision.reasons.iter().any(|r| r.starts_with("warning:")));
    }

    #[test]
    fn vite_plus_beats_oxfmt_config() {
        let fx = Fixture::new();
        fx.file(".git/HEAD", "")
            .file(".oxfmtrc.json", "{}")
            .file("vite.config.ts", "")
            .file(
                "package.json",
                r#"{ "devDependencies": { "vite-plus": "1" } }"#,
            )
            .file("node_modules/.bin/vp", "");

        let decision = resolver(&[]).resolve(&fx.dir("src"), Language::TypeScript);
        assert_eq!(formatter(&decision), Some(Formatter::VitePlus));
    }

    #[test]
    fn signal_for_other_language_is_ignored() {
        let fx = Fixture::new();
        fx.file(".git/HEAD", "")
            .file("Cargo.toml", "[package]\nname = \"x\"\n");

        let r = resolver(&["oxfmt", "rustfmt"]);
        assert_eq!(
            formatter(&r.resolve(&fx.root, Language::Rust)),
            Some(Formatter::Rustfmt)
        );
        // The project has no formatter for Markdown. Do not use a default.
        let md = r.resolve(&fx.root, Language::Markdown);
        assert_eq!(md.outcome, Outcome::Skip);
    }

    #[test]
    fn package_json_without_formatter_is_a_project() {
        let fx = Fixture::new();
        fx.file("package.json", r#"{ "name": "x" }"#);

        let decision = resolver(&["oxfmt"]).resolve(&fx.dir("src"), Language::TypeScript);
        assert_eq!(decision.outcome, Outcome::Skip);
    }

    #[test]
    fn outside_project_uses_first_installed_default() {
        let fx = Fixture::new();
        let notes = fx.dir("notes");

        let decision = resolver(&["oxfmt"]).resolve(&notes, Language::Markdown);
        assert_eq!(formatter(&decision), Some(Formatter::Oxfmt));

        let decision = resolver(&["oxfmt", "prettier"]).resolve(&notes, Language::Markdown);
        assert_eq!(formatter(&decision), Some(Formatter::Prettier));
        assert!(matches!(
            decision.outcome,
            Outcome::Format { default: true, .. }
        ));

        let decision = resolver(&[]).resolve(&notes, Language::Markdown);
        assert_eq!(decision.outcome, Outcome::Skip);
    }

    #[test]
    fn default_must_support_language() {
        let fx = Fixture::new();
        let notes = fx.dir("notes");

        // Prettier cannot format TOML. Do not select it as the default.
        let decision = resolver(&["prettier"]).resolve(&notes, Language::Toml);
        assert_eq!(decision.outcome, Outcome::Skip);
    }

    #[test]
    fn prefers_node_modules_binary() {
        let fx = Fixture::new();
        fx.file(".git/HEAD", "")
            .file(
                "package.json",
                r#"{ "devDependencies": { "rstack": "1" } }"#,
            )
            .file("node_modules/.bin/rs", "");

        let decision = resolver(&["rstack"]).resolve(&fx.dir("app"), Language::TypeScript);
        let Outcome::Format {
            formatter,
            root,
            binary,
            ..
        } = decision.outcome
        else {
            panic!("expected a formatter");
        };
        assert_eq!(formatter, Formatter::Rstack);
        assert_eq!(root, fx.root);
        assert_eq!(binary, Some(fx.root.join("node_modules/.bin/rs")));
    }

    #[test]
    fn missing_binary_is_reported() {
        let fx = Fixture::new();
        fx.file(".git/HEAD", "").file(".prettierrc", "{}");

        let decision = resolver(&[]).resolve(&fx.root, Language::Json);
        assert!(matches!(
            decision.outcome,
            Outcome::Format { binary: None, .. }
        ));
    }

    #[test]
    fn does_not_look_above_git_root() {
        let fx = Fixture::new();
        fx.file(".prettierrc", "{}").file("repo/.git/HEAD", "");

        let decision = resolver(&[]).resolve(&fx.dir("repo"), Language::Json);
        assert_eq!(decision.outcome, Outcome::Skip);
    }
}
