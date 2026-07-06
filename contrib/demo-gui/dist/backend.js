export class PtjBackendError extends Error {
    status;
    constructor(status, message) {
        super(message);
        this.name = "PtjBackendError";
        this.status = status;
        Object.setPrototypeOf(this, PtjBackendError.prototype);
    }
}
async function postJson(fetchImpl, path, body) {
    const response = await fetchImpl(path, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body),
    });
    const payload = await response.json();
    if (!response.ok) {
        throw new PtjBackendError(response.status, errorMessage(response.status, payload));
    }
    return payload;
}
function errorMessage(status, payload) {
    return isErrorPayload(payload) ? payload.error : `ptj backend request failed with HTTP ${status}`;
}
function isErrorPayload(payload) {
    return typeof payload === "object"
        && payload !== null
        && "error" in payload
        && typeof payload.error === "string";
}
export function inspectPsbt(fetchImpl, psbt) {
    return postJson(fetchImpl, "/api/inspect", { psbt });
}
export function createPsbt(fetchImpl, request) {
    return postJson(fetchImpl, "/api/create", {
        network: request.network,
        ordering: request.ordering,
        seed_hex: request.seedHex,
        inputs: request.inputs,
        outputs: request.outputs.map((output) => ({
            address: output.address,
            amount_btc: output.amountBtc,
        })),
    });
}
export function joinPsbts(fetchImpl, psbts) {
    return postJson(fetchImpl, "/api/join", { psbts });
}
export function sortPsbt(fetchImpl, psbt, seedHex) {
    return postJson(fetchImpl, "/api/sort", { psbt, seed_hex: seedHex });
}
export function makeUnordered(fetchImpl, psbt) {
    return postJson(fetchImpl, "/api/make-unordered", { psbt });
}
export function atomizePsbt(fetchImpl, psbt) {
    return postJson(fetchImpl, "/api/atomize", { psbt });
}
export function concatenatePsbts(fetchImpl, psbts) {
    return postJson(fetchImpl, "/api/concatenate", { psbts });
}
export function exportBip174(fetchImpl, psbt) {
    return postJson(fetchImpl, "/api/export-bip174", { psbt });
}
export function importBip174(fetchImpl, psbt) {
    return postJson(fetchImpl, "/api/import-bip174", { psbt });
}
