use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::str::FromStr as _;

use crate::cli::{CreateConfig, NetworkArg, OrderingArg, OutPointArg, OutputArg, WebguiConfig};
use crate::{Error, Result};

const INDEX_HTML: &[u8] = include_bytes!("../../../contrib/demo-gui/index.html");
const STYLES_CSS: &[u8] = include_bytes!("../../../contrib/demo-gui/styles.css");
const APP_JS: &[u8] = include_bytes!("../../../contrib/demo-gui/dist/app.js");
const BACKEND_JS: &[u8] = include_bytes!("../../../contrib/demo-gui/dist/backend.js");
const MODEL_JS: &[u8] = include_bytes!("../../../contrib/demo-gui/dist/model.js");

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

pub fn asset(path: &str) -> Option<Asset> {
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    match path {
        "/" | "/index.html" => Some(Asset {
            content_type: "text/html; charset=utf-8",
            body: INDEX_HTML,
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
        _ => None,
    }
}

pub(crate) fn response_for(method: &str, path: &str, body: &[u8]) -> Response {
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    if method == "POST" {
        match path {
            "/api/atomize" => return atomize_response(body),
            "/api/concatenate" => return concatenate_response(body),
            "/api/create" => return create_response(body),
            "/api/export-bip174" => return export_bip174_response(body),
            "/api/import-bip174" => return import_bip174_response(body),
            "/api/inspect" => return inspect_response(body),
            "/api/join" => return join_response(body),
            "/api/make-unordered" => return make_unordered_response(body),
            "/api/sort" => return sort_response(body),
            _ => {}
        }
    }

    if method != "GET" && method != "HEAD" {
        return text_response(405, "Method Not Allowed", "method not allowed");
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

    Ok(CreateConfig {
        inputs,
        outputs,
        seed,
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
    let psbt = crate::io::parse_bip174_bytes("request psbt", psbt.as_bytes())?;
    let imported = crate::commands::import_bip174::import_bip174_psbt(psbt)?;
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
    let psbt = crate::io::parse_psbt_bytes("request psbt", psbt.as_bytes())?;
    let constructor =
        concurrent_psbt::roles::constructor::dynamic::Constructor::try_from_psbt(psbt)
            .map_err(|error| Error::new(format!("request psbt: {error}")))?;
    let sorted = crate::commands::sort::sort_psbt(constructor.into_inner(), seed)?;
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
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        response.body.len(),
        status = response.status,
        reason = response.reason,
        content_type = response.content_type,
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
        assert_eq!(inspected["sort"]["seed_hex"], "abcd");
    }

    #[test]
    fn response_for_preserves_static_asset_http_behavior() {
        let index = response_for("GET", "/", b"");
        assert_eq!(index.status, 200);
        assert_eq!(index.content_type, "text/html; charset=utf-8");
        assert!(!index.body.is_empty());

        let app = response_for("GET", "/dist/app.js?v=cache-busted", b"");
        assert_eq!(app.status, 200);
        assert_eq!(app.content_type, "text/javascript; charset=utf-8");
        assert!(String::from_utf8(app.body).unwrap().contains("backend.js"));

        let backend = response_for("GET", "/dist/backend.js?v=cache-busted", b"");
        assert_eq!(backend.status, 200);
        assert_eq!(backend.content_type, "text/javascript; charset=utf-8");
        assert!(
            String::from_utf8(backend.body)
                .unwrap()
                .contains("export function joinPsbts")
        );

        let head = response_for("HEAD", "/dist/app.js?v=cache-busted", b"");
        assert_eq!(head.status, 200);
        assert_eq!(head.content_type, "text/javascript; charset=utf-8");
        assert!(head.body.is_empty());

        assert_eq!(response_for("GET", "/missing.js", b"").status, 404);
        assert_eq!(response_for("PUT", "/", b"").status, 405);
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
    fn sort_endpoint_returns_ordered_psbt_and_inspection() {
        let request = serde_json::json!({
            "psbt": encoded_psbt(),
            "seed_hex": "deadbeef",
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
            "seed_hex": "abcd",
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
        assert_eq!(body["inspect"]["sort"]["seed_hex"], "abcd");
    }

    #[test]
    fn create_endpoint_preserves_unset_ordering_seed_without_deterministic_mode() {
        let request = serde_json::json!({
            "network": "regtest",
            "inputs": [
                { "txid": TXID, "vout": 7 },
            ],
            "seed_hex": "abcd",
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
            crate::commands::sort::sort_psbt(constructor.into_inner(), Some(vec![0xab, 0xcd]))
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
                "abcd",
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
                "abcd",
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
}
