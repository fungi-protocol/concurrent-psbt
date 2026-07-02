use std::io::Read as _;
use std::process;

use clap::Parser;
use ptj::cli::Cli;
#[cfg(any(feature = "webgui", feature = "tui"))]
use ptj::cli::Command;

fn main() {
    let cli = Cli::parse();
    let stdin = match read_stdin_if_needed(&cli) {
        Ok(stdin) => stdin,
        Err(error) => {
            eprintln!("error: {error}");
            process::exit(1);
        }
    };
    let result = match cli.command.clone() {
        #[cfg(feature = "webgui")]
        Command::Webgui(config) => {
            if cli.output.is_some() {
                Err(ptj::Error::new("webgui does not write PSBT output"))
            } else {
                ptj::webgui::serve(config).map(|()| None)
            }
        }
        #[cfg(feature = "tui")]
        Command::Tui(config) => {
            if cli.output.is_some() {
                Err(ptj::Error::new("tui does not write PSBT output"))
            } else {
                ptj::tui::run(config).map(|()| None)
            }
        }
        _ => ptj::run_or_write_with_stdin(cli, stdin.as_deref()),
    };

    match result {
        Ok(Some(output)) => println!("{output}"),
        Ok(None) => {}
        Err(error) => {
            eprintln!("error: {error}");
            process::exit(1);
        }
    }
}

fn read_stdin_if_needed(cli: &Cli) -> Result<Option<Vec<u8>>, ptj::Error> {
    if !cli.command.reads_stdin() {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .map_err(|error| ptj::Error::new(format!("reading stdin: {error}")))?;
    Ok(Some(bytes))
}
