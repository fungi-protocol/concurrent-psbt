//! WIP: desktop app shell (Tauri) around the ptj webgui.
//!
//! The webgui is already a self-contained offline server (`ptj webgui`, all
//! assets embedded via include_bytes!). This crate will wrap it in a native
//! window: start (or embed) that asset server, then open a wry webview on it —
//! no browser, no separate frontend build.
//!
//! Current state: a stub binary only. The `tauri` cargo feature is declared
//! but empty and the desktop SDK deps are deferred (see Cargo.toml
//! TODO(ground-deps)), so nothing here opens a window yet. Do not read this
//! crate as a working desktop app.
//!
//! TODO checklist (roughly in order):
//! - [ ] Ground the deps: pin tauri 2.x (+ tauri-build) or bare wry + tao via
//!       `cargo add --dry-run`; populate the `tauri` feature with them.
//! - [ ] tauri.conf.json: replace the placeholder (window geometry, identifier,
//!       CSP for the local asset server, bundle targets).
//! - [ ] Embed the webgui: depend on ptj (feature `webgui`), bind its asset
//!       server on an ephemeral localhost port in-process, and point the wry
//!       webview at it (no fixed :8035 assumption).
//! - [ ] Build wiring: tauri-build in build.rs, icons, `just`/nix targets so
//!       the shell only builds behind the feature (default build stays clean).
//! - [ ] Lifecycle: shut the asset server down when the window closes; single
//!       instance; window title reflecting the open PSBT document.
//! - [ ] Decide packaging: sidecar `ptj` binary vs library-linking the core.

fn main() {
    #[cfg(not(feature = "tauri"))]
    {
        eprintln!(
            "ptj-tauri: WIP scaffold built without the 'tauri' feature; \
             the desktop shell is not implemented"
        );
        std::process::exit(1);
    }
    #[cfg(feature = "tauri")]
    {
        // TODO(ground-deps): once the tauri/wry deps land, this arm becomes
        // the real shell: start the embedded webgui asset server, open the
        // webview, run the app loop.
        eprintln!(
            "ptj-tauri: the 'tauri' feature is declared but its deps are \
             deferred (TODO(ground-deps) in Cargo.toml); the desktop shell \
             is not implemented"
        );
        std::process::exit(1);
    }
}
