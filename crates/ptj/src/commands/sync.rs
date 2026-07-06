use std::path::{Path, PathBuf};

use psbt_v2::v2::Psbt;

use crate::cli::SyncConfig;
use crate::{Error, Result, io};

pub(super) fn run(config: SyncConfig) -> Result<Psbt> {
    io::with_file_lock(&config.state, || {
        sync_locked(&config.state, &config.files, &config.directories)
    })
}

fn sync_locked(state: &Path, files: &[PathBuf], directories: &[PathBuf]) -> Result<Psbt> {
    let state_exists = state
        .try_exists()
        .map_err(|error| Error::new(format!("checking {}: {error}", state.display())))?;
    let mut paths = Vec::with_capacity(files.len() + usize::from(state_exists));
    if state_exists {
        paths.push(state.to_path_buf());
    }
    paths.extend(files.iter().cloned());
    for directory in directories {
        paths.extend(psbt_files_in_directory(directory)?);
    }
    if state_exists {
        paths.retain(|path| !same_existing_path(path, state));
        paths.insert(0, state.to_path_buf());
    }

    if !state_exists && paths.is_empty() {
        return Err(Error::new(format!(
            "no existing state at {} and no PSBT files provided",
            state.display()
        )));
    }

    let psbt = super::join::join_paths(paths.iter().map(PathBuf::as_path))?;
    io::write_text_atomic(state, &io::encode_psbt(&psbt))?;
    Ok(psbt)
}

fn psbt_files_in_directory(directory: &Path) -> Result<Vec<PathBuf>> {
    let entries = std::fs::read_dir(directory).map_err(|error| {
        Error::new(format!(
            "reading directory {}: {error}",
            directory.display()
        ))
    })?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            Error::new(format!(
                "reading directory entry in {}: {error}",
                directory.display()
            ))
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| {
            Error::new(format!("reading file type for {}: {error}", path.display()))
        })?;
        if file_type.is_file() && has_psbt_extension(&path) {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn has_psbt_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("psbt"))
}

fn same_existing_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}
