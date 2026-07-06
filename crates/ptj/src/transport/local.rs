//! `LocalTransport`: the existing file/dir sync behavior, repackaged behind the
//! `Transport` trait. Same files read, same `--state` self-fold, same locked
//! atomic-rename publish — observable behavior is byte-for-byte unchanged; only
//! the call structure moved behind the trait.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use transport_core::{Message, Transport};

use crate::cli::OutputFileFormat;
use crate::{Error, Result, io};

/// File/dir transport: positional PSBT files/directories plus an optional
/// `--state` file that is both an input (re-folded on each `collect`) and the
/// `publish` target, plus an optional one-shot stdin source.
pub(crate) struct LocalTransport {
    /// Positional PSBT files/directories (`SyncConfig.sources`).
    sources: Vec<PathBuf>,
    /// `--state` / `-o` path: re-folded on `collect`, written on `publish`.
    state: Option<PathBuf>,
    /// One-shot `-` stdin source. The outer `Option` tracks whether a stdin
    /// source is configured and undrained: `Some(_)` until the first `collect`
    /// consumes it, then `None`. The inner `Option<Vec<u8>>` is the runner's
    /// stdin bytes exactly as passed (`None` = runner gave no stdin), preserving
    /// today's distinction between "no runner stdin" and "empty stdin".
    stdin: Option<Option<Vec<u8>>>,
    /// Where `publish` writes the converged result (the state/output path).
    publish_target: Option<PathBuf>,
    /// base64 vs binary on `publish`, matching today's `--output-file-format`.
    output_format: OutputFileFormat,
}

impl LocalTransport {
    /// Build a `LocalTransport` from the sync sources and the resolved publish
    /// target. `state` is the `--state` input (re-folded each step); for `-o`
    /// sync the runner passes the same path as `publish_target` with `state` set
    /// so the prior result is folded back in.
    pub(crate) fn new(
        sources: Vec<PathBuf>,
        state: Option<PathBuf>,
        stdin: Option<&[u8]>,
        publish_target: Option<PathBuf>,
        output_format: OutputFileFormat,
    ) -> Self {
        let stdin = if sources.iter().any(|source| io::is_stdin_path(source)) {
            Some(stdin.map(<[u8]>::to_vec))
        } else {
            None
        };
        Self {
            sources,
            state,
            stdin,
            publish_target,
            output_format,
        }
    }

    /// The file/dir gather, in ptj's own error type. The `Transport` impl wraps
    /// this and maps the error into `transport_core::Error`.
    fn collect_local(&mut self) -> Result<Vec<Vec<u8>>> {
        // Same gather as today's `sync_sources`, but emitting validated bytes
        // instead of folding: directory expansion (sorted *.psbt), `--state`
        // self-inclusion, and the one-shot stdin source (drained once).
        let mut paths = psbt_paths_from_sources(&self.sources)?;
        if let Some(state) = self.state.as_deref() {
            let state_exists = state
                .try_exists()
                .map_err(|error| Error::new(format!("checking {}: {error}", state.display())))?;
            if state_exists && !paths.iter().any(|path| same_existing_path(path, state)) {
                paths.insert(0, state.to_path_buf());
            }
        }

        // Drain the one-shot stdin source: present only on the first collect.
        let stdin_source = self.stdin.take();
        let has_stdin = stdin_source.is_some();

        if paths.is_empty() && !has_stdin {
            return Err(Error::new("no PSBT sources provided"));
        }

        let mut messages = Vec::with_capacity(paths.len() + usize::from(has_stdin));
        for path in &paths {
            // Validate with the path as the error label (preserves the exact
            // diagnostics today's file reader produces), then re-encode to the
            // opaque byte payload `sync_once_over` will fold.
            let psbt = io::read_psbt(path)?;
            messages.push(io::encode_psbt(&psbt).into_bytes());
        }
        if let Some(runner_stdin) = stdin_source {
            // Pass the runner stdin through unchanged: `None` yields today's
            // "stdin PSBT source requires stdin bytes from the runner" error.
            let psbt = io::read_psbt_source(Path::new("-"), runner_stdin.as_deref())?;
            messages.push(io::encode_psbt(&psbt).into_bytes());
        }
        Ok(messages)
    }

    /// The file/dir publish, in ptj's own error type. The `Transport` impl wraps
    /// this and maps the error into `transport_core::Error`.
    fn publish_local(&mut self, message: Vec<u8>) -> Result<()> {
        // No publish target (pure one-shot stdout path) => no-op, matching
        // today's behavior where the caller emits to stdout instead. The lock +
        // temp-file rename + fsync durability lives here, in the file transport,
        // never in the convergence engine.
        let Some(target) = self.publish_target.clone() else {
            return Ok(());
        };
        // The envelope terminates at the disk boundary: state files stay raw
        // PSBTs. Non-PSBT messages do not persist locally — in the sneakernet
        // setting negotiation rides inside the PSBT as proprietary fields.
        // `Message::decode` returns a `transport_core::Result`; the `?` maps it
        // into ptj's `Error` via the `From` bridge in `error.rs`.
        let Message::Psbt(payload) = Message::decode(&message)? else {
            return Ok(());
        };
        let text = String::from_utf8(payload)
            .map_err(|_| Error::new("converged PSBT payload is not valid UTF-8"))?;
        write_output_file(&target, &text, self.output_format)
    }
}

#[async_trait]
impl Transport for LocalTransport {
    // File/dir I/O is synchronous and fast; the async methods just call the
    // existing sync gather/publish. The `--ongoing` change-detection is what went
    // event-driven (see `run_ongoing_sync` in lib.rs), not the per-step I/O, so
    // behavior stays byte-for-byte identical.
    async fn collect(&mut self) -> transport_core::Result<Vec<Vec<u8>>> {
        self.collect_local()
            .map_err(|error| transport_core::Error::new(error.to_string()))
    }

    async fn publish(&mut self, message: Vec<u8>) -> transport_core::Result<()> {
        self.publish_local(message)
            .map_err(|error| transport_core::Error::new(error.to_string()))
    }
}

fn write_output_file(path: &Path, output: &str, format: OutputFileFormat) -> Result<()> {
    match format {
        OutputFileFormat::Base64 => io::write_text_atomic(path, output),
        OutputFileFormat::Binary => {
            io::write_binary_atomic(path, &single_psbt_output_bytes(output)?)
        }
    }
}

fn single_psbt_output_bytes(output: &str) -> Result<Vec<u8>> {
    let trimmed = output.trim();
    if trimmed.lines().count() != 1 {
        return Err(Error::new(
            "binary --output-file-format requires a command that emits exactly one PSBT",
        ));
    }
    use psbt_v2::bitcoin::base64::prelude::{BASE64_STANDARD, Engine as _};
    BASE64_STANDARD
        .decode(trimmed)
        .map_err(|error| Error::new(format!("decoding command output as PSBT base64: {error}")))
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
