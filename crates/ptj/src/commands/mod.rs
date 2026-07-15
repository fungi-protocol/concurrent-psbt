pub(crate) mod assign_ids;
pub(crate) mod atomize;
#[cfg(feature = "webgui")]
pub(crate) mod classify;
pub(crate) mod concatenate;
pub(crate) mod create;
pub(crate) mod export_bip174;
pub(crate) mod fee;
#[cfg(feature = "webgui")]
pub(crate) mod field_edit;
pub(crate) mod import_bip174;
pub(crate) mod inspect;
pub(crate) mod join;
#[cfg(feature = "webgui")]
pub(crate) mod lifehash;
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
        Command::AssignIds(config) => {
            assign_ids::run(config, stdin).map(|psbt| crate::io::encode_psbt(&psbt))
        }
        Command::Atomize(config) => atomize::run(config, stdin),
        // The same wire value GET /api/capabilities serves — one catalog
        // across shells. `{:#}` is serde_json::Value's pretty form.
        Command::Capabilities(_) => Ok(format!("{:#}", crate::capabilities::catalog_json())),
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
        Command::Fee(config) => fee::run(config, stdin).map(|psbt| crate::io::encode_psbt(&psbt)),
        Command::Pay(config) => {
            negotiation::run_pay(config, stdin).map(|psbt| crate::io::encode_psbt(&psbt))
        }
        Command::Confirm(config) => {
            negotiation::run_confirm(config, stdin).map(|psbt| crate::io::encode_psbt(&psbt))
        }
        Command::Payments(config) => negotiation::run_payments(config, stdin),
        Command::Sort(config) => sort::run(config, stdin).map(|psbt| crate::io::encode_psbt(&psbt)),
        Command::Sync(config) => sync::run(config, stdin).map(|psbt| crate::io::encode_psbt(&psbt)),
        #[cfg(feature = "webgui")]
        Command::Webgui(_) => Err(crate::Error::new(
            "webgui is an interactive command; call ptj::webgui::serve",
        )),
        #[cfg(feature = "tui")]
        Command::Tui(_) => Err(crate::Error::new(
            "tui is an interactive command; call ptj::tui::run",
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

/// Boundary gate for user-supplied ordering seeds, in every ordering mode.
///
/// The spec states the 128-bit minimum as a MUST for deterministic ordering
/// (`PSBT_GLOBAL_SORT_DETERMINISTIC = 0x01`); a seed supplied without the flag
/// feeds the identical derivation (`H(seed || id)`), so the same minimum is
/// applied at the acceptance boundary. Strict by default, overridable always:
/// `--allow-short-seed` / `allow_short_seed` accepts a short seed explicitly.
pub(crate) fn require_spec_minimum_seed(seed: &[u8], allow_short_seed: bool) -> Result<()> {
    let len = seed.len();
    if allow_short_seed || len >= concurrent_psbt::sorter::SPEC_MIN_SEED_BYTES {
        return Ok(());
    }
    Err(Error::new(format!(
        "ordering seed is {len} byte{}; the spec requires at least 128 bits (16 bytes) of \
         randomness; pass --allow-short-seed (allow_short_seed on the web API) to accept it anyway",
        if len == 1 { "" } else { "s" },
    )))
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
