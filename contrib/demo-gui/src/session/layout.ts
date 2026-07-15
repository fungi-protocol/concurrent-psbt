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

export interface LayoutNode {
  key: string;
  width: number;
  height: number;
}

export interface LayoutRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface LaneLayoutInput {
  // Bridge groups render adjacent with a shared port: one entry per group,
  // singleton peers as one-element groups.
  peerGroups: LayoutNode[][];
  sessions: LayoutNode[];
  // Local-only fragment cards, wrapped into rows inside the Me frame.
  mine: LayoutNode[];
  // Viewport width — the world never shrinks below it.
  minWidth: number;
}

export interface LaneLayout {
  positions: Map<string, LayoutRect>;
  world: { width: number; height: number };
  lanes: { peersY: number; sessionsY: number; mineY: number };
  mineFrame: LayoutRect;
}

// Side margin of the world; also the frame's outer margin.
export const CANVAS_EDGE = 24;
// Vertical gap between lanes — the room the wire curves cross.
export const LANE_GAP = 64;
// Nodes never sit closer than this; spare lane width widens the gaps.
export const NODE_GAP = 24;
// Bridge-group members sit nearly flush, reading as one block.
export const GROUP_INNER_GAP = 6;
// Me-frame interior: padding plus a label strip across the top.
export const FRAME_PAD = 14;
export const FRAME_LABEL = 30;

function groupWidth(group: LayoutNode[]): number {
  const widths = group.reduce((sum, node) => sum + node.width, 0);
  return widths + Math.max(0, group.length - 1) * GROUP_INNER_GAP;
}

function groupHeight(group: LayoutNode[]): number {
  return Math.max(0, ...group.map((node) => node.height));
}

function laneNaturalWidth(slotWidths: number[]): number {
  const total = slotWidths.reduce((sum, width) => sum + width, 0);
  return total + (slotWidths.length + 1) * NODE_GAP + 2 * CANVAS_EDGE;
}

// The mockup's spreadSized: center the slots by growing the gaps evenly
// once the lane has spare width; never drop under the floor gap.
function spreadSlots(slotWidths: number[], worldWidth: number): number[] {
  const total = slotWidths.reduce((sum, width) => sum + width, 0);
  const gap = Math.max(
    NODE_GAP,
    (worldWidth - 2 * CANVAS_EDGE - total) / (slotWidths.length + 1),
  );
  const xs: number[] = [];
  let x = CANVAS_EDGE + gap;
  for (const width of slotWidths) {
    xs.push(x);
    x += width + gap;
  }
  return xs;
}

export function laneLayout(input: LaneLayoutInput): LaneLayout {
  const positions = new Map<string, LayoutRect>();
  const peerSlotWidths = input.peerGroups.map(groupWidth);
  const sessionSlotWidths = input.sessions.map((node) => node.width);

  const worldWidth = Math.max(
    input.minWidth,
    laneNaturalWidth(peerSlotWidths),
    laneNaturalWidth(sessionSlotWidths),
  );

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
      rowY += rowHeight + NODE_GAP;
      rowHeight = 0;
    }
    positions.set(node.key, { x: rowX, y: rowY, width: node.width, height: node.height });
    rowX += node.width + NODE_GAP;
    rowHeight = Math.max(rowHeight, node.height);
  }
  const contentBottom = input.mine.length ? rowY + rowHeight : rowY;
  const mineFrame: LayoutRect = {
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

// Edge geometry: a vertical-S cubic between two rects, leaving the lower
// edge of the upper rect and entering the upper edge of the lower one (the
// mockup's curvePath). Pure functions of two rects so the edge layer draws
// entirely from layout output — no DOM measurement.
export function curveBetween(from: LayoutRect, to: LayoutRect): string {
  const [upper, lower] = from.y <= to.y ? [from, to] : [to, from];
  const startX = upper.x + upper.width / 2;
  const startY = upper.y + upper.height;
  const endX = lower.x + lower.width / 2;
  const endY = lower.y;
  const midY = (startY + endY) / 2;
  return `M ${startX} ${startY} C ${startX} ${midY}, ${endX} ${midY}, ${endX} ${endY}`;
}

export function curveMidpoint(from: LayoutRect, to: LayoutRect): { x: number; y: number } {
  const [upper, lower] = from.y <= to.y ? [from, to] : [to, from];
  return {
    x: (upper.x + upper.width / 2 + lower.x + lower.width / 2) / 2,
    y: (upper.y + upper.height + lower.y) / 2,
  };
}
