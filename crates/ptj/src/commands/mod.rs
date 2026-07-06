pub(crate) mod atomize;
pub(crate) mod concatenate;
pub(crate) mod create;
pub(crate) mod export_bip174;
pub(crate) mod import_bip174;
pub(crate) mod inspect;
pub(crate) mod join;
pub(crate) mod make_unordered;
pub(crate) mod negotiation;
pub(crate) mod sort;
pub(crate) mod sync;

use crate::cli::Command;
use crate::{Error, Result};

pub(crate) fn run(command: Command) -> Result<String> {
    run_with_stdin(command, None)
}

pub(crate) fn run_with_stdin(command: Command, stdin: Option<&[u8]>) -> Result<String> {
    validate_stdin_shape(&command, stdin)?;
    match command {
        Command::Atomize(config) => atomize::run(config, stdin),
        Command::Concatenate(config) => {
            concatenate::run(config, stdin).map(|psbt| crate::io::encode_psbt(&psbt))
        }
        Command::Create(config) => create::run(config).map(|psbt| crate::io::encode_psbt(&psbt)),
        Command::ExportBip174(config) => export_bip174::run(config, stdin),
        Command::ImportBip174(config) => {
            import_bip174::run(config, stdin).map(|psbt| crate::io::encode_psbt(&psbt))
        }
        Command::Inspect(config) => inspect::run(config, stdin),
        Command::Join(config) => join::run(config, stdin).map(|psbt| crate::io::encode_psbt(&psbt)),
        Command::MakeUnordered(config) => {
            make_unordered::run(config, stdin).map(|psbt| crate::io::encode_psbt(&psbt))
        }
        Command::Pay(config) => {
            negotiation::run_pay(config, stdin).map(|psbt| crate::io::encode_psbt(&psbt))
        }
        Command::Confirm(config) => {
            negotiation::run_confirm(config, stdin).map(|psbt| crate::io::encode_psbt(&psbt))
        }
        Command::Payments(config) => negotiation::run_payments(config, stdin),
        Command::Sort(config) => sort::run(config, stdin).map(|psbt| crate::io::encode_psbt(&psbt)),
        Command::Sync(config) => sync::run(config, stdin).map(|psbt| crate::io::encode_psbt(&psbt)),
        Command::Webgui(_) => Err(crate::Error::new(
            "webgui is an interactive command; call ptj::webgui::serve",
        )),
    }
}

/// Drive one convergence step over a file/dir transport that publishes the
/// converged result to `publish_target` in `output_format`. The runner
/// (`lib.rs`) wraps this in a file lock so collect+publish are atomic.
pub(crate) fn run_sync_over_local(
    config: &crate::cli::SyncConfig,
    stdin: Option<&[u8]>,
    publish_target: std::path::PathBuf,
    output_format: crate::cli::OutputFileFormat,
) -> Result<()> {
    validate_stdin_shape(&Command::Sync(config.clone()), stdin)?;
    let mut transport = sync::local_transport(config, stdin, Some(publish_target), output_format);
    // Drive the async convergence step on the single sync-driver runtime edge.
    sync::drive_async(sync::sync_once_over(&mut transport))?;
    Ok(())
}

pub(crate) fn validate_ongoing_sync(
    config: &crate::cli::SyncConfig,
    stdin: Option<&[u8]>,
) -> Result<()> {
    sync::validate_ongoing(config, stdin)
}

pub(crate) fn sync_poll_interval(config: &crate::cli::SyncConfig) -> std::time::Duration {
    sync::poll_interval(config)
}

fn validate_stdin_shape(command: &Command, stdin: Option<&[u8]>) -> Result<()> {
    let stdin_sources = command.stdin_source_count();
    if stdin_sources > 1 {
        return Err(Error::new("stdin can only be used as one PSBT source"));
    }
    if stdin.is_some_and(|bytes| !bytes.is_empty()) && !command.reads_stdin() {
        return Err(Error::new(
            "stdin input was provided, but no command argument reads '-'",
        ));
    }
    Ok(())
}
