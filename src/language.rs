//! Find the language of a file from its name.

use std::fmt;

use camino::Utf8Path;
use clap::ValueEnum;

/// The language of a file. omni uses the language to find the formatters that can format a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, ValueEnum)]
pub enum Language {
    /// JavaScript: `.js`, `.mjs`, `.cjs`.
    #[value(name = "javascript")]
    JavaScript,
    /// JavaScript with JSX: `.jsx`.
    Jsx,
    /// TypeScript: `.ts`, `.mts`, `.cts`.
    #[value(name = "typescript")]
    TypeScript,
    /// TypeScript with JSX: `.tsx`.
    Tsx,
    /// JSON: `.json`.
    Json,
    /// JSON with comments: `.jsonc`, `tsconfig.json`, and some other configuration files.
    Jsonc,
    /// JSON5: `.json5`.
    Json5,
    /// YAML: `.yaml`, `.yml`.
    Yaml,
    /// TOML: `.toml`.
    Toml,
    /// HTML: `.html`, `.htm`.
    Html,
    /// Vue single-file components: `.vue`.
    Vue,
    /// Svelte components: `.svelte`.
    Svelte,
    /// CSS: `.css`.
    Css,
    /// SCSS: `.scss`.
    Scss,
    /// Less: `.less`.
    Less,
    /// Markdown: `.md`, `.markdown`.
    Markdown,
    /// MDX: `.mdx`.
    Mdx,
    /// GraphQL: `.graphql`, `.gql`.
    #[value(name = "graphql")]
    GraphQl,
    /// Handlebars templates: `.hbs`, `.handlebars`.
    Handlebars,
    /// Rust: `.rs`.
    Rust,
}

impl Language {
    /// All languages, in the order that `doctor` shows them.
    pub const ALL: &[Language] = &[
        Language::JavaScript,
        Language::Jsx,
        Language::TypeScript,
        Language::Tsx,
        Language::Json,
        Language::Jsonc,
        Language::Json5,
        Language::Yaml,
        Language::Toml,
        Language::Html,
        Language::Vue,
        Language::Svelte,
        Language::Css,
        Language::Scss,
        Language::Less,
        Language::Markdown,
        Language::Mdx,
        Language::GraphQl,
        Language::Handlebars,
        Language::Rust,
    ];

    /// Find the language of a file from its name. Returns `None` if omni does not know the file.
    pub fn from_path(path: &Utf8Path) -> Option<Language> {
        let name = path.file_name()?;

        // Some files have a special name. Examine the full name before the extension.
        match name {
            // Lock files are generated. Do not format them.
            "package-lock.json" | "Cargo.lock" | "pnpm-lock.yaml" | "yarn.lock" | "bun.lock" => {
                return None;
            }
            "tsconfig.json" | "jsconfig.json" | ".eslintrc" | ".babelrc" | ".swcrc" => {
                return Some(Language::Jsonc);
            }
            ".prettierrc" => return Some(Language::Json),
            _ => {}
        }

        let ext = path.extension()?.to_ascii_lowercase();
        let language = match ext.as_str() {
            "js" | "mjs" | "cjs" => Language::JavaScript,
            "jsx" => Language::Jsx,
            "ts" | "mts" | "cts" => Language::TypeScript,
            "tsx" => Language::Tsx,
            "json" => Language::Json,
            "jsonc" => Language::Jsonc,
            "json5" => Language::Json5,
            "yaml" | "yml" => Language::Yaml,
            "toml" => Language::Toml,
            "html" | "htm" => Language::Html,
            "vue" => Language::Vue,
            "svelte" => Language::Svelte,
            "css" => Language::Css,
            "scss" => Language::Scss,
            "less" => Language::Less,
            "md" | "markdown" => Language::Markdown,
            "mdx" => Language::Mdx,
            "graphql" | "gql" => Language::GraphQl,
            "hbs" | "handlebars" => Language::Handlebars,
            "rs" => Language::Rust,
            _ => return None,
        };
        Some(language)
    }

    /// The usual extension for the language. Formatters use the extension to select a parser.
    pub fn extension(self) -> &'static str {
        match self {
            Language::JavaScript => "js",
            Language::Jsx => "jsx",
            Language::TypeScript => "ts",
            Language::Tsx => "tsx",
            Language::Json => "json",
            Language::Jsonc => "jsonc",
            Language::Json5 => "json5",
            Language::Yaml => "yaml",
            Language::Toml => "toml",
            Language::Html => "html",
            Language::Vue => "vue",
            Language::Svelte => "svelte",
            Language::Css => "css",
            Language::Scss => "scss",
            Language::Less => "less",
            Language::Markdown => "md",
            Language::Mdx => "mdx",
            Language::GraphQl => "graphql",
            Language::Handlebars => "hbs",
            Language::Rust => "rs",
        }
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Use the same name as the `--language` flag.
        match self.to_possible_value() {
            Some(value) => f.write_str(value.get_name()),
            None => write!(f, "{self:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_by_extension() {
        assert_eq!(
            Language::from_path("a/b/README.md".into()),
            Some(Language::Markdown)
        );
        assert_eq!(Language::from_path("x.TSX".into()), Some(Language::Tsx));
        assert_eq!(Language::from_path("main.rs".into()), Some(Language::Rust));
        assert_eq!(Language::from_path("image.png".into()), None);
        assert_eq!(Language::from_path("Makefile".into()), None);
    }

    #[test]
    fn detects_by_name() {
        assert_eq!(
            Language::from_path("tsconfig.json".into()),
            Some(Language::Jsonc)
        );
        assert_eq!(Language::from_path("Cargo.lock".into()), None);
        assert_eq!(Language::from_path("package-lock.json".into()), None);
    }

    #[test]
    fn display_names() {
        assert_eq!(Language::JavaScript.to_string(), "javascript");
        assert_eq!(Language::GraphQl.to_string(), "graphql");
    }

    #[test]
    fn extension_round_trips() {
        for &language in Language::ALL {
            let path = format!("file.{}", language.extension());
            assert_eq!(Language::from_path(path.as_str().into()), Some(language));
        }
    }
}
