import type { EditViolation, FieldEdit, InspectResponse } from "../shared-frontend/core/backend.js";
import { type Network } from "./encoding.js";
export declare const EDIT_SAVE_SEAM = "applyPsbtEdits";
export type FieldContext = "flags" | "ordering" | "sort-mode" | "hex" | "hex32" | "hex32-optional" | "integer" | "u32" | "script";
export interface EditorField {
    path: string;
    label: string;
    value: string;
    context: FieldContext;
    error: string | null;
    note: string | null;
}
export interface EditorSection {
    key: string;
    title: string;
    fields: EditorField[];
}
export interface EditorModel {
    fragmentKey: string;
    network: Network;
    sections: EditorSection[];
}
export declare function editorModel(fragmentKey: string, inspect: InspectResponse | null, network: Network): EditorModel;
export declare function isRawPath(path: string): boolean;
export declare function rawEditsForSave(pristine: EditorModel, edited: EditorModel): FieldEdit[];
export declare function decodedEditsLeftBehind(pristine: EditorModel, edited: EditorModel): string[];
export declare function fieldAt(model: EditorModel, path: string): EditorField | null;
export declare function applyEdit(model: EditorModel, path: string, text: string): EditorModel;
export interface ViolationFix {
    id: string;
    label: string;
    warning: string;
}
export interface Violation {
    path: string | null;
    message: string;
    fix: ViolationFix | null;
    source?: "local" | "server";
    overrideParam?: string;
}
export declare const ASSIGN_UIDS_FIX: ViolationFix;
export declare function validateEditor(model: EditorModel): Violation[];
export declare function violationsFromServer(violations: EditViolation[]): Violation[];
export declare function applyFix(model: EditorModel, fixId: string, randomBytes: (length: number) => Uint8Array): EditorModel;
