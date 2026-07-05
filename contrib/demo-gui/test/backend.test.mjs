import test from "node:test";
import assert from "node:assert/strict";

import {
  PtjBackendError,
  atomizePsbt,
  concatenatePsbts,
  createPsbt,
  exportBip174,
  importBip174,
  inspectPsbt,
  joinPsbts,
  makeUnordered,
  sortPsbt,
  syncPsbts,
} from "../dist/backend.js";

function jsonResponse(status, body) {
  return {
    ok: status >= 200 && status < 300,
    status,
    async json() {
      return body;
    },
  };
}

function recordingFetch(response) {
  const calls = [];
  const fetch = async (path, init) => {
    calls.push({
      path,
      method: init?.method,
      headers: init?.headers,
      body: JSON.parse(init?.body || "{}"),
    });
    return response;
  };
  return { fetch, calls };
}

test("backend client posts typed JSON to offline ptj endpoints", async () => {
  const { fetch, calls } = recordingFetch(jsonResponse(200, {
    psbt: "created-base64",
    inspect: { ordering: "unordered" },
  }));

  const created = await createPsbt(fetch, {
    network: "regtest",
    ordering: "deterministic",
    seedHex: "abcd",
    inputs: [{ txid: "00".repeat(32), vout: 7 }],
    outputs: [{ address: "bcrt1qexample", amountBtc: "0.00050000" }],
  });

  assert.deepEqual(created, { psbt: "created-base64", inspect: { ordering: "unordered" } });
  assert.deepEqual(calls, [{
    path: "/api/create",
    method: "POST",
    headers: { "content-type": "application/json" },
    body: {
      network: "regtest",
      ordering: "deterministic",
      seed_hex: "abcd",
      inputs: [{ txid: "00".repeat(32), vout: 7 }],
      outputs: [{ address: "bcrt1qexample", amount_btc: "0.00050000" }],
    },
  }]);
});

test("backend client exposes every offline PSBT transform endpoint", async () => {
  const inspect = recordingFetch(jsonResponse(200, { input_count: 1 }));
  assert.deepEqual(await inspectPsbt(inspect.fetch, "a"), { input_count: 1 });
  assert.equal(inspect.calls[0].path, "/api/inspect");
  assert.deepEqual(inspect.calls[0].body, { psbt: "a" });

  const join = recordingFetch(jsonResponse(200, { psbt: "joined" }));
  assert.deepEqual(await joinPsbts(join.fetch, ["a", "b"]), { psbt: "joined" });
  assert.equal(join.calls[0].path, "/api/join");
  assert.deepEqual(join.calls[0].body, { psbts: ["a", "b"] });

  const sort = recordingFetch(jsonResponse(200, { psbt: "sorted" }));
  assert.deepEqual(await sortPsbt(sort.fetch, "u", "abcd"), { psbt: "sorted" });
  assert.equal(sort.calls[0].path, "/api/sort");
  assert.deepEqual(sort.calls[0].body, { psbt: "u", seed_hex: "abcd" });

  const unordered = recordingFetch(jsonResponse(200, { psbt: "unordered" }));
  assert.deepEqual(await makeUnordered(unordered.fetch, "o"), { psbt: "unordered" });
  assert.equal(unordered.calls[0].path, "/api/make-unordered");
  assert.deepEqual(unordered.calls[0].body, { psbt: "o" });

  const atoms = recordingFetch(jsonResponse(200, { fragments: [] }));
  assert.deepEqual(await atomizePsbt(atoms.fetch, "psbt"), { fragments: [] });
  assert.equal(atoms.calls[0].path, "/api/atomize");
  assert.deepEqual(atoms.calls[0].body, { psbt: "psbt" });

  const concatenated = recordingFetch(jsonResponse(200, { psbt: "concat" }));
  assert.deepEqual(await concatenatePsbts(concatenated.fetch, ["x", "y"]), { psbt: "concat" });
  assert.equal(concatenated.calls[0].path, "/api/concatenate");
  assert.deepEqual(concatenated.calls[0].body, { psbts: ["x", "y"] });

  const exported = recordingFetch(jsonResponse(200, { format: "bip174", psbt: "core" }));
  assert.deepEqual(await exportBip174(exported.fetch, "ordered"), { format: "bip174", psbt: "core" });
  assert.equal(exported.calls[0].path, "/api/export-bip174");
  assert.deepEqual(exported.calls[0].body, { psbt: "ordered" });

  const imported = recordingFetch(jsonResponse(200, { psbt: "bip370", inspect: { format: "bip370" } }));
  assert.deepEqual(await importBip174(imported.fetch, "core"), { psbt: "bip370", inspect: { format: "bip370" } });
  assert.equal(imported.calls[0].path, "/api/import-bip174");
  assert.deepEqual(imported.calls[0].body, { psbt: "core" });

  const synced = recordingFetch(jsonResponse(200, { psbt: "lub", payments: [], confirmations: [] }));
  assert.deepEqual(
    await syncPsbts(synced.fetch, { psbts: ["a", "b"], irohTicket: "docabc", irohWaitMs: 100 }),
    { psbt: "lub", payments: [], confirmations: [] },
  );
  assert.equal(synced.calls[0].path, "/api/sync");
  assert.deepEqual(synced.calls[0].body, { psbts: ["a", "b"], iroh_ticket: "docabc", iroh_wait_ms: 100 });
});

test("backend client raises structured ptj errors", async () => {
  const { fetch } = recordingFetch(jsonResponse(400, { error: "run sort first" }));

  await assert.rejects(
    () => makeUnordered(fetch, "bad"),
    (error) => {
      assert(error instanceof PtjBackendError);
      assert.equal(error.status, 400);
      assert.equal(error.message, "run sort first");
      return true;
    },
  );

  const fallback = recordingFetch(jsonResponse(500, { message: "not a ptj error" }));
  await assert.rejects(
    () => makeUnordered(fallback.fetch, "bad"),
    (error) => {
      assert(error instanceof PtjBackendError);
      assert.equal(error.status, 500);
      assert.equal(error.message, "ptj backend request failed with HTTP 500");
      return true;
    },
  );
});
