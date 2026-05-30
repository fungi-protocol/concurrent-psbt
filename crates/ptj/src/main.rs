use std::env;
use std::fs;
use std::process;

use concurrent_psbt::Join;
use concurrent_psbt::roles::constructor::dynamic;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 4 || args[1] != "join" {
        eprintln!("usage: ptj join <file1> <file2> [file3 ...]");
        process::exit(2);
    }

    let files = &args[2..];

    let constructors: Vec<_> = files.iter().map(|f| parse_psbt(f)).collect();

    let result = constructors
        .into_iter()
        .map(dynamic::ResultConstructor::wrap)
        .reduce(|a, b| a.join(b))
        .unwrap();

    if !result.is_ok() {
        eprintln!("error: join produced conflicting fields");
        eprintln!();
        result.for_each_conflict(|section, field, conflict| {
            eprintln!("  {section}.{field}: {conflict:?}");
        });
        process::exit(1);
    }

    let ctor = result.try_unwrap().expect("checked is_ok");
    let psbt = ctor.into_psbt();

    use psbt_v2::bitcoin::base64::prelude::{BASE64_STANDARD, Engine as _};
    let bytes = psbt_v2::v2::Psbt::serialize(&psbt);
    println!("{}", BASE64_STANDARD.encode(&bytes));
}

fn parse_psbt(path: &str) -> dynamic::Constructor {
    let raw = fs::read(path).unwrap_or_else(|e| {
        eprintln!("error reading {path}: {e}");
        process::exit(1);
    });

    let bytes = if raw.starts_with(b"psbt") {
        raw
    } else {
        let text = String::from_utf8(raw).unwrap_or_else(|_| {
            eprintln!("error: {path} is neither binary PSBT nor valid UTF-8");
            process::exit(1);
        });
        use psbt_v2::bitcoin::base64::prelude::{BASE64_STANDARD, Engine as _};
        BASE64_STANDARD.decode(text.trim()).unwrap_or_else(|e| {
            eprintln!("error decoding base64 {path}: {e}");
            process::exit(1);
        })
    };

    let psbt = psbt_v2::v2::Psbt::deserialize(&bytes).unwrap_or_else(|e| {
        eprintln!("error parsing {path}: {e}");
        process::exit(1);
    });

    dynamic::Constructor::try_from_psbt(psbt).unwrap_or_else(|e| {
        eprintln!("error: {path}: {e}");
        process::exit(1);
    })
}
