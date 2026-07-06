use std::process;

use clap::Parser;
use ptj::cli::{Cli, Command};

fn main() {
    let cli = Cli::parse();
    let result = match cli.command.clone() {
        Command::Webgui(config) => {
            if cli.output.is_some() {
                Err(ptj::Error::new("webgui does not write PSBT output"))
            } else {
                ptj::webgui::serve(config).map(|()| None)
            }
        }
        _ => ptj::run_or_write(cli),
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
