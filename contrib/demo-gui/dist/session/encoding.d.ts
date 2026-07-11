export type EncodingKind = "hex" | "base58" | "bech32" | "bech32m" | "base64";
export declare function classifyCharset(text: string): EncodingKind[];
export declare function normalizeHexInput(text: string): string;
export declare function hexToBytes(hex: string): Uint8Array | null;
export declare function bytesToHex(bytes: Uint8Array): string;
export declare function base64ToBytes(text: string): Uint8Array | null;
export declare function sha256(bytes: Uint8Array): Uint8Array;
export declare function base58Decode(text: string): Uint8Array | null;
export declare function base58CheckDecode(text: string): {
    version: number;
    payload: Uint8Array;
} | null;
export declare function base58CheckEncode(version: number, payload: Uint8Array): string;
export interface SegwitAddress {
    hrp: string;
    version: number;
    program: Uint8Array;
    encoding: "bech32" | "bech32m";
}
export declare function segwitAddressDecode(text: string): SegwitAddress | null;
export declare function segwitAddressEncode(hrp: string, version: number, program: Uint8Array): string | null;
export type Network = "bitcoin" | "testnet" | "signet" | "regtest";
export declare function addressFromScript(scriptHex: string, network: Network): string | null;
export declare function scriptFromAddress(text: string): {
    scriptHex: string;
    networks: Network[];
} | null;
export type ParseContext = "hex-bytes" | "hex-bytes-32" | "address" | "script" | "psbt";
export type FlexibleParse = {
    ok: true;
    kind: string;
    canonical: string;
    note?: string;
    error?: undefined;
    tried?: undefined;
} | {
    ok: false;
    error: string;
    tried: string[];
    kind?: undefined;
    canonical?: undefined;
    note?: undefined;
};
export declare function parseFlexible(text: string, context: ParseContext, network?: Network): FlexibleParse;
