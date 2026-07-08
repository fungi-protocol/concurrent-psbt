//! Browser/wasm boundary tests (run with `wasm-pack test --headless --firefox`
//! or `--node`). These exercise the `#[wasm_bindgen]` JsValue marshaling that
//! the native tests in src/ops.rs cannot reach, and — critically — the
//! getrandom `wasm_js` path (UniqueId::generate) which only exists on wasm.
//!
//! NOT part of the repo's default check; the PWA build runs these.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

// Re-import the crate's public exports.
use concurrent_psbt_wasm::{create, inspect, join, local_sync, sort, version};

fn js_obj(pairs: &[(&str, JsValue)]) -> JsValue {
    let obj = js_sys::Object::new();
    for (k, v) in pairs {
        js_sys::Reflect::set(&obj, &JsValue::from_str(k), v).unwrap();
    }
    obj.into()
}

#[wasm_bindgen_test]
fn version_is_nonempty() {
    assert!(!version().is_empty());
}

#[wasm_bindgen_test]
fn create_empty_regtest_uses_browser_rng() {
    // create -> UniqueId::generate -> rand::random -> getrandom wasm_js ->
    // crypto.getRandomValues. If the getrandom backend is misconfigured this
    // panics/aborts; success proves the whole RNG chain works in the browser.
    let req = js_obj(&[("network", JsValue::from_str("regtest"))]);
    let resp = create(req).expect("create in browser");
    let psbt = js_sys::Reflect::get(&resp, &JsValue::from_str("psbt")).unwrap();
    assert!(psbt.as_string().is_some());

    // Round-trip: the created base64 PSBT inspects as an unordered bip370 PSBT.
    let inspected = inspect(psbt.as_string().unwrap()).expect("inspect");
    let ordering = js_sys::Reflect::get(&inspected, &JsValue::from_str("ordering")).unwrap();
    assert_eq!(ordering.as_string().as_deref(), Some("unordered"));
}

#[wasm_bindgen_test]
fn join_of_two_created_psbts_folds() {
    let req = js_obj(&[("network", JsValue::from_str("regtest"))]);
    let a = create(req).unwrap();
    let a_psbt = js_sys::Reflect::get(&a, &JsValue::from_str("psbt"))
        .unwrap()
        .as_string()
        .unwrap();

    let arr = js_sys::Array::new();
    arr.push(&JsValue::from_str(&a_psbt));
    arr.push(&JsValue::from_str(&a_psbt));
    let joined = join(arr.into()).expect("join in browser");
    let psbt = js_sys::Reflect::get(&joined, &JsValue::from_str("psbt")).unwrap();
    assert!(psbt.as_string().is_some());
}

#[wasm_bindgen_test]
fn local_sync_folds_in_browser_with_no_network() {
    // LOCAL-FIRST: sync's local fold must work in-browser with zero server /
    // transport. Exercises the localSync export (js_name) end to end.
    let req = js_obj(&[("network", JsValue::from_str("regtest"))]);
    let a = create(req).unwrap();
    let a_psbt = js_sys::Reflect::get(&a, &JsValue::from_str("psbt"))
        .unwrap()
        .as_string()
        .unwrap();

    let arr = js_sys::Array::new();
    arr.push(&JsValue::from_str(&a_psbt));
    arr.push(&JsValue::from_str(&a_psbt));
    let synced = local_sync(arr.into()).expect("localSync in browser");
    let psbt = js_sys::Reflect::get(&synced, &JsValue::from_str("psbt")).unwrap();
    assert!(psbt.as_string().is_some());
    let payments = js_sys::Reflect::get(&synced, &JsValue::from_str("payments")).unwrap();
    assert_eq!(js_sys::Array::from(&payments).length(), 0);
}

#[wasm_bindgen_test]
fn sort_takes_positional_psbt_and_optional_seed() {
    // Canonical sort arity (Backend.sortPsbt(psbt, seedHex?, allowShortSeed?)).
    let req = js_obj(&[("network", JsValue::from_str("regtest"))]);
    let created = create(req).unwrap();
    let psbt = js_sys::Reflect::get(&created, &JsValue::from_str("psbt"))
        .unwrap()
        .as_string()
        .unwrap();
    let sorted = sort(
        psbt,
        Some("abcdabcdabcdabcdabcdabcdabcdabcd".to_string()),
        None,
    )
    .expect("sort in browser");
    let out = js_sys::Reflect::get(&sorted, &JsValue::from_str("psbt")).unwrap();
    assert!(out.as_string().is_some());
}

#[wasm_bindgen_test]
fn error_surfaces_as_thrown_jserror_message() {
    // A malformed PSBT must throw a JsError whose message == the webgui text,
    // so the frontend adapter's `err.message` -> PtjBackendError works.
    let err = inspect("not a psbt".to_string()).unwrap_err();
    let msg = JsValue::from(err).as_string().unwrap_or_default();
    assert!(msg.contains("decoding base64"), "unexpected message: {msg}");
}
