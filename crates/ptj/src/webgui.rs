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
    let mut buffer = [0; 8192];
    let read = stream
        .read(&mut buffer)
        .map_err(|error| Error::new(format!("reading HTTP request: {error}")))?;
    let request = std::str::from_utf8(&buffer[..read])
        .map_err(|error| Error::new(format!("HTTP request was not UTF-8: {error}")))?;
    let Some(request_line) = request.lines().next() else {
        return write_response(&mut stream, 400, "Bad Request", "text/plain; charset=utf-8", b"bad request");
    };
    let parts = request_line.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 3 {
        return write_response(&mut stream, 400, "Bad Request", "text/plain; charset=utf-8", b"bad request");
    }
    let method = parts[0];
    let path = parts[1];
    if method != "GET" && method != "HEAD" {
        return write_response(
            &mut stream,
            405,
            "Method Not Allowed",
            "text/plain; charset=utf-8",
            b"method not allowed",
        );
    }
    let Some(asset) = asset(path) else {
        return write_response(&mut stream, 404, "Not Found", "text/plain; charset=utf-8", b"not found");
    };
    write_response(
        &mut stream,
        200,
        "OK",
        asset.content_type,
        if method == "HEAD" { b"" } else { asset.body },
    )
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    content_type: &str,
    body: &[u8],
) -> Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .map_err(|error| Error::new(format!("writing HTTP headers: {error}")))?;
    stream
        .write_all(body)
        .map_err(|error| Error::new(format!("writing HTTP body: {error}")))
}
