use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::Path;

use concurrent_psbt::roles::constructor::dynamic;
use psbt_v2::v0::bitcoin as bip174;
use psbt_v2::v2::Psbt;

use crate::{Error, Result};

pub(crate) fn read_modifiable(path: &Path) -> Result<dynamic::Constructor> {
    let psbt = read_psbt(path)?;
    dynamic::Constructor::try_from_psbt(psbt)
        .map_err(|error| Error::new(format!("{}: {error}", path.display())))
}

pub(crate) fn encode_psbt(psbt: &Psbt) -> String {
    use psbt_v2::bitcoin::base64::prelude::{BASE64_STANDARD, Engine as _};
    let bytes = Psbt::serialize(psbt);
    BASE64_STANDARD.encode(&bytes)
}

pub(crate) fn write_text_atomic(path: &Path, text: &str) -> Result<()> {
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
        file.write_all(text.as_bytes())?;
        file.write_all(b"\n")?;
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

pub(crate) fn read_psbt(path: &Path) -> Result<Psbt> {
    let raw = fs::read(path)
        .map_err(|error| Error::new(format!("reading {}: {error}", path.display())))?;
    let bytes = psbt_bytes(path, raw)?;
    if bip174::Psbt::deserialize(&bytes).is_ok() {
        return Err(Error::new(format!(
            "{} is a BIP 174 PSBT; importing or upgrading BIP 174 inputs is not implemented yet",
            path.display()
        )));
    }

    match std::panic::catch_unwind(|| Psbt::deserialize(&bytes)) {
        Ok(Ok(psbt)) => Ok(psbt),
        Ok(Err(error)) => Err(Error::new(format!("parsing {}: {error}", path.display()))),
        Err(_) => Err(Error::new(format!(
            "parsing {}: unsupported or malformed PSBT",
            path.display()
        ))),
    }
}

fn psbt_bytes(path: &Path, raw: Vec<u8>) -> Result<Vec<u8>> {
    if raw.starts_with(b"psbt") {
        return Ok(raw);
    }
    let text = String::from_utf8(raw).map_err(|_| {
        Error::new(format!(
            "{} is neither binary PSBT nor valid UTF-8",
            path.display()
        ))
    })?;
    use psbt_v2::bitcoin::base64::prelude::{BASE64_STANDARD, Engine as _};
    BASE64_STANDARD
        .decode(text.trim())
        .map_err(|error| Error::new(format!("decoding base64 {}: {error}", path.display())))
}

pub(crate) fn wrap_constructor(constructor: dynamic::Constructor) -> dynamic::ResultConstructor {
    dynamic::ResultConstructor::wrap(constructor)
}
