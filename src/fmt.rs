//! The `fmt` command: format one file in place, or from stdin to stdout.

use std::io::{Read, Write};
use std::process::{Command, ExitCode, Stdio};

use anyhow::{Context, Result, bail};
use camino::{Utf8Component, Utf8Path, Utf8PathBuf};

use crate::config::Config;
use crate::formatter::{Formatter, rust_edition};
use crate::language::Language;
use crate::resolve::{Outcome, Resolver};

/// Where omni reads the source, and where it writes the result.
pub enum Input {
    /// Format this file in place.
    File(Utf8PathBuf),
    /// Read stdin and write to stdout. The path is the name of the file, and it does not have to
    /// exist.
    Stdin(Utf8PathBuf),
}

/// The result of one format.
enum Formatted {
    /// The formatter succeeded. This is the new source.
    Output(Vec<u8>),
    /// omni did not format the source. The source stays the same.
    Skipped,
    /// The formatter failed. It wrote the error to stderr.
    Failed,
}

/// Format one file, from `input`. `language` replaces the language from the file name.
///
/// Returns exit code 1 if the formatter fails. Returns an error if omni itself fails.
pub fn run(input: Input, language: Option<Language>) -> Result<ExitCode> {
    let cwd = current_dir()?;
    let resolver = Resolver::new();

    match input {
        Input::File(path) => {
            let path = absolute(&cwd, &path);
            let source = std::fs::read(&path).with_context(|| format!("cannot read `{path}`"))?;
            match format(&resolver, &path, language, &source)? {
                Formatted::Output(output) => {
                    // Do not write the file if nothing changed. This keeps the modification time,
                    // so that the editor does not reload the file.
                    if output != source {
                        std::fs::write(&path, output)
                            .with_context(|| format!("cannot write `{path}`"))?;
                    }
                    Ok(ExitCode::SUCCESS)
                }
                Formatted::Skipped => Ok(ExitCode::SUCCESS),
                Formatted::Failed => Ok(ExitCode::FAILURE),
            }
        }
        Input::Stdin(filepath) => {
            let filepath = absolute(&cwd, &filepath);
            let mut source = Vec::new();
            std::io::stdin()
                .read_to_end(&mut source)
                .context("cannot read stdin")?;
            let mut stdout = std::io::stdout().lock();
            match format(&resolver, &filepath, language, &source)? {
                Formatted::Output(output) => stdout.write_all(&output)?,
                // Give the source back without changes. An editor that formats on save must not
                // lose the text of the buffer.
                Formatted::Skipped => stdout.write_all(&source)?,
                // Write nothing. The editor keeps the buffer when the command fails.
                Formatted::Failed => return Ok(ExitCode::FAILURE),
            }
            stdout.flush()?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Format `source`, which is the text of the file at `filepath`.
fn format(
    resolver: &Resolver,
    filepath: &Utf8Path,
    language: Option<Language>,
    source: &[u8],
) -> Result<Formatted> {
    let detected = Language::from_path(filepath);
    let Some(language) = language.or(detected) else {
        // Editors can send every file to omni. An unknown file is not an error.
        log::warn!("not formatted: omni does not know the language of `{filepath}`");
        return Ok(Formatted::Skipped);
    };

    let dir = filepath.parent().unwrap_or(filepath);
    let decision = resolver.resolve(dir, language);
    let (formatter, root, binary, default) = match decision.outcome {
        Outcome::Format {
            formatter,
            root,
            binary: Some(binary),
            default,
        } => (formatter, root, binary, default),
        Outcome::Format {
            formatter,
            root,
            binary: None,
            ..
        } => bail!("`{root}` uses {formatter}, but {formatter} is not installed"),
        Outcome::Skip => {
            log::warn!("not formatted: {}", decision.reasons.join("; "));
            return Ok(Formatted::Skipped);
        }
    };

    // The formatter selects a parser from the file name. If the user set a different language,
    // add its extension so that the formatter uses the correct parser.
    let name = if detected == Some(language) {
        filepath.to_owned()
    } else {
        Utf8PathBuf::from(format!("{filepath}.{}", language.extension()))
    };
    let edition = match formatter {
        Formatter::Rustfmt => rust_edition(dir),
        _ => None,
    };

    // Apply the user's default options only to a default formatter. Inside a project, the
    // project's own configuration controls the formatter. Read the configuration file only here,
    // so that an error in it cannot stop omni from formatting a project.
    let option_args = if default {
        let config = Config::load()?;
        config
            .options(formatter)
            .map(|options| formatter.option_args(options))
            .transpose()
            .with_context(|| match &config.path {
                Some(path) => format!("invalid default options for {formatter} in `{path}`"),
                None => format!("invalid default options for {formatter}"),
            })?
    } else {
        None
    };
    let extra_args = option_args
        .as_ref()
        .map(|o| o.args.as_slice())
        .unwrap_or_default();

    log::debug!("formatting `{filepath}` with {formatter} in `{root}`");
    let mut child = Command::new(&binary)
        .args(extra_args)
        .args(formatter.stdin_args(&name, edition.as_deref()))
        .current_dir(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("cannot run `{binary}`"))?;

    // Write stdin on a different thread. If the formatter fills the stdout pipe before it reads
    // all of stdin, one thread for both operations can block forever.
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let output = std::thread::scope(|scope| {
        scope.spawn(move || {
            // If the formatter stops early, the write fails. The exit status shows the problem.
            let _ = stdin.write_all(source);
        });
        child.wait_with_output()
    })
    .with_context(|| format!("cannot run `{binary}`"))?;

    if output.status.success() {
        Ok(Formatted::Output(output.stdout))
    } else {
        Ok(Formatted::Failed)
    }
}

/// Make `path` absolute. Remove the `.` and `..` components.
///
/// The resolver examines the ancestors of the path. A `..` component makes a directory that is not
/// an ancestor of the file look like an ancestor, so remove each `..` with the component before it.
pub fn absolute(cwd: &Utf8Path, path: &Utf8Path) -> Utf8PathBuf {
    let mut result = Utf8PathBuf::new();
    for component in cwd.join(path).components() {
        match component {
            Utf8Component::CurDir => {}
            Utf8Component::ParentDir => {
                result.pop();
            }
            _ => result.push(component),
        }
    }
    result
}

/// The current directory, as a UTF-8 path.
pub fn current_dir() -> Result<Utf8PathBuf> {
    let cwd = std::env::current_dir().context("cannot read the current directory")?;
    Utf8PathBuf::from_path_buf(cwd)
        .map_err(|p| anyhow::anyhow!("the current directory is not UTF-8: {}", p.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_removes_dot_components() {
        let cwd = Utf8Path::new("/repo/pkg");
        assert_eq!(
            absolute(cwd, Utf8Path::new("../other/./x.ts")),
            Utf8PathBuf::from("/repo/other/x.ts")
        );
        assert_eq!(
            absolute(cwd, Utf8Path::new("/a/b")),
            Utf8PathBuf::from("/a/b")
        );
    }
}
