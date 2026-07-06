pub mod cli;
pub mod webgui;

mod commands;
mod error;
mod io;

pub use error::{Error, Result};

pub fn run(cli: cli::Cli) -> Result<String> {
    commands::run(cli.command)
}

pub fn run_or_write(cli: cli::Cli) -> Result<Option<String>> {
    let output_path = cli.output.clone();
    if let Some(path) = output_path.as_ref() {
        reject_destructive_output_alias(path, &cli.command)?;
    }
    let output = run(cli)?;
    if let Some(path) = output_path {
        io::write_text_atomic(&path, &output)?;
        Ok(None)
    } else {
        Ok(Some(output))
    }
}

fn reject_destructive_output_alias(output: &std::path::Path, command: &cli::Command) -> Result<()> {
    match command {
        cli::Command::Atomize(config) if same_existing_path(output, &config.file) => {
            Err(Error::new(
                "refusing to overwrite atomize input: atomize writes multiple PSBTs, so choose a distinct --output-file",
            ))
        }
        cli::Command::ExportBip174(config) if same_existing_path(output, &config.file) => {
            Err(Error::new(
                "refusing to overwrite export-bip174 input: export changes the PSBT file format, so choose a distinct --output-file",
            ))
        }
        _ => Ok(()),
    }
}

fn same_existing_path(left: &std::path::Path, right: &std::path::Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}
