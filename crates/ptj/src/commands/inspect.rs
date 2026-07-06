use concurrent_psbt::global::GlobalSortExt;
use serde_json::json;

use crate::Result;
use crate::cli::InspectConfig;
use crate::io;

pub(super) fn run(config: InspectConfig) -> Result<String> {
    let psbt = io::read_psbt(&config.file)?;
    let flags = psbt.global.tx_modifiable_flags & 0x03;

    Ok(json!({
        "format": "bip370",
        "ordering": if psbt.global.is_unordered() { "unordered" } else { "ordered" },
        "input_count": psbt.global.input_count,
        "output_count": psbt.global.output_count,
        "modifiability": {
            "flags": flags,
            "inputs": flags & 0x01 != 0,
            "outputs": flags & 0x02 != 0,
        },
        "sort": {
            "mode": sort_mode(psbt.global.sort_deterministic()),
            "seed_hex": psbt.global.sort_seed().map(hex_encode),
        },
    })
    .to_string())
}

fn sort_mode(mode: Option<u8>) -> &'static str {
    match mode {
        Some(0x00) => "explicit",
        Some(0x01) => "deterministic",
        _ => "unset",
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
