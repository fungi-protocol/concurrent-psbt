// contrib/demo-gui/src/session/layout.ts
//
// Lane layout — pure geometry for the spatial canvas. Pure data-in/data-out
// (node --test covered by test/session-canvas-layout.test.mjs); the DOM
// shell measures its cards, hands the sizes in, and paints the rects out.
//
// Three lanes, top to bottom, the demo mockup's rows: remote peers,
// sessions, and a framed "Me" workspace whose local-only fragment cards
// wrap into rows inside the frame. Unlike the mockup, nothing here is a
// hard-coded height: every node size is MEASURED by the caller, each lane's
// Y derives from the tallest card in the lane above, and the frame grows
// with its wrapped rows. Nodes spread their lane's spare width into the
// gaps (the mockup's spreadSized); bridge groups stay adjacent and share
// one slot. The world is never narrower than the viewport, so lanes only
// compress down to a floor gap before the canvas starts scrolling.
// Side margin of the world; also the frame's outer margin.
export const CANVAS_EDGE = 24;
// Vertical gap between lanes — the room the wire curves cross.
export const LANE_GAP = 64;
// Nodes never sit closer than this; spare lane width widens the gaps.
export const NODE_GAP = 24;
// Bridge-group members sit nearly flush, reading as one block.
export const GROUP_INNER_GAP = 6;
// Me-frame interior: padding plus a label strip across the top. Fragment
// cards inside the frame sit wider apart than lane nodes — the gap is where
// their pending-wire curves and Join pills live.
export const MINE_GAP = 48;
export const FRAME_PAD = 14;
export const FRAME_LABEL = 30;
function groupWidth(group) {
    const widths = group.reduce((sum, node) => sum + node.width, 0);
    return widths + Math.max(0, group.length - 1) * GROUP_INNER_GAP;
}
function groupHeight(group) {
    return Math.max(0, ...group.map((node) => node.height));
}
function laneNaturalWidth(slotWidths) {
    const total = slotWidths.reduce((sum, width) => sum + width, 0);
    return total + (slotWidths.length + 1) * NODE_GAP + 2 * CANVAS_EDGE;
}
// The mockup's spreadSized: center the slots by growing the gaps evenly
// once the lane has spare width; never drop under the floor gap.
function spreadSlots(slotWidths, worldWidth) {
    const total = slotWidths.reduce((sum, width) => sum + width, 0);
    const gap = Math.max(NODE_GAP, (worldWidth - 2 * CANVAS_EDGE - total) / (slotWidths.length + 1));
    const xs = [];
    let x = CANVAS_EDGE + gap;
    for (const width of slotWidths) {
        xs.push(x);
        x += width + gap;
    }
    return xs;
}
export function laneLayout(input) {
    const positions = new Map();
    const peerSlotWidths = input.peerGroups.map(groupWidth);
    const sessionSlotWidths = input.sessions.map((node) => node.width);
    const worldWidth = Math.max(input.minWidth, laneNaturalWidth(peerSlotWidths), laneNaturalWidth(sessionSlotWidths));
    // Peers lane.
    const peersY = CANVAS_EDGE;
    const peerXs = spreadSlots(peerSlotWidths, worldWidth);
    input.peerGroups.forEach((group, groupIndex) => {
        let x = peerXs[groupIndex];
        for (const node of group) {
            positions.set(node.key, { x, y: peersY, width: node.width, height: node.height });
            x += node.width + GROUP_INNER_GAP;
        }
    });
    const peersHeight = Math.max(0, ...input.peerGroups.map(groupHeight));
    // Sessions lane — below the tallest peer card.
    const sessionsY = peersY + peersHeight + LANE_GAP;
    const sessionXs = spreadSlots(sessionSlotWidths, worldWidth);
    input.sessions.forEach((node, index) => {
        positions.set(node.key, {
            x: sessionXs[index],
            y: sessionsY,
            width: node.width,
            height: node.height,
        });
    });
    const sessionsHeight = Math.max(0, ...input.sessions.map((node) => node.height));
    // Me frame — full width, its cards wrapping into rows.
    const mineY = sessionsY + sessionsHeight + LANE_GAP;
    const frameX = CANVAS_EDGE;
    const frameWidth = worldWidth - 2 * CANVAS_EDGE;
    const rowLeft = frameX + FRAME_PAD;
    const rowRight = frameX + frameWidth - FRAME_PAD;
    let rowX = rowLeft;
    let rowY = mineY + FRAME_LABEL + FRAME_PAD;
    let rowHeight = 0;
    for (const node of input.mine) {
        if (rowX > rowLeft && rowX + node.width > rowRight) {
            rowX = rowLeft;
            rowY += rowHeight + MINE_GAP;
            rowHeight = 0;
        }
        positions.set(node.key, { x: rowX, y: rowY, width: node.width, height: node.height });
        rowX += node.width + MINE_GAP;
        rowHeight = Math.max(rowHeight, node.height);
    }
    const contentBottom = input.mine.length ? rowY + rowHeight : rowY;
    const mineFrame = {
        x: frameX,
        y: mineY,
        width: frameWidth,
        height: contentBottom + FRAME_PAD - mineY,
    };
    return {
        positions,
        world: { width: worldWidth, height: mineY + mineFrame.height + CANVAS_EDGE },
        lanes: { peersY, sessionsY, mineY },
        mineFrame,
    };
}
// Edge geometry: an S-shaped cubic between two rects (the mockup's
// curvePath). Rects in different lanes connect vertically — leaving the
// lower edge of the upper rect, entering the upper edge of the lower one.
// Rects whose vertical spans overlap are row-mates (fragments side by side
// in the Me frame): they connect horizontally between their facing edges.
// Pure functions of two rects so the edge layer draws entirely from layout
// output — no DOM measurement.
function rowMates(a, b) {
    return Math.min(a.y + a.height, b.y + b.height) > Math.max(a.y, b.y);
}
export function curveBetween(from, to) {
    if (rowMates(from, to)) {
        const [left, right] = from.x <= to.x ? [from, to] : [to, from];
        const startX = left.x + left.width;
        const startY = left.y + left.height / 2;
        const endX = right.x;
        const endY = right.y + right.height / 2;
        const midX = (startX + endX) / 2;
        return `M ${startX} ${startY} C ${midX} ${startY}, ${midX} ${endY}, ${endX} ${endY}`;
    }
    const [upper, lower] = from.y <= to.y ? [from, to] : [to, from];
    const startX = upper.x + upper.width / 2;
    const startY = upper.y + upper.height;
    const endX = lower.x + lower.width / 2;
    const endY = lower.y;
    const midY = (startY + endY) / 2;
    return `M ${startX} ${startY} C ${startX} ${midY}, ${endX} ${midY}, ${endX} ${endY}`;
}
export function curveMidpoint(from, to) {
    if (rowMates(from, to)) {
        const [left, right] = from.x <= to.x ? [from, to] : [to, from];
        return {
            x: (left.x + left.width + right.x) / 2,
            y: (left.y + left.height / 2 + right.y + right.height / 2) / 2,
        };
    }
    const [upper, lower] = from.y <= to.y ? [from, to] : [to, from];
    return {
        x: (upper.x + upper.width / 2 + lower.x + lower.width / 2) / 2,
        y: (upper.y + upper.height + lower.y) / 2,
    };
}
