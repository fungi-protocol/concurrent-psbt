//! Liberal parsing for byte-string arguments (seeds, ids, opaque records).
//!
//! Port of ptj's `bytes_arg` module: behavior and error text match the
//! CLI/webgui exactly so seed and record validation is indistinguishable
//! across the HttpBackend/WasmBackend seam (the same rule as `decode_hex`,
//! which lives here now).
//!
//! CLI arguments and route parameters that carry raw bytes accept any
//! encoding that the character set identifies unambiguously, instead of
//! demanding one format:
//!
//! - **hex** — the canonical form. A string made entirely of hex digits is
//!   ALWAYS interpreted as hex (every hex string is also base58-alphabet, so
//!   context resolves the ambiguity in favor of hex); an odd-length hex-only
//!   string is an error, not a base58 fallback.
//! - **bech32/bech32m** — recognized by the `1` separator plus a valid
//!   checksum; decodes to the data-part bytes (HRP is dropped).
//! - **base58** — recognized by base58-alphabet characters that rule out hex
//!   (e.g. `x`, `Q`). If the trailing 4-byte double-SHA256 checksum validates
//!   (base58check) the checksum is stripped; otherwise the string decodes as
//!   plain base58.
//!
//! Errors name every decoding that was attempted and why it failed.

/// Decode a byte-string argument, detecting the encoding from its character
/// set as described in the module docs.
pub(crate) fn parse_bytes_arg(value: &str) -> Result<Vec<u8>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("empty byte string".to_string());
    }

    if value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        if value.len().is_multiple_of(2) {
            return decode_hex(value);
        }
        return Err(format!(
            "hex string has odd length: {value} (every character is a hex digit, \
             so it was not tried as base58; pad to full bytes)"
        ));
    }

    let mut attempts: Vec<String> = vec!["not hex (contains non-hex characters)".to_string()];

    if value.contains('1') {
        match bitcoin::bech32::decode(value) {
            Ok((_hrp, data)) => return Ok(data),
            Err(error) => attempts.push(format!("not bech32 ({error})")),
        }
    } else {
        attempts.push("not bech32 (no `1` separator)".to_string());
    }

    if value.bytes().all(is_base58_char) {
        if let Ok(payload) = bitcoin::base58::decode_check(value) {
            return Ok(payload);
        }
        match bitcoin::base58::decode(value) {
            Ok(bytes) => return Ok(bytes),
            Err(error) => attempts.push(format!("not base58 ({error})")),
        }
    } else {
        attempts.push("not base58 (contains characters outside the base58 alphabet)".to_string());
    }

    Err(format!(
        "could not decode byte string {value}: {}",
        attempts.join("; ")
    ))
}

/// Strict hex decoding (used by the hex branch and callers that require hex).
pub(crate) fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    if !value.len().is_multiple_of(2) {
        return Err(format!("hex string has odd length: {value}"));
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_nibble(pair[0]).ok_or_else(|| format!("invalid hex: {value}"))?;
            let low = hex_nibble(pair[1]).ok_or_else(|| format!("invalid hex: {value}"))?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn is_base58_char(byte: u8) -> bool {
    const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    ALPHABET.contains(&byte)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn hex_wins_for_hex_charset() {
        assert_eq!(parse_bytes_arg("deadbeef").unwrap(), vec![0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(parse_bytes_arg("ABCD").unwrap(), vec![0xab, 0xcd]);
    }

    #[test]
    fn odd_length_hex_charset_is_an_error_not_base58() {
        // "abc" is valid base58, but the hex charset takes precedence.
        let error = parse_bytes_arg("abc").unwrap_err();
        assert!(error.contains("odd length"), "{error}");
    }

    #[test]
    fn whitespace_is_trimmed() {
        assert_eq!(parse_bytes_arg(" 0102 \n").unwrap(), vec![1, 2]);
    }

    #[test]
    fn empty_is_an_error() {
        assert!(parse_bytes_arg("  ").is_err());
    }

    #[test]
    fn base58_decodes_when_charset_rules_out_hex() {
        // 'x' and 'z' are not hex digits; base58("xyz") is well-defined.
        let decoded = parse_bytes_arg("xyz").unwrap();
        assert_eq!(decoded, bitcoin::base58::decode("xyz").unwrap());
    }

    #[test]
    fn base58check_strips_the_checksum() {
        let encoded = bitcoin::base58::encode_check(&[0x6f, 1, 2, 3]);
        assert_eq!(parse_bytes_arg(&encoded).unwrap(), vec![0x6f, 1, 2, 3]);
    }

    #[test]
    fn bech32_decodes_the_data_part() {
        use bitcoin::bech32::{self, Hrp};
        let encoded = bech32::encode::<bech32::Bech32m>(
            Hrp::parse("seed").unwrap(),
            &[0xde, 0xad, 0xbe, 0xef],
        )
        .unwrap();
        assert_eq!(parse_bytes_arg(&encoded).unwrap(), vec![0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn undecodable_error_names_the_attempts() {
        let error = parse_bytes_arg("0O0O!").unwrap_err();
        assert!(error.contains("not hex"), "{error}");
        assert!(error.contains("not bech32"), "{error}");
        assert!(error.contains("not base58"), "{error}");
    }

    #[test]
    fn decode_hex_still_rejects_non_hex() {
        assert!(decode_hex("zz").is_err());
        assert!(decode_hex("abc").is_err());
        assert_eq!(decode_hex("0A0b").unwrap(), vec![0x0a, 0x0b]);
    }
}
