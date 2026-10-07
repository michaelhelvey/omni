mod config;
mod doctor;
mod fmt;
mod formatter;
mod language;
mod resolve;

use std::io::Write;
use std::process::ExitCode;

use anyhow::Result;
use camino::Utf8PathBuf;
use clap::{ArgGroup, Parser, Subcommand};

use crate::language::Language;

/// A universal formatter for editors. It selects the correct formatter for each file.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Format one file.
    #[command(group(ArgGroup::new("input").required(true).args(["path", "stdin_filepath"])))]
    Fmt {
        /// The file to format in place.
        path: Option<Utf8PathBuf>,

        /// Read the source from stdin and write the result to stdout. omni uses this path to
        /// find the language and the formatter. The file does not have to exist.
        #[arg(long, value_name = "PATH")]
        stdin_filepath: Option<Utf8PathBuf>,

        /// Use this language, and not the language from the file name.
        #[arg(long)]
        language: Option<Language>,
    },
    /// Show which formatter omni selects, and why.
    Doctor {
        /// A file name or a directory. The default is the current directory.
        #[arg(default_value = ".")]
        path: Utf8PathBuf,
    },
}

fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::Fmt {
            path,
            stdin_filepath,
            language,
        } => {
            let input = match (path, stdin_filepath) {
                (Some(path), None) => fmt::Input::File(path),
                (None, Some(filepath)) => fmt::Input::Stdin(filepath),
                _ => unreachable!("clap requires exactly one input"),
            };
            fmt::run(input, language)
        }
        Command::Doctor { path } => doctor::run(&path).map(|()| ExitCode::SUCCESS),
    }
}

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .format(|buf, record| {
            let level = record.level().as_str().to_ascii_lowercase();
            writeln!(buf, "omni: {level}: {}", record.args())
        })
        .init();

    match run(Cli::parse()) {
        Ok(code) => code,
        Err(err) => {
            log::error!("{err:#}");
            ExitCode::from(2)
        }
    }
}
