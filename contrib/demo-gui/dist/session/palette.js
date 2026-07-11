// contrib/demo-gui/src/session/palette.ts
//
// Categorical color identity: every descriptor AND pseudo-descriptor gets a
// unique color from the Tableau 10 categorical palette, and grouped
// inputs/outputs are delineated in their descriptor's color — the demo's
// colored-rectangle convention (descriptor dock chips, coin borders and
// stripes; src/app.ts descriptorPalette) rebuilt for the session page.
//
// Assignment is FIRST-SEEN STABLE within a page session: a key keeps its
// color for the registry's lifetime no matter what arrives later or how
// often the page re-renders; colors wrap after ten identities. Pure over an
// explicit registry so node tests cover assignment stability.
export const TABLEAU10 = [
    "#4e79a7", // blue
    "#f28e2b", // orange
    "#e15759", // red
    "#76b7b2", // teal
    "#59a14f", // green
    "#edc948", // yellow
    "#b07aa1", // purple
    "#ff9da7", // pink
    "#9c755f", // brown
    "#bab0ac", // grey
];
export function paletteRegistry() {
    return new Map();
}
export function paletteColor(registry, key) {
    let index = registry.get(key);
    if (index === undefined) {
        index = registry.size % TABLEAU10.length;
        registry.set(key, index);
    }
    return TABLEAU10[index];
}
// Color identity keys — descriptor identity where derivable today:
//   - a provenance card group IS a pseudo-descriptor (the peer the rows
//     came from): its `peer:<name>` group key;
//   - a script-template group is the real-descriptor stand-in derivable
//     from inspect data: its `template:<kind>` group key;
//   - unattributed groups carry no identity → null (neutral border).
export function groupColorKey(group) {
    return group.kind === "unattributed" ? null : group.key;
}
// Descriptor cards key by the descriptor's TEXTUAL identity — the
// normalized public form once deep classification supplies it, the raw
// string before — not by the card key, so re-pasting the same descriptor
// keeps its color.
export function descriptorColorKey(descriptor) {
    return `descriptor:${descriptor.normalized ?? descriptor.descriptor}`;
}
// Peer card color follows the immutable ephemeral transport address. A local
// display-label edit must never change color or imply a new peer.
export function peerColorKey(peer) {
    return `peer:${peer.transport}:${peer.identity}`;
}
