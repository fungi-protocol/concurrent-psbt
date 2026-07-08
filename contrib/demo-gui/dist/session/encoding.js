// contrib/demo-gui/src/session/encoding.ts
//
// The liberal "bitvomit" parsing layer behind every encoded-data input on the
// session page. Design principle: the user pastes WHATEVER they have — hex
// with a 0x prefix or colon separators, base64 with line wraps, bech32 in
// either case — and the input detects its encoding from character set plus
// context instead of demanding one format. Three stages:
//
//   1. charset classification (classifyCharset): which encodings are even
//      POSSIBLE for these characters, roughly ordered most-specific first
//      (hex, bech32/bech32m, base58, base64);
//   2. candidate decodings: real decoders that accept or reject, never guess
//      — hex, base64, base58check with the sha256d checksum, and segwit
//      addresses under the full BIP 173 (bech32) / BIP 350 (bech32m) rules;
//   3. context filter (parseFlexible): the field being filled decides which
//      decodings make sense, converts between representations where that is
//      unambiguous (base64 bytes -> hex, address <-> scriptPubKey, hex PSBT
//      -> base64 PSBT), and on failure names exactly what was tried so the
//      error teaches instead of stonewalling.
//
// Like src/session/state.ts this is data-in / data-out: no DOM, no fetch, no
// Backend calls, no dependencies. sha256 is implemented here (FIPS 180-4)
// precisely so the checksummed formats stay dependency-free and node --test
// coverable (test/session-encoding.test.mjs) without Buffer/atob — browser
// atob is not byte-safe and Buffer is node-only.
// Bitcoin base58 alphabet (no 0, O, I, l — the visually ambiguous glyphs).
const BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const BASE58_RE = /^[1-9A-HJ-NP-Za-km-z]+$/;
// BIP 173 data charset: 32 symbols, again excluding 1/b/i/o.
const BECH32_CHARSET = "qpzry9x8gf2tvdw0s3jn54khce6mua7l";
// BIP 350 checksum constant for witness v1+; BIP 173 (v0) uses 1.
const BECH32M_CONST = 0x2bc830a3;
const BASE64_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const BASE64_RE = /^[A-Za-z0-9+/]+={0,2}$/;
function isMixedCase(text) {
    return /[a-z]/.test(text) && /[A-Z]/.test(text);
}
// Charset-only bech32/bech32m candidacy. The two share an alphabet — only the
// checksum constant differs — so they are always candidates together.
function bech32Candidate(trimmed) {
    // BIP 173 forbids mixed case outright, so it disqualifies candidacy too.
    if (isMixedCase(trimmed))
        return false;
    const lowered = trimmed.toLowerCase();
    const separator = lowered.lastIndexOf("1");
    // Nonempty hrp before the separator, at least a checksum's worth after.
    if (separator < 1 || lowered.length - separator - 1 < 6)
        return false;
    for (const ch of lowered.slice(separator + 1)) {
        if (BECH32_CHARSET.indexOf(ch) === -1)
            return false;
    }
    return true;
}
export function classifyCharset(text) {
    const trimmed = text.trim();
    if (!trimmed)
        return [];
    const candidates = [];
    // Hex tolerates presentation noise (0x prefix, whitespace, colons) but
    // requires an even digit count — bytes, not nibbles.
    const hex = normalizeHexInput(trimmed);
    if (hex.length > 0 && hex.length % 2 === 0 && /^[0-9a-f]+$/.test(hex)) {
        candidates.push("hex");
    }
    if (bech32Candidate(trimmed)) {
        candidates.push("bech32", "bech32m");
    }
    if (BASE58_RE.test(trimmed)) {
        candidates.push("base58");
    }
    if (trimmed.length % 4 === 0 && BASE64_RE.test(trimmed)) {
        candidates.push("base64");
    }
    return candidates;
}
// ---------------------------------------------------------------------------
// Hex: the tolerant reader half (normalizeHexInput) is split from the strict
// decoder half (hexToBytes) so callers can report WHICH normalized string
// failed to decode.
// ---------------------------------------------------------------------------
export function normalizeHexInput(text) {
    // Strip presentation noise first (whitespace anywhere, colon separators à
    // la fingerprints), THEN the single leading 0x — "0 x" is noise too.
    const stripped = text.replace(/[\s:]+/g, "");
    return stripped.replace(/^0[xX]/, "").toLowerCase();
}
export function hexToBytes(hex) {
    if (hex.length % 2 !== 0 || !/^[0-9a-fA-F]*$/.test(hex))
        return null;
    const bytes = new Uint8Array(hex.length / 2);
    for (let i = 0; i < bytes.length; i++) {
        bytes[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
    }
    return bytes;
}
export function bytesToHex(bytes) {
    let out = "";
    for (const byte of bytes)
        out += byte.toString(16).padStart(2, "0");
    return out;
}
// ---------------------------------------------------------------------------
// Base64: pure decoder/encoder pair (no atob/Buffer — see header). Liberal on
// whitespace (pasted PSBTs are often line-wrapped) and strict on structure:
// length % 4, padding only at the end, at most two '='.
// ---------------------------------------------------------------------------
export function base64ToBytes(text) {
    const compact = text.replace(/\s+/g, "");
    if (compact.length === 0)
        return new Uint8Array(0);
    if (compact.length % 4 !== 0 || !BASE64_RE.test(compact))
        return null;
    const pad = compact.endsWith("==") ? 2 : compact.endsWith("=") ? 1 : 0;
    const body = compact.slice(0, compact.length - pad);
    const bytes = new Uint8Array((compact.length / 4) * 3 - pad);
    let acc = 0;
    let bits = 0;
    let at = 0;
    for (const ch of body) {
        const value = BASE64_ALPHABET.indexOf(ch);
        if (value === -1)
            return null; // unreachable after BASE64_RE, kept defensive
        acc = ((acc << 6) | value) & 0xffffff;
        bits += 6;
        if (bits >= 8) {
            bits -= 8;
            bytes[at++] = (acc >>> bits) & 0xff;
        }
    }
    return bytes;
}
// Internal counterpart, needed to render a hex PSBT in its canonical base64
// form (same algorithm as src/session/state.ts bytesToBase64).
function bytesToBase64(bytes) {
    let out = "";
    for (let i = 0; i < bytes.length; i += 3) {
        const a = bytes[i];
        const b = i + 1 < bytes.length ? bytes[i + 1] : 0;
        const c = i + 2 < bytes.length ? bytes[i + 2] : 0;
        out += BASE64_ALPHABET[a >> 2];
        out += BASE64_ALPHABET[((a & 0x03) << 4) | (b >> 4)];
        out += i + 1 < bytes.length ? BASE64_ALPHABET[((b & 0x0f) << 2) | (c >> 6)] : "=";
        out += i + 2 < bytes.length ? BASE64_ALPHABET[c & 0x3f] : "=";
    }
    return out;
}
// ---------------------------------------------------------------------------
// SHA-256 (FIPS 180-4), here only because base58check's checksum is
// sha256(sha256(payload)) and this module must stay dependency-free. All
// arithmetic is normalized through >>> 0 to stay in unsigned 32-bit land.
// ---------------------------------------------------------------------------
// prettier-ignore
const SHA256_K = new Uint32Array([
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
]);
function rotr(x, n) {
    return ((x >>> n) | (x << (32 - n))) >>> 0;
}
export function sha256(bytes) {
    // Pad: 0x80, zeros to 56 mod 64, then the bit length as 64-bit big-endian.
    const bitLengthHigh = Math.floor(bytes.length / 0x20000000);
    const bitLengthLow = (bytes.length * 8) >>> 0;
    const paddedLength = (Math.floor((bytes.length + 8) / 64) + 1) * 64;
    const padded = new Uint8Array(paddedLength);
    padded.set(bytes);
    padded[bytes.length] = 0x80;
    for (let i = 0; i < 4; i++) {
        padded[paddedLength - 8 + i] = (bitLengthHigh >>> (24 - 8 * i)) & 0xff;
        padded[paddedLength - 4 + i] = (bitLengthLow >>> (24 - 8 * i)) & 0xff;
    }
    const state = new Uint32Array([
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ]);
    const w = new Uint32Array(64);
    for (let offset = 0; offset < paddedLength; offset += 64) {
        for (let i = 0; i < 16; i++) {
            const at = offset + i * 4;
            w[i] =
                ((padded[at] << 24) | (padded[at + 1] << 16) | (padded[at + 2] << 8) | padded[at + 3]) >>> 0;
        }
        for (let i = 16; i < 64; i++) {
            const s0 = rotr(w[i - 15], 7) ^ rotr(w[i - 15], 18) ^ (w[i - 15] >>> 3);
            const s1 = rotr(w[i - 2], 17) ^ rotr(w[i - 2], 19) ^ (w[i - 2] >>> 10);
            w[i] = w[i - 16] + s0 + w[i - 7] + s1; // Uint32Array wraps mod 2^32
        }
        let [a, b, c, d, e, f, g, h] = state;
        for (let i = 0; i < 64; i++) {
            const s1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
            const ch = (e & f) ^ (~e & g);
            const temp1 = h + s1 + ch + SHA256_K[i] + w[i];
            const s0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
            const maj = (a & b) ^ (a & c) ^ (b & c);
            const temp2 = s0 + maj;
            h = g;
            g = f;
            f = e;
            e = (d + temp1) >>> 0;
            d = c;
            c = b;
            b = a;
            a = (temp1 + temp2) >>> 0;
        }
        state[0] += a;
        state[1] += b;
        state[2] += c;
        state[3] += d;
        state[4] += e;
        state[5] += f;
        state[6] += g;
        state[7] += h;
    }
    const digest = new Uint8Array(32);
    for (let i = 0; i < 8; i++) {
        digest[i * 4] = state[i] >>> 24;
        digest[i * 4 + 1] = (state[i] >>> 16) & 0xff;
        digest[i * 4 + 2] = (state[i] >>> 8) & 0xff;
        digest[i * 4 + 3] = state[i] & 0xff;
    }
    return digest;
}
function sha256d(bytes) {
    return sha256(sha256(bytes));
}
// ---------------------------------------------------------------------------
// Base58 / base58check (the pre-segwit address encoding). Big-integer base
// conversion via byte-array long multiplication; leading zero bytes map to
// leading '1' characters and vice versa — the classic footgun.
// ---------------------------------------------------------------------------
export function base58Decode(text) {
    const bytes = []; // little-endian accumulator
    for (const ch of text) {
        let carry = BASE58_ALPHABET.indexOf(ch);
        if (carry === -1)
            return null;
        for (let i = 0; i < bytes.length; i++) {
            carry += bytes[i] * 58;
            bytes[i] = carry & 0xff;
            carry >>= 8;
        }
        while (carry > 0) {
            bytes.push(carry & 0xff);
            carry >>= 8;
        }
    }
    // Each leading '1' is a literal zero byte the base conversion cannot see.
    for (const ch of text) {
        if (ch !== "1")
            break;
        bytes.push(0);
    }
    return Uint8Array.from(bytes.reverse());
}
function base58Encode(bytes) {
    const digits = []; // little-endian base58
    for (const byte of bytes) {
        let carry = byte;
        for (let i = 0; i < digits.length; i++) {
            carry += digits[i] * 256;
            digits[i] = carry % 58;
            carry = Math.floor(carry / 58);
        }
        while (carry > 0) {
            digits.push(carry % 58);
            carry = Math.floor(carry / 58);
        }
    }
    let out = "";
    for (const byte of bytes) {
        if (byte !== 0)
            break;
        out += "1";
    }
    for (let i = digits.length - 1; i >= 0; i--)
        out += BASE58_ALPHABET[digits[i]];
    return out;
}
export function base58CheckDecode(text) {
    const decoded = base58Decode(text.trim());
    // Minimum: 1 version byte + 4 checksum bytes (empty payload is legal).
    if (!decoded || decoded.length < 5)
        return null;
    const body = decoded.slice(0, decoded.length - 4);
    const checksum = decoded.slice(decoded.length - 4);
    const digest = sha256d(body);
    for (let i = 0; i < 4; i++) {
        if (digest[i] !== checksum[i])
            return null;
    }
    return { version: body[0], payload: body.slice(1) };
}
export function base58CheckEncode(version, payload) {
    // Encoding is programmatic (never raw user input), so a non-byte version is
    // a caller bug worth throwing on rather than a soft null.
    if (!Number.isInteger(version) || version < 0 || version > 0xff) {
        throw new RangeError(`base58check version must be a byte, got ${version}`);
    }
    const body = new Uint8Array(1 + payload.length);
    body[0] = version;
    body.set(payload, 1);
    const full = new Uint8Array(body.length + 4);
    full.set(body);
    full.set(sha256d(body).subarray(0, 4), body.length);
    return base58Encode(full);
}
const BECH32_GENERATORS = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3];
function bech32Polymod(values) {
    let chk = 1;
    for (const value of values) {
        const top = chk >>> 25;
        chk = (((chk & 0x1ffffff) << 5) ^ value) >>> 0;
        for (let i = 0; i < 5; i++) {
            if ((top >>> i) & 1)
                chk = (chk ^ BECH32_GENERATORS[i]) >>> 0;
        }
    }
    return chk;
}
function bech32HrpExpand(hrp) {
    const values = [];
    for (const ch of hrp)
        values.push(ch.charCodeAt(0) >> 5);
    values.push(0);
    for (const ch of hrp)
        values.push(ch.charCodeAt(0) & 31);
    return values;
}
function bech32CreateChecksum(hrp, data, constant) {
    const mod = bech32Polymod([...bech32HrpExpand(hrp), ...data, 0, 0, 0, 0, 0, 0]) ^ constant;
    const out = [];
    for (let i = 0; i < 6; i++)
        out.push((mod >>> (5 * (5 - i))) & 31);
    return out;
}
// General power-of-two base conversion (BIP 173 reference algorithm). With
// pad=false (decoding), leftover bits must be zero padding of less than one
// input group — anything else is a corrupt encoding.
function convertBits(data, from, to, pad) {
    let acc = 0;
    let bits = 0;
    const out = [];
    const maxv = (1 << to) - 1;
    for (let i = 0; i < data.length; i++) {
        const value = data[i];
        if (value < 0 || value >>> from !== 0)
            return null;
        acc = ((acc << from) | value) >>> 0;
        bits += from;
        while (bits >= to) {
            bits -= to;
            out.push((acc >>> bits) & maxv);
        }
    }
    if (pad) {
        if (bits > 0)
            out.push((acc << (to - bits)) & maxv);
    }
    else if (bits >= from || ((acc << (to - bits)) & maxv) !== 0) {
        return null;
    }
    return out;
}
export function segwitAddressDecode(text) {
    const trimmed = text.trim();
    // BIP 173 case rules: all-lower or all-upper, never mixed; 90 chars max.
    if (trimmed.length === 0 || trimmed.length > 90 || isMixedCase(trimmed))
        return null;
    const lowered = trimmed.toLowerCase();
    const separator = lowered.lastIndexOf("1");
    // Nonempty hrp; data part needs at least a version char + 6 checksum chars.
    if (separator < 1 || lowered.length - separator - 1 < 7)
        return null;
    const hrp = lowered.slice(0, separator);
    for (const ch of hrp) {
        const code = ch.charCodeAt(0);
        if (code < 33 || code > 126)
            return null;
    }
    const data = [];
    for (const ch of lowered.slice(separator + 1)) {
        const value = BECH32_CHARSET.indexOf(ch);
        if (value === -1)
            return null;
        data.push(value);
    }
    const checksum = bech32Polymod([...bech32HrpExpand(hrp), ...data]);
    const encoding = checksum === 1 ? "bech32" : checksum === BECH32M_CONST ? "bech32m" : null;
    if (encoding === null)
        return null;
    const version = data[0];
    if (version > 16)
        return null;
    // BIP 350: the checksum constant must match the witness version epoch.
    if (version === 0 && encoding !== "bech32")
        return null;
    if (version >= 1 && encoding !== "bech32m")
        return null;
    const programBytes = convertBits(data.slice(1, data.length - 6), 5, 8, false);
    if (programBytes === null)
        return null;
    const program = Uint8Array.from(programBytes);
    if (program.length < 2 || program.length > 40)
        return null;
    // BIP 141: v0 programs are exactly a 20-byte pubkey hash or 32-byte script hash.
    if (version === 0 && program.length !== 20 && program.length !== 32)
        return null;
    return { hrp, version, program, encoding };
}
export function segwitAddressEncode(hrp, version, program) {
    // Canonical output is lowercase, so an uppercase hrp is rejected rather
    // than silently case-folded.
    if (hrp.length === 0 || /[A-Z]/.test(hrp))
        return null;
    for (const ch of hrp) {
        const code = ch.charCodeAt(0);
        if (code < 33 || code > 126)
            return null;
    }
    if (!Number.isInteger(version) || version < 0 || version > 16)
        return null;
    if (program.length < 2 || program.length > 40)
        return null;
    if (version === 0 && program.length !== 20 && program.length !== 32)
        return null;
    const grouped = convertBits(program, 8, 5, true);
    if (grouped === null)
        return null;
    const data = [version, ...grouped];
    const checksum = bech32CreateChecksum(hrp, data, version === 0 ? 1 : BECH32M_CONST);
    let address = `${hrp}1`;
    for (const value of [...data, ...checksum])
        address += BECH32_CHARSET[value];
    return address.length <= 90 ? address : null;
}
function hrpForNetwork(network) {
    switch (network) {
        case "bitcoin":
            return "bc";
        case "testnet":
        case "signet":
            return "tb"; // BIP 325 signet reuses the testnet hrp
        case "regtest":
            return "bcrt";
    }
}
// The reverse mapping is one-to-many: shared prefixes mean one address text
// is valid on several networks (the whole reason scriptFromAddress returns a
// networks LIST instead of demanding the caller pre-commit to one).
function networksForHrp(hrp) {
    switch (hrp) {
        case "bc":
            return ["bitcoin"];
        case "tb":
            return ["testnet", "signet"];
        case "bcrt":
            return ["regtest"];
        default:
            return null;
    }
}
function networksForBase58Version(version) {
    switch (version) {
        case 0x00: // p2pkh mainnet
        case 0x05: // p2sh mainnet (BIP 13)
            return ["bitcoin"];
        case 0x6f: // p2pkh test networks
        case 0xc4: // p2sh test networks
            return ["testnet", "signet", "regtest"];
        default:
            return null;
    }
}
const BASE58_ADDRESS_VERSIONS = new Set([0x00, 0x05, 0x6f, 0xc4]);
export function addressFromScript(scriptHex, network) {
    const bytes = hexToBytes(normalizeHexInput(scriptHex));
    if (bytes === null || bytes.length === 0)
        return null;
    // p2pkh: OP_DUP OP_HASH160 <20> OP_EQUALVERIFY OP_CHECKSIG
    if (bytes.length === 25 &&
        bytes[0] === 0x76 &&
        bytes[1] === 0xa9 &&
        bytes[2] === 0x14 &&
        bytes[23] === 0x88 &&
        bytes[24] === 0xac) {
        return base58CheckEncode(network === "bitcoin" ? 0x00 : 0x6f, bytes.slice(3, 23));
    }
    // p2sh: OP_HASH160 <20> OP_EQUAL (BIP 13)
    if (bytes.length === 23 && bytes[0] === 0xa9 && bytes[1] === 0x14 && bytes[22] === 0x87) {
        return base58CheckEncode(network === "bitcoin" ? 0x05 : 0xc4, bytes.slice(2, 22));
    }
    // witness: OP_0 / OP_1..OP_16 then a single 2..40-byte push (BIP 141).
    // segwitAddressEncode still applies the v0 20/32-byte rule, so a v0 script
    // with an off-size program correctly yields null.
    if (bytes.length >= 4 && bytes[1] === bytes.length - 2) {
        const opcode = bytes[0];
        const version = opcode === 0x00 ? 0 : opcode >= 0x51 && opcode <= 0x60 ? opcode - 0x50 : -1;
        if (version >= 0 && bytes[1] >= 2 && bytes[1] <= 40) {
            return segwitAddressEncode(hrpForNetwork(network), version, bytes.slice(2));
        }
    }
    return null;
}
export function scriptFromAddress(text) {
    const trimmed = text.trim();
    const segwit = segwitAddressDecode(trimmed);
    if (segwit) {
        const networks = networksForHrp(segwit.hrp);
        if (networks === null)
            return null; // valid bech32, but not a bitcoin network hrp
        const opcode = segwit.version === 0 ? 0x00 : 0x50 + segwit.version;
        const script = Uint8Array.from([opcode, segwit.program.length, ...segwit.program]);
        return { scriptHex: bytesToHex(script), networks };
    }
    const check = base58CheckDecode(trimmed);
    if (check && check.payload.length === 20) {
        const networks = networksForBase58Version(check.version);
        if (networks === null)
            return null;
        const hash = bytesToHex(check.payload);
        const scriptHex = check.version === 0x00 || check.version === 0x6f
            ? `76a914${hash}88ac`
            : `a914${hash}87`;
        return { scriptHex, networks };
    }
    return null;
}
export function parseFlexible(text, context, network = "bitcoin") {
    const trimmed = text.trim();
    if (!trimmed)
        return { ok: false, error: "empty input", tried: [] };
    switch (context) {
        case "hex-bytes":
            return parseBytes(trimmed, null);
        case "hex-bytes-32":
            return parseBytes(trimmed, 32);
        case "address":
            return parseAddressInput(trimmed, network);
        case "script":
            return parseScriptInput(trimmed);
        case "psbt":
            return parsePsbtInput(trimmed);
    }
}
function parseBytes(trimmed, exactBytes) {
    const tried = ["hex", "base64"];
    const hex = normalizeHexInput(trimmed);
    const hexBytes = hex.length > 0 ? hexToBytes(hex) : null;
    if (hexBytes && hexBytes.length > 0 && (exactBytes === null || hexBytes.length === exactBytes)) {
        return { ok: true, kind: "hex", canonical: hex };
    }
    // Hex first, base64 second: every even-digit hex string is ALSO valid
    // base64 charset-wise, and hex is what these fields overwhelmingly carry.
    const base64Bytes = base64ToBytes(trimmed);
    if (base64Bytes &&
        base64Bytes.length > 0 &&
        (exactBytes === null || base64Bytes.length === exactBytes)) {
        return { ok: true, kind: "base64", canonical: bytesToHex(base64Bytes), note: "decoded base64" };
    }
    // Decoded but the wrong size: say what WAS understood before what failed.
    if (exactBytes !== null && hexBytes && hexBytes.length > 0) {
        return {
            ok: false,
            error: `expected exactly ${exactBytes} bytes, got ${hexBytes.length} hex bytes (tried: hex, base64)`,
            tried,
        };
    }
    if (exactBytes !== null && base64Bytes && base64Bytes.length > 0) {
        return {
            ok: false,
            error: `expected exactly ${exactBytes} bytes, got ${base64Bytes.length} base64-decoded bytes (tried: hex, base64)`,
            tried,
        };
    }
    return { ok: false, error: "not recognizable as bytes (tried: hex, base64)", tried };
}
function parseAddressInput(trimmed, network) {
    const tried = ["segwit address", "base58check address", "script hex"];
    const segwit = segwitAddressDecode(trimmed);
    if (segwit) {
        // Canonical bech32 is lowercase; re-encoding from the decoded parts also
        // normalizes any (all-)uppercase input.
        const canonical = segwitAddressEncode(segwit.hrp, segwit.version, segwit.program);
        if (canonical !== null)
            return { ok: true, kind: "segwit-address", canonical };
    }
    const check = base58CheckDecode(trimmed);
    if (check && check.payload.length === 20 && BASE58_ADDRESS_VERSIONS.has(check.version)) {
        return {
            ok: true,
            kind: "base58check-address",
            canonical: base58CheckEncode(check.version, check.payload),
        };
    }
    // Context conversion: a pasted scriptPubKey is rendered as the address for
    // the caller's network when the script family has one.
    const hex = normalizeHexInput(trimmed);
    const scriptBytes = hex.length > 0 ? hexToBytes(hex) : null;
    if (scriptBytes && scriptBytes.length > 0) {
        const address = addressFromScript(hex, network);
        if (address !== null) {
            return { ok: true, kind: "script", canonical: address, note: `rendered scriptPubKey as ${network} address` };
        }
        return { ok: false, error: "script has no address form", tried };
    }
    return {
        ok: false,
        error: "not an address or scriptPubKey (tried: segwit address, base58check address, script hex)",
        tried,
    };
}
function parseScriptInput(trimmed) {
    const tried = ["script hex", "segwit address", "base58check address"];
    // Any byte string is a script (scripts are arbitrary programs), so hex is
    // accepted as-is; only the address forms need converting.
    const hex = normalizeHexInput(trimmed);
    const scriptBytes = hex.length > 0 ? hexToBytes(hex) : null;
    if (scriptBytes && scriptBytes.length > 0) {
        return { ok: true, kind: "script", canonical: hex };
    }
    const converted = scriptFromAddress(trimmed);
    if (converted) {
        return {
            ok: true,
            kind: "address",
            canonical: converted.scriptHex,
            note: `converted address to scriptPubKey (address valid on: ${converted.networks.join(", ")})`,
        };
    }
    return {
        ok: false,
        error: "not a scriptPubKey or a convertible address (tried: script hex, segwit address, base58check address)",
        tried,
    };
}
// BIP 174 magic "psbt\xff" — 70736274ff in hex, cHNidP8 as a base64 prefix.
const PSBT_BASE64_MAGIC = "cHNidP8";
const PSBT_HEX_MAGIC = "70736274ff";
function parsePsbtInput(trimmed) {
    const tried = ["base64 psbt", "hex psbt"];
    const compact = trimmed.replace(/\s+/g, "");
    if (compact.startsWith(PSBT_BASE64_MAGIC)) {
        // Whitespace-compacted is the canonical interchange form (what the
        // Backend seam and `ptj` stdin expect).
        if (base64ToBytes(compact) !== null) {
            return { ok: true, kind: "base64", canonical: compact };
        }
        return {
            ok: false,
            error: "starts with the base64 PSBT magic cHNidP8 but is not valid base64 (tried: base64 psbt, hex psbt)",
            tried,
        };
    }
    const hex = normalizeHexInput(trimmed);
    if (hex.startsWith(PSBT_HEX_MAGIC)) {
        const bytes = hexToBytes(hex);
        if (bytes !== null) {
            return { ok: true, kind: "hex", canonical: bytesToBase64(bytes), note: "converted hex PSBT to base64" };
        }
        return {
            ok: false,
            error: "starts with the hex PSBT magic 70736274ff but is not valid hex (tried: base64 psbt, hex psbt)",
            tried,
        };
    }
    return {
        ok: false,
        error: "not a PSBT: neither base64 starting cHNidP8 nor hex starting 70736274ff (tried: base64 psbt, hex psbt)",
        tried,
    };
}
