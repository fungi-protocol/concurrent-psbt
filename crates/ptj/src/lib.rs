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
    let output = run(cli)?;
    if let Some(path) = output_path {
        io::write_text_atomic(&path, &output)?;
        Ok(None)
    } else {
        Ok(Some(output))
    }
}
