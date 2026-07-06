use std::path::{Path, PathBuf};

use psbt_v2::v2::Psbt;

use crate::cli::SyncConfig;
use crate::{Error, Result, io};

pub(super) fn run(config: SyncConfig) -> Result<Psbt> {
    io::with_file_lock(&config.state, || sync_locked(&config.state, &config.files))
}

fn sync_locked(state: &Path, files: &[PathBuf]) -> Result<Psbt> {
    let state_exists = state
        .try_exists()
        .map_err(|error| Error::new(format!("checking {}: {error}", state.display())))?;
    if !state_exists && files.is_empty() {
        return Err(Error::new(format!(
            "no existing state at {} and no PSBT files provided",
            state.display()
        )));
    }

    let mut paths = Vec::with_capacity(files.len() + usize::from(state_exists));
    if state_exists {
        paths.push(state);
    }
    paths.extend(files.iter().map(PathBuf::as_path));

    let psbt = super::join::join_paths(paths)?;
    io::write_text_atomic(state, &io::encode_psbt(&psbt))?;
    Ok(psbt)
}
