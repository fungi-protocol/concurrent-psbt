use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use concurrent_psbt::roles::constructor::dynamic;
use psbt_v2::v0::bitcoin as bip174;
use psbt_v2::v2::Psbt;

use crate::{Error, Result};

#[allow(dead_code)]
pub(crate) fn read_modifiable(path: &Path) -> Result<dynamic::Constructor> {
    let psbt = read_psbt(path)?;
    dynamic::Constructor::try_from_psbt(psbt)
        .map_err(|error| Error::new(format!("{}: {error}", path.display())))
}

pub(crate) fn read_modifiable_source(
    path: &Path,
    stdin: Option<&[u8]>,
) -> Result<dynamic::Constructor> {
    let psbt = read_psbt_source(path, stdin)?;
    let label = source_label(path);
    dynamic::Constructor::try_from_psbt(psbt)
        .map_err(|error| Error::new(format!("{label}: {error}")))
}

pub(crate) fn encode_psbt(psbt: &Psbt) -> String {
    use psbt_v2::bitcoin::base64::prelude::{BASE64_STANDARD, Engine as _};
    let bytes = Psbt::serialize(psbt);
    BASE64_STANDARD.encode(&bytes)
}

pub(crate) fn write_text_atomic(path: &Path, text: &str) -> Result<()> {
    let mut bytes = Vec::with_capacity(text.len() + 1);
    bytes.extend_from_slice(text.as_bytes());
    bytes.push(b'\n');
    write_bytes_atomic(path, &bytes)
}

pub(crate) fn write_binary_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    write_bytes_atomic(path, bytes)
}

fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .ok_or_else(|| Error::new(format!("{} is not a file path", path.display())))?;
    let temp_path = parent.join(format!(
        ".{}.tmp-{}-{:016x}",
        file_name.to_string_lossy(),
        std::process::id(),
        rand::random::<u64>()
    ));

    let result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp_path, path)?;
        if let Ok(parent_dir) = File::open(parent) {
            let _ = parent_dir.sync_all();
        }
        Ok(())
    })();

    if let Err(error) = result {
        let _ = fs::remove_file(&temp_path);
        return Err(Error::new(format!("writing {}: {error}", path.display())));
    }

    Ok(())
}

pub(crate) fn with_file_lock<T>(path: &Path, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let lock_path = lock_path(path)?;
    let mut lock = create_lock_file(path, &lock_path)?;
    writeln!(lock, "pid={}", std::process::id())
        .map_err(|error| Error::new(format!("writing {}: {error}", lock_path.display())))?;
    lock.sync_all()
        .map_err(|error| Error::new(format!("syncing {}: {error}", lock_path.display())))?;

    let result = f();
    drop(lock);
    let remove_result = fs::remove_file(&lock_path);
    match (result, remove_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Ok(_), Err(error)) => Err(Error::new(format!("unlocking {}: {error}", path.display()))),
        (Err(error), _) => Err(error),
    }
}

fn create_lock_file(path: &Path, lock_path: &Path) -> Result<File> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(lock_path)
        {
            Ok(file) => return Ok(file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if Instant::now() >= deadline {
                    return Err(Error::new(format!(
                        "locking {}: timed out waiting for {}",
                        path.display(),
                        lock_path.display()
                    )));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(Error::new(format!("locking {}: {error}", path.display()))),
        }
    }
}

fn lock_path(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .ok_or_else(|| Error::new(format!("{} is not a file path", path.display())))?;
    Ok(parent.join(format!(".{}.lock", file_name.to_string_lossy())))
}

pub(crate) fn read_psbt(path: &Path) -> Result<Psbt> {
    let raw = fs::read(path)
        .map_err(|error| Error::new(format!("reading {}: {error}", path.display())))?;
    parse_psbt_bytes(&path.display().to_string(), &raw)
}

pub(crate) fn read_psbt_source(path: &Path, stdin: Option<&[u8]>) -> Result<Psbt> {
    if is_stdin_path(path) {
        let bytes = stdin
            .ok_or_else(|| Error::new("stdin PSBT source requires stdin bytes from the runner"))?;
        return parse_psbt_bytes("stdin", bytes);
    }
    read_psbt(path)
}

pub(crate) fn read_bip174(path: &Path) -> Result<bip174::Psbt> {
    let raw = fs::read(path)
        .map_err(|error| Error::new(format!("reading {}: {error}", path.display())))?;
    parse_bip174_bytes(&path.display().to_string(), &raw)
}

pub(crate) fn read_bip174_source(path: &Path, stdin: Option<&[u8]>) -> Result<bip174::Psbt> {
    if is_stdin_path(path) {
        let bytes = stdin.ok_or_else(|| {
            Error::new("stdin BIP 174 source requires stdin bytes from the runner")
        })?;
        return parse_bip174_bytes("stdin", bytes);
    }
    read_bip174(path)
}

pub(crate) fn is_stdin_path(path: &Path) -> bool {
    path == Path::new("-")
}

pub(crate) fn source_label(path: &Path) -> String {
    if is_stdin_path(path) {
        "stdin".to_string()
    } else {
        path.display().to_string()
    }
}

pub(crate) fn parse_psbt_bytes(label: &str, raw: &[u8]) -> Result<Psbt> {
    let bytes = psbt_bytes(label, raw.to_vec())?;
    if bip174::Psbt::deserialize(&bytes).is_ok() {
        return Err(Error::new(format!(
            "{label} is a BIP 174 PSBT; run `ptj import-bip174` before using BIP 370 operations"
        )));
    }

    // The crate's single panic boundary for BIP 370 parsing. psbt_v2 0.3.0
    // panics (todo!()) not only inside `deserialize` but also while
    // DISPLAYING some deserialize errors (e.g. v2::error::DeserializeError's
    // `fmt` impl), so the error must be formatted inside the same
    // catch_unwind that guards the deserialize itself. The `move` closure
    // owns `bytes`, keeping it UnwindSafe without AssertUnwindSafe. Callers
    // (webgui handlers, field_edit's constitutive re-parse) rely on this
    // function returning Err instead of unwinding on any malformed input.
    match std::panic::catch_unwind(move || {
        Psbt::deserialize(&bytes).map_err(|error| error.to_string())
    }) {
        Ok(Ok(psbt)) => Ok(psbt),
        Ok(Err(error)) => Err(Error::new(format!("parsing {label}: {error}"))),
        Err(_) => Err(Error::new(format!(
            "parsing {label}: invalid PSBT (deserialization failed)"
        ))),
    }
}

pub(crate) fn parse_bip174_bytes(label: &str, raw: &[u8]) -> Result<bip174::Psbt> {
    let bytes = psbt_bytes(label, raw.to_vec())?;
    bip174::Psbt::deserialize(&bytes)
        .map_err(|error| Error::new(format!("parsing BIP 174 {label}: {error}")))
}

fn psbt_bytes(label: &str, raw: Vec<u8>) -> Result<Vec<u8>> {
    if raw.starts_with(b"psbt") {
        return Ok(raw);
    }
    let text = String::from_utf8(raw)
        .map_err(|_| Error::new(format!("{label} is neither binary PSBT nor valid UTF-8")))?;
    use psbt_v2::bitcoin::base64::prelude::{BASE64_STANDARD, Engine as _};
    BASE64_STANDARD
        .decode(text.trim())
        .map_err(|error| Error::new(format!("decoding base64 {label}: {error}")))
}
