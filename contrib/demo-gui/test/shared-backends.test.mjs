// Runtime conformance for the shared-frontend adapters (the type-level
// conformance lives in src/shared-frontend/backends/conformance.test.ts and
// runs under tsc). These tests pin the WIRE behavior of the adapter methods
// that carry real branching — starting with HttpBackend.applyPsbtEdits,
// whose 400-with-violations outcome is a structured seam response (the
// violation -> fix -> revalidate loop), not a transport error.

import test from "node:test";
import assert from "node:assert/strict";

import { HttpBackend } from "../dist/shared-frontend/backends/http.js";
import { PtjBackendError } from "../dist/shared-frontend/core/types.js";

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
    calls.push({ path, init });
    return response;
  };
  return { fetch, calls };
}

test("applyPsbtEdits posts edits, apply_fixes, and named override booleans to /api/edit", async () => {
  const success = {
    psbt: "cHNidP-edited",
    inspect: { format: "bip370" },
    violations: [],
    overridden: [],
    applied_fixes: [
      {
        fix_id: "assign-ids",
        warning_text:
          "Automatically generating unique IDs may result in duplicate txouts if done more than once.",
      },
    ],
  };
  const { fetch, calls } = recordingFetch(jsonResponse(200, success));
  const backend = new HttpBackend(fetch);

  const response = await backend.applyPsbtEdits(
    "cHNidP-original",
    [
      { map: "global", key: "fc0470736274ab", value: "abcd" },
      { map: "output:0", key: "09", value: null },
    ],
    { applyFixes: ["assign-ids"], overrides: ["allow_duplicate_output_ids"] },
  );

  assert.equal(calls.length, 1);
  assert.equal(calls[0].path, "/api/edit");
  assert.equal(calls[0].init.method, "POST");
  const body = JSON.parse(calls[0].init.body);
  assert.deepEqual(body.edits, [
    { map: "global", key: "fc0470736274ab", value: "abcd" },
    { map: "output:0", key: "09", value: null },
  ]);
  assert.deepEqual(body.apply_fixes, ["assign-ids"]);
  // Overrides are TOP-LEVEL named booleans (the allow_short_seed convention).
  assert.equal(body.allow_duplicate_output_ids, true);
  assert.equal(body.psbt, "cHNidP-original");

  // The applied-fix warning comes back verbatim.
  assert.deepEqual(response, success);
});

test("applyPsbtEdits returns a 400 body carrying violations[] instead of throwing", async () => {
  const failed = {
    error: "save-time validation failed (1 violation); apply an offered fix, set the named override, or amend the edits",
    violations: [
      {
        id: "unordered-missing-output-ids",
        message: "the PSBT is unordered but 1 output lacks PSBT_OUT_UNIQUE_ID",
        override_param: "allow_missing_output_ids",
        fix_id: "assign-ids",
        fix_label: "Generate missing output unique IDs",
        warning_text:
          "Automatically generating unique IDs may result in duplicate txouts if done more than once.",
      },
    ],
  };
  const { fetch } = recordingFetch(jsonResponse(400, failed));
  const backend = new HttpBackend(fetch);

  const response = await backend.applyPsbtEdits("cHNidP-original", []);
  assert.equal(response.psbt, undefined);
  assert.deepEqual(response, failed);
});

test("applyPsbtEdits still throws PtjBackendError on non-violation errors", async () => {
  const { fetch } = recordingFetch(jsonResponse(400, { error: "request psbt: not base64" }));
  const backend = new HttpBackend(fetch);
  await assert.rejects(
    backend.applyPsbtEdits("junk", []),
    (error) =>
      error instanceof PtjBackendError &&
      error.status === 400 &&
      /not base64/.test(error.message),
  );
});

// A response whose body is NOT JSON — what real fetch yields for the e2e
// harness's plain-text 404 "not found": response.json() rejects with the
// same SyntaxError the browser raises.
function textResponse(status, text) {
  return {
    ok: status >= 200 && status < 300,
    status,
    async json() {
      return JSON.parse(text);
    },
  };
}

test("postJson surfaces a non-JSON error body as PtjBackendError, not SyntaxError", async () => {
  const { fetch } = recordingFetch(textResponse(404, "not found"));
  const backend = new HttpBackend(fetch);
  await assert.rejects(
    backend.inspectPsbt("cHNidP..."),
    (error) =>
      error instanceof PtjBackendError &&
      error.status === 404 &&
      /HTTP 404/.test(error.message),
  );
});

test("applyPsbtEdits surfaces a non-JSON error body as PtjBackendError too", async () => {
  const { fetch } = recordingFetch(textResponse(404, "not found"));
  const backend = new HttpBackend(fetch);
  await assert.rejects(
    backend.applyPsbtEdits("cHNidP...", []),
    (error) => error instanceof PtjBackendError && error.status === 404,
  );
});

test("classifyPaste posts {payload, network} to /api/classify", async () => {
  const classified = {
    kind: "descriptor",
    descriptor: "wpkh(xpub.../0/*)#checksum",
    has_private_keys: false,
  };
  const { fetch, calls } = recordingFetch(jsonResponse(200, classified));
  const backend = new HttpBackend(fetch);

  const response = await backend.classifyPaste("wpkh(xpub.../0/*)", "regtest");
  assert.equal(calls.length, 1);
  assert.equal(calls[0].path, "/api/classify");
  assert.deepEqual(JSON.parse(calls[0].init.body), {
    payload: "wpkh(xpub.../0/*)",
    network: "regtest",
  });
  assert.deepEqual(response, classified);
});

test("classifyPaste surfaces the route's redirect/parse errors as PtjBackendError", async () => {
  // PSBT pastes are REDIRECTED by the route (400 naming the PSBT flows).
  const { fetch } = recordingFetch(
    jsonResponse(400, { error: "the payload is a PSBT; paste it into the PSBT flows instead" }),
  );
  const backend = new HttpBackend(fetch);
  await assert.rejects(
    backend.classifyPaste("cHNidP..."),
    (error) =>
      error instanceof PtjBackendError && error.status === 400 && /PSBT flows/.test(error.message),
  );
});
