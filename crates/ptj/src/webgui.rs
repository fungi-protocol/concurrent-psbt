use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};

use crate::cli::WebguiConfig;
use crate::{Error, Result};

const INDEX_HTML: &[u8] = include_bytes!("../../../contrib/demo-gui/index.html");
const STYLES_CSS: &[u8] = include_bytes!("../../../contrib/demo-gui/styles.css");
const APP_JS: &[u8] = include_bytes!("../../../contrib/demo-gui/dist/app.js");
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
        "/dist/model.js" => Some(Asset {
            content_type: "text/javascript; charset=utf-8",
            body: MODEL_JS,
        }),
        _ => None,
    }
}

pub(crate) fn response_for(method: &str, path: &str, body: &[u8]) -> Response {
    if method == "POST" && path.split_once('?').map_or(path, |(path, _)| path) == "/api/inspect" {
        return inspect_response(body);
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
        assert_eq!(inspected["sort"]["mode"], "deterministic");
        assert_eq!(inspected["sort"]["seed_hex"], "abcd");
    }

    #[test]
    fn response_for_preserves_static_asset_http_behavior() {
        let index = response_for("GET", "/", b"");
        assert_eq!(index.status, 200);
        assert_eq!(index.content_type, "text/html; charset=utf-8");
        assert!(!index.body.is_empty());

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
        crate::run(
            Cli::try_parse_from([
                "ptj",
                "create",
                "--network",
                "regtest",
                "--input",
                &format!("{TXID}:7"),
                "--output",
                &format!("{}:0.00123456", regtest_address()),
                "--seed",
                "abcd",
            ])
            .unwrap(),
        )
        .unwrap()
    }

    fn regtest_address() -> String {
        let secp = bitcoin::secp256k1::Secp256k1::new();
        let secret = bitcoin::secp256k1::SecretKey::from_slice(&[1; 32]).unwrap();
        let public_key = bitcoin::secp256k1::PublicKey::from_secret_key(&secp, &secret);
        let public_key = bitcoin::CompressedPublicKey::from_slice(&public_key.serialize()).unwrap();
        bitcoin::Address::p2wpkh(&public_key, bitcoin::Network::Regtest).to_string()
    }
}
