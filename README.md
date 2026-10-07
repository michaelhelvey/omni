# omni

omni is one formatter command for your editor. You give it one file. omni finds the formatter that
the project uses for that file, runs it, and gives you the result.

You configure your editor one time, with one command. You do not write rules like "if this project
has `.prettierrc`, use prettier" in your editor configuration.

omni formats one file at a time. It is not a tool to format a full project. Use the project's own
command (for example `npm run fmt` or `cargo fmt`) for that.

## Supported formatters

| Formatter | Languages                                       | omni uses it when                                                                |
| --------- | ----------------------------------------------- | -------------------------------------------------------------------------------- |
| Vite+     | JS, TS, JSON, YAML, TOML, HTML, CSS, Markdown…  | `vite.config.*` exists and `vite-plus` is in `package.json` ([Vite+](https://viteplus.dev)), or `vite-plus` is in `package.json` |
| oxfmt     | JS, TS, JSON, YAML, TOML, HTML, CSS, Markdown…  | `.oxfmtrc.json`, `.oxfmtrc.jsonc`, `oxfmt.config.ts` or `oxfmt.config.mts` exists, or `oxfmt` is in `package.json` |
| rs fmt    | JS, TS, JSON, YAML, HTML, CSS, Markdown…        | `rstack.config.{ts,js,mts,mjs}` exists, or `rstack` is in `package.json`          |
| prettier  | JS, TS, JSON, YAML, HTML, CSS, Markdown…        | a prettier configuration file exists, `package.json` has a `prettier` key, or `prettier` is in `package.json` |
| ruff      | Python                                          | `ruff.toml` or `.ruff.toml` exists, `pyproject.toml` has a `[tool.ruff]` table, or `ruff` is a dependency in `pyproject.toml` |
| rustfmt   | Rust                                            | `Cargo.toml`, `rustfmt.toml` or `.rustfmt.toml` exists                            |

omni does not include these formatters. Install the formatters that you use.

## Install

You need a Rust toolchain. From a clone of this repository, run:

```sh
cargo install --path .
```

This puts the `omni` binary in `~/.cargo/bin`. Make sure that this directory is on your `PATH`.

To make sure that omni works, run this command in one of your projects:

```sh
omni doctor
```

## Usage

```sh
# Format a file in place.
omni fmt src/main.rs

# Read the source from stdin and write the result to stdout. Editors use this mode.
# The path tells omni the language and the project. The file does not have to exist.
omni fmt --stdin-filepath src/main.rs < src/main.rs

# Set the language when the file name does not show it.
omni fmt --stdin-filepath scratch --language typescript < scratch

# Show which formatter omni selects, and why.
omni doctor              # each language in the current directory
omni doctor src/app.tsx  # one file
```

## Editor setup

All editors use stdin mode. The editor sends the text of the buffer on stdin, and omni writes the
formatted text to stdout. Unsaved changes are formatted correctly.

### Neovim

Use [conform.nvim](https://github.com/stevearc/conform.nvim):

```lua
require("conform").setup({
  formatters = {
    omni = {
      command = "omni",
      args = { "fmt", "--stdin-filepath", "$FILENAME" },
      stdin = true,
    },
  },
  formatters_by_ft = {
    -- Send every file type to omni. omni ignores the files that it cannot format.
    ["*"] = { "omni" },
  },
  format_on_save = {
    timeout_ms = 2000,
  },
})
```

### Zed

Add this to your `settings.json`:

```json
{
  "format_on_save": "on",
  "formatter": {
    "external": {
      "command": "omni",
      "arguments": ["fmt", "--stdin-filepath", "{buffer_path}"]
    }
  }
}
```

This setting replaces the formatter for all languages. To use omni for only some languages, put
the `formatter` setting below `"languages"`, for example `"languages": { "Markdown": { ... } }`.

### VS Code

VS Code cannot run an external formatter without an extension. Install
[Custom Local Formatters](https://marketplace.visualstudio.com/items?itemName=jkillian.custom-local-formatters)
(`jkillian.custom-local-formatters`), then add this to your `settings.json`:

```json
{
  "customLocalFormatters.formatters": [
    {
      "command": "omni fmt --stdin-filepath \"${file}\"",
      "languages": [
        "javascript",
        "javascriptreact",
        "typescript",
        "typescriptreact",
        "json",
        "jsonc",
        "yaml",
        "toml",
        "html",
        "vue",
        "svelte",
        "css",
        "scss",
        "less",
        "markdown",
        "mdx",
        "graphql",
        "handlebars",
        "python",
        "rust"
      ]
    }
  ],
  "[javascript][javascriptreact][typescript][typescriptreact][json][jsonc][yaml][toml][html][vue][svelte][css][scss][less][markdown][mdx][graphql][handlebars][python][rust]": {
    "editor.defaultFormatter": "jkillian.custom-local-formatters",
    "editor.formatOnSave": true
  }
}
```

The extension runs the command in a shell. Keep the quotes around `${file}`, because paths can
contain spaces.

## How omni selects a formatter

1. omni finds the language from the file name. `--language` replaces this.
2. omni examines the directory of the file, then each parent directory. It stops at the root of the
   git repository.
3. In the first directory that has a signal for a formatter that supports the language, omni
   selects that formatter. A configuration file is a stronger signal than a `package.json` key,
   and a `package.json` key is stronger than a dependency. If two signals have the same strength,
   the priority is Vite+, then oxfmt, then rs fmt, then prettier, then rustfmt.
4. If the file is in a project (a directory with `.git`, `package.json`, `Cargo.toml` or
   `pyproject.toml`), but no
   formatter has a signal, omni does not format the file.
5. If the file is not in a project, omni uses the first default formatter on `PATH` that supports
   the language: prettier, then oxfmt. For Rust, the default is rustfmt. To format files outside a
   project, install prettier globally (`npm install -g prettier`). For Python, the default is
   ruff.

omni reads `package.json` and `pyproject.toml` to find signals. omni does not read the contents of formatter
configuration files. If a configuration file exists, omni uses its formatter. The only exception
is `vite.config.*`: plain Vite projects also have this file, so omni uses Vite+ only if
`package.json` also has `vite-plus`.

For a Vite+ project, omni does not run `vp fmt`, because it is slow for one file. omni runs
`node_modules/.bin/oxfmt`. `vite-plus` installs this program for editors, and it applies the `fmt`
block in `vite.config.*`. omni does not use an `oxfmt` from `PATH` for a Vite+ project. Run your
package manager's install command first.

omni prefers the binary that the project installed: `node_modules/.bin` for npm packages, and
`.venv/bin` for ruff. If it does not find one, it uses the binary on `PATH`.

For Python, omni sorts the imports and then formats the file. This is the same as
`ruff check --select I001 --fix` and then `ruff format`. omni does not apply other lint fixes.

## Options for files outside a project

You can set options for the default formatters in `~/.config/omni/config.toml`. If
`XDG_CONFIG_HOME` is set, omni uses `$XDG_CONFIG_HOME/omni/config.toml`.

```toml
[defaults.prettier]
printWidth = 100
proseWrap = "always"

[defaults.oxfmt]
printWidth = 100
proseWrap = "always"

[defaults.rustfmt]
max_width = 100

[defaults.ruff]
line-length = 100

[defaults.ruff.format]
quote-style = "single"
```

Each table uses the option names of its formatter. You can use `prettier`, `oxfmt`, `ruff` and
`rustfmt`.

omni uses these options only when it selects a default formatter, so only for files that are not
in a project. In a project, the configuration of the project controls the formatter, and omni
does not read this file. An error in this file does not stop omni from formatting files in
projects.

To see the options that omni uses for a file, run `omni doctor <file>`.

## Errors and exit codes

| Exit code | Meaning                                                                                       |
| --------- | --------------------------------------------------------------------------------------------- |
| 0         | omni formatted the file, or did not format it on purpose (unknown language, or no formatter configured) |
| 1         | The formatter failed, for example because of a syntax error. omni did not change the file.     |
| 2         | omni failed, for example because the project uses a formatter that is not installed.         |

When omni does not format a file, it writes a warning to stderr. In stdin mode, it also writes the
source to stdout without changes, so your editor does not lose text.

To see more information, set `RUST_LOG=debug`.
