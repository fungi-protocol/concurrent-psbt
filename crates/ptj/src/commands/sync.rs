use std::path::{Path, PathBuf};
use std::time::Duration;

use psbt_v2::v2::Psbt;

use crate::cli::SyncConfig;
use crate::{Error, Result, io};

pub(super) fn run(config: SyncConfig, stdin: Option<&[u8]>) -> Result<Psbt> {
    if config.ongoing {
        return Err(Error::new(
            "ongoing sync requires --state or --output-file so the runner can update the state file",
        ));
    }
    run_once(&config, stdin)
}

pub(crate) fn run_once(config: &SyncConfig, stdin: Option<&[u8]>) -> Result<Psbt> {
    sync_sources(&config.sources, config.state.as_deref(), stdin)
}

pub(crate) fn validate_ongoing(config: &SyncConfig, stdin: Option<&[u8]>) -> Result<()> {
    if config.sources.iter().any(|source| io::is_stdin_path(source)) {
        return Err(Error::new("ongoing sync cannot use '-' because stdin is a one-shot source"));
    }
    if stdin.is_some_and(|bytes| !bytes.is_empty()) {
        return Err(Error::new("ongoing sync cannot consume runner stdin"));
    }
    if config.max_iterations == Some(0) {
        return Err(Error::new("--max-iterations must be greater than zero"));
    }
    Ok(())
}

pub(crate) fn poll_interval(config: &SyncConfig) -> Duration {
    Duration::from_millis(config.poll_interval_ms)
}

fn sync_sources(sources: &[PathBuf], state: Option<&Path>, stdin: Option<&[u8]>) -> Result<Psbt> {
    let stdin_psbt = stdin_source(sources, stdin)?;
    let mut paths = psbt_paths_from_sources(sources)?;
    if let Some(state) = state {
        let state_exists = state
            .try_exists()
            .map_err(|error| Error::new(format!("checking {}: {error}", state.display())))?;
        if state_exists && !paths.iter().any(|path| same_existing_path(path, state)) {
            paths.insert(0, state.to_path_buf());
        }
    }
    let has_stdin_psbt = stdin_psbt.is_some();
    if paths.is_empty() && !has_stdin_psbt {
        return Err(Error::new("no PSBT sources provided"));
    }

    let mut psbts =
        Vec::with_capacity(usize::from(!paths.is_empty()) + usize::from(has_stdin_psbt));
    if !paths.is_empty() {
        psbts.push(super::join::join_paths(paths.iter().map(PathBuf::as_path))?);
    }
    if let Some(psbt) = stdin_psbt {
        psbts.push(psbt);
    }
    let psbt = super::join::join_psbts(psbts)?;
    Ok(psbt)
}

fn stdin_source(sources: &[PathBuf], stdin: Option<&[u8]>) -> Result<Option<Psbt>> {
    if sources.iter().any(|source| io::is_stdin_path(source)) {
        return io::read_psbt_source(Path::new("-"), stdin).map(Some);
    }
    Ok(None)
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

fn psbt_paths_from_sources(sources: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for source in sources {
        if io::is_stdin_path(source) {
            continue;
        }
        let metadata = std::fs::metadata(source)
            .map_err(|error| Error::new(format!("reading source {}: {error}", source.display())))?;
        if metadata.is_dir() {
            paths.extend(psbt_files_in_directory(source)?);
        } else if metadata.is_file() {
            paths.push(source.clone());
        } else {
            return Err(Error::new(format!(
                "{} is neither a PSBT file nor a directory",
                source.display()
            )));
        }
    }
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
