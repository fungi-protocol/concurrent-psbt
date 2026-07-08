use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::str::FromStr as _;

use crate::transport::message::Message;

use crate::cli::{CreateConfig, NetworkArg, OrderingArg, OutPointArg, OutputArg, WebguiConfig};
use crate::{Error, Result};

// The REAL session UI (contrib/demo-gui/session.html + src/session/), served
// at "/": real PSBTs through the Backend seam, no fixtures. The demo sandbox
// (index.html + src/app.ts, synthetic payloads) stays fully served behind the
// explicit "/demo" route — WIP-retained, still the playwright surface.
const SESSION_HTML: &[u8] = include_bytes!("../../../contrib/demo-gui/session.html");
const SESSION_APP_JS: &[u8] = include_bytes!("../../../contrib/demo-gui/dist/session/app.js");
const SESSION_STATE_JS: &[u8] =
    include_bytes!("../../../contrib/demo-gui/dist/session/state.js");
const INDEX_HTML: &[u8] = include_bytes!("../../../contrib/demo-gui/index.html");
const STYLES_CSS: &[u8] = include_bytes!("../../../contrib/demo-gui/styles.css");
const APP_JS: &[u8] = include_bytes!("../../../contrib/demo-gui/dist/app.js");
const BACKEND_JS: &[u8] = include_bytes!("../../../contrib/demo-gui/dist/backend.js");
const MODEL_JS: &[u8] = include_bytes!("../../../contrib/demo-gui/dist/model.js");
// The canonical shared-frontend Backend seam (contrib/demo-gui/src/
// shared-frontend/): app.js constructs ONE HttpBackend against this server's
// own /api/* routes instead of the retired free-function client in backend.js
// (still served: it remains the node --test coverage surface). Only the
// modules the browser actually loads are embedded — http.js plus its one
// runtime import, core/types.js; core/backend.js is type-only and the
// wasm/tauri adapters belong to the PWA/tauri shells.
const SHARED_FRONTEND_HTTP_BACKEND_JS: &[u8] =
    include_bytes!("../../../contrib/demo-gui/dist/shared-frontend/backends/http.js");
const SHARED_FRONTEND_TYPES_JS: &[u8] =
    include_bytes!("../../../contrib/demo-gui/dist/shared-frontend/core/types.js");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asset {
    pub content_type: &'static str,
    pub body: &'static [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Response {
    pub(crate) status: u16,
    pub(crate) reason: &'static str,
    pub(crate) content_type: &'static str,
    pub(crate) body: Vec<u8>,
}

impl Response {
    /// The Cache-Control policy for this response. Everything the webgui
    /// serves is per-request state (`no-store`) EXCEPT the lifehash PNGs:
    /// those are content-addressed — the digest in the URL fully determines
    /// the (release-stable) image — so they are immutable.
    fn cache_control(&self) -> &'static str {
        if self.content_type == "image/png" {
            "public, max-age=31536000, immutable"
        } else {
            "no-store"
        }
    }
}

pub fn asset(path: &str) -> Option<Asset> {
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    match path {
        "/" | "/session.html" => Some(Asset {
            content_type: "text/html; charset=utf-8",
            body: SESSION_HTML,
        }),
        // The demo sandbox, explicitly opt-in. Its relative asset URLs
        // (styles.css, dist/app.js) resolve against "/" from "/demo" (no
        // trailing slash), so the page works unchanged; "/index.html" keeps
        // serving it under its historical name.
        "/demo" | "/index.html" => Some(Asset {
            content_type: "text/html; charset=utf-8",
            body: INDEX_HTML,
        }),
        "/dist/session/app.js" => Some(Asset {
            content_type: "text/javascript; charset=utf-8",
            body: SESSION_APP_JS,
        }),
        "/dist/session/state.js" => Some(Asset {
            content_type: "text/javascript; charset=utf-8",
            body: SESSION_STATE_JS,
        }),
        "/styles.css" => Some(Asset {
            content_type: "text/css; charset=utf-8",
            body: STYLES_CSS,
        }),
        "/dist/app.js" => Some(Asset {
            content_type: "text/javascript; charset=utf-8",
            body: APP_JS,
        }),
        "/dist/backend.js" => Some(Asset {
            content_type: "text/javascript; charset=utf-8",
            body: BACKEND_JS,
        }),
        "/dist/model.js" => Some(Asset {
            content_type: "text/javascript; charset=utf-8",
            body: MODEL_JS,
        }),
        "/dist/shared-frontend/backends/http.js" => Some(Asset {
            content_type: "text/javascript; charset=utf-8",
            body: SHARED_FRONTEND_HTTP_BACKEND_JS,
        }),
        "/dist/shared-frontend/core/types.js" => Some(Asset {
            content_type: "text/javascript; charset=utf-8",
            body: SHARED_FRONTEND_TYPES_JS,
        }),
        _ => None,
    }
}

pub(crate) fn response_for(method: &str, path: &str, body: &[u8]) -> Response {
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    if method == "POST" {
        match path {
            "/api/assign-ids" => return assign_ids_response(body),
            "/api/atomize" => return atomize_response(body),
            "/api/classify" => return classify_response(body),
            "/api/concatenate" => return concatenate_response(body),
            "/api/confirm" => return confirm_response(body),
            "/api/create" => return create_response(body),
            "/api/edit" => return edit_response(body),
            "/api/export-bip174" => return export_bip174_response(body),
            "/api/import-bip174" => return import_bip174_response(body),
            "/api/inspect" => return inspect_response(body),
            "/api/join" => return join_response(body),
            "/api/make-unordered" => return make_unordered_response(body),
            "/api/pay" => return pay_response(body),
            "/api/payments" => return payments_response(body),
            "/api/sort" => return sort_response(body),
            "/api/sync" => return sync_response(body),
            _ => {}
        }
    }

    if method != "GET" && method != "HEAD" {
        return text_response(405, "Method Not Allowed", "method not allowed");
    }

    // GET /api/lifehash/<hex-digest> -> PNG fingerprint (content-addressed:
    // the digest fully determines the image, so responses are immutable).
    if let Some(digest) = path.strip_prefix("/api/lifehash/") {
        let mut response = lifehash_response(digest);
        if method == "HEAD" {
            response.body = Vec::new();
        }
        return response;
    }

    let Some(asset) = asset(path) else {
        return text_response(404, "Not Found", "not found");
    };
    Response {
        status: 200,
        reason: "OK",
        content_type: asset.content_type,
        body: if method == "HEAD" {
            Vec::new()
        } else {
            asset.body.to_vec()
        },
    }
}

pub fn serve(config: WebguiConfig) -> Result<()> {
    let bind_addr = SocketAddr::new(config.host, config.port);
    let listener = TcpListener::bind(bind_addr)
        .map_err(|error| Error::new(format!("binding webgui to {bind_addr}: {error}")))?;
    let local_addr = listener
        .local_addr()
        .map_err(|error| Error::new(format!("reading webgui bind address: {error}")))?;
    eprintln!("ptj webgui listening on http://{local_addr}/");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(error) = handle_connection(stream) {
                    eprintln!("ptj webgui request error: {error}");
                }
            }
            Err(error) => eprintln!("ptj webgui accept error: {error}"),
        }
    }

    Ok(())
}

fn handle_connection(mut stream: TcpStream) -> Result<()> {
    let request = read_http_request(&mut stream)?;
    let header_end = find_header_end(&request)
        .ok_or_else(|| Error::new("HTTP request did not contain a complete header"))?;
    let headers = std::str::from_utf8(&request[..header_end])
        .map_err(|error| Error::new(format!("HTTP request was not UTF-8: {error}")))?;
    let Some(request_line) = headers.lines().next() else {
        return write_http_response(
            &mut stream,
            &text_response(400, "Bad Request", "bad request"),
        );
    };
    let parts = request_line.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 3 {
        return write_http_response(
            &mut stream,
            &text_response(400, "Bad Request", "bad request"),
        );
    }
    let body_start = header_end + b"\r\n\r\n".len();
    let content_length = content_length(headers)?;
    let body_end = body_start.saturating_add(content_length).min(request.len());
    let response = response_for(parts[0], parts[1], &request[body_start..body_end]);
    write_http_response(&mut stream, &response)
}

fn inspect_response(body: &[u8]) -> Response {
    match inspect_response_result(body) {
        Ok(body) => Response {
            status: 200,
            reason: "OK",
            content_type: "application/json; charset=utf-8",
            body,
        },
        Err(error) => json_error_response(400, "Bad Request", &error.to_string()),
    }
}

fn inspect_response_result(body: &[u8]) -> Result<Vec<u8>> {
    let request: serde_json::Value = serde_json::from_slice(body)
        .map_err(|error| Error::new(format!("parsing JSON request: {error}")))?;
    let psbt = request
        .get("psbt")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::new("request JSON must contain string field `psbt`"))?;
    let psbt = crate::io::parse_psbt_bytes("request psbt", psbt.as_bytes())?;
    Ok(crate::commands::inspect::inspect_psbt(&psbt)
        .to_string()
        .into_bytes())
}

fn atomize_response(body: &[u8]) -> Response {
    match atomize_response_result(body) {
        Ok(body) => Response {
            status: 200,
            reason: "OK",
            content_type: "application/json; charset=utf-8",
            body,
        },
        Err(error) => json_error_response(400, "Bad Request", &error.to_string()),
    }
}

fn atomize_response_result(body: &[u8]) -> Result<Vec<u8>> {
    let request: serde_json::Value = serde_json::from_slice(body)
        .map_err(|error| Error::new(format!("parsing JSON request: {error}")))?;
    let psbt = request
        .get("psbt")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::new("request JSON must contain string field `psbt`"))?;
    let psbt = crate::io::parse_psbt_bytes("request psbt", psbt.as_bytes())?;
    let fragments = crate::commands::atomize::atomize_psbt(psbt)?
        .into_iter()
        .map(|fragment| {
            serde_json::json!({
                "psbt": crate::io::encode_psbt(&fragment),
                "inspect": crate::commands::inspect::inspect_psbt(&fragment),
            })
        })
        .collect::<Vec<_>>();
    Ok(serde_json::json!({ "fragments": fragments })
        .to_string()
        .into_bytes())
}

fn sync_response(body: &[u8]) -> Response {
    match sync_response_result(body) {
        Ok(body) => Response {
            status: 200,
            reason: "OK",
            content_type: "application/json; charset=utf-8",
            body,
        },
        Err(error) => json_error_response(400, "Bad Request", &error.to_string()),
    }
}

// webgui-layer23: /api/sync no longer hardcodes the iroh transport. It builds
// a `SyncConfig` from the request and hands it to the SAME selector the CLI
// uses (`crate::commands::sync::build_transport`), then drives it with the
// SAME transport-agnostic step (`crate::commands::sync::sync_step`). Every
// per-transport cargo feature (iroh-sync/arti/nym/emissary/mdk/str0m/
// webrtc-rs/payjoin-dir — plus nostr once TODO(transport-nostr) is authored)
// flows through automatically.
fn sync_response_result(body: &[u8]) -> Result<Vec<u8>> {
    let request: serde_json::Value = serde_json::from_slice(body)
        .map_err(|error| Error::new(format!("parsing JSON request: {error}")))?;

    // ---- Layer 2: local sources fold (unchanged) --------------------------
    // Parse `psbts[]` and fold them with the same idempotent/commutative/
    // associative lattice join the CLI uses. No transport is touched here.
    let psbts = match request.get("psbts") {
        None => Vec::new(),
        Some(value) => value
            .as_array()
            .ok_or_else(|| Error::new("request JSON field `psbts` must be an array"))?
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let psbt = value.as_str().ok_or_else(|| {
                    Error::new(format!("request psbts[{index}] must be a string"))
                })?;
                crate::io::parse_psbt_bytes(&format!("request psbts[{index}]"), psbt.as_bytes())
            })
            .collect::<Result<Vec<_>>>()?,
    };
    let local = if psbts.is_empty() {
        None
    } else {
        Some(crate::commands::join::join_psbts(psbts)?)
    };

    // ---- Layer 3: transport selection (mirrors the CLI's --transport) -----
    let config = sync_config_from_request(&request)?;

    if config.transport == crate::cli::TransportKind::Local
        && config.sources.is_empty()
        && config.state.is_none()
    {
        // No network, no server-side sources: return the locally-joined PSBT
        // (or error if nothing to fold). Identical to the old no-ticket branch.
        let joined = local
            .ok_or_else(|| Error::new("request must contain `psbts` or a network transport"))?;
        return sync_json(&joined, &[], None);
    }

    if config.transport == crate::cli::TransportKind::Local {
        // Local file/dir transport: fold the request's in-band `psbts[]`
        // together with the server-side `sources` paths and `state` file —
        // the same LocalTransport the CLI's `ptj sync <sources>` drives. The
        // in-band fold rides the CLI's stdin-source mechanism (a `-` source),
        // so everything converges in ONE lattice join. Read-only: the runner
        // owns state writing on the CLI path, and this route only reports the
        // converged result (`publish_target` stays `None`).
        let mut config = config;
        let stdin = local.map(|psbt| crate::io::encode_psbt(&psbt).into_bytes());
        if stdin.is_some() {
            config.sources.push(std::path::PathBuf::from("-"));
        }
        let mut transport = crate::commands::sync::local_transport(
            &config,
            stdin.as_deref(),
            None,
            crate::cli::OutputFileFormat::Base64,
        );
        let (joined, messages) = crate::commands::sync::drive_async(async move {
            crate::commands::sync::sync_step(&mut transport).await
        })?;
        return sync_json(&joined, &messages, None);
    }

    // Network transport: build it through the shared selector, publish our
    // local state, wait for peers, then fold the collected frontier — the same
    // shape as the CLI's `run_over_network`, but transport-agnostic. The
    // interactive server handler is sync, so this is the webgui's async→sync
    // edge, driven on the shared sync-driver runtime.
    let ticket_out_path = config.iroh_ticket_out.clone();
    let mut transport = crate::commands::sync::build_transport(&config)?;
    let wait_ms = config.iroh_wait_ms;
    let (joined, messages) = crate::commands::sync::drive_async(async move {
        if let Some(local) = local {
            transport
                .publish(Message::Psbt(crate::io::encode_psbt(&local).into_bytes()).encode())
                .await?;
        }
        tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
        crate::commands::sync::sync_step(transport.as_mut()).await
    })?;
    // `iroh_ticket_out: true` asked the selector to create a fresh document;
    // the ticket it wrote (server-side temp file, exactly like the inbound
    // ticket) is read back into the response so the browser can hand it out.
    let ticket_out = ticket_out_path
        .map(|path| {
            std::fs::read_to_string(&path)
                .map(|ticket| ticket.trim().to_owned())
                .map_err(|error| Error::new(format!("reading created iroh ticket: {error}")))
        })
        .transpose()?;
    sync_json(&joined, &messages, ticket_out.as_deref())
}

/// Build a `SyncConfig` from a `/api/sync` JSON request.
///
/// PSBTs arrive in-band via `psbts[]` (folded before the transport step) and
/// optionally from server-side `sources` paths (files or directories of .psbt
/// files, the CLI's positional sources) plus a `state` PSBT file, read-only.
/// The `transport` field maps 1:1 onto the CLI's
/// `TransportKind` (same `ValueEnum`); absent, it is inferred from the legacy
/// request shape (a pasted `iroh_ticket` selects Iroh) so the existing
/// frontend keeps working with no JS change. The WebRTC signaling params
/// (`webrtc_role`, `signal_out`, `signal_in`, `webrtc_bind`, `ice_servers`,
/// `signal_timeout_ms`) mirror the CLI flags of the same names for the
/// str0m / webrtc-rs transports.
fn sync_config_from_request(request: &serde_json::Value) -> Result<crate::cli::SyncConfig> {
    use clap::ValueEnum as _;

    let transport = match request.get("transport").and_then(serde_json::Value::as_str) {
        Some(name) => crate::cli::TransportKind::from_str(name, /* ignore_case */ true)
            .map_err(|_| {
                Error::new(format!(
                    "unknown transport '{name}' (expected one of: local, iroh, arti, nym, \
                     emissary, mdk, str0m, webrtc-rs, payjoin-dir)"
                ))
            })?,
        // Back-compat inference: a pasted iroh doc-ticket selects Iroh; no
        // ticket + no transport means a pure local Layer-2 fold.
        None => {
            if request
                .get("iroh_ticket")
                .and_then(serde_json::Value::as_str)
                .is_some()
            {
                crate::cli::TransportKind::Iroh
            } else {
                crate::cli::TransportKind::Local
            }
        }
    };

    let iroh_wait_ms = request
        .get("iroh_wait_ms")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(5000);

    // `iroh_ticket_out: true` asks the selector to CREATE a fresh iroh
    // document (the CLI's --iroh-ticket-out) and write its ticket to a
    // server-side temp file; the sync handler reads it back into the response.
    let iroh_ticket_out = match request.get("iroh_ticket_out") {
        None => None,
        Some(value) => {
            let requested = value.as_bool().ok_or_else(|| {
                Error::new("request JSON field `iroh_ticket_out` must be a boolean")
            })?;
            if requested {
                let nonce: u64 = rand::random();
                let mut path = std::env::temp_dir();
                path.push(format!("ptj-webgui-iroh-ticket-out-{nonce:016x}"));
                Some(path)
            } else {
                None
            }
        }
    };

    // The iroh selector reads the ticket from a *file path* (commands/sync.rs),
    // but the webgui receives it as a *string* in the request body, so we
    // materialize it to a temp file the selector can read. (Only the Iroh arm
    // needs this; other transports ignore these fields. This keeps
    // `build_transport` the single selector rather than special-casing
    // string-vs-path here. TODO: an in-memory ticket variant on `SyncConfig`
    // would let the webgui skip the filesystem.)
    let iroh_ticket = match (
        transport,
        request.get("iroh_ticket").and_then(serde_json::Value::as_str),
    ) {
        (crate::cli::TransportKind::Iroh, Some(ticket)) => Some(write_ticket_tempfile(ticket)?),
        (crate::cli::TransportKind::Iroh, None) => {
            if iroh_ticket_out.is_none() {
                return Err(Error::new(
                    "iroh transport requires an `iroh_ticket` in the request or `iroh_ticket_out: true`",
                ));
            }
            None
        }
        _ => None,
    };

    // Server-side local sources: PSBT files or directories of .psbt files
    // (the CLI's positional sources) plus the `state` PSBT file — paths on
    // the machine running `ptj webgui` (an offline localhost GUI: the server
    // IS the user's machine), exactly like the signaling files below.
    let sources = match request.get("sources") {
        None => Vec::new(),
        Some(value) => value
            .as_array()
            .ok_or_else(|| Error::new("request JSON field `sources` must be an array"))?
            .iter()
            .enumerate()
            .map(|(index, value)| {
                value
                    .as_str()
                    .map(std::path::PathBuf::from)
                    .ok_or_else(|| {
                        Error::new(format!("request sources[{index}] must be a string"))
                    })
            })
            .collect::<Result<Vec<_>>>()?,
    };

    // WebRTC signaling/session params (str0m / webrtc-rs), mirroring the CLI
    // flags 1:1 — see `SyncConfig` for each field's meaning. All optional and
    // opaque pass-throughs; the shared selector validates presence per
    // transport and reports the missing flag. The signal files are paths on
    // the machine running `ptj webgui` (an offline localhost GUI: the server
    // IS the user's machine), exactly as the iroh ticket is a server-side
    // temp file above.
    let webrtc_role = match request.get("webrtc_role").and_then(serde_json::Value::as_str) {
        Some(name) => Some(
            crate::cli::WebrtcRoleArg::from_str(name, /* ignore_case */ true).map_err(|_| {
                Error::new(format!(
                    "unknown webrtc_role '{name}' (expected: offer, answer)"
                ))
            })?,
        ),
        None => None,
    };
    let signal_path = |field: &str| -> Result<Option<std::path::PathBuf>> {
        match request.get(field) {
            None => Ok(None),
            Some(value) => value
                .as_str()
                .map(|path| Some(std::path::PathBuf::from(path)))
                .ok_or_else(|| Error::new(format!("request JSON field `{field}` must be a string"))),
        }
    };
    let signal_out = signal_path("signal_out")?;
    let signal_in = signal_path("signal_in")?;
    // The `state` PSBT file rides the same optional-path parsing (it is read
    // as one more local source; this route never writes it).
    let state = signal_path("state")?;
    let webrtc_bind = match request.get("webrtc_bind") {
        None => "0.0.0.0:0".to_string(),
        Some(value) => value
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| Error::new("request JSON field `webrtc_bind` must be a string"))?,
    };
    let ice_servers = match request.get("ice_servers") {
        None => Vec::new(),
        Some(value) => value
            .as_array()
            .ok_or_else(|| Error::new("request JSON field `ice_servers` must be an array"))?
            .iter()
            .enumerate()
            .map(|(index, value)| {
                value.as_str().map(str::to_string).ok_or_else(|| {
                    Error::new(format!("request ice_servers[{index}] must be a string"))
                })
            })
            .collect::<Result<Vec<_>>>()?,
    };
    let signal_timeout_ms = request
        .get("signal_timeout_ms")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(60_000);

    Ok(crate::cli::SyncConfig {
        transport,
        state,
        iroh_ticket,
        iroh_ticket_out,
        iroh_wait_ms,
        webrtc_role,
        signal_out,
        signal_in,
        webrtc_bind,
        ice_servers,
        signal_timeout_ms,
        ongoing: false,
        poll_interval_ms: 1000,
        max_iterations: None,
        sources,
    })
}

/// Write a pasted iroh ticket to a private temp file so the shared selector
/// can read it as a path. The file is a short-lived runtime artifact in
/// `std::env::temp_dir()`; the OS reclaims it.
#[cfg(feature = "iroh-sync")]
fn write_ticket_tempfile(ticket: &str) -> Result<std::path::PathBuf> {
    use std::io::Write as _;
    let mut path = std::env::temp_dir();
    // A per-request unique name; the value is opaque and short-lived.
    let nonce: u64 = rand::random();
    path.push(format!("ptj-webgui-iroh-ticket-{nonce:016x}"));
    let mut file = std::fs::File::create(&path)
        .map_err(|error| Error::new(format!("writing iroh ticket temp file: {error}")))?;
    file.write_all(ticket.trim().as_bytes())
        .map_err(|error| Error::new(format!("writing iroh ticket temp file: {error}")))?;
    Ok(path)
}

// Feature-off: the Iroh arm of `build_transport` returns the rebuild error, but
// the inference path above still resolves a pasted ticket to `Iroh` and calls
// this helper first — so it reports the same clear rebuild error.
#[cfg(not(feature = "iroh-sync"))]
fn write_ticket_tempfile(_ticket: &str) -> Result<std::path::PathBuf> {
    Err(Error::new(
        "ptj webgui was built without iroh sync support; rebuild with feature `iroh-sync`",
    ))
}

/// Serialize a sync result: converged PSBT plus any out-of-band negotiation
/// messages (hex-encoded; payments and confirmations are opaque records).
/// `iroh_ticket_out` is the ticket of a document freshly created for this
/// request (`iroh_ticket_out: true`), echoed back so the browser can share it.
fn sync_json(
    joined: &psbt_v2::v2::Psbt,
    messages: &[Message],
    iroh_ticket_out: Option<&str>,
) -> Result<Vec<u8>> {
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let mut payments = Vec::new();
    let mut confirmations = Vec::new();
    for message in messages {
        match message {
            Message::Payment(value) => payments.push(hex(value)),
            Message::Confirmation(value) => confirmations.push(hex(value)),
            Message::Psbt(_) => {}
        }
    }
    let mut body = serde_json::json!({
        "psbt": crate::io::encode_psbt(joined),
        "inspect": crate::commands::inspect::inspect_psbt(joined),
        "payments": payments,
        "confirmations": confirmations,
    });
    if let Some(ticket) = iroh_ticket_out {
        body["iroh_ticket_out"] = serde_json::Value::String(ticket.to_owned());
    }
    Ok(body.to_string().into_bytes())
}

fn join_response(body: &[u8]) -> Response {
    match join_response_result(body) {
        Ok(body) => Response {
            status: 200,
            reason: "OK",
            content_type: "application/json; charset=utf-8",
            body,
        },
        Err(error) => json_error_response(400, "Bad Request", &error.to_string()),
    }
}

fn join_response_result(body: &[u8]) -> Result<Vec<u8>> {
    let request: serde_json::Value = serde_json::from_slice(body)
        .map_err(|error| Error::new(format!("parsing JSON request: {error}")))?;
    let psbts = request
        .get("psbts")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| Error::new("request JSON must contain array field `psbts`"))?;
    if psbts.is_empty() {
        return Err(Error::new("request JSON field `psbts` must not be empty"));
    }

    let psbts = psbts
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let psbt = value
                .as_str()
                .ok_or_else(|| Error::new(format!("request psbts[{index}] must be a string")))?;
            crate::io::parse_psbt_bytes(&format!("request psbts[{index}]"), psbt.as_bytes())
        })
        .collect::<Result<Vec<_>>>()?;
    let joined = crate::commands::join::join_psbts(psbts)?;
    Ok(serde_json::json!({
        "psbt": crate::io::encode_psbt(&joined),
        "inspect": crate::commands::inspect::inspect_psbt(&joined),
    })
    .to_string()
    .into_bytes())
}

fn concatenate_response(body: &[u8]) -> Response {
    match concatenate_response_result(body) {
        Ok(body) => Response {
            status: 200,
            reason: "OK",
            content_type: "application/json; charset=utf-8",
            body,
        },
        Err(error) => json_error_response(400, "Bad Request", &error.to_string()),
    }
}

fn concatenate_response_result(body: &[u8]) -> Result<Vec<u8>> {
    let request: serde_json::Value = serde_json::from_slice(body)
        .map_err(|error| Error::new(format!("parsing JSON request: {error}")))?;
    let psbts = request
        .get("psbts")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| Error::new("request JSON must contain array field `psbts`"))?;
    let psbts = psbts
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let psbt = value
                .as_str()
                .ok_or_else(|| Error::new(format!("request psbts[{index}] must be a string")))?;
            crate::io::parse_psbt_bytes(&format!("request psbts[{index}]"), psbt.as_bytes())
                .map(|psbt| (format!("request psbts[{index}]"), psbt))
        })
        .collect::<Result<Vec<_>>>()?;
    let concatenated = crate::commands::concatenate::concatenate_psbts(psbts)?;
    Ok(serde_json::json!({
        "psbt": crate::io::encode_psbt(&concatenated),
        "inspect": crate::commands::inspect::inspect_psbt(&concatenated),
    })
    .to_string()
    .into_bytes())
}

fn classify_response(body: &[u8]) -> Response {
    match classify_response_result(body) {
        Ok(body) => Response {
            status: 200,
            reason: "OK",
            content_type: "application/json; charset=utf-8",
            body,
        },
        Err(error) => json_error_response(400, "Bad Request", &error.to_string()),
    }
}

/// `/api/classify`: universal paste ingestion. Request `{payload, network?}`
/// (network is the `/api/create` selector, default bitcoin); the response is
/// `{kind, ...details}` from `crate::commands::classify` — descriptors,
/// BIP 21/321 payment instructions (incl. bare addresses and BOLT 11/12),
/// npub peer ids, and raw signed transactions. PSBT pastes are redirected to
/// the existing PSBT routes by the error text.
fn classify_response_result(body: &[u8]) -> Result<Vec<u8>> {
    let request: serde_json::Value = serde_json::from_slice(body)
        .map_err(|error| Error::new(format!("parsing JSON request: {error}")))?;
    let payload = request_string(&request, "payload")?;
    let network = match request.get("network") {
        Some(value) => {
            let value = value
                .as_str()
                .ok_or_else(|| Error::new("request JSON field `network` must be a string"))?;
            NetworkArg::from_str(value).map_err(Error::new)?
        }
        None => NetworkArg(bitcoin::Network::Bitcoin),
    };
    Ok(crate::commands::classify::classify(payload, network.0)?
        .to_string()
        .into_bytes())
}

fn assign_ids_response(body: &[u8]) -> Response {
    match assign_ids_response_result(body) {
        Ok(body) => Response {
            status: 200,
            reason: "OK",
            content_type: "application/json; charset=utf-8",
            body,
        },
        Err(error) => json_error_response(400, "Bad Request", &error.to_string()),
    }
}

fn assign_ids_response_result(body: &[u8]) -> Result<Vec<u8>> {
    let request: serde_json::Value = serde_json::from_slice(body)
        .map_err(|error| Error::new(format!("parsing JSON request: {error}")))?;
    let psbt = request
        .get("psbt")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::new("request JSON must contain string field `psbt`"))?;
    let psbt = crate::io::parse_psbt_bytes("request psbt", psbt.as_bytes())?;
    let ids = parse_id_assignments(&request)?;
    // CLI parity: no directives means auto-assign; `auto` also combines with
    // manual directives to fill the remainder.
    let auto = optional_bool(&request, "auto")? || ids.is_empty();
    let overwrite = optional_bool(&request, "overwrite")?;
    let assigned = crate::commands::assign_ids::assign_ids_psbt(psbt, &ids, auto, overwrite)?;
    Ok(serde_json::json!({
        "psbt": crate::io::encode_psbt(&assigned),
        "inspect": crate::commands::inspect::inspect_psbt(&assigned),
    })
    .to_string()
    .into_bytes())
}

/// Parse the optional `ids` array: `[{"target":"in"|"out","index":n,"id":"<bytes>"}]`
/// (id bytes accept hex/base58/bech32, like the CLI's --id values).
fn parse_id_assignments(request: &serde_json::Value) -> Result<Vec<crate::cli::IdAssignment>> {
    let Some(value) = request.get("ids") else {
        return Ok(vec![]);
    };
    let items = value
        .as_array()
        .ok_or_else(|| Error::new("request JSON field `ids` must be an array"))?;
    items
        .iter()
        .enumerate()
        .map(|(position, item)| {
            let object = item.as_object().ok_or_else(|| {
                Error::new(format!("request JSON ids[{position}] must be an object"))
            })?;
            let target = match object_string(object, "target", &format!("ids[{position}]"))? {
                "in" | "input" => crate::cli::IdTarget::Input,
                "out" | "output" => crate::cli::IdTarget::Output,
                other => {
                    return Err(Error::new(format!(
                        "request JSON ids[{position}].target must be `in` or `out`, got {other}"
                    )));
                }
            };
            let index = object
                .get("index")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| {
                    Error::new(format!(
                        "request JSON ids[{position}].index must be a non-negative integer"
                    ))
                })
                .and_then(|index| {
                    usize::try_from(index).map_err(|_| {
                        Error::new(format!("request JSON ids[{position}].index exceeds usize"))
                    })
                })?;
            let id = object_string(object, "id", &format!("ids[{position}]"))?;
            let id = crate::bytes_arg::parse_bytes_arg(id).map_err(Error::new)?;
            Ok(crate::cli::IdAssignment { target, index, id })
        })
        .collect()
}

fn edit_response(body: &[u8]) -> Response {
    match edit_response_result(body) {
        Ok(response) => response,
        Err(error) => json_error_response(400, "Bad Request", &error.to_string()),
    }
}

/// `/api/edit`: field-level raw-keymap editing with save-time validation and
/// structured fix offers (`crate::commands::field_edit`). Grow-only: edits
/// mint a NEW fragment; the submitted PSBT is never mutated in place.
///
/// Request: `{psbt, edits: [{map, key, value|null}], apply_fixes?: [fix_id],
/// <override_param>?: bool}` — `map` selects `global` / `input:<i>` /
/// `output:<i>`, `key` is the full raw key (`inspect`'s `raw.*[].key_hex`;
/// hex/base58/bech32 accepted), `value` sets bytes or deletes on `null`, and
/// `edits: []` is a pure validation pass. Violations return 400 with
/// `violations[]` (each `{id, message, override_param}` plus flat `fix_id` /
/// `fix_label` / `warning_text` when a server-side fix is offered); every
/// gate is waived by its named `override_param` boolean, and requested
/// `apply_fixes` run before validation with their caveats echoed in
/// `applied_fixes[].warning_text`.
fn edit_response_result(body: &[u8]) -> Result<Response> {
    let request: serde_json::Value = serde_json::from_slice(body)
        .map_err(|error| Error::new(format!("parsing JSON request: {error}")))?;
    let psbt = request_string(&request, "psbt")?;
    let psbt = crate::io::parse_psbt_bytes("request psbt", psbt.as_bytes())?;
    let edits = parse_field_edits(&request)?;

    let mut edited = crate::commands::field_edit::apply_edits(&psbt, &edits)?;

    let mut applied_fixes = Vec::new();
    if let Some(fixes) = request.get("apply_fixes") {
        let fixes = fixes
            .as_array()
            .ok_or_else(|| Error::new("request JSON field `apply_fixes` must be an array"))?;
        for (position, fix) in fixes.iter().enumerate() {
            let fix_id = fix.as_str().ok_or_else(|| {
                Error::new(format!("request apply_fixes[{position}] must be a string"))
            })?;
            edited = crate::commands::field_edit::apply_fix(edited, fix_id)?;
            let mut applied = serde_json::json!({ "fix_id": fix_id });
            if let Some(warning) = crate::commands::field_edit::fix_warning(fix_id) {
                applied["warning_text"] = serde_json::Value::String(warning.to_owned());
            }
            applied_fixes.push(applied);
        }
    }

    let mut remaining = Vec::new();
    let mut overridden = Vec::new();
    for violation in crate::commands::field_edit::validate(&edited) {
        if optional_bool(&request, violation.override_param)? {
            overridden.push(violation_json(&violation));
        } else {
            remaining.push(violation_json(&violation));
        }
    }

    if !remaining.is_empty() {
        let count = remaining.len();
        return Ok(Response {
            status: 400,
            reason: "Bad Request",
            content_type: "application/json; charset=utf-8",
            body: serde_json::json!({
                "error": format!(
                    "save-time validation failed ({count} violation{}); apply an offered \
                     fix, set the named override, or amend the edits",
                    if count == 1 { "" } else { "s" },
                ),
                "violations": remaining,
            })
            .to_string()
            .into_bytes(),
        });
    }

    Ok(Response {
        status: 200,
        reason: "OK",
        content_type: "application/json; charset=utf-8",
        body: serde_json::json!({
            "psbt": crate::io::encode_psbt(&edited),
            "inspect": crate::commands::inspect::inspect_psbt(&edited),
            "violations": [],
            "overridden": overridden,
            "applied_fixes": applied_fixes,
        })
        .to_string()
        .into_bytes(),
    })
}

/// Parse the required `edits` array:
/// `[{"map": "global"|"input:<i>"|"output:<i>", "key": "<bytes>",
/// "value": "<bytes>"|null}]` (key/value bytes accept hex/base58/bech32,
/// like the CLI's byte arguments).
fn parse_field_edits(
    request: &serde_json::Value,
) -> Result<Vec<crate::commands::field_edit::FieldEdit>> {
    let edits = request
        .get("edits")
        .ok_or_else(|| Error::new("request JSON must contain array field `edits`"))?
        .as_array()
        .ok_or_else(|| Error::new("request JSON field `edits` must be an array"))?;
    edits
        .iter()
        .enumerate()
        .map(|(position, item)| {
            let object = item.as_object().ok_or_else(|| {
                Error::new(format!("request JSON edits[{position}] must be an object"))
            })?;
            let map = object_string(object, "map", &format!("edits[{position}]"))?;
            let map = crate::commands::field_edit::MapTarget::parse(map)
                .map_err(|error| Error::new(format!("request edits[{position}]: {error}")))?;
            let key = object_string(object, "key", &format!("edits[{position}]"))?;
            let key = crate::bytes_arg::parse_bytes_arg(key)
                .map_err(|error| Error::new(format!("request edits[{position}].key: {error}")))?;
            let value = match object.get("value") {
                None => {
                    return Err(Error::new(format!(
                        "request edits[{position}] must contain `value` (bytes to set) or \
                         `value: null` (delete the entry)"
                    )));
                }
                Some(serde_json::Value::Null) => None,
                Some(value) => {
                    let value = value.as_str().ok_or_else(|| {
                        Error::new(format!(
                            "request edits[{position}].value must be a string or null"
                        ))
                    })?;
                    Some(crate::bytes_arg::parse_bytes_arg(value).map_err(|error| {
                        Error::new(format!("request edits[{position}].value: {error}"))
                    })?)
                }
            };
            Ok(crate::commands::field_edit::FieldEdit { map, key, value })
        })
        .collect()
}

/// Serialize a save-time violation, flattening the fix offer per the seam
/// contract: `{id, message, override_param}` plus `fix_id` / `fix_label` /
/// `warning_text` when a fix is offered.
fn violation_json(violation: &crate::commands::field_edit::Violation) -> serde_json::Value {
    let mut value = serde_json::json!({
        "id": violation.id,
        "message": violation.message,
        "override_param": violation.override_param,
    });
    if let Some(fix) = &violation.fix {
        value["fix_id"] = serde_json::Value::String(fix.fix_id.to_owned());
        value["fix_label"] = serde_json::Value::String(fix.fix_label.to_owned());
        value["warning_text"] = serde_json::Value::String(fix.warning_text.to_owned());
    }
    value
}

fn create_response(body: &[u8]) -> Response {
    match create_response_result(body) {
        Ok(body) => Response {
            status: 200,
            reason: "OK",
            content_type: "application/json; charset=utf-8",
            body,
        },
        Err(error) => json_error_response(400, "Bad Request", &error.to_string()),
    }
}

fn create_response_result(body: &[u8]) -> Result<Vec<u8>> {
    let request: serde_json::Value = serde_json::from_slice(body)
        .map_err(|error| Error::new(format!("parsing JSON request: {error}")))?;
    let config = create_config_from_request(&request)?;
    let created = crate::commands::create::create_psbt(config)?;
    Ok(serde_json::json!({
        "psbt": crate::io::encode_psbt(&created),
        "inspect": crate::commands::inspect::inspect_psbt(&created),
    })
    .to_string()
    .into_bytes())
}

fn create_config_from_request(request: &serde_json::Value) -> Result<CreateConfig> {
    let object = request
        .as_object()
        .ok_or_else(|| Error::new("request JSON must be an object"))?;
    let network = match object.get("network") {
        Some(value) => {
            let value = value
                .as_str()
                .ok_or_else(|| Error::new("request JSON field `network` must be a string"))?;
            NetworkArg::from_str(value).map_err(Error::new)?
        }
        None => NetworkArg(bitcoin::Network::Bitcoin),
    };
    let seed = object
        .get("seed_hex")
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| Error::new("request JSON field `seed_hex` must be a string"))
                .and_then(|seed| crate::cli::HexSeed::from_str(seed).map_err(Error::new))
        })
        .transpose()?;
    let ordering = match object.get("ordering") {
        Some(value) => {
            let value = value
                .as_str()
                .ok_or_else(|| Error::new("request JSON field `ordering` must be a string"))?;
            OrderingArg::from_str(value).map_err(Error::new)?
        }
        None => OrderingArg::Unset,
    };
    let inputs = object
        .get("inputs")
        .map(parse_create_inputs)
        .transpose()?
        .unwrap_or_default();
    let outputs = object
        .get("outputs")
        .map(parse_create_outputs)
        .transpose()?
        .unwrap_or_default();
    let allow_short_seed = optional_bool(request, "allow_short_seed")?;

    Ok(CreateConfig {
        inputs,
        outputs,
        seed,
        allow_short_seed,
        ordering,
        network,
    })
}

fn parse_create_inputs(value: &serde_json::Value) -> Result<Vec<OutPointArg>> {
    let inputs = value
        .as_array()
        .ok_or_else(|| Error::new("request JSON field `inputs` must be an array"))?;
    inputs
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let object = value.as_object().ok_or_else(|| {
                Error::new(format!("request JSON inputs[{index}] must be an object"))
            })?;
            let txid = object_string(object, "txid", &format!("inputs[{index}]"))?;
            let vout = object
                .get("vout")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| {
                    Error::new(format!(
                        "request JSON inputs[{index}].vout must be a non-negative integer"
                    ))
                })
                .and_then(|vout| {
                    u32::try_from(vout).map_err(|_| {
                        Error::new(format!("request JSON inputs[{index}].vout exceeds u32"))
                    })
                })?;
            Ok(OutPointArg {
                txid: txid
                    .parse()
                    .map_err(|error| Error::new(format!("invalid txid {txid}: {error}")))?,
                vout,
            })
        })
        .collect()
}

fn parse_create_outputs(value: &serde_json::Value) -> Result<Vec<OutputArg>> {
    let outputs = value
        .as_array()
        .ok_or_else(|| Error::new("request JSON field `outputs` must be an array"))?;
    outputs
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let object = value.as_object().ok_or_else(|| {
                Error::new(format!("request JSON outputs[{index}] must be an object"))
            })?;
            let address_text = object_string(object, "address", &format!("outputs[{index}]"))?;
            let amount_text = object_string(object, "amount_btc", &format!("outputs[{index}]"))?;
            Ok(OutputArg {
                address_text: address_text.to_owned(),
                address: address_text.parse().map_err(|error| {
                    Error::new(format!("invalid address {address_text}: {error}"))
                })?,
                amount: bitcoin::Amount::from_str_in(amount_text, bitcoin::Denomination::Bitcoin)
                    .map_err(|error| {
                    Error::new(format!("invalid amount {amount_text}: {error}"))
                })?,
            })
        })
        .collect()
}

fn object_string<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    field: &str,
    label: &str,
) -> Result<&'a str> {
    object
        .get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::new(format!("request JSON {label}.{field} must be a string")))
}

fn export_bip174_response(body: &[u8]) -> Response {
    match export_bip174_response_result(body) {
        Ok(body) => Response {
            status: 200,
            reason: "OK",
            content_type: "application/json; charset=utf-8",
            body,
        },
        Err(error) => json_error_response(400, "Bad Request", &error.to_string()),
    }
}

fn export_bip174_response_result(body: &[u8]) -> Result<Vec<u8>> {
    let request: serde_json::Value = serde_json::from_slice(body)
        .map_err(|error| Error::new(format!("parsing JSON request: {error}")))?;
    let psbt = request
        .get("psbt")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::new("request JSON must contain string field `psbt`"))?;
    let psbt = crate::io::parse_psbt_bytes("request psbt", psbt.as_bytes())?;
    let exported = crate::commands::export_bip174::export_bip174_psbt(psbt)?;
    Ok(serde_json::json!({
        "format": "bip174",
        "psbt": exported,
    })
    .to_string()
    .into_bytes())
}

fn import_bip174_response(body: &[u8]) -> Response {
    match import_bip174_response_result(body) {
        Ok(body) => Response {
            status: 200,
            reason: "OK",
            content_type: "application/json; charset=utf-8",
            body,
        },
        Err(error) => json_error_response(400, "Bad Request", &error.to_string()),
    }
}

fn import_bip174_response_result(body: &[u8]) -> Result<Vec<u8>> {
    let request: serde_json::Value = serde_json::from_slice(body)
        .map_err(|error| Error::new(format!("parsing JSON request: {error}")))?;
    let psbt = request
        .get("psbt")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::new("request JSON must contain string field `psbt`"))?;
    let modifiable = optional_bool(&request, "modifiable")?;
    let psbt = crate::io::parse_bip174_bytes("request psbt", psbt.as_bytes())?;
    let imported = crate::commands::import_bip174::import_bip174_psbt(psbt, modifiable)?;
    Ok(serde_json::json!({
        "psbt": crate::io::encode_psbt(&imported),
        "inspect": crate::commands::inspect::inspect_psbt(&imported),
    })
    .to_string()
    .into_bytes())
}

fn sort_response(body: &[u8]) -> Response {
    match sort_response_result(body) {
        Ok(body) => Response {
            status: 200,
            reason: "OK",
            content_type: "application/json; charset=utf-8",
            body,
        },
        Err(error) => json_error_response(400, "Bad Request", &error.to_string()),
    }
}

fn sort_response_result(body: &[u8]) -> Result<Vec<u8>> {
    let request: serde_json::Value = serde_json::from_slice(body)
        .map_err(|error| Error::new(format!("parsing JSON request: {error}")))?;
    let psbt = request
        .get("psbt")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::new("request JSON must contain string field `psbt`"))?;
    let seed = request
        .get("seed_hex")
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| Error::new("request JSON field `seed_hex` must be a string"))
                .and_then(|seed| crate::cli::HexSeed::from_str(seed).map_err(Error::new))
                .map(crate::cli::HexSeed::into_bytes)
        })
        .transpose()?;
    let allow_short_seed = optional_bool(&request, "allow_short_seed")?;
    let psbt = crate::io::parse_psbt_bytes("request psbt", psbt.as_bytes())?;
    let constructor =
        concurrent_psbt::roles::constructor::dynamic::Constructor::try_from_psbt(psbt)
            .map_err(|error| Error::new(format!("request psbt: {error}")))?;
    let sorted = crate::commands::sort::sort_psbt(constructor.into_inner(), seed, allow_short_seed)?;
    Ok(serde_json::json!({
        "psbt": crate::io::encode_psbt(&sorted),
        "inspect": crate::commands::inspect::inspect_psbt(&sorted),
    })
    .to_string()
    .into_bytes())
}

fn make_unordered_response(body: &[u8]) -> Response {
    match make_unordered_response_result(body) {
        Ok(body) => Response {
            status: 200,
            reason: "OK",
            content_type: "application/json; charset=utf-8",
            body,
        },
        Err(error) => json_error_response(400, "Bad Request", &error.to_string()),
    }
}

fn make_unordered_response_result(body: &[u8]) -> Result<Vec<u8>> {
    let request: serde_json::Value = serde_json::from_slice(body)
        .map_err(|error| Error::new(format!("parsing JSON request: {error}")))?;
    let psbt = request
        .get("psbt")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::new("request JSON must contain string field `psbt`"))?;
    let psbt = crate::io::parse_psbt_bytes("request psbt", psbt.as_bytes())?;
    let unordered = crate::commands::make_unordered::make_unordered_psbt(psbt)?;
    Ok(serde_json::json!({
        "psbt": crate::io::encode_psbt(&unordered),
        "inspect": crate::commands::inspect::inspect_psbt(&unordered),
    })
    .to_string()
    .into_bytes())
}

// --- negotiation band: /api/{pay,confirm,payments} --------------------------
//
// Mechanism-only, mirroring the concurrent-psbt-wasm surface so the shared
// frontend cannot tell HttpBackend and WasmBackend apart: `payment_hex` /
// `confirmation_hex` are OPAQUE record bytes the frontend builds; the server
// appends them to the grow-only negotiation band (`secret_hex` opt-in enables
// the deterministic AEAD from commands/negotiation.rs) and `payments` decodes
// the band back to opaque hex blobs.

fn pay_response(body: &[u8]) -> Response {
    match pay_response_result(body) {
        Ok(body) => Response {
            status: 200,
            reason: "OK",
            content_type: "application/json; charset=utf-8",
            body,
        },
        Err(error) => json_error_response(400, "Bad Request", &error.to_string()),
    }
}

fn pay_response_result(body: &[u8]) -> Result<Vec<u8>> {
    let request: serde_json::Value = serde_json::from_slice(body)
        .map_err(|error| Error::new(format!("parsing JSON request: {error}")))?;
    let psbt = request_string(&request, "psbt")?;
    let mut psbt = crate::io::parse_psbt_bytes("request psbt", psbt.as_bytes())?;
    // Two request variants: an OPAQUE pre-built record (`payment_hex`, the
    // wasm-parity mechanism), or the address variant (`address` +
    // `amount_btc`), where THIS route builds the txout-shaped record with the
    // same network validation as `ptj pay --to` — the frontend never parses
    // addresses.
    let record = match request.get("payment_hex") {
        Some(_) => crate::cli::HexSeed::from_str(request_string(&request, "payment_hex")?)
            .map_err(Error::new)?
            .into_bytes(),
        None => payment_record_from_request(&request)?,
    };
    let secret = optional_hex_field(&request, "secret_hex")?;
    let dummy = match request.get("dummy") {
        None => 0,
        Some(value) => value.as_u64().ok_or_else(|| {
            Error::new("request JSON field `dummy` must be a non-negative integer")
        })?,
    };
    if dummy > 0 && secret.is_none() {
        return Err(Error::new(
            "dummy padding requires secret_hex; plaintext dummies are trivially distinguishable",
        ));
    }
    crate::commands::negotiation::add_opaque_payment(&mut psbt, &record, secret.as_deref())?;
    for _ in 0..dummy {
        let record = crate::commands::negotiation::random_dummy_payment().encode();
        crate::commands::negotiation::add_opaque_payment(&mut psbt, &record, secret.as_deref())?;
    }
    Ok(serde_json::json!({
        "psbt": crate::io::encode_psbt(&psbt),
        "inspect": crate::commands::inspect::inspect_psbt(&psbt),
    })
    .to_string()
    .into_bytes())
}

fn confirm_response(body: &[u8]) -> Response {
    match confirm_response_result(body) {
        Ok(body) => Response {
            status: 200,
            reason: "OK",
            content_type: "application/json; charset=utf-8",
            body,
        },
        Err(error) => json_error_response(400, "Bad Request", &error.to_string()),
    }
}

fn confirm_response_result(body: &[u8]) -> Result<Vec<u8>> {
    let request: serde_json::Value = serde_json::from_slice(body)
        .map_err(|error| Error::new(format!("parsing JSON request: {error}")))?;
    let psbt = request_string(&request, "psbt")?;
    let mut psbt = crate::io::parse_psbt_bytes("request psbt", psbt.as_bytes())?;
    let secret = optional_hex_field(&request, "secret_hex")?;
    // Two request variants: an OPAQUE pre-built record (`confirmation_hex`,
    // the wasm-parity mechanism), or `derive: true`, where THIS route derives
    // a confirmation of the submitted PSBT's current unordered unique id via
    // the same builder as `ptj confirm` (optional `peer_id_hex` = the CLI's
    // --peer-id; defaults to the unspecified/zero id).
    match request.get("confirmation_hex") {
        Some(_) => {
            let record =
                crate::cli::HexSeed::from_str(request_string(&request, "confirmation_hex")?)
                    .map_err(Error::new)?
                    .into_bytes();
            crate::commands::negotiation::add_opaque_confirmation(
                &mut psbt,
                &record,
                secret.as_deref(),
            )?;
        }
        None if request.get("derive").and_then(serde_json::Value::as_bool) == Some(true) => {
            let peer_id = optional_hex32_field(&request, "peer_id_hex")?.unwrap_or([0u8; 32]);
            crate::commands::negotiation::add_derived_confirmation(
                &mut psbt,
                peer_id,
                secret.as_deref(),
            )?;
        }
        None => {
            return Err(Error::new(
                "request JSON must contain a `confirmation_hex` record or `derive: true`",
            ));
        }
    }
    Ok(serde_json::json!({
        "psbt": crate::io::encode_psbt(&psbt),
        "inspect": crate::commands::inspect::inspect_psbt(&psbt),
    })
    .to_string()
    .into_bytes())
}

fn payments_response(body: &[u8]) -> Response {
    match payments_response_result(body) {
        Ok(body) => Response {
            status: 200,
            reason: "OK",
            content_type: "application/json; charset=utf-8",
            body,
        },
        Err(error) => json_error_response(400, "Bad Request", &error.to_string()),
    }
}

fn payments_response_result(body: &[u8]) -> Result<Vec<u8>> {
    let request: serde_json::Value = serde_json::from_slice(body)
        .map_err(|error| Error::new(format!("parsing JSON request: {error}")))?;
    let psbt = request_string(&request, "psbt")?;
    let psbt = crate::io::parse_psbt_bytes("request psbt", psbt.as_bytes())?;
    let secret = optional_hex_field(&request, "secret_hex")?;
    let (payments, confirmations) =
        crate::commands::negotiation::decode_band(&psbt, secret.as_deref())?;
    Ok(serde_json::json!({
        "payments": payments,
        "confirmations": confirmations,
    })
    .to_string()
    .into_bytes())
}

/// Build a real payment record from the `/api/pay` address variant:
/// `address` + `amount_btc` (BTC denomination string, like create's outputs),
/// optional `network` (same selector as `/api/create`; defaults to bitcoin
/// like `ptj pay`), optional `label`, and optional `payer_hex` — an OPAQUE
/// 32-byte hex id stored in the record unchanged (defaults to the
/// unspecified/zero id, mirroring `ptj pay` without `--payer`).
fn payment_record_from_request(request: &serde_json::Value) -> Result<Vec<u8>> {
    let address = request
        .get("address")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            Error::new(
                "request JSON must contain a `payment_hex` record or an `address` + `amount_btc` pair",
            )
        })?;
    let amount_btc = request_string(request, "amount_btc")?;
    let network = match request.get("network") {
        Some(value) => {
            let value = value
                .as_str()
                .ok_or_else(|| Error::new("request JSON field `network` must be a string"))?;
            NetworkArg::from_str(value).map_err(Error::new)?
        }
        None => NetworkArg(bitcoin::Network::Bitcoin),
    };
    let label = match request.get("label") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or_else(|| Error::new("request JSON field `label` must be a string"))?,
        ),
    };
    let payer = optional_hex32_field(request, "payer_hex")?;
    Ok(
        crate::commands::negotiation::payment_from_parts(address, amount_btc, network, label, payer)?
            .encode(),
    )
}

/// Read a required string field from a request object.
fn request_string<'a>(request: &'a serde_json::Value, field: &str) -> Result<&'a str> {
    request
        .get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::new(format!("request JSON must contain string field `{field}`")))
}

/// Read an optional boolean field (`allow_short_seed`, ...); absent or null
/// means `false`.
fn optional_bool(request: &serde_json::Value, field: &str) -> Result<bool> {
    match request.get(field) {
        None | Some(serde_json::Value::Null) => Ok(false),
        Some(value) => value.as_bool().ok_or_else(|| {
            Error::new(format!("request JSON field `{field}` must be a boolean"))
        }),
    }
}

/// Read an optional hex-string field (`secret_hex`, ...) to raw bytes, with
/// the same error text as the CLI's hex arguments.
fn optional_hex_field(request: &serde_json::Value, field: &str) -> Result<Option<Vec<u8>>> {
    request
        .get(field)
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| Error::new(format!("request JSON field `{field}` must be a string")))
                .and_then(|hex| crate::cli::HexSeed::from_str(hex).map_err(Error::new))
                .map(crate::cli::HexSeed::into_bytes)
        })
        .transpose()
}

/// Read an optional 32-byte hex field (`payer_hex`, `peer_id_hex`) with the
/// CLI's `Hex32` parsing and error text.
fn optional_hex32_field(request: &serde_json::Value, field: &str) -> Result<Option<[u8; 32]>> {
    request
        .get(field)
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| Error::new(format!("request JSON field `{field}` must be a string")))
                .and_then(|hex| crate::cli::Hex32::from_str(hex).map_err(Error::new))
                .map(crate::cli::Hex32::into_array)
        })
        .transpose()
}

/// `GET /api/lifehash/<hex-digest>`: the LifeHash fingerprint of the digest
/// as a PNG (`crate::commands::lifehash` — Version2, 32x32 RGB, the frozen
/// digest→image mapping the later wasm export must reproduce). The digest
/// path segment rides the liberal bytes_arg parsing (hex canonical); 32
/// bytes render as a digest, other lengths as data. Errors are the usual
/// JSON `{error}` with status 400.
fn lifehash_response(digest: &str) -> Response {
    match lifehash_response_result(digest) {
        Ok(body) => Response {
            status: 200,
            reason: "OK",
            content_type: "image/png",
            body,
        },
        Err(error) => json_error_response(400, "Bad Request", &error.to_string()),
    }
}

fn lifehash_response_result(digest: &str) -> Result<Vec<u8>> {
    let input = crate::bytes_arg::parse_bytes_arg(digest)
        .map_err(|error| Error::new(format!("lifehash digest: {error}")))?;
    crate::commands::lifehash::png_for_input(&input)
}

fn text_response(status: u16, reason: &'static str, body: &'static str) -> Response {
    Response {
        status,
        reason,
        content_type: "text/plain; charset=utf-8",
        body: body.as_bytes().to_vec(),
    }
}

fn json_error_response(status: u16, reason: &'static str, error: &str) -> Response {
    Response {
        status,
        reason,
        content_type: "application/json; charset=utf-8",
        body: serde_json::json!({ "error": error })
            .to_string()
            .into_bytes(),
    }
}

fn read_http_request(stream: &mut TcpStream) -> Result<Vec<u8>> {
    let mut request = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let read = stream
            .read(&mut buffer)
            .map_err(|error| Error::new(format!("reading HTTP request: {error}")))?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
        let Some(header_end) = find_header_end(&request) else {
            continue;
        };
        let headers = std::str::from_utf8(&request[..header_end])
            .map_err(|error| Error::new(format!("HTTP request was not UTF-8: {error}")))?;
        let body_start = header_end + b"\r\n\r\n".len();
        let expected_len = body_start + content_length(headers)?;
        if request.len() >= expected_len {
            break;
        }
    }
    Ok(request)
}

fn find_header_end(request: &[u8]) -> Option<usize> {
    request
        .windows(b"\r\n\r\n".len())
        .position(|window| window == b"\r\n\r\n")
}

fn content_length(headers: &str) -> Result<usize> {
    headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>())
        })
        .transpose()
        .map_err(|error| Error::new(format!("invalid Content-Length: {error}")))
        .map(|length| length.unwrap_or(0))
}

fn write_http_response(stream: &mut TcpStream, response: &Response) -> Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: {cache_control}\r\nConnection: close\r\n\r\n",
        response.body.len(),
        status = response.status,
        reason = response.reason,
        content_type = response.content_type,
        cache_control = response.cache_control(),
    )
    .map_err(|error| Error::new(format!("writing HTTP headers: {error}")))?;
    stream
        .write_all(&response.body)
        .map_err(|error| Error::new(format!("writing HTTP body: {error}")))
}

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};

    use clap::Parser as _;
    use concurrent_psbt::global::GlobalSortExt as _;

    use super::*;
    use crate::cli::Cli;

    const TXID: &str = "0000000000000000000000000000000000000000000000000000000000000001";
    /// A 16-byte (spec-minimum) ordering seed for fixtures and requests.
    const SEED_HEX: &str = "abcdabcdabcdabcdabcdabcdabcdabcd";

    fn seed_bytes() -> Vec<u8> {
        [0xab, 0xcd].repeat(8)
    }

    #[test]
    fn inspect_endpoint_parses_real_psbt_bytes() {
        let request = serde_json::json!({ "psbt": encoded_psbt() }).to_string();

        let response = response_for("POST", "/api/inspect", request.as_bytes());

        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "application/json; charset=utf-8");
        let inspected: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(inspected["format"], "bip370");
        assert_eq!(inspected["ordering"], "unordered");
        assert_eq!(inspected["input_count"], 1);
        assert_eq!(inspected["output_count"], 1);
        assert_eq!(inspected["sort"]["mode"], "unset");
        assert_eq!(inspected["sort"]["seed_hex"], SEED_HEX);
    }

    #[test]
    fn inspect_endpoint_exposes_raw_keymap_entries() {
        let request = serde_json::json!({ "psbt": encoded_psbt() }).to_string();

        let response = response_for("POST", "/api/inspect", request.as_bytes());

        assert_eq!(response.status, 200);
        let inspected: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        let raw = &inspected["raw"];
        assert!(raw["global"].as_array().unwrap().len() >= 4, "{raw}");
        assert_eq!(raw["inputs"].as_array().unwrap().len(), 1);
        assert_eq!(raw["outputs"].as_array().unwrap().len(), 1);

        // Every entry carries the raw handle plus its classification.
        for entry in raw["global"].as_array().unwrap() {
            assert!(entry["key_hex"].is_string(), "{entry}");
            assert!(entry["value_hex"].is_string(), "{entry}");
            assert!(entry["key_type"].is_u64(), "{entry}");
            assert!(
                matches!(
                    entry["kind"].as_str(),
                    Some("known" | "unknown" | "proprietary")
                ),
                "{entry}"
            );
        }

        // The fixture's output unique id is a proprietary entry with the
        // BIP 174 envelope broken out.
        let output_entries = raw["outputs"][0].as_array().unwrap();
        let proprietary = output_entries
            .iter()
            .find(|entry| entry["kind"] == "proprietary")
            .expect("output unique id must appear as a proprietary raw entry");
        assert_eq!(proprietary["key_type"], 0xFC);
        assert!(proprietary["proprietary"]["prefix_hex"].is_string());

        // Typed fields appear as `known` raw entries (e.g. the output amount).
        assert!(
            output_entries.iter().any(|entry| entry["kind"] == "known"),
            "{output_entries:?}"
        );
    }

    #[test]
    fn response_for_preserves_static_asset_http_behavior() {
        // "/" is the REAL session UI; the demo sandbox is explicit at /demo
        // (and its historical /index.html name).
        let index = response_for("GET", "/", b"");
        assert_eq!(index.status, 200);
        assert_eq!(index.content_type, "text/html; charset=utf-8");
        assert!(
            String::from_utf8(index.body)
                .unwrap()
                .contains("dist/session/app.js")
        );

        for path in ["/demo", "/demo?from=header", "/index.html"] {
            let demo = response_for("GET", path, b"");
            assert_eq!(demo.status, 200, "{path}");
            assert_eq!(demo.content_type, "text/html; charset=utf-8", "{path}");
            assert!(
                String::from_utf8(demo.body).unwrap().contains("dist/app.js"),
                "{path} must serve the demo shell"
            );
        }

        let session_app = response_for("GET", "/dist/session/app.js?v=cache-busted", b"");
        assert_eq!(session_app.status, 200);
        assert_eq!(session_app.content_type, "text/javascript; charset=utf-8");
        assert!(
            String::from_utf8(session_app.body)
                .unwrap()
                .contains("shared-frontend/backends/http.js")
        );

        let session_state = response_for("GET", "/dist/session/state.js?v=cache-busted", b"");
        assert_eq!(session_state.status, 200);
        assert_eq!(session_state.content_type, "text/javascript; charset=utf-8");
        assert!(
            String::from_utf8(session_state.body)
                .unwrap()
                .contains("buildSyncRequest")
        );

        let app = response_for("GET", "/dist/app.js?v=cache-busted", b"");
        assert_eq!(app.status, 200);
        assert_eq!(app.content_type, "text/javascript; charset=utf-8");
        assert!(
            String::from_utf8(app.body)
                .unwrap()
                .contains("shared-frontend/backends/http.js")
        );

        let backend = response_for("GET", "/dist/backend.js?v=cache-busted", b"");
        assert_eq!(backend.status, 200);
        assert_eq!(backend.content_type, "text/javascript; charset=utf-8");
        assert!(
            String::from_utf8(backend.body)
                .unwrap()
                .contains("export function joinPsbts")
        );

        // The shared-frontend seam modules app.js loads at runtime.
        let http_backend =
            response_for("GET", "/dist/shared-frontend/backends/http.js?v=cache-busted", b"");
        assert_eq!(http_backend.status, 200);
        assert_eq!(http_backend.content_type, "text/javascript; charset=utf-8");
        assert!(
            String::from_utf8(http_backend.body)
                .unwrap()
                .contains("class HttpBackend")
        );

        let types = response_for("GET", "/dist/shared-frontend/core/types.js?v=cache-busted", b"");
        assert_eq!(types.status, 200);
        assert_eq!(types.content_type, "text/javascript; charset=utf-8");
        assert!(
            String::from_utf8(types.body)
                .unwrap()
                .contains("class PtjBackendError")
        );

        let head = response_for("HEAD", "/dist/app.js?v=cache-busted", b"");
        assert_eq!(head.status, 200);
        assert_eq!(head.content_type, "text/javascript; charset=utf-8");
        assert!(head.body.is_empty());

        assert_eq!(response_for("GET", "/missing.js", b"").status, 404);
        assert_eq!(response_for("PUT", "/", b"").status, 405);
    }

    #[test]
    fn lifehash_endpoint_serves_stable_png_fingerprints() {
        let response = response_for("GET", "/api/lifehash/deadbeef", b"");
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "image/png");
        assert_eq!(&response.body[..8], b"\x89PNG\r\n\x1a\n");
        // Deterministic: the same digest yields byte-identical PNGs (query
        // strings are ignored like every other route).
        let again = response_for("GET", "/api/lifehash/deadbeef?v=1", b"");
        assert_eq!(again.body, response.body);
        // Distinct digests yield distinct fingerprints.
        let other = response_for("GET", "/api/lifehash/00ff80", b"");
        assert_ne!(other.body, response.body);

        // HEAD mirrors GET with an empty body.
        let head = response_for("HEAD", "/api/lifehash/deadbeef", b"");
        assert_eq!(head.status, 200);
        assert_eq!(head.content_type, "image/png");
        assert!(head.body.is_empty());

        // POST is not an API the fingerprint route serves.
        assert_eq!(response_for("POST", "/api/lifehash/deadbeef", b"").status, 405);
    }

    #[test]
    fn lifehash_endpoint_reports_json_errors() {
        // Liberal digest parsing: odd-length hex is the bytes_arg error.
        let response = response_for("GET", "/api/lifehash/abc", b"");
        assert_eq!(response.status, 400);
        assert_eq!(response.content_type, "application/json; charset=utf-8");
        assert!(
            String::from_utf8(response.body)
                .unwrap()
                .contains("odd length")
        );

        let response = response_for("GET", "/api/lifehash/", b"");
        assert_eq!(response.status, 400);
        assert!(
            String::from_utf8(response.body)
                .unwrap()
                .contains("empty byte string")
        );
    }

    #[test]
    fn lifehash_responses_are_cacheable_on_the_wire() {
        // The PNG body is binary, so this rides the bytes round-trip and
        // inspects the header block lossily.
        let response =
            round_trip_http_bytes("GET /api/lifehash/deadbeef HTTP/1.1\r\nHost: x\r\n\r\n");
        let headers = String::from_utf8_lossy(&response);
        assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"), "{headers}");
        assert!(headers.contains("Content-Type: image/png\r\n"), "{headers}");
        assert!(
            headers.contains("Cache-Control: public, max-age=31536000, immutable\r\n"),
            "{headers}"
        );

        // Everything else stays no-store.
        let response = round_trip_http("GET / HTTP/1.1\r\nHost: x\r\n\r\n");
        assert!(response.contains("Cache-Control: no-store\r\n"), "{response}");
    }

    /// Like `round_trip_http`, but tolerating a binary response body.
    fn round_trip_http_bytes(request: &str) -> Vec<u8> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            handle_connection(stream).unwrap();
        });

        let mut client = TcpStream::connect(addr).unwrap();
        client.write_all(request.as_bytes()).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();

        let mut response = Vec::new();
        client.read_to_end(&mut response).unwrap();
        server.join().unwrap();
        response
    }

    #[test]
    fn inspect_endpoint_reports_json_errors() {
        let missing = response_for("POST", "/api/inspect", b"{}");
        assert_eq!(missing.status, 400);
        assert_eq!(missing.content_type, "application/json; charset=utf-8");
        assert!(String::from_utf8(missing.body).unwrap().contains("`psbt`"));

        let malformed = response_for("POST", "/api/inspect", br#"{"psbt":"not a psbt"}"#);
        assert_eq!(malformed.status, 400);
        assert!(
            String::from_utf8(malformed.body)
                .unwrap()
                .contains("decoding base64")
        );
    }

    #[test]
    fn join_endpoint_returns_joined_psbt_and_inspection() {
        let request = serde_json::json!({
            "psbts": [
                encoded_psbt_with(TXID, 7, 1, 50_000),
                encoded_psbt_with(
                    "0000000000000000000000000000000000000000000000000000000000000002",
                    8,
                    2,
                    70_000,
                ),
            ],
        })
        .to_string();

        let response = response_for("POST", "/api/join", request.as_bytes());

        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "application/json; charset=utf-8");
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        let joined = crate::io::parse_psbt_bytes(
            "joined response psbt",
            body["psbt"].as_str().unwrap().as_bytes(),
        )
        .unwrap();
        assert_eq!(joined.global.input_count, 2);
        assert_eq!(joined.global.output_count, 2);
        assert_eq!(body["inspect"]["input_count"], 2);
        assert_eq!(body["inspect"]["output_count"], 2);
    }

    #[test]
    fn join_endpoint_reports_json_errors() {
        let missing = response_for("POST", "/api/join", b"{}");
        assert_eq!(missing.status, 400);
        assert!(String::from_utf8(missing.body).unwrap().contains("`psbts`"));

        let malformed = response_for("POST", "/api/join", br#"{"psbts":["not a psbt"]}"#);
        assert_eq!(malformed.status, 400);
        assert!(
            String::from_utf8(malformed.body)
                .unwrap()
                .contains("decoding base64")
        );
    }

    #[test]
    fn classify_endpoint_parses_descriptors() {
        const XPUB: &str = "xpub661MyMwAqRbcFtXgS5sYJABqqG9YLmC4Q1Rdap9gSE8NqtwybGhePY2gZ29ESFjqJoCu1Rupje8YtGqsefD265TMg7usUDFdp6W1EGMcet8";
        let request = serde_json::json!({ "payload": format!("wpkh({XPUB}/0/*)") }).to_string();

        let response = response_for("POST", "/api/classify", request.as_bytes());

        assert_eq!(
            response.status,
            200,
            "{}",
            String::from_utf8_lossy(&response.body)
        );
        assert_eq!(response.content_type, "application/json; charset=utf-8");
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(body["kind"], "descriptor");
        assert_eq!(body["has_private_keys"], false);
        assert_eq!(body["is_ranged"], true);
        assert_eq!(body["derived"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn classify_endpoint_parses_payment_uris_with_network_selector() {
        let request = serde_json::json!({
            "payload": format!(
                "bitcoin:{}?amount=0.00025&label=lunch",
                regtest_address(3),
            ),
            "network": "regtest",
        })
        .to_string();

        let response = response_for("POST", "/api/classify", request.as_bytes());

        assert_eq!(
            response.status,
            200,
            "{}",
            String::from_utf8_lossy(&response.body)
        );
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(body["kind"], "payment");
        assert_eq!(body["amount_sats"], 25_000);
        assert_eq!(body["label"], "lunch");
        assert_eq!(body["methods"][0]["type"], "onchain");
        assert_eq!(body["methods"][0]["address"], regtest_address(3));
    }

    #[test]
    fn classify_endpoint_parses_peer_ids_and_transactions() {
        use bitcoin::bech32::{self, Hrp};
        let npub =
            bech32::encode::<bech32::Bech32>(Hrp::parse("npub").unwrap(), &[0x22; 32]).unwrap();
        let request = serde_json::json!({ "payload": npub }).to_string();
        let response = response_for("POST", "/api/classify", request.as_bytes());
        assert_eq!(response.status, 200);
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(body["kind"], "peer_id");
        assert_eq!(body["format"], "npub");
        assert_eq!(body["id_hex"], "22".repeat(32));

        // A raw signed transaction pastes into spendable outpoints.
        let transaction = bitcoin::Transaction {
            version: bitcoin::transaction::Version::TWO,
            lock_time: bitcoin::absolute::LockTime::ZERO,
            input: vec![bitcoin::TxIn {
                previous_output: format!("{TXID}:7").parse().unwrap(),
                script_sig: bitcoin::ScriptBuf::new(),
                sequence: bitcoin::Sequence::MAX,
                witness: bitcoin::Witness::from_slice(&[vec![0xAA; 71], vec![0xBB; 33]]),
            }],
            output: vec![bitcoin::TxOut {
                value: bitcoin::Amount::from_sat(70_000),
                script_pubkey: regtest_address(2)
                    .parse::<bitcoin::Address<bitcoin::address::NetworkUnchecked>>()
                    .unwrap()
                    .assume_checked()
                    .script_pubkey(),
            }],
        };
        let request = serde_json::json!({
            "payload": bitcoin::consensus::encode::serialize_hex(&transaction),
            "network": "regtest",
        })
        .to_string();
        let response = response_for("POST", "/api/classify", request.as_bytes());
        assert_eq!(
            response.status,
            200,
            "{}",
            String::from_utf8_lossy(&response.body)
        );
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(body["kind"], "transaction");
        assert_eq!(body["fully_signed"], true);
        assert_eq!(body["outputs"][0]["amount_sats"], 70_000);
        assert_eq!(body["outputs"][0]["address"], regtest_address(2));
    }

    #[test]
    fn classify_endpoint_reports_json_errors() {
        let missing = response_for("POST", "/api/classify", b"{}");
        assert_eq!(missing.status, 400);
        assert!(
            String::from_utf8(missing.body)
                .unwrap()
                .contains("`payload`")
        );

        // PSBT pastes are redirected to the PSBT routes.
        let request = serde_json::json!({ "payload": encoded_psbt() }).to_string();
        let response = response_for("POST", "/api/classify", request.as_bytes());
        assert_eq!(response.status, 400);
        assert!(
            String::from_utf8(response.body)
                .unwrap()
                .contains("/api/inspect")
        );

        // Unclassifiable payloads name every decoder that was tried.
        let request = serde_json::json!({ "payload": "!!!" }).to_string();
        let response = response_for("POST", "/api/classify", request.as_bytes());
        assert_eq!(response.status, 400);
        let error = String::from_utf8(response.body).unwrap();
        assert!(error.contains("not an output descriptor"), "{error}");
        assert!(error.contains("not payment instructions"), "{error}");
    }

    /// The fixture PSBT with its output unique ids stripped — imported
    /// BIP 174 data before `assign-ids`.
    fn encoded_psbt_without_uids() -> String {
        let mut psbt =
            crate::io::parse_psbt_bytes("fixture psbt", encoded_psbt().as_bytes()).unwrap();
        for output in &mut psbt.outputs {
            output.proprietaries.clear();
        }
        crate::io::encode_psbt(&psbt)
    }

    #[test]
    fn assign_ids_endpoint_auto_assigns_missing_output_ids() {
        use concurrent_psbt::output::OutputUniqueIdExt as _;

        let request = serde_json::json!({ "psbt": encoded_psbt_without_uids() }).to_string();
        let response = response_for("POST", "/api/assign-ids", request.as_bytes());

        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "application/json; charset=utf-8");
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        let assigned = crate::io::parse_psbt_bytes(
            "assigned response psbt",
            body["psbt"].as_str().unwrap().as_bytes(),
        )
        .unwrap();
        assert!(assigned.outputs.iter().all(|output| output.has_unique_id()));
        assert_eq!(body["inspect"]["output_count"], 1);

        // Idempotent: a second pass returns the identical PSBT.
        let again = serde_json::json!({ "psbt": body["psbt"] }).to_string();
        let second = response_for("POST", "/api/assign-ids", again.as_bytes());
        assert_eq!(second.status, 200);
        let second_body: serde_json::Value = serde_json::from_slice(&second.body).unwrap();
        assert_eq!(second_body["psbt"], body["psbt"]);
    }

    #[test]
    fn assign_ids_endpoint_applies_manual_directives() {
        use concurrent_psbt::output::OutputUniqueIdExt as _;

        let request = serde_json::json!({
            "psbt": encoded_psbt_without_uids(),
            "ids": [
                { "target": "out", "index": 0, "id": "0102030405060708090a0b0c0d0e0f10" },
                { "target": "in", "index": 0, "id": "aa11" },
            ],
        })
        .to_string();
        let response = response_for("POST", "/api/assign-ids", request.as_bytes());

        assert_eq!(response.status, 200);
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        let assigned = crate::io::parse_psbt_bytes(
            "assigned response psbt",
            body["psbt"].as_str().unwrap().as_bytes(),
        )
        .unwrap();
        assert_eq!(
            assigned.outputs[0].unique_id().unwrap().into_bytes(),
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        );
        assert_eq!(
            concurrent_psbt::removal::InputUniqueIdExt::unique_id(&assigned.inputs[0]),
            Some(vec![0xaa, 0x11]),
        );
    }

    #[test]
    fn assign_ids_endpoint_reports_json_errors() {
        let missing = response_for("POST", "/api/assign-ids", b"{}");
        assert_eq!(missing.status, 400);
        assert!(String::from_utf8(missing.body).unwrap().contains("`psbt`"));

        let bad_id = serde_json::json!({
            "psbt": encoded_psbt_without_uids(),
            "ids": [{ "target": "out", "index": 0, "id": "0!z" }],
        })
        .to_string();
        let response = response_for("POST", "/api/assign-ids", bad_id.as_bytes());
        assert_eq!(response.status, 400);
        assert!(
            String::from_utf8(response.body)
                .unwrap()
                .contains("could not decode byte string")
        );

        let bad_target = serde_json::json!({
            "psbt": encoded_psbt_without_uids(),
            "ids": [{ "target": "sideways", "index": 0, "id": "abcd" }],
        })
        .to_string();
        let response = response_for("POST", "/api/assign-ids", bad_target.as_bytes());
        assert_eq!(response.status, 400);
        assert!(String::from_utf8(response.body).unwrap().contains("target"));
    }

    #[test]
    fn edit_endpoint_sets_and_deletes_raw_entries() {
        // Set an unknown global key: the edit mints a NEW fragment whose
        // inspect raw view classifies the entry as `unknown`.
        let original = encoded_psbt();
        let request = serde_json::json!({
            "psbt": original,
            "edits": [{ "map": "global", "key": "ef01", "value": "aabb" }],
        })
        .to_string();
        let response = response_for("POST", "/api/edit", request.as_bytes());
        assert_eq!(
            response.status,
            200,
            "{}",
            String::from_utf8_lossy(&response.body)
        );
        assert_eq!(response.content_type, "application/json; charset=utf-8");
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        let edited = body["psbt"].as_str().unwrap();
        assert_ne!(edited, original, "edits must mint a new fragment");
        assert_eq!(body["violations"], serde_json::json!([]));
        assert_eq!(body["applied_fixes"], serde_json::json!([]));
        let unknown = body["inspect"]["raw"]["global"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["key_hex"] == "ef01")
            .expect("the new entry must appear in the raw view");
        assert_eq!(unknown["kind"], "unknown");
        assert_eq!(unknown["value_hex"], "aabb");

        // Deleting the entry again round-trips back to the original bytes.
        let request = serde_json::json!({
            "psbt": edited,
            "edits": [{ "map": "global", "key": "ef01", "value": null }],
        })
        .to_string();
        let response = response_for("POST", "/api/edit", request.as_bytes());
        assert_eq!(response.status, 200);
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(body["psbt"].as_str().unwrap(), original);
    }

    #[test]
    fn edit_endpoint_edits_known_fields() {
        // PSBT_OUT_AMOUNT (keytype 0x03) is a typed field; raw edits reach it
        // like any other entry, and the typed inspect view reflects the edit.
        let request = serde_json::json!({
            "psbt": encoded_psbt(),
            // 2_000_000 sats as the 8-byte little-endian PSBT_OUT_AMOUNT.
            "edits": [{ "map": "out:0", "key": "03", "value": "80841e0000000000" }],
        })
        .to_string();
        let response = response_for("POST", "/api/edit", request.as_bytes());
        assert_eq!(
            response.status,
            200,
            "{}",
            String::from_utf8_lossy(&response.body)
        );
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(body["inspect"]["outputs"][0]["amount_sats"], 2_000_000);
    }

    #[test]
    fn edit_endpoint_reports_structured_violations_with_fix_offers() {
        // The canonical save-time case: an unordered PSBT whose outputs lack
        // unique ids. `edits: []` is a pure validation pass.
        let request = serde_json::json!({
            "psbt": encoded_psbt_without_uids(),
            "edits": [],
        })
        .to_string();
        let response = response_for("POST", "/api/edit", request.as_bytes());
        assert_eq!(response.status, 400);
        assert_eq!(response.content_type, "application/json; charset=utf-8");
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert!(
            body["error"]
                .as_str()
                .unwrap()
                .contains("save-time validation failed")
        );
        let violations = body["violations"].as_array().unwrap();
        assert_eq!(violations.len(), 1);
        let violation = &violations[0];
        assert_eq!(violation["id"], "unordered-missing-output-ids");
        assert_eq!(violation["override_param"], "allow_missing_output_ids");
        assert!(
            violation["message"]
                .as_str()
                .unwrap()
                .contains("PSBT_OUT_UNIQUE_ID")
        );
        // The structured fix offer wires the assign-ids machinery, warning
        // text included (it is part of the contract).
        assert_eq!(violation["fix_id"], "assign-ids");
        assert!(violation["fix_label"].is_string());
        assert!(
            violation["warning_text"]
                .as_str()
                .unwrap()
                .contains("duplicate txouts if done more than once")
        );
    }

    #[test]
    fn edit_endpoint_applies_offered_fixes() {
        use concurrent_psbt::output::OutputUniqueIdExt as _;

        let request = serde_json::json!({
            "psbt": encoded_psbt_without_uids(),
            "edits": [],
            "apply_fixes": ["assign-ids"],
        })
        .to_string();
        let response = response_for("POST", "/api/edit", request.as_bytes());
        assert_eq!(
            response.status,
            200,
            "{}",
            String::from_utf8_lossy(&response.body)
        );
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        let fixed = crate::io::parse_psbt_bytes(
            "fixed response psbt",
            body["psbt"].as_str().unwrap().as_bytes(),
        )
        .unwrap();
        assert!(fixed.outputs.iter().all(|output| output.has_unique_id()));
        assert_eq!(body["violations"], serde_json::json!([]));
        let applied = body["applied_fixes"].as_array().unwrap();
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0]["fix_id"], "assign-ids");
        // Applying the fix re-informs about the duplicate-txout caveat.
        assert!(
            applied[0]["warning_text"]
                .as_str()
                .unwrap()
                .contains("duplicate txouts")
        );
    }

    #[test]
    fn edit_endpoint_honors_explicit_overrides() {
        use concurrent_psbt::output::OutputUniqueIdExt as _;

        let request = serde_json::json!({
            "psbt": encoded_psbt_without_uids(),
            "edits": [],
            "allow_missing_output_ids": true,
        })
        .to_string();
        let response = response_for("POST", "/api/edit", request.as_bytes());
        assert_eq!(
            response.status,
            200,
            "{}",
            String::from_utf8_lossy(&response.body)
        );
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        // The gate is waived, not fixed: the fragment still lacks the ids and
        // the response says which gates were overridden.
        let saved = crate::io::parse_psbt_bytes(
            "overridden response psbt",
            body["psbt"].as_str().unwrap().as_bytes(),
        )
        .unwrap();
        assert!(saved.outputs.iter().all(|output| !output.has_unique_id()));
        let overridden = body["overridden"].as_array().unwrap();
        assert_eq!(overridden.len(), 1);
        assert_eq!(overridden[0]["id"], "unordered-missing-output-ids");
    }

    #[test]
    fn edit_endpoint_reports_json_errors() {
        let missing = response_for("POST", "/api/edit", b"{}");
        assert_eq!(missing.status, 400);
        assert!(String::from_utf8(missing.body).unwrap().contains("`psbt`"));

        let no_edits = serde_json::json!({ "psbt": encoded_psbt() }).to_string();
        let response = response_for("POST", "/api/edit", no_edits.as_bytes());
        assert_eq!(response.status, 400);
        assert!(String::from_utf8(response.body).unwrap().contains("`edits`"));

        let bad_map = serde_json::json!({
            "psbt": encoded_psbt(),
            "edits": [{ "map": "sideways", "key": "ef", "value": "01" }],
        })
        .to_string();
        let response = response_for("POST", "/api/edit", bad_map.as_bytes());
        assert_eq!(response.status, 400);
        assert!(String::from_utf8(response.body).unwrap().contains("sideways"));

        let bad_key = serde_json::json!({
            "psbt": encoded_psbt(),
            "edits": [{ "map": "global", "key": "0!z", "value": "01" }],
        })
        .to_string();
        let response = response_for("POST", "/api/edit", bad_key.as_bytes());
        assert_eq!(response.status, 400);
        assert!(
            String::from_utf8(response.body)
                .unwrap()
                .contains("could not decode byte string")
        );

        let missing_value = serde_json::json!({
            "psbt": encoded_psbt(),
            "edits": [{ "map": "global", "key": "ef" }],
        })
        .to_string();
        let response = response_for("POST", "/api/edit", missing_value.as_bytes());
        assert_eq!(response.status, 400);
        assert!(
            String::from_utf8(response.body)
                .unwrap()
                .contains("`value: null`")
        );

        let delete_absent = serde_json::json!({
            "psbt": encoded_psbt(),
            "edits": [{ "map": "in:0", "key": "ef", "value": null }],
        })
        .to_string();
        let response = response_for("POST", "/api/edit", delete_absent.as_bytes());
        assert_eq!(response.status, 400);
        assert!(
            String::from_utf8(response.body)
                .unwrap()
                .contains("nothing to delete")
        );

        let unknown_fix = serde_json::json!({
            "psbt": encoded_psbt(),
            "edits": [],
            "apply_fixes": ["reticulate-splines"],
        })
        .to_string();
        let response = response_for("POST", "/api/edit", unknown_fix.as_bytes());
        assert_eq!(response.status, 400);
        assert!(
            String::from_utf8(response.body)
                .unwrap()
                .contains("unknown fix id")
        );
    }

    #[test]
    fn sort_endpoint_returns_ordered_psbt_and_inspection() {
        let request = serde_json::json!({
            "psbt": encoded_psbt(),
            "seed_hex": "deadbeefdeadbeefdeadbeefdeadbeef",
        })
        .to_string();

        let response = response_for("POST", "/api/sort", request.as_bytes());

        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "application/json; charset=utf-8");
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        let sorted = crate::io::parse_psbt_bytes(
            "sorted response psbt",
            body["psbt"].as_str().unwrap().as_bytes(),
        )
        .unwrap();
        assert_eq!(sorted.global.input_count, 1);
        assert_eq!(sorted.global.output_count, 1);
        assert_eq!(body["inspect"]["ordering"], "ordered");
        assert_eq!(body["inspect"]["sort"]["mode"], "unset");
        assert_eq!(
            body["inspect"]["sort"]["seed_hex"],
            "deadbeefdeadbeefdeadbeefdeadbeef"
        );
    }

    #[test]
    fn sort_endpoint_rejects_short_seed_unless_overridden() {
        let short = serde_json::json!({
            "psbt": encoded_psbt(),
            "seed_hex": "deadbeef",
        })
        .to_string();
        let rejected = response_for("POST", "/api/sort", short.as_bytes());
        assert_eq!(rejected.status, 400);
        let message = String::from_utf8(rejected.body).unwrap();
        assert!(message.contains("128 bits"), "{message}");
        assert!(message.contains("allow_short_seed"), "{message}");

        let overridden = serde_json::json!({
            "psbt": encoded_psbt(),
            "seed_hex": "deadbeef",
            "allow_short_seed": true,
        })
        .to_string();
        let response = response_for("POST", "/api/sort", overridden.as_bytes());
        assert_eq!(response.status, 200);
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(body["inspect"]["sort"]["seed_hex"], "deadbeef");
    }

    #[test]
    fn sort_endpoint_reports_json_errors() {
        let missing = response_for("POST", "/api/sort", b"{}");
        assert_eq!(missing.status, 400);
        assert!(String::from_utf8(missing.body).unwrap().contains("`psbt`"));

        let bad_seed = response_for(
            "POST",
            "/api/sort",
            br#"{"psbt":"cHNidP8BAAoBAAAAAA==","seed_hex":"abc"}"#,
        );
        assert_eq!(bad_seed.status, 400);
        assert!(
            String::from_utf8(bad_seed.body)
                .unwrap()
                .contains("odd length")
        );
    }

    #[test]
    fn make_unordered_endpoint_returns_unordered_psbt_and_inspection() {
        let request = serde_json::json!({ "psbt": encoded_ordered_psbt() }).to_string();

        let response = response_for("POST", "/api/make-unordered", request.as_bytes());

        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "application/json; charset=utf-8");
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        let unordered = crate::io::parse_psbt_bytes(
            "unordered response psbt",
            body["psbt"].as_str().unwrap().as_bytes(),
        )
        .unwrap();
        assert!(concurrent_psbt::global::GlobalSortExt::is_unordered(
            &unordered.global
        ));
        assert_eq!(body["inspect"]["ordering"], "unordered");
        assert_eq!(body["inspect"]["input_count"], 1);
        assert_eq!(body["inspect"]["output_count"], 1);
    }

    #[test]
    fn make_unordered_endpoint_reports_json_errors() {
        let missing = response_for("POST", "/api/make-unordered", b"{}");
        assert_eq!(missing.status, 400);
        assert!(String::from_utf8(missing.body).unwrap().contains("`psbt`"));

        let malformed = response_for("POST", "/api/make-unordered", br#"{"psbt":"not a psbt"}"#);
        assert_eq!(malformed.status, 400);
        assert!(
            String::from_utf8(malformed.body)
                .unwrap()
                .contains("decoding base64")
        );
    }

    #[test]
    fn atomize_endpoint_returns_atomic_fragments_and_inspection() {
        let request = serde_json::json!({ "psbt": encoded_ordered_psbt() }).to_string();

        let response = response_for("POST", "/api/atomize", request.as_bytes());

        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "application/json; charset=utf-8");
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        let fragments = body["fragments"].as_array().unwrap();
        assert_eq!(fragments.len(), 2);
        let atoms = fragments
            .iter()
            .map(|fragment| {
                crate::io::parse_psbt_bytes(
                    "atomized response psbt",
                    fragment["psbt"].as_str().unwrap().as_bytes(),
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(
            atoms
                .iter()
                .all(|atom| { concurrent_psbt::global::GlobalSortExt::is_unordered(&atom.global) })
        );
        assert_eq!(
            atoms
                .iter()
                .map(|atom| atom.global.input_count + atom.global.output_count)
                .collect::<Vec<_>>(),
            vec![1, 1]
        );
        assert_eq!(fragments[0]["inspect"]["ordering"], "unordered");
        assert_eq!(fragments[1]["inspect"]["ordering"], "unordered");
    }

    #[test]
    fn atomize_endpoint_reports_json_errors() {
        let missing = response_for("POST", "/api/atomize", b"{}");
        assert_eq!(missing.status, 400);
        assert!(String::from_utf8(missing.body).unwrap().contains("`psbt`"));

        let atom = serde_json::json!({ "psbt": encoded_input_atom() }).to_string();
        let already_atomic = response_for("POST", "/api/atomize", atom.as_bytes());
        assert_eq!(already_atomic.status, 400);
        assert!(
            String::from_utf8(already_atomic.body)
                .unwrap()
                .contains("already atomic")
        );
    }

    #[test]
    fn concatenate_endpoint_returns_appended_ordered_psbt_and_inspection() {
        let request = serde_json::json!({
            "psbts": [
                encoded_ordered_psbt_with(TXID, 7, 1, 50_000),
                encoded_ordered_psbt_with(
                    "0000000000000000000000000000000000000000000000000000000000000002",
                    8,
                    2,
                    70_000,
                ),
            ],
        })
        .to_string();

        let response = response_for("POST", "/api/concatenate", request.as_bytes());

        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "application/json; charset=utf-8");
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        let concatenated = crate::io::parse_psbt_bytes(
            "concatenated response psbt",
            body["psbt"].as_str().unwrap().as_bytes(),
        )
        .unwrap();
        assert_eq!(concatenated.global.input_count, 2);
        assert_eq!(concatenated.global.output_count, 2);
        assert!(!concurrent_psbt::global::GlobalSortExt::is_unordered(
            &concatenated.global
        ));
        assert_eq!(body["inspect"]["ordering"], "ordered");
        assert_eq!(body["inspect"]["input_count"], 2);
        assert_eq!(body["inspect"]["output_count"], 2);
    }

    #[test]
    fn concatenate_endpoint_reports_json_errors() {
        let missing = response_for("POST", "/api/concatenate", b"{}");
        assert_eq!(missing.status, 400);
        assert!(String::from_utf8(missing.body).unwrap().contains("`psbts`"));

        let unordered = serde_json::json!({
            "psbts": [encoded_psbt(), encoded_ordered_psbt()],
        })
        .to_string();
        let response = response_for("POST", "/api/concatenate", unordered.as_bytes());
        assert_eq!(response.status, 400);
        assert!(
            String::from_utf8(response.body)
                .unwrap()
                .contains("ordered PSBT")
        );
    }

    #[test]
    fn create_endpoint_returns_constructed_psbt_and_inspection() {
        let request = serde_json::json!({
            "network": "regtest",
            "inputs": [
                { "txid": TXID, "vout": 7 },
            ],
            "outputs": [
                { "address": regtest_address(1), "amount_btc": "0.00050000" },
            ],
            "ordering": "deterministic",
            "seed_hex": SEED_HEX,
        })
        .to_string();

        let response = response_for("POST", "/api/create", request.as_bytes());

        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "application/json; charset=utf-8");
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        let created = crate::io::parse_psbt_bytes(
            "created response psbt",
            body["psbt"].as_str().unwrap().as_bytes(),
        )
        .unwrap();
        assert_eq!(created.global.input_count, 1);
        assert_eq!(created.global.output_count, 1);
        assert!(concurrent_psbt::global::GlobalSortExt::is_unordered(
            &created.global
        ));
        assert_eq!(created.global.tx_modifiable_flags, 0x03);
        assert_eq!(body["inspect"]["ordering"], "unordered");
        assert_eq!(body["inspect"]["input_count"], 1);
        assert_eq!(body["inspect"]["output_count"], 1);
        assert_eq!(body["inspect"]["sort"]["mode"], "deterministic");
        assert_eq!(body["inspect"]["sort"]["seed_hex"], SEED_HEX);
    }

    #[test]
    fn create_endpoint_rejects_short_seed_unless_overridden() {
        let short = serde_json::json!({
            "network": "regtest",
            "inputs": [{ "txid": TXID, "vout": 7 }],
            "ordering": "deterministic",
            "seed_hex": "abcd",
        })
        .to_string();
        let rejected = response_for("POST", "/api/create", short.as_bytes());
        assert_eq!(rejected.status, 400);
        let message = String::from_utf8(rejected.body).unwrap();
        assert!(message.contains("128 bits"), "{message}");
        assert!(message.contains("allow_short_seed"), "{message}");
    }

    #[test]
    fn create_endpoint_preserves_unset_ordering_seed_without_deterministic_mode() {
        // The short legacy seed rides on the explicit allow_short_seed
        // override; without it the boundary rejects seeds below 128 bits.
        let request = serde_json::json!({
            "network": "regtest",
            "inputs": [
                { "txid": TXID, "vout": 7 },
            ],
            "seed_hex": "abcd",
            "allow_short_seed": true,
        })
        .to_string();

        let response = response_for("POST", "/api/create", request.as_bytes());

        assert_eq!(response.status, 200);
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        let created = crate::io::parse_psbt_bytes(
            "created response psbt",
            body["psbt"].as_str().unwrap().as_bytes(),
        )
        .unwrap();
        assert_eq!(created.global.sort_seed(), Some(&[0xab, 0xcd][..]));
        assert_eq!(created.global.sort_deterministic(), None);
        assert_eq!(body["inspect"]["sort"]["mode"], "unset");
        assert_eq!(body["inspect"]["sort"]["seed_hex"], "abcd");
    }

    #[test]
    fn create_endpoint_reports_json_errors() {
        let bad_network = response_for(
            "POST",
            "/api/create",
            br#"{"network":"sidechain","inputs":[],"outputs":[]}"#,
        );
        assert_eq!(bad_network.status, 400);
        assert!(
            String::from_utf8(bad_network.body)
                .unwrap()
                .contains("unknown network")
        );

        let bad_input = response_for(
            "POST",
            "/api/create",
            br#"{"network":"regtest","inputs":[{"txid":"bad","vout":0}],"outputs":[]}"#,
        );
        assert_eq!(bad_input.status, 400);
        assert!(
            String::from_utf8(bad_input.body)
                .unwrap()
                .contains("invalid txid")
        );

        let bad_ordering = response_for(
            "POST",
            "/api/create",
            br#"{"network":"regtest","ordering":"sideways"}"#,
        );
        assert_eq!(bad_ordering.status, 400);
        assert!(
            String::from_utf8(bad_ordering.body)
                .unwrap()
                .contains("unknown ordering")
        );
    }

    #[test]
    fn export_bip174_endpoint_returns_core_compatible_psbt() {
        let request = serde_json::json!({ "psbt": encoded_ordered_psbt() }).to_string();

        let response = response_for("POST", "/api/export-bip174", request.as_bytes());

        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "application/json; charset=utf-8");
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(body["format"], "bip174");
        let exported: psbt_v2::v0::bitcoin::Psbt = body["psbt"].as_str().unwrap().parse().unwrap();
        assert_eq!(exported.unsigned_tx.input.len(), 1);
        assert_eq!(exported.unsigned_tx.output.len(), 1);
        assert_eq!(
            exported.unsigned_tx.input[0]
                .previous_output
                .txid
                .to_string(),
            TXID
        );
    }

    #[test]
    fn export_bip174_endpoint_reports_json_errors() {
        let missing = response_for("POST", "/api/export-bip174", b"{}");
        assert_eq!(missing.status, 400);
        assert!(String::from_utf8(missing.body).unwrap().contains("`psbt`"));

        let unordered = serde_json::json!({ "psbt": encoded_psbt() }).to_string();
        let response = response_for("POST", "/api/export-bip174", unordered.as_bytes());
        assert_eq!(response.status, 400);
        assert!(
            String::from_utf8(response.body)
                .unwrap()
                .contains("run `ptj sort` first")
        );
    }

    #[test]
    fn import_bip174_endpoint_returns_ordered_bip370_psbt_and_inspection() {
        let exported = crate::commands::export_bip174::export_bip174_psbt(
            crate::io::parse_psbt_bytes("fixture psbt", encoded_ordered_psbt().as_bytes()).unwrap(),
        )
        .unwrap();
        let request = serde_json::json!({ "psbt": exported }).to_string();

        let response = response_for("POST", "/api/import-bip174", request.as_bytes());

        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "application/json; charset=utf-8");
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        let imported = crate::io::parse_psbt_bytes(
            "imported response psbt",
            body["psbt"].as_str().unwrap().as_bytes(),
        )
        .unwrap();
        assert_eq!(imported.global.input_count, 1);
        assert_eq!(imported.global.output_count, 1);
        assert!(!concurrent_psbt::global::GlobalSortExt::is_unordered(
            &imported.global
        ));
        assert_eq!(imported.global.tx_modifiable_flags, 0);
        assert_eq!(body["inspect"]["format"], "bip370");
        assert_eq!(body["inspect"]["ordering"], "ordered");
        assert_eq!(body["inspect"]["input_count"], 1);
        assert_eq!(body["inspect"]["output_count"], 1);
    }

    #[test]
    fn import_bip174_endpoint_marks_modifiable_on_request() {
        let exported = crate::commands::export_bip174::export_bip174_psbt(
            crate::io::parse_psbt_bytes("fixture psbt", encoded_ordered_psbt().as_bytes()).unwrap(),
        )
        .unwrap();
        let request = serde_json::json!({ "psbt": exported, "modifiable": true }).to_string();

        let response = response_for("POST", "/api/import-bip174", request.as_bytes());

        assert_eq!(response.status, 200);
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        let imported = crate::io::parse_psbt_bytes(
            "imported response psbt",
            body["psbt"].as_str().unwrap().as_bytes(),
        )
        .unwrap();
        assert_eq!(imported.global.tx_modifiable_flags, 0x03);
        assert_eq!(body["inspect"]["modifiability"]["inputs"], true);
        assert_eq!(body["inspect"]["modifiability"]["outputs"], true);
    }

    #[test]
    fn import_bip174_endpoint_reports_json_errors() {
        let missing = response_for("POST", "/api/import-bip174", b"{}");
        assert_eq!(missing.status, 400);
        assert!(String::from_utf8(missing.body).unwrap().contains("`psbt`"));

        let bip370 = serde_json::json!({ "psbt": encoded_ordered_psbt() }).to_string();
        let response = response_for("POST", "/api/import-bip174", bip370.as_bytes());
        assert_eq!(response.status, 400);
        assert!(
            String::from_utf8(response.body)
                .unwrap()
                .contains("parsing BIP 174")
        );
    }

    #[test]
    fn http_handler_dispatches_post_body_to_inspect_endpoint() {
        let request_body = serde_json::json!({ "psbt": encoded_psbt() }).to_string();
        let request = format!(
            "POST /api/inspect HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{request_body}",
            request_body.len()
        );
        let response = round_trip_http(&request);

        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.contains("Content-Type: application/json; charset=utf-8\r\n"));
        assert!(response.contains(r#""ordering":"unordered""#));
    }

    fn round_trip_http(request: &str) -> String {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            handle_connection(stream).unwrap();
        });

        let mut client = TcpStream::connect(addr).unwrap();
        client.write_all(request.as_bytes()).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();

        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        server.join().unwrap();
        response
    }

    fn encoded_psbt() -> String {
        encoded_psbt_with(TXID, 7, 1, 123_456)
    }

    fn encoded_ordered_psbt() -> String {
        encoded_ordered_psbt_with(TXID, 7, 1, 123_456)
    }

    fn encoded_ordered_psbt_with(
        txid: &str,
        vout: u32,
        address_seed: u8,
        amount_sats: u64,
    ) -> String {
        let psbt = crate::io::parse_psbt_bytes(
            "fixture psbt",
            encoded_psbt_with(txid, vout, address_seed, amount_sats).as_bytes(),
        )
        .unwrap();
        let constructor =
            concurrent_psbt::roles::constructor::dynamic::Constructor::try_from_psbt(psbt).unwrap();
        let sorted =
            crate::commands::sort::sort_psbt(constructor.into_inner(), Some(seed_bytes()), false)
                .unwrap();
        crate::io::encode_psbt(&sorted)
    }

    fn encoded_input_atom() -> String {
        crate::run(
            Cli::try_parse_from([
                "ptj",
                "create",
                "--network",
                "regtest",
                "--input",
                &format!("{TXID}:7"),
                "--seed",
                SEED_HEX,
            ])
            .unwrap(),
        )
        .unwrap()
    }

    fn encoded_psbt_with(txid: &str, vout: u32, address_seed: u8, amount_sats: u64) -> String {
        crate::run(
            Cli::try_parse_from([
                "ptj",
                "create",
                "--network",
                "regtest",
                "--input",
                &format!("{txid}:{vout}"),
                "--output",
                &format!(
                    "{}:{}",
                    regtest_address(address_seed),
                    btc_value(amount_sats)
                ),
                "--seed",
                SEED_HEX,
            ])
            .unwrap(),
        )
        .unwrap()
    }

    fn regtest_address(seed: u8) -> String {
        let secp = bitcoin::secp256k1::Secp256k1::new();
        let secret = bitcoin::secp256k1::SecretKey::from_slice(&[seed; 32]).unwrap();
        let public_key = bitcoin::secp256k1::PublicKey::from_secret_key(&secp, &secret);
        let public_key = bitcoin::CompressedPublicKey::from_slice(&public_key.serialize()).unwrap();
        bitcoin::Address::p2wpkh(&public_key, bitcoin::Network::Regtest).to_string()
    }

    fn btc_value(amount_sats: u64) -> String {
        bitcoin::Amount::from_sat(amount_sats).to_btc().to_string()
    }
    #[test]
    fn sync_endpoint_folds_psbts_locally() {
        let empty = crate::commands::create::create_psbt(crate::cli::CreateConfig {
            inputs: vec![],
            outputs: vec![],
            ordering: crate::cli::OrderingArg::Unset,
            seed: None,
            allow_short_seed: false,
            network: crate::cli::NetworkArg(bitcoin::Network::Regtest),
        })
        .expect("empty create");
        let encoded = crate::io::encode_psbt(&empty);
        let request = serde_json::json!({ "psbts": [encoded, encoded] }).to_string();
        let response = response_for("POST", "/api/sync", request.as_bytes());
        assert_eq!(
            response.status,
            200,
            "{}",
            String::from_utf8_lossy(&response.body)
        );
        let value: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert!(
            value
                .get("psbt")
                .and_then(serde_json::Value::as_str)
                .is_some()
        );
        assert_eq!(value["payments"], serde_json::json!([]));
        assert_eq!(value["confirmations"], serde_json::json!([]));
    }

    #[test]
    fn sync_endpoint_requires_input() {
        let response = response_for("POST", "/api/sync", b"{}");
        assert_eq!(response.status, 400);
    }

    /// A pasted iroh ticket with the `iroh-sync` feature off returns the clear
    /// rebuild error (back-compat inference: absent `transport`, present
    /// `iroh_ticket` => Iroh through the shared selector).
    #[cfg(not(feature = "iroh-sync"))]
    #[test]
    fn sync_endpoint_iroh_gated() {
        let request = serde_json::json!({ "iroh_ticket": "doc..." }).to_string();
        let response = response_for("POST", "/api/sync", request.as_bytes());
        assert_eq!(response.status, 400);
        assert!(String::from_utf8_lossy(&response.body).contains("iroh-sync"));
    }

    /// Explicitly selecting a transport whose feature is off returns the shared
    /// selector's clear rebuild error. Exercised for arti (any non-local kind
    /// whose feature is off behaves the same way).
    #[cfg(not(feature = "arti"))]
    #[test]
    fn sync_endpoint_reports_missing_transport_feature() {
        let request = serde_json::json!({
            "transport": "arti",
            "psbts": [sync_test_psbt()],
        })
        .to_string();
        let response = response_for("POST", "/api/sync", request.as_bytes());
        assert_eq!(response.status, 400);
        let body = String::from_utf8_lossy(&response.body);
        assert!(body.contains("arti"), "expected arti rebuild hint, got: {body}");
    }

    /// An unknown transport name is a clean 400 listing the accepted kinds.
    #[test]
    fn sync_endpoint_rejects_unknown_transport() {
        let request = serde_json::json!({
            "transport": "carrier-pigeon",
            "psbts": [sync_test_psbt()],
        })
        .to_string();
        let response = response_for("POST", "/api/sync", request.as_bytes());
        assert_eq!(response.status, 400);
        assert!(String::from_utf8_lossy(&response.body).contains("unknown transport"));
    }

    /// Explicit `"transport":"local"` folds locally with no network egress,
    /// same as omitting the field entirely.
    #[test]
    fn sync_endpoint_explicit_local_folds_without_network() {
        let encoded = sync_test_psbt();
        let request = serde_json::json!({
            "transport": "local",
            "psbts": [encoded, encoded],
        })
        .to_string();
        let response = response_for("POST", "/api/sync", request.as_bytes());
        assert_eq!(
            response.status,
            200,
            "{}",
            String::from_utf8_lossy(&response.body)
        );
        let value: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert!(
            value
                .get("psbt")
                .and_then(serde_json::Value::as_str)
                .is_some()
        );
        assert_eq!(value["payments"], serde_json::json!([]));
        assert_eq!(value["confirmations"], serde_json::json!([]));
    }

    /// The new browser-transport kinds parse (they are real `TransportKind`
    /// values); selecting one with its feature off reports the rebuild hint.
    #[cfg(not(any(feature = "str0m", feature = "webrtc-rs", feature = "payjoin-dir")))]
    #[test]
    fn sync_endpoint_browser_transports_parse_and_gate() {
        for (name, hint) in [
            ("str0m", "str0m"),
            ("webrtc-rs", "webrtc-rs"),
            ("payjoin-dir", "payjoin-dir"),
        ] {
            let request = serde_json::json!({
                "transport": name,
                "psbts": [sync_test_psbt()],
            })
            .to_string();
            let response = response_for("POST", "/api/sync", request.as_bytes());
            assert_eq!(response.status, 400, "transport {name}");
            let body = String::from_utf8_lossy(&response.body);
            assert!(
                !body.contains("unknown transport"),
                "{name} must parse as a TransportKind, got: {body}"
            );
            assert!(body.contains(hint), "expected {hint} rebuild hint, got: {body}");
        }
    }

    /// The `/api/sync` request DTO carries the WebRTC signaling params through
    /// to `SyncConfig` 1:1 with the CLI flags (feature-independent mapping —
    /// presence validation happens later, in the shared selector).
    #[test]
    fn sync_request_maps_webrtc_signaling_params() {
        let request = serde_json::json!({
            "transport": "str0m",
            "webrtc_role": "offer",
            "signal_out": "/tmp/us.sig",
            "signal_in": "/tmp/peer.sig",
            "webrtc_bind": "127.0.0.1:0",
            "ice_servers": ["stun:stun.example.org:3478"],
            "signal_timeout_ms": 1234,
        });
        let config = sync_config_from_request(&request).unwrap();
        assert_eq!(config.transport, crate::cli::TransportKind::Str0m);
        assert_eq!(config.webrtc_role, Some(crate::cli::WebrtcRoleArg::Offer));
        assert_eq!(
            config.signal_out,
            Some(std::path::PathBuf::from("/tmp/us.sig"))
        );
        assert_eq!(
            config.signal_in,
            Some(std::path::PathBuf::from("/tmp/peer.sig"))
        );
        assert_eq!(config.webrtc_bind, "127.0.0.1:0");
        assert_eq!(
            config.ice_servers,
            vec!["stun:stun.example.org:3478".to_string()]
        );
        assert_eq!(config.signal_timeout_ms, 1234);
    }

    /// Absent WebRTC fields fall back to the CLI defaults; a bad role is a
    /// clean error naming the accepted values.
    #[test]
    fn sync_request_webrtc_defaults_and_role_validation() {
        let request = serde_json::json!({ "transport": "webrtc-rs" });
        let config = sync_config_from_request(&request).unwrap();
        assert_eq!(config.transport, crate::cli::TransportKind::WebrtcRs);
        assert_eq!(config.webrtc_role, None);
        assert_eq!(config.signal_out, None);
        assert_eq!(config.signal_in, None);
        assert_eq!(config.webrtc_bind, "0.0.0.0:0");
        assert!(config.ice_servers.is_empty());
        assert_eq!(config.signal_timeout_ms, 60_000);

        let request = serde_json::json!({ "transport": "str0m", "webrtc_role": "sideways" });
        let error = sync_config_from_request(&request).unwrap_err().to_string();
        assert!(error.contains("unknown webrtc_role"), "got: {error}");
        assert!(error.contains("offer, answer"), "got: {error}");
    }

    /// Server-side `sources` (a directory of .psbt files) and a `state` file
    /// fold together with the in-band `psbts[]` in ONE lattice join, exactly
    /// like `ptj sync <sources>`; the route stays read-only.
    #[test]
    fn sync_endpoint_folds_server_side_sources() {
        let nonce: u64 = rand::random();
        let dir = std::env::temp_dir().join(format!("ptj-webgui-test-sources-{nonce:016x}"));
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.psbt"), encoded_psbt_with(TXID, 7, 1, 50_000)).unwrap();
        let state = dir.join("state-psbt");
        std::fs::write(
            &state,
            encoded_psbt_with(
                "0000000000000000000000000000000000000000000000000000000000000002",
                8,
                2,
                70_000,
            ),
        )
        .unwrap();

        let request = serde_json::json!({
            "transport": "local",
            "psbts": [encoded_psbt_with(
                "0000000000000000000000000000000000000000000000000000000000000003",
                9,
                3,
                90_000,
            )],
            "sources": [dir.join("a.psbt").to_string_lossy()],
            "state": state.to_string_lossy(),
        })
        .to_string();
        let response = response_for("POST", "/api/sync", request.as_bytes());
        assert_eq!(
            response.status,
            200,
            "{}",
            String::from_utf8_lossy(&response.body)
        );
        let value: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(value["inspect"]["input_count"], 3);
        assert_eq!(value["inspect"]["output_count"], 3);
        // Read-only: the state file was folded, not rewritten.
        let state_after = std::fs::read_to_string(&state).unwrap();
        let state_psbt =
            crate::io::parse_psbt_bytes("state after sync", state_after.as_bytes()).unwrap();
        assert_eq!(state_psbt.global.input_count, 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `sources`/`state` map into `SyncConfig` and `iroh_ticket_out: true`
    /// allocates a server-side ticket-out path (feature-independent mapping —
    /// the selector performs the iroh work later).
    #[test]
    fn sync_request_maps_sources_state_and_ticket_out() {
        let request = serde_json::json!({
            "transport": "iroh",
            "iroh_ticket_out": true,
            "sources": ["/tmp/psbts", "/tmp/one.psbt"],
            "state": "/tmp/state.psbt",
        });
        let config = sync_config_from_request(&request).unwrap();
        assert_eq!(config.transport, crate::cli::TransportKind::Iroh);
        assert_eq!(config.iroh_ticket, None);
        let ticket_out = config.iroh_ticket_out.expect("ticket-out path");
        assert!(
            ticket_out
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("ptj-webgui-iroh-ticket-out-")
        );
        assert_eq!(
            config.sources,
            vec![
                std::path::PathBuf::from("/tmp/psbts"),
                std::path::PathBuf::from("/tmp/one.psbt"),
            ]
        );
        assert_eq!(config.state, Some(std::path::PathBuf::from("/tmp/state.psbt")));
    }

    /// Selecting iroh with neither a ticket nor `iroh_ticket_out: true` names
    /// both options (feature-independent: the config step rejects it before
    /// any transport is built).
    #[test]
    fn sync_endpoint_iroh_requires_ticket_or_ticket_out() {
        let request = serde_json::json!({
            "transport": "iroh",
            "psbts": [sync_test_psbt()],
        })
        .to_string();
        let response = response_for("POST", "/api/sync", request.as_bytes());
        assert_eq!(response.status, 400);
        let body = String::from_utf8_lossy(&response.body);
        assert!(body.contains("`iroh_ticket`"), "got: {body}");
        assert!(body.contains("`iroh_ticket_out: true`"), "got: {body}");
    }

    /// Test helper: one empty regtest PSBT, encoded (the same shape
    /// `sync_endpoint_folds_psbts_locally` builds inline).
    fn sync_test_psbt() -> String {
        let empty = crate::commands::create::create_psbt(crate::cli::CreateConfig {
            inputs: vec![],
            outputs: vec![],
            ordering: crate::cli::OrderingArg::Unset,
            seed: None,
            allow_short_seed: false,
            network: crate::cli::NetworkArg(bitcoin::Network::Regtest),
        })
        .expect("empty create");
        crate::io::encode_psbt(&empty)
    }

    /// POST helper for the negotiation endpoints: send `request`, expect 200,
    /// return the parsed JSON body.
    fn negotiation_ok(path: &str, request: &serde_json::Value) -> serde_json::Value {
        let response = response_for("POST", path, request.to_string().as_bytes());
        assert_eq!(
            response.status,
            200,
            "{path}: {}",
            String::from_utf8_lossy(&response.body)
        );
        assert_eq!(response.content_type, "application/json; charset=utf-8");
        serde_json::from_slice(&response.body).unwrap()
    }

    #[test]
    fn pay_endpoint_appends_plaintext_record_and_payments_decodes_it() {
        let paid = negotiation_ok(
            "/api/pay",
            &serde_json::json!({ "psbt": encoded_psbt(), "payment_hex": "deadbeef" }),
        );
        assert!(paid["inspect"].is_object());
        let paid_psbt = paid["psbt"].as_str().unwrap();

        let decoded = negotiation_ok("/api/payments", &serde_json::json!({ "psbt": paid_psbt }));
        assert_eq!(decoded["payments"], serde_json::json!(["deadbeef"]));
        assert_eq!(decoded["confirmations"], serde_json::json!([]));
    }

    #[test]
    fn pay_endpoint_encrypted_roundtrip_with_dummy_padding() {
        let paid = negotiation_ok(
            "/api/pay",
            &serde_json::json!({
                "psbt": encoded_psbt(),
                "payment_hex": "deadbeef",
                "secret_hex": "0011",
                "dummy": 2,
            }),
        );
        let paid_psbt = paid["psbt"].as_str().unwrap();

        // Correct secret: the real record decrypts back; two dummies ride along.
        let decoded = negotiation_ok(
            "/api/payments",
            &serde_json::json!({ "psbt": paid_psbt, "secret_hex": "0011" }),
        );
        let payments = decoded["payments"].as_array().unwrap();
        assert_eq!(payments.len(), 3);
        assert_eq!(
            payments
                .iter()
                .filter(|payment| payment.as_str() == Some("deadbeef"))
                .count(),
            1
        );

        // No secret: every entry stays an opaque ciphertext blob.
        let opaque = negotiation_ok("/api/payments", &serde_json::json!({ "psbt": paid_psbt }));
        assert!(
            opaque["payments"]
                .as_array()
                .unwrap()
                .iter()
                .all(|payment| payment.as_str() != Some("deadbeef"))
        );

        // Wrong secret: decryption failure is a clean 400 {error}.
        let request = serde_json::json!({ "psbt": paid_psbt, "secret_hex": "ffff" }).to_string();
        let response = response_for("POST", "/api/payments", request.as_bytes());
        assert_eq!(response.status, 400);
        assert!(
            String::from_utf8(response.body)
                .unwrap()
                .contains("failed to decrypt")
        );
    }

    #[test]
    fn pay_endpoint_builds_record_from_address() {
        let payer_hex = "11".repeat(32);
        let paid = negotiation_ok(
            "/api/pay",
            &serde_json::json!({
                "psbt": encoded_psbt(),
                "address": regtest_address(3),
                "amount_btc": "0.00025000",
                "network": "regtest",
                "label": "lunch",
                "payer_hex": payer_hex,
            }),
        );
        let paid_psbt = paid["psbt"].as_str().unwrap();

        let decoded = negotiation_ok("/api/payments", &serde_json::json!({ "psbt": paid_psbt }));
        let payments = decoded["payments"].as_array().unwrap();
        assert_eq!(payments.len(), 1);
        let record_hex = payments[0].as_str().unwrap();
        let record_bytes = (0..record_hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&record_hex[i..i + 2], 16).unwrap())
            .collect::<Vec<_>>();
        let payment =
            concurrent_psbt::payments::negotiation::Payment::decode(&record_bytes).unwrap();
        assert!(!payment.is_dummy());
        assert_eq!(payment.amount_sats, 25_000);
        assert_eq!(payment.label, "lunch");
        assert_eq!(payment.payer, [0x11; 32]);
        let expected_spk = regtest_address(3)
            .parse::<bitcoin::Address<bitcoin::address::NetworkUnchecked>>()
            .unwrap()
            .require_network(bitcoin::Network::Regtest)
            .unwrap()
            .script_pubkey();
        assert_eq!(payment.script_pubkey, expected_spk.into_bytes());
    }

    #[test]
    fn pay_endpoint_validates_address_network_and_payer() {
        // Network defaults to bitcoin, exactly like `ptj pay`: a regtest
        // address must be rejected unless the request selects regtest.
        let request = serde_json::json!({
            "psbt": encoded_psbt(),
            "address": regtest_address(3),
            "amount_btc": "0.00025000",
        })
        .to_string();
        let response = response_for("POST", "/api/pay", request.as_bytes());
        assert_eq!(response.status, 400);
        assert!(
            String::from_utf8(response.body)
                .unwrap()
                .contains("address not valid for bitcoin")
        );

        // payer_hex must be exactly 32 bytes (the CLI's Hex32 error text).
        let request = serde_json::json!({
            "psbt": encoded_psbt(),
            "address": regtest_address(3),
            "amount_btc": "0.00025000",
            "network": "regtest",
            "payer_hex": "1234",
        })
        .to_string();
        let response = response_for("POST", "/api/pay", request.as_bytes());
        assert_eq!(response.status, 400);
        assert!(
            String::from_utf8(response.body)
                .unwrap()
                .contains("expected 32 bytes")
        );
    }

    #[test]
    fn confirm_endpoint_appends_record_plaintext_and_encrypted() {
        let confirmed = negotiation_ok(
            "/api/confirm",
            &serde_json::json!({ "psbt": encoded_psbt(), "confirmation_hex": "c0ffee00" }),
        );
        assert!(confirmed["inspect"].is_object());
        let decoded = negotiation_ok(
            "/api/payments",
            &serde_json::json!({ "psbt": confirmed["psbt"].as_str().unwrap() }),
        );
        assert_eq!(decoded["payments"], serde_json::json!([]));
        assert_eq!(decoded["confirmations"], serde_json::json!(["c0ffee00"]));

        let confirmed = negotiation_ok(
            "/api/confirm",
            &serde_json::json!({
                "psbt": encoded_psbt(),
                "confirmation_hex": "c0ffee00",
                "secret_hex": "0011",
            }),
        );
        let decoded = negotiation_ok(
            "/api/payments",
            &serde_json::json!({
                "psbt": confirmed["psbt"].as_str().unwrap(),
                "secret_hex": "0011",
            }),
        );
        assert_eq!(decoded["confirmations"], serde_json::json!(["c0ffee00"]));
    }

    #[test]
    fn confirm_endpoint_derives_confirmation_from_current_psbt() {
        let peer_hex = "22".repeat(32);
        // One fixture instance: creation shuffles per-output unique ids, so
        // the derived unique id must be compared against THIS encoding.
        let source_psbt = encoded_psbt();
        let confirmed = negotiation_ok(
            "/api/confirm",
            &serde_json::json!({
                "psbt": source_psbt,
                "derive": true,
                "peer_id_hex": peer_hex,
            }),
        );
        let confirmed_psbt = confirmed["psbt"].as_str().unwrap();

        let decoded =
            negotiation_ok("/api/payments", &serde_json::json!({ "psbt": confirmed_psbt }));
        let confirmations = decoded["confirmations"].as_array().unwrap();
        assert_eq!(confirmations.len(), 1);
        let record_hex = confirmations[0].as_str().unwrap();
        let record_bytes = (0..record_hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&record_hex[i..i + 2], 16).unwrap())
            .collect::<Vec<_>>();
        let confirmation =
            concurrent_psbt::payments::negotiation::Confirmation::decode(&record_bytes).unwrap();
        assert_eq!(confirmation.peer_id, [0x22; 32]);
        let source = crate::io::parse_psbt_bytes("fixture", source_psbt.as_bytes()).unwrap();
        assert_eq!(
            confirmation.unique_id,
            concurrent_psbt::payments::negotiation::unordered_unique_id(&source)
        );

        // Re-deriving the identical confirmation deduplicates (the derived id
        // matches `ptj confirm`), instead of growing the band.
        let again = negotiation_ok(
            "/api/confirm",
            &serde_json::json!({
                "psbt": confirmed_psbt,
                "derive": true,
                "peer_id_hex": peer_hex,
            }),
        );
        let decoded = negotiation_ok(
            "/api/payments",
            &serde_json::json!({ "psbt": again["psbt"].as_str().unwrap() }),
        );
        assert_eq!(decoded["confirmations"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn negotiation_endpoints_report_json_errors() {
        // Missing `psbt` (all three routes share the shape).
        for path in ["/api/pay", "/api/confirm", "/api/payments"] {
            let response = response_for("POST", path, b"{}");
            assert_eq!(response.status, 400, "{path}");
            assert_eq!(response.content_type, "application/json; charset=utf-8");
            let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
            assert!(body["error"].as_str().unwrap().contains("`psbt`"), "{path}");
        }

        // Missing record field.
        let request = serde_json::json!({ "psbt": encoded_psbt() }).to_string();
        let response = response_for("POST", "/api/pay", request.as_bytes());
        assert_eq!(response.status, 400);
        assert!(
            String::from_utf8(response.body)
                .unwrap()
                .contains("`payment_hex`")
        );
        let response = response_for("POST", "/api/confirm", request.as_bytes());
        assert_eq!(response.status, 400);
        assert!(
            String::from_utf8(response.body)
                .unwrap()
                .contains("`confirmation_hex`")
        );

        // Malformed hex reports the CLI's exact error text.
        let request =
            serde_json::json!({ "psbt": encoded_psbt(), "payment_hex": "abc" }).to_string();
        let response = response_for("POST", "/api/pay", request.as_bytes());
        assert_eq!(response.status, 400);
        assert!(
            String::from_utf8(response.body)
                .unwrap()
                .contains("odd length")
        );

        // Dummy padding without a secret is refused (plaintext dummies are
        // trivially distinguishable), mirroring the CLI's --dummy guard.
        let request = serde_json::json!({
            "psbt": encoded_psbt(),
            "payment_hex": "deadbeef",
            "dummy": 1,
        })
        .to_string();
        let response = response_for("POST", "/api/pay", request.as_bytes());
        assert_eq!(response.status, 400);
        assert!(
            String::from_utf8(response.body)
                .unwrap()
                .contains("requires secret_hex")
        );
    }
}
