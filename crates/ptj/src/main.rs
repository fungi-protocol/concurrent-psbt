use std::process;

use clap::Parser;
use ptj::cli::Cli;

fn main() {
    let cli = Cli::parse();
    let result = ptj::run_or_write(cli);

    match result {
        Ok(Some(output)) => println!("{output}"),
        Ok(None) => {}
        Err(error) => {
            eprintln!("error: {error}");
            process::exit(1);
        }
    }
}
