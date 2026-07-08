//! LifeHash fingerprint rendering (`GET /api/lifehash/<hex-digest>`).
//!
//! Server-side generation for now — the webgui renders fingerprints with
//! zero frontend logic. A LATER task exposes the same crate as a
//! concurrent-psbt-wasm export (`lifehash(digest) -> {width, height, rgba}`)
//! so the PWA can render offline; the digest→image mapping here is the
//! contract BOTH surfaces must reproduce byte-for-byte.
//!
//! ## Version choice (deliberate, frozen)
//!
//! `Version2`, module size 1, no alpha — 32x32 RGB pixels. Version2 is the
//! LifeHash version Blockchain Commons recommends for general fingerprints
//! (CMYK-friendly gamut, no Version1 gradient bugs) and the one their
//! reference implementations default to across platforms; the snapshot
//! tests below pin the output to the bc-lifehash reference test vectors, so
//! a crate upgrade that changed the mapping would fail loudly. Fingerprints
//! must be stable across releases and surfaces: changing the version,
//! module size, or alpha is a BREAKING identity change, not a tweak.
//!
//! ## Input rule (part of the contract)
//!
//! - a 32-byte input IS a digest: rendered via `from_digest`, so pasting the
//!   sha256 identity of an object (txid, unordered unique id, peer id)
//!   fingerprints that identity directly;
//! - any other length is DATA: rendered via `from_data` (= `from_digest`
//!   over its sha256), the canonical LifeHash behavior for arbitrary bytes
//!   (16-byte output unique ids land here).

use crate::{Error, Result};

/// The frozen version/variant parameters (see the module docs).
const VERSION: lifehash_lib::Version = lifehash_lib::Version::Version2;
const MODULE_SIZE: usize = 1;
const HAS_ALPHA: bool = false;

/// Render the LifeHash image for `input` under the frozen parameters and the
/// 32-bytes-is-a-digest rule.
pub(crate) fn image_for_input(input: &[u8]) -> Result<lifehash_lib::Image> {
    let result = if input.len() == 32 {
        lifehash_lib::lifehash::from_digest(input, VERSION, MODULE_SIZE, HAS_ALPHA)
    } else {
        lifehash_lib::lifehash::from_data(input, VERSION, MODULE_SIZE, HAS_ALPHA)
    };
    result
        .map(|(image, _digest)| image)
        .map_err(|error| Error::new(format!("rendering lifehash: {error}")))
}

/// Render the LifeHash image for `input` and encode it as a PNG.
pub(crate) fn png_for_input(input: &[u8]) -> Result<Vec<u8>> {
    let image = image_for_input(input)?;
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(
        &mut bytes,
        u32::try_from(image.width).map_err(|_| Error::new("lifehash width exceeds u32"))?,
        u32::try_from(image.height).map_err(|_| Error::new("lifehash height exceeds u32"))?,
    );
    encoder.set_color(match image.channels {
        3 => png::ColorType::Rgb,
        4 => png::ColorType::Rgba,
        channels => {
            return Err(Error::new(format!(
                "unexpected lifehash channel count {channels}"
            )));
        }
    });
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|error| Error::new(format!("encoding lifehash PNG header: {error}")))?;
    writer
        .write_image_data(&image.pixels)
        .map_err(|error| Error::new(format!("encoding lifehash PNG data: {error}")))?;
    writer
        .finish()
        .map_err(|error| Error::new(format!("finishing lifehash PNG: {error}")))?;
    Ok(bytes)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use bitcoin::hashes::{Hash as _, sha256};

    use super::*;

    fn pixels_sha256_hex(image: &lifehash_lib::Image) -> String {
        sha256::Hash::hash(&image.pixels)
            .to_byte_array()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// Snapshot vectors from the bc-lifehash REFERENCE test vectors
    /// (version2, module_size 1, no alpha; sha256 over the raw RGB pixel
    /// bytes). These pin the digest→image mapping across crate upgrades AND
    /// across surfaces (the later wasm export must reproduce them).
    #[test]
    fn version2_pixels_match_the_reference_vectors() {
        for (input_hex, expected_pixels_sha256) in [
            (
                "deadbeef",
                "cb5c61fdbab952cd54b86824291d14e36255df58c80d25f7463db369e2d1ccf6",
            ),
            (
                "00ff80",
                "d93068597e777fc6ea6ac2bfce8c3bfcc9f24b3d671a190d75b3e127e1e443c0",
            ),
        ] {
            let input = crate::bytes_arg::decode_hex(input_hex).unwrap();
            let image = image_for_input(&input).unwrap();
            assert_eq!(image.width, 32, "{input_hex}");
            assert_eq!(image.height, 32, "{input_hex}");
            assert_eq!(image.channels, 3, "{input_hex}");
            assert_eq!(
                pixels_sha256_hex(&image),
                expected_pixels_sha256,
                "{input_hex}: the digest→image mapping changed — this is a \
                 BREAKING fingerprint identity change",
            );
        }
    }

    /// The 32-bytes-is-a-digest rule: `from_data(x)` ≡ `from_digest(sha256(x))`,
    /// so feeding the sha256 of a reference vector through the digest path
    /// must reproduce the vector's image.
    #[test]
    fn digest_inputs_render_via_from_digest() {
        let data = crate::bytes_arg::decode_hex("deadbeef").unwrap();
        let digest = sha256::Hash::hash(&data).to_byte_array();
        assert_eq!(digest.len(), 32);
        let via_digest = image_for_input(&digest).unwrap();
        assert_eq!(
            pixels_sha256_hex(&via_digest),
            "cb5c61fdbab952cd54b86824291d14e36255df58c80d25f7463db369e2d1ccf6",
        );
    }

    /// The PNG wrapper is lossless: decoding it returns the exact pixels
    /// (structure-level snapshot that survives png-crate re-encodings).
    #[test]
    fn png_round_trips_the_pixels() {
        let input = crate::bytes_arg::decode_hex("deadbeef").unwrap();
        let image = image_for_input(&input).unwrap();
        let bytes = png_for_input(&input).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");

        let decoder = png::Decoder::new(std::io::Cursor::new(&bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut pixels).unwrap();
        assert_eq!(info.width, 32);
        assert_eq!(info.height, 32);
        assert_eq!(info.color_type, png::ColorType::Rgb);
        assert_eq!(info.bit_depth, png::BitDepth::Eight);
        pixels.truncate(info.buffer_size());
        assert_eq!(pixels, image.pixels);
    }
}
