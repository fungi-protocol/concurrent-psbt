// Tests for the liberal "bitvomit" parsing layer (src/session/encoding.ts).
// The module itself is dependency-free; the tests are allowed node builtins
// (Buffer, node:crypto) precisely to cross-check the pure implementations
// against an independent one.
import test from "node:test";
import assert from "node:assert/strict";
import crypto from "node:crypto";

import {
  addressFromScript,
  base58CheckDecode,
  base58CheckEncode,
  base58Decode,
  base64ToBytes,
  bytesToHex,
  classifyCharset,
  hexToBytes,
  normalizeHexInput,
  parseFlexible,
  scriptFromAddress,
  segwitAddressDecode,
  segwitAddressEncode,
  sha256,
} from "../dist/session/encoding.js";

// --- shared vectors ---------------------------------------------------------

// BIP 173 / BIP 350 reference vectors.
const V0_UPPER = "BC1QW508D6QEJXTDG4Y5R3ZARVARY0C5XW7KV8F3T4";
const V0_LOWER = V0_UPPER.toLowerCase();
const V0_SCRIPT = "0014751e76e8199196d454941c45d1b3a323f1433bd6";
const V0_PROGRAM = "751e76e8199196d454941c45d1b3a323f1433bd6";
const V1_ADDR = "bc1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vqzk5jj0";
const V1_SCRIPT = "512079be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
const TB_ADDR = "tb1qw508d6qejxtdg4y5r3zarvary0c5xw7kxpjzsx";

// Base58check vectors (independently re-derived from node crypto — see the
// verification note in the module header).
const GENESIS_ADDR = "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa";
const GENESIS_HASH160 = "62e907b15cbf27d5425399ebf6f0fb50ebb88f18";
const PI_P2SH_ADDR = "3P14159f73E4gFr7JterCCQh9QjiTjiZrG";
const PI_P2SH_HASH160 = "e9c3dd0c07aac76179ebc76a6c78d4d67c6c160a";
const BURN_ADDR = "1111111111111111111114oLvT2"; // 21 leading zero bytes

// Base64 BIP 370 PSBT (same fixture as test/session.test.mjs).
const PSBT_B64 = "cHNidP8BAgQCAAAAAQMEAAAAAAEEAQABBQEAAQb8BHBzYnQBAA==";
const PSBT_HEX = Buffer.from(PSBT_B64, "base64").toString("hex");

const hexOf = (bytes) => Buffer.from(bytes).toString("hex");

// --- sha256 ------------------------------------------------------------------

test("sha256 matches the FIPS 180-4 short vectors", () => {
  assert.equal(
    bytesToHex(sha256(new Uint8Array(0))),
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
  );
  assert.equal(
    bytesToHex(sha256(new Uint8Array(Buffer.from("abc", "utf8")))),
    "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
  );
});

test("sha256 matches node crypto on multi-block inputs and padding boundaries", () => {
  // > 64 bytes forces multiple compression blocks; the boundary lengths hit
  // every padding branch (55/56 straddle the length-field cutoff).
  for (const length of [55, 56, 57, 63, 64, 65, 119, 127, 128, 200]) {
    const input = Uint8Array.from({ length }, (_, i) => (i * 7 + 3) & 0xff);
    const expected = crypto.createHash("sha256").update(input).digest("hex");
    assert.equal(bytesToHex(sha256(input)), expected, `length ${length}`);
  }
});

// --- hex helpers -------------------------------------------------------------

test("normalizeHexInput strips presentation noise", () => {
  assert.equal(normalizeHexInput("0xDE:AD be\nef"), "deadbeef");
  assert.equal(normalizeHexInput("de:ad:be:ef"), "deadbeef");
  assert.equal(normalizeHexInput(""), "");
  assert.equal(normalizeHexInput("0x"), "");
  // Only ONE leading 0x is presentation; a second is data (and will fail decode).
  assert.equal(normalizeHexInput("0x0xab"), "0xab");
});

test("hexToBytes/bytesToHex roundtrip and reject malformed input", () => {
  assert.deepEqual(hexToBytes("00ff10"), Uint8Array.from([0, 255, 16]));
  assert.deepEqual(hexToBytes("DEAD"), Uint8Array.from([0xde, 0xad]));
  assert.equal(bytesToHex(Uint8Array.from([0, 255, 16])), "00ff10");
  assert.equal(hexToBytes("abc"), null); // odd length
  assert.equal(hexToBytes("zz"), null); // bad chars
  assert.deepEqual(hexToBytes(""), new Uint8Array(0));
  const roundtrip = "00ff0180fe";
  assert.equal(bytesToHex(hexToBytes(roundtrip)), roundtrip);
});

// --- base64 ------------------------------------------------------------------

test("base64ToBytes agrees with Buffer across lengths and tolerates line wraps", () => {
  for (let length = 0; length <= 48; length++) {
    const bytes = Uint8Array.from({ length }, (_, i) => (i * 37 + length) & 0xff);
    const encoded = Buffer.from(bytes).toString("base64");
    assert.deepEqual(base64ToBytes(encoded), bytes, `length ${length}`);
  }
  // Pasted blobs are often line-wrapped: internal whitespace is compacted.
  const wrapped = `${PSBT_B64.slice(0, 20)}\n  ${PSBT_B64.slice(20)}`;
  assert.deepEqual(base64ToBytes(wrapped), new Uint8Array(Buffer.from(PSBT_B64, "base64")));
});

test("base64ToBytes rejects structural garbage", () => {
  assert.equal(base64ToBytes("AQID!"), null); // bad char
  assert.equal(base64ToBytes("AQIDA"), null); // length % 4 != 0
  assert.equal(base64ToBytes("A==="), null); // over-padded
  assert.equal(base64ToBytes("=AAA"), null); // padding not at the end
  assert.deepEqual(base64ToBytes(""), new Uint8Array(0));
});

// --- classifyCharset ---------------------------------------------------------

test("classifyCharset finds every plausible encoding, ordered hex-first", () => {
  // Plain hex is ALSO base58- and base64-plausible: all three are candidates.
  assert.deepEqual(classifyCharset("deadbeef"), ["hex", "base58", "base64"]);
  // 0x/colon noise keeps hex candidacy but breaks the other charsets.
  assert.deepEqual(classifyCharset(" 0xDE:AD "), ["hex"]);
  // Odd digit count is nibbles, not bytes: no hex candidate.
  assert.deepEqual(classifyCharset("abc"), ["base58"]);
});

test("classifyCharset handles bech32, base58, base64, and garbage", () => {
  // bech32 and bech32m share a charset, so both are always candidates together.
  assert.deepEqual(classifyCharset(V0_LOWER), ["bech32", "bech32m"]);
  assert.deepEqual(classifyCharset(V0_UPPER), ["bech32", "bech32m"]); // all-upper is fine
  assert.deepEqual(classifyCharset(GENESIS_ADDR), ["base58"]);
  assert.deepEqual(classifyCharset(PSBT_B64), ["base64"]);
  assert.deepEqual(classifyCharset("hello world!"), []);
  assert.deepEqual(classifyCharset(""), []);
  // Mixed case is never valid bech32 (BIP 173), so it is not even a candidate.
  const mixed = V0_LOWER.slice(0, 10) + V0_LOWER.slice(10).toUpperCase();
  assert.ok(!classifyCharset(mixed).includes("bech32"));
  assert.ok(!classifyCharset(mixed).includes("bech32m"));
});

// --- base58 / base58check ----------------------------------------------------

test("base58Decode decodes the classic vector and rejects excluded glyphs", () => {
  assert.equal(Buffer.from(base58Decode("StV1DL6CwTryKyV")).toString("utf8"), "hello world");
  // 0, O, I, l are not in the bitcoin alphabet.
  for (const bad of ["0", "O", "I", "l"]) {
    assert.equal(base58Decode(`abc${bad}def`), null, `glyph ${bad}`);
  }
  assert.deepEqual(base58Decode(""), new Uint8Array(0));
  assert.deepEqual(base58Decode("11"), Uint8Array.from([0, 0])); // leading '1's are zero bytes
});

test("base58check decodes the genesis address and detects corruption", () => {
  const decoded = base58CheckDecode(GENESIS_ADDR);
  assert.ok(decoded);
  assert.equal(decoded.version, 0x00);
  assert.equal(hexOf(decoded.payload), GENESIS_HASH160);
  assert.equal(base58CheckEncode(0x00, hexToBytes(GENESIS_HASH160)), GENESIS_ADDR);
  // Any single-character corruption breaks the sha256d checksum.
  assert.equal(base58CheckDecode(GENESIS_ADDR.slice(0, -1) + "b"), null);
});

test("base58check handles the p2sh version byte", () => {
  const decoded = base58CheckDecode(PI_P2SH_ADDR);
  assert.ok(decoded);
  assert.equal(decoded.version, 0x05);
  assert.equal(hexOf(decoded.payload), PI_P2SH_HASH160);
  assert.equal(base58CheckEncode(0x05, decoded.payload), PI_P2SH_ADDR);
});

test("base58check maps leading zero bytes to leading '1' characters", () => {
  // The all-zero hash160 burn address: version 0x00 + 20 zero bytes = 21
  // leading zeros, hence 21 leading '1's.
  assert.equal(base58CheckEncode(0x00, new Uint8Array(20)), BURN_ADDR);
  assert.ok(BURN_ADDR.startsWith("1".repeat(21)));
  const decoded = base58CheckDecode(BURN_ADDR);
  assert.ok(decoded);
  assert.equal(decoded.version, 0x00);
  assert.deepEqual(decoded.payload, new Uint8Array(20));
});

// --- segwit addresses (BIP 173 / BIP 350) ------------------------------------

test("segwit v0 mainnet reference vector decodes in either case", () => {
  for (const input of [V0_UPPER, V0_LOWER]) {
    const decoded = segwitAddressDecode(input);
    assert.ok(decoded, input);
    assert.equal(decoded.hrp, "bc");
    assert.equal(decoded.version, 0);
    assert.equal(decoded.encoding, "bech32");
    assert.equal(hexOf(decoded.program), V0_PROGRAM);
  }
  assert.equal(segwitAddressEncode("bc", 0, hexToBytes(V0_PROGRAM)), V0_LOWER);
  assert.equal(addressFromScript(V0_SCRIPT, "bitcoin"), V0_LOWER);
  assert.deepEqual(scriptFromAddress(V0_UPPER), { scriptHex: V0_SCRIPT, networks: ["bitcoin"] });
});

test("segwit v1 taproot reference vector uses bech32m", () => {
  const decoded = segwitAddressDecode(V1_ADDR);
  assert.ok(decoded);
  assert.equal(decoded.version, 1);
  assert.equal(decoded.encoding, "bech32m");
  assert.equal(hexOf(decoded.program), V1_SCRIPT.slice(4));
  assert.equal(segwitAddressEncode("bc", 1, decoded.program), V1_ADDR);
  assert.equal(addressFromScript(V1_SCRIPT, "bitcoin"), V1_ADDR);
  assert.deepEqual(scriptFromAddress(V1_ADDR), { scriptHex: V1_SCRIPT, networks: ["bitcoin"] });
});

test("tb addresses are valid on both testnet and signet", () => {
  const decoded = segwitAddressDecode(TB_ADDR);
  assert.ok(decoded);
  assert.equal(decoded.hrp, "tb");
  assert.equal(hexOf(decoded.program), V0_PROGRAM);
  assert.deepEqual(scriptFromAddress(TB_ADDR), {
    scriptHex: V0_SCRIPT,
    networks: ["testnet", "signet"],
  });
  assert.equal(addressFromScript(V0_SCRIPT, "testnet"), TB_ADDR);
  assert.equal(addressFromScript(V0_SCRIPT, "signet"), TB_ADDR);
});

test("bcrt regtest addresses roundtrip through encode/decode/script", () => {
  const address = segwitAddressEncode("bcrt", 0, hexToBytes(V0_PROGRAM));
  assert.ok(address);
  assert.ok(address.startsWith("bcrt1q"));
  const decoded = segwitAddressDecode(address);
  assert.ok(decoded);
  assert.equal(decoded.hrp, "bcrt");
  assert.equal(hexOf(decoded.program), V0_PROGRAM);
  assert.equal(addressFromScript(V0_SCRIPT, "regtest"), address);
  assert.deepEqual(scriptFromAddress(address), { scriptHex: V0_SCRIPT, networks: ["regtest"] });
});

test("segwitAddressDecode rejects the BIP 173/350 invalid families", () => {
  // Mixed case (BIP 173 MUST-reject).
  assert.equal(segwitAddressDecode(V0_LOWER.slice(0, 10) + V0_LOWER.slice(10).toUpperCase()), null);
  // Corrupted checksum (last char flipped).
  assert.equal(segwitAddressDecode(V0_LOWER.slice(0, -1) + "5"), null);
  // v0 encoded with the bech32m constant (BIP 350 invalid vector).
  assert.equal(segwitAddressDecode("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kemeawh"), null);
  // v1 encoded with the bech32 constant: valid under BIP 173, MOVED to
  // invalid by BIP 350.
  assert.equal(
    segwitAddressDecode(
      "bc1pw508d6qejxtdg4y5r3zarvary0c5xw7kw508d6qejxtdg4y5r3zarvary0c5xw7k7grplx",
    ),
    null,
  );
  // v0 with a 16-byte program (BIP 173 invalid vector: bad length for v0).
  assert.equal(segwitAddressDecode("bc1qr508d6qejxtdg4y5r3zarvaryv98gj9p"), null);
  // Program length out of the 2..40 window (BIP 350 invalid vectors).
  assert.equal(segwitAddressDecode("bc1pw5dgrnzv"), null);
  assert.equal(
    segwitAddressDecode(
      "bc1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vq8n0nx0muaewav253zgeav",
    ),
    null,
  );
});

test("segwitAddressEncode refuses out-of-spec parameters", () => {
  const program20 = hexToBytes(V0_PROGRAM);
  assert.equal(segwitAddressEncode("bc", 0, new Uint8Array(16)), null); // v0 must be 20 or 32
  assert.equal(segwitAddressEncode("bc", 1, new Uint8Array(1)), null); // program < 2
  assert.equal(segwitAddressEncode("bc", 1, new Uint8Array(41)), null); // program > 40
  assert.equal(segwitAddressEncode("bc", 17, program20), null); // version > 16
  assert.equal(segwitAddressEncode("bc", -1, program20), null);
  assert.equal(segwitAddressEncode("bc", 0.5, program20), null);
  assert.equal(segwitAddressEncode("", 0, program20), null); // empty hrp
  assert.equal(segwitAddressEncode("BC", 0, program20), null); // canonical output is lowercase
  // v1..v16 with 2..40-byte programs ARE encodable (bech32m).
  const v16 = segwitAddressEncode("bc", 16, Uint8Array.from([0x75, 0x1e]));
  assert.ok(v16);
  assert.equal(segwitAddressDecode(v16).encoding, "bech32m");
});

// --- scriptPubKey <-> address ------------------------------------------------

test("p2pkh and p2sh scripts render per-network base58check addresses", () => {
  const p2pkh = `76a914${GENESIS_HASH160}88ac`;
  assert.equal(addressFromScript(p2pkh, "bitcoin"), GENESIS_ADDR);
  // Non-mainnet networks share the 0x6f/0xc4 version bytes.
  const testnetAddr = addressFromScript(p2pkh, "testnet");
  assert.equal(testnetAddr, addressFromScript(p2pkh, "signet"));
  assert.equal(testnetAddr, addressFromScript(p2pkh, "regtest"));
  assert.equal(base58CheckDecode(testnetAddr).version, 0x6f);
  assert.deepEqual(scriptFromAddress(testnetAddr), {
    scriptHex: p2pkh,
    networks: ["testnet", "signet", "regtest"],
  });

  const p2sh = `a914${PI_P2SH_HASH160}87`;
  assert.equal(addressFromScript(p2sh, "bitcoin"), PI_P2SH_ADDR);
  assert.equal(base58CheckDecode(addressFromScript(p2sh, "signet")).version, 0xc4);
  assert.deepEqual(scriptFromAddress(PI_P2SH_ADDR), { scriptHex: p2sh, networks: ["bitcoin"] });
});

test("non-template scripts and non-address inputs convert to null", () => {
  assert.equal(addressFromScript("6a0568656c6c6f", "bitcoin"), null); // op_return
  assert.equal(addressFromScript("0014751e", "bitcoin"), null); // truncated push
  assert.equal(addressFromScript("00", "bitcoin"), null);
  assert.equal(addressFromScript("not hex", "bitcoin"), null);
  // v0 witness script with a 21-byte program: opcode shape matches but BIP
  // 141 forbids the length, so there is no address form.
  assert.equal(addressFromScript(`0015${V0_PROGRAM}00`, "bitcoin"), null);
  assert.equal(scriptFromAddress("garbage"), null);
  // Checksum-valid base58check that is NOT an address version (0x80 = WIF).
  const wifLike = base58CheckEncode(0x80, hexToBytes(GENESIS_HASH160));
  assert.equal(scriptFromAddress(wifLike), null);
});

// --- parseFlexible -----------------------------------------------------------

test("parseFlexible hex-bytes accepts noisy hex and falls back to base64", () => {
  assert.deepEqual(parseFlexible(" 0xDE:AD be ef ", "hex-bytes"), {
    ok: true,
    kind: "hex",
    canonical: "deadbeef",
  });
  // Buffer.from([1,2,3]) -> "AQID": not hex, decoded as base64 with a note.
  const viaBase64 = parseFlexible("AQID", "hex-bytes");
  assert.equal(viaBase64.ok, true);
  assert.equal(viaBase64.kind, "base64");
  assert.equal(viaBase64.canonical, "010203");
  assert.equal(viaBase64.note, "decoded base64");
  const failed = parseFlexible("!!!", "hex-bytes");
  assert.equal(failed.ok, false);
  assert.deepEqual(failed.tried, ["hex", "base64"]);
  assert.match(failed.error, /hex, base64/);
});

test("parseFlexible hex-bytes-32 enforces the exact size", () => {
  const txid = "11".repeat(32);
  assert.deepEqual(parseFlexible(txid.toUpperCase(), "hex-bytes-32"), {
    ok: true,
    kind: "hex",
    canonical: txid,
  });
  const b64 = Buffer.alloc(32, 7).toString("base64");
  const viaBase64 = parseFlexible(b64, "hex-bytes-32");
  assert.equal(viaBase64.ok, true);
  assert.equal(viaBase64.kind, "base64");
  assert.equal(viaBase64.canonical, "07".repeat(32));
  const short = parseFlexible("abcd", "hex-bytes-32");
  assert.equal(short.ok, false);
  assert.match(short.error, /expected exactly 32 bytes, got 2/);
  assert.deepEqual(short.tried, ["hex", "base64"]);
});

test("parseFlexible address canonicalizes addresses and renders scripts", () => {
  // Uppercase bech32 in, canonical lowercase out.
  const segwit = parseFlexible(V0_UPPER, "address");
  assert.deepEqual(segwit, { ok: true, kind: "segwit-address", canonical: V0_LOWER });
  const base58 = parseFlexible(` ${GENESIS_ADDR} `, "address");
  assert.deepEqual(base58, { ok: true, kind: "base58check-address", canonical: GENESIS_ADDR });
  // A pasted scriptPubKey renders as the address for the requested network.
  const rendered = parseFlexible(V0_SCRIPT, "address", "regtest");
  assert.equal(rendered.ok, true);
  assert.equal(rendered.kind, "script");
  assert.equal(rendered.canonical, addressFromScript(V0_SCRIPT, "regtest"));
  assert.match(rendered.note, /regtest/);
  // Default network is bitcoin.
  assert.equal(parseFlexible(V0_SCRIPT, "address").canonical, V0_LOWER);
  // Valid hex, but a script family with no address form.
  const opReturn = parseFlexible("6a0568656c6c6f", "address");
  assert.equal(opReturn.ok, false);
  assert.equal(opReturn.error, "script has no address form");
  assert.deepEqual(opReturn.tried, ["segwit address", "base58check address", "script hex"]);
  const garbage = parseFlexible("!!!", "address");
  assert.equal(garbage.ok, false);
  assert.deepEqual(garbage.tried, ["segwit address", "base58check address", "script hex"]);
  assert.match(garbage.error, /segwit address, base58check address, script hex/);
});

test("parseFlexible script accepts hex and converts addresses", () => {
  assert.deepEqual(parseFlexible(` ${V0_SCRIPT.toUpperCase()} `, "script"), {
    ok: true,
    kind: "script",
    canonical: V0_SCRIPT,
  });
  const converted = parseFlexible(GENESIS_ADDR, "script");
  assert.equal(converted.ok, true);
  assert.equal(converted.kind, "address");
  assert.equal(converted.canonical, `76a914${GENESIS_HASH160}88ac`);
  assert.match(converted.note, /converted address to scriptPubKey/);
  const tb = parseFlexible(TB_ADDR, "script");
  assert.equal(tb.canonical, V0_SCRIPT);
  assert.match(tb.note, /testnet, signet/);
  const failed = parseFlexible("!!!", "script");
  assert.equal(failed.ok, false);
  assert.deepEqual(failed.tried, ["script hex", "segwit address", "base58check address"]);
});

test("parseFlexible psbt compacts base64 and converts hex", () => {
  const wrapped = `${PSBT_B64.slice(0, 12)}\n ${PSBT_B64.slice(12)}`;
  assert.deepEqual(parseFlexible(wrapped, "psbt"), {
    ok: true,
    kind: "base64",
    canonical: PSBT_B64,
  });
  // Hex PSBT (magic 70736274ff) converts to the canonical base64 form.
  const fromHex = parseFlexible(PSBT_HEX, "psbt");
  assert.equal(fromHex.ok, true);
  assert.equal(fromHex.kind, "hex");
  assert.equal(fromHex.canonical, PSBT_B64);
  assert.match(fromHex.note, /converted hex PSBT to base64/);
  // 0x prefix and uppercase are presentation noise on the hex path too.
  assert.equal(parseFlexible(`0x${PSBT_HEX.toUpperCase()}`, "psbt").canonical, PSBT_B64);
  const failed = parseFlexible("definitely not a psbt", "psbt");
  assert.equal(failed.ok, false);
  assert.deepEqual(failed.tried, ["base64 psbt", "hex psbt"]);
  assert.match(failed.error, /cHNidP8/);
  assert.match(failed.error, /70736274ff/);
  // Right magic, broken body: the error names the format that ALMOST matched.
  const truncated = parseFlexible("cHNidP8BAgQ", "psbt");
  assert.equal(truncated.ok, false);
  assert.match(truncated.error, /not valid base64/);
});

test("parseFlexible reports blank input uniformly", () => {
  for (const context of ["hex-bytes", "hex-bytes-32", "address", "script", "psbt"]) {
    assert.deepEqual(parseFlexible("", context), { ok: false, error: "empty input", tried: [] });
    assert.deepEqual(parseFlexible("   \n ", context), {
      ok: false,
      error: "empty input",
      tried: [],
    });
  }
});

// --- determinism / roundtrips -------------------------------------------------

test("encode(decode(x)) is the identity on the reference vectors", () => {
  for (const address of [GENESIS_ADDR, PI_P2SH_ADDR, BURN_ADDR]) {
    const decoded = base58CheckDecode(address);
    assert.ok(decoded, address);
    assert.equal(base58CheckEncode(decoded.version, decoded.payload), address);
  }
  for (const address of [V0_UPPER, V0_LOWER, V1_ADDR, TB_ADDR]) {
    const decoded = segwitAddressDecode(address);
    assert.ok(decoded, address);
    assert.equal(
      segwitAddressEncode(decoded.hrp, decoded.version, decoded.program),
      address.toLowerCase(),
    );
  }
});
