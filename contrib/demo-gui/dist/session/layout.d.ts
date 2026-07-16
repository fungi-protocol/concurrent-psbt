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
    peerGroups: LayoutNode[][];
    sessions: LayoutNode[];
    mine: LayoutNode[];
    minWidth: number;
}
export interface LaneLayout {
    positions: Map<string, LayoutRect>;
    world: {
        width: number;
        height: number;
    };
    lanes: {
        peersY: number;
        sessionsY: number;
        mineY: number;
    };
    mineFrame: LayoutRect;
}
export declare const CANVAS_EDGE = 24;
export declare const LANE_GAP = 64;
export declare const NODE_GAP = 24;
export declare const GROUP_INNER_GAP = 6;
export declare const MINE_GAP = 48;
export declare const FRAME_PAD = 14;
export declare const FRAME_LABEL = 30;
export declare function laneLayout(input: LaneLayoutInput): LaneLayout;
export declare function curveBetween(from: LayoutRect, to: LayoutRect): string;
export declare function curveMidpoint(from: LayoutRect, to: LayoutRect): {
    x: number;
    y: number;
};
