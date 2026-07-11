export declare const TABLEAU10: readonly string[];
export type PaletteRegistry = Map<string, number>;
export declare function paletteRegistry(): PaletteRegistry;
export declare function paletteColor(registry: PaletteRegistry, key: string): string;
export declare function groupColorKey(group: {
    kind: string;
    key: string;
}): string | null;
export declare function descriptorColorKey(descriptor: {
    descriptor: string;
    normalized: string | null;
}): string;
export declare function peerColorKey(peer: {
    transport: string;
    identity: string;
}): string;
