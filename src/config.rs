//! The user configuration file, `~/.config/omni/config.toml`.
//!
//! The file sets options for the default formatters. omni uses default formatters only for files
//! that are not in a project, so these options never change how a project is formatted.
//!
//! ```toml
//! [defaults.prettier]
//! printWidth = 100
//! proseWrap = "always"
//! ```

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use camino::{Utf8Path, Utf8PathBuf};

use crate::formatter::Formatter;

/// The formatters that can have default options. These are the formatters that omni can select
/// as a default.
const CONFIGURABLE: &[Formatter] = &[
    Formatter::Prettier,
    Formatter::Oxfmt,
    Formatter::Ruff,
    Formatter::Rustfmt,
];

/// The contents of the user configuration file.
#[derive(Debug, Default)]
pub struct Config {
    /// The file that omni read. `None` if the file does not exist.
    pub path: Option<Utf8PathBuf>,
    defaults: BTreeMap<Formatter, toml::Table>,
}

impl Config {
    /// Read the configuration file from its usual location. If the file does not exist, return
    /// an empty configuration.
    pub fn load() -> Result<Config> {
        match default_path() {
            Some(path) if path.is_file() => Config::load_from(&path),
            _ => Ok(Config::default()),
        }
    }

    /// Read the configuration file at `path`.
    pub fn load_from(path: &Utf8Path) -> Result<Config> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("cannot read `{path}`"))?;
        let mut config =
            Config::parse(&text).with_context(|| format!("invalid configuration in `{path}`"))?;
        config.path = Some(path.to_owned());
        Ok(config)
    }

    fn parse(text: &str) -> Result<Config> {
        let table: toml::Table = text.parse()?;
        let mut defaults = BTreeMap::new();

        for (key, value) in table {
            if key != "defaults" {
                bail!("unknown key `{key}`");
            }
            let Some(sections) = value.as_table() else {
                bail!("`defaults` must be a table");
            };
            for (name, options) in sections {
                let Some(&formatter) = CONFIGURABLE.iter().find(|f| f.name() == name) else {
                    let names: Vec<&str> = CONFIGURABLE.iter().map(|f| f.name()).collect();
                    bail!(
                        "`defaults.{name}` is not a default formatter. Use one of: {}",
                        names.join(", ")
                    );
                };
                let Some(options) = options.as_table() else {
                    bail!("`defaults.{name}` must be a table");
                };
                defaults.insert(formatter, options.clone());
            }
        }

        Ok(Config {
            path: None,
            defaults,
        })
    }

    /// The default options for `formatter`, if the user set some.
    pub fn options(&self, formatter: Formatter) -> Option<&toml::Table> {
        self.defaults.get(&formatter).filter(|o| !o.is_empty())
    }
}

/// The usual location of the configuration file: `$XDG_CONFIG_HOME/omni/config.toml`, or
/// `~/.config/omni/config.toml`.
pub fn default_path() -> Option<Utf8PathBuf> {
    let env = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    let config_dir = match env("XDG_CONFIG_HOME") {
        Some(dir) => Utf8PathBuf::from(dir),
        None => Utf8PathBuf::from(env("HOME").or_else(|| env("USERPROFILE"))?).join(".config"),
    };
    Some(config_dir.join("omni").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults() {
        let config = Config::parse(
            "[defaults.prettier]\nprintWidth = 100\nproseWrap = \"always\"\n\
             [defaults.rustfmt]\nmax_width = 100\n",
        )
        .unwrap();
        let prettier = config.options(Formatter::Prettier).unwrap();
        assert_eq!(prettier["printWidth"].as_integer(), Some(100));
        assert!(config.options(Formatter::Rustfmt).is_some());
        assert!(config.options(Formatter::Oxfmt).is_none());
    }

    #[test]
    fn rejects_formatters_that_are_never_defaults() {
        let err = Config::parse("[defaults.\"vp fmt\"]\nprintWidth = 100\n").unwrap_err();
        assert!(err.to_string().contains("not a default formatter"));
    }

    #[test]
    fn rejects_unknown_keys() {
        assert!(Config::parse("printWidth = 100\n").is_err());
    }

    #[test]
    fn empty_file_is_valid() {
        assert!(
            Config::parse("")
                .unwrap()
                .options(Formatter::Prettier)
                .is_none()
        );
    }
}
