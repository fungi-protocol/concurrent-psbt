pub(crate) mod atomize;
pub(crate) mod concatenate;
pub(crate) mod create;
pub(crate) mod export_bip174;
pub(crate) mod inspect;
pub(crate) mod join;
pub(crate) mod make_unordered;
pub(crate) mod sort;
mod sync;

use crate::Result;
use crate::cli::Command;

pub(crate) fn run(command: Command) -> Result<String> {
    match command {
        Command::Atomize(config) => atomize::run(config),
        Command::Concatenate(config) => {
            concatenate::run(config).map(|psbt| crate::io::encode_psbt(&psbt))
        }
        Command::Create(config) => create::run(config).map(|psbt| crate::io::encode_psbt(&psbt)),
        Command::ExportBip174(config) => export_bip174::run(config),
        Command::Inspect(config) => inspect::run(config),
        Command::Join(config) => join::run(config).map(|psbt| crate::io::encode_psbt(&psbt)),
        Command::MakeUnordered(config) => {
            make_unordered::run(config).map(|psbt| crate::io::encode_psbt(&psbt))
        }
        Command::Sort(config) => sort::run(config).map(|psbt| crate::io::encode_psbt(&psbt)),
        Command::Sync(config) => sync::run(config).map(|psbt| crate::io::encode_psbt(&psbt)),
        Command::Webgui(_) => Err(crate::Error::new(
            "webgui is an interactive command; call ptj::webgui::serve",
        )),
    }
}
