//! The `doctor` command: show which formatter omni selects, and why.

use anyhow::Result;
use camino::Utf8Path;

use crate::config::Config;
use crate::fmt::{absolute, current_dir};
use crate::formatter::Formatter;
use crate::language::Language;
use crate::resolve::{Decision, Outcome, Resolver};

/// Show the formatter that omni selects for `path`, and why.
///
/// If `path` is a directory, show the decision for each language in that directory. If not, use
/// `path` as a file name. The file does not have to exist.
pub fn run(path: &Utf8Path) -> Result<()> {
    let cwd = current_dir()?;
    let resolver = Resolver::new();
    // Keep an error, and show it next to the decisions that use the options.
    let config = Config::load();
    let path = absolute(&cwd, path);
    println!("{path}");

    if path.is_dir() {
        // Show languages with the same decision together, so that the output stays short.
        let mut rows: Vec<(Vec<Language>, Decision)> = Vec::new();
        for &language in Language::ALL {
            let decision = resolver.resolve(&path, language);
            match rows.iter_mut().find(|(_, d)| *d == decision) {
                Some((languages, _)) => languages.push(language),
                None => rows.push((vec![language], decision)),
            }
        }
        for (languages, decision) in rows {
            let names: Vec<String> = languages.iter().map(|l| l.to_string()).collect();
            println!("  {}", names.join(", "));
            print_decision(&decision, &config);
        }
        return Ok(());
    }

    let Some(language) = Language::from_path(&path) else {
        println!("  omni does not know the language of this file");
        return Ok(());
    };
    let dir = path.parent().unwrap_or(&path);
    println!("  {language}");
    print_decision(&resolver.resolve(dir, language), &config);
    Ok(())
}

/// Show the default options that omni gives to a default formatter.
fn print_default_options(config: &Result<Config>, formatter: Formatter) {
    let config = match config {
        Ok(config) => config,
        Err(err) => {
            println!("       options: ERROR: {err:#}");
            return;
        }
    };
    if let (Some(options), Some(path)) = (config.options(formatter), &config.path) {
        let pairs: Vec<String> = options.iter().map(|(k, v)| format!("{k} = {v}")).collect();
        println!("       options from `{path}`: {}", pairs.join(", "));
    }
}

fn print_decision(decision: &Decision, config: &Result<Config>) {
    match &decision.outcome {
        Outcome::Format {
            formatter,
            root,
            binary,
            default,
        } => {
            let binary = match binary {
                Some(binary) => binary.to_string(),
                None => "NOT INSTALLED".to_owned(),
            };
            println!("    -> {formatter} ({binary}), runs in `{root}`");
            if *default {
                print_default_options(config, *formatter);
            }
        }
        Outcome::Skip => println!("    -> skip"),
    }
    for reason in &decision.reasons {
        println!("       - {reason}");
    }
}
