import test from "node:test";
import assert from "node:assert/strict";

import {
  CANVAS_EDGE,
  FRAME_LABEL,
  FRAME_PAD,
  GROUP_INNER_GAP,
  LANE_GAP,
  NODE_GAP,
  curveBetween,
  curveMidpoint,
  laneLayout,
} from "../dist/session/layout.js";

const node = (key, width = 200, height = 100) => ({ key, width, height });

const world = (overrides = {}) =>
  laneLayout({
    peerGroups: [[node("peer-a", 180, 60)], [node("peer-b", 180, 72)]],
    sessions: [node("s-1", 340, 220), node("s-2", 340, 180)],
    mine: [node("f-1", 320, 240), node("f-2", 320, 200)],
    minWidth: 1200,
    ...overrides,
  });

test("lanes stack peers over sessions over the Me frame, by measured heights", () => {
  const layout = world();
  const { peersY, sessionsY, mineY } = layout.lanes;
  assert.equal(peersY, CANVAS_EDGE);
  // The sessions lane clears the TALLEST peer card (72, not the first's 60).
  assert.equal(sessionsY, peersY + 72 + LANE_GAP);
  // The Me frame clears the tallest session container (220).
  assert.equal(mineY, sessionsY + 220 + LANE_GAP);
  // Every node landed in its lane.
  assert.equal(layout.positions.get("peer-a").y, peersY);
  assert.equal(layout.positions.get("s-2").y, sessionsY);
  assert.ok(layout.positions.get("f-1").y > mineY);
});

test("a lane spreads its spare width into the gaps, centered", () => {
  const layout = world();
  const s1 = layout.positions.get("s-1");
  const s2 = layout.positions.get("s-2");
  const leading = s1.x - CANVAS_EDGE;
  const between = s2.x - (s1.x + s1.width);
  const trailing = layout.world.width - CANVAS_EDGE - (s2.x + s2.width);
  assert.ok(leading > NODE_GAP, "spare width grew the gaps");
  assert.ok(Math.abs(leading - between) < 0.001, "gaps are even");
  assert.ok(Math.abs(between - trailing) < 0.001, "the row is centered");
});

test("under pressure the gaps floor and the world grows past the viewport", () => {
  const layout = world({
    sessions: [node("s-1", 500), node("s-2", 500), node("s-3", 500)],
    minWidth: 600,
  });
  const s1 = layout.positions.get("s-1");
  const s2 = layout.positions.get("s-2");
  assert.equal(s2.x - (s1.x + s1.width), NODE_GAP, "gap stops at the floor");
  assert.ok(layout.world.width > 600, "the world scrolls instead of crushing cards");
  // An uncrowded layout hugs the viewport instead.
  assert.equal(world().world.width, 1200);
});

test("bridge groups stay adjacent and occupy one spread slot", () => {
  const layout = world({
    peerGroups: [
      [node("alice", 180, 60), node("alice-npub", 180, 60)],
      [node("bob", 180, 60)],
    ],
  });
  const alice = layout.positions.get("alice");
  const alter = layout.positions.get("alice-npub");
  assert.equal(alter.x - (alice.x + alice.width), GROUP_INNER_GAP);
  assert.equal(alice.y, alter.y);
  const bob = layout.positions.get("bob");
  assert.ok(bob.x > alter.x + alter.width + NODE_GAP, "the group reads as one block");
});

test("the Me frame spans the world and wraps its cards into rows", () => {
  const layout = world({
    mine: [node("f-1", 400, 200), node("f-2", 400, 260), node("f-3", 400, 200)],
    minWidth: 1000,
  });
  const frame = layout.mineFrame;
  assert.equal(frame.x, CANVAS_EDGE);
  assert.equal(frame.width, layout.world.width - 2 * CANVAS_EDGE);
  const [f1, f2, f3] = ["f-1", "f-2", "f-3"].map((key) => layout.positions.get(key));
  // Two fit on the first row of a 1000-wide world; the third wraps.
  assert.equal(f1.y, f2.y);
  assert.equal(f1.y, frame.y + FRAME_LABEL + FRAME_PAD);
  assert.equal(f3.x, f1.x, "the wrapped row restarts at the left");
  assert.equal(f3.y, f1.y + 260 + NODE_GAP, "the next row clears the tallest card");
  // The frame contains its last row plus padding; the world contains the frame.
  assert.equal(frame.height, f3.y + 200 + FRAME_PAD - frame.y);
  assert.equal(layout.world.height, frame.y + frame.height + CANVAS_EDGE);
});

test("empty lanes collapse without breaking the stack", () => {
  const layout = laneLayout({ peerGroups: [], sessions: [], mine: [], minWidth: 800 });
  assert.equal(layout.lanes.sessionsY, CANVAS_EDGE + LANE_GAP);
  assert.equal(layout.positions.size, 0);
  assert.equal(layout.world.width, 800);
  assert.ok(layout.world.height > 0);
});

test("edges leave the upper rect's bottom and enter the lower rect's top", () => {
  const peer = { x: 100, y: 24, width: 180, height: 60 };
  const session = { x: 400, y: 200, width: 340, height: 220 };
  const path = curveBetween(peer, session);
  assert.equal(path, "M 190 84 C 190 142, 570 142, 570 200");
  // Direction-agnostic: the same edge regardless of argument order.
  assert.equal(curveBetween(session, peer), path);
  const mid = curveMidpoint(peer, session);
  assert.deepEqual(mid, { x: 380, y: 142 });
});
