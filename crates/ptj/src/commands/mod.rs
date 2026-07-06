mod concatenate;
mod create;
mod export_bip174;
mod inspect;
mod join;
mod sort;

use crate::Result;
use crate::cli::Command;

pub(crate) fn run(command: Command) -> Result<String> {
    match command {
        Command::Concatenate(config) => {
            concatenate::run(config).map(|psbt| crate::io::encode_psbt(&psbt))
        }
        Command::Create(config) => create::run(config).map(|psbt| crate::io::encode_psbt(&psbt)),
        Command::ExportBip174(config) => export_bip174::run(config),
        Command::Inspect(config) => inspect::run(config),
        Command::Join(config) => join::run(config).map(|psbt| crate::io::encode_psbt(&psbt)),
        Command::Sort(config) => sort::run(config).map(|psbt| crate::io::encode_psbt(&psbt)),
    }
}
