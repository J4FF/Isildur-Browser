//! WebSocket (RFC 6455, WHATWG WebSockets) and EventSource (HTML
//! "server-sent events"): the Rust half. The WebIDL half is
//! `web_net.js`, loaded on the first use of `WebSocket` or `EventSource`.
//!
//! Every connection runs on its own thread with plain std sockets (TLS for
//! wss/https through narsil-tls). The thread and the engine talk over
//! channels: commands in (send, close), events out (open, message, close,
//! error). The engine drains the events when the glue's poll timer calls
//! `N.poll()` — the timer exists only while a connection is alive, so pages
//! without connections pay nothing.
//!
//! The connection thread watches its socket with a short `peek` and only
//! reads once bytes are there (with a long timeout for the read itself), so
//! a TLS record is never cut by a poll timeout.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::Duration;

use crate::heap::GVal;
use crate::interp::{Engine, Stoerung};
use crate::web_platform::PlatformOp;

const GLUE: &str = include_str!("web_net.js");

/// The GUID of the opening handshake (RFC 6455 section 1.3).
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
/// Poll interval of the socket watch.
const PEEK: Duration = Duration::from_millis(5);
/// Timeout of a read once data is there, and of connecting.
const IO: Duration = Duration::from_secs(30);
/// Largest message the engine accepts (a bigger one closes with 1009).
const MAX_MESSAGE: usize = 64 * 1024 * 1024;
/// Largest response header block.
const MAX_HEADER: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NetOp {
    WsOpen,
    WsSend,
    WsClose,
    EsOpen,
    EsClose,
    Poll,
}

const NATIVES: &[(&str, NetOp, u32)] = &[
    ("wsOpen", NetOp::WsOpen, 3),
    ("wsSend", NetOp::WsSend, 3),
    ("wsClose", NetOp::WsClose, 3),
    ("esOpen", NetOp::EsOpen, 3),
    ("esClose", NetOp::EsClose, 1),
    ("poll", NetOp::Poll, 0),
];

/// Commands to a connection thread.
enum Cmd {
    Text(String),
    Binary(Vec<u8>),
    Close(Option<u16>, String),
    Stop,
}

/// Events from a connection thread.
enum Ev {
    /// WebSocket open (protocol, extensions) / EventSource open.
    Open(String, String),
    Text(String),
    Binary(Vec<u8>),
    /// WebSocket closed (code, reason, clean).
    Closed(u16, String, bool),
    /// EventSource message (type, data, lastEventId).
    Message(String, String, String),
    /// Connection failure; `true` = an EventSource reconnects.
    Error(bool),
    /// The WebSocket's buffered amount went down by this many bytes.
    Sent(usize),
}

struct Conn {
    tx: Sender<Cmd>,
    rx: Receiver<Ev>,
}

/// Connection table of one engine.
#[derive(Default)]
pub(crate) struct NetState {
    conns: HashMap<u32, Conn>,
    next: u32,
}

// ---------------------------------------------------------------------------
// URLs and streams
// ---------------------------------------------------------------------------

struct Target {
    tls: bool,
    host: String,
    port: u16,
    /// Path and query.
    path: String,
}

fn parse_target(url: &str) -> Result<Target, String> {
    let (scheme, rest) = url.split_once("://").ok_or("URL without scheme")?;
    let tls = match scheme.to_ascii_lowercase().as_str() {
        "ws" | "http" => false,
        "wss" | "https" => true,
        s => return Err(format!("unsupported scheme {s}")),
    };
    let (authority, path) = match rest.find(['/', '?']) {
        Some(i) => (&rest[..i], rest[i..].to_string()),
        None => (rest, "/".to_string()),
    };
    let path = match path.find('#') {
        Some(i) => path[..i].to_string(),
        None => path,
    };
    let path = if path.starts_with('?') { format!("/{path}") } else { path };
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let default = if tls { 443 } else { 80 };
    let (host, port) = if let Some(stripped) = authority.strip_prefix('[') {
        let end = stripped.find(']').ok_or("bad IPv6 host")?;
        let host = stripped[..end].to_string();
        let port = stripped[end + 1..].strip_prefix(':').map(|p| p.parse::<u16>()).transpose().map_err(|_| "bad port")?;
        (host, port.unwrap_or(default))
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse::<u16>().map_err(|_| "bad port")?),
            None => (authority.to_string(), default),
        }
    };
    Ok(Target { tls, host, port, path })
}

/// A plain or TLS stream with a handle on the socket for `peek`.
struct Stream {
    io: Box<dyn ReadWrite + Send>,
    sock: TcpStream,
}

trait ReadWrite: Read + Write {}
impl<T: Read + Write> ReadWrite for T {}

fn trust() -> Result<&'static narsil_tls::kette::Vertrauensspeicher, String> {
    use std::sync::OnceLock;
    static T: OnceLock<Result<narsil_tls::kette::Vertrauensspeicher, String>> = OnceLock::new();
    T.get_or_init(|| match std::env::var("NARSIL_CA_BUNDLE") {
        Ok(p) if !p.trim().is_empty() => std::fs::read_to_string(&p)
            .map_err(|e| e.to_string())
            .and_then(|t| narsil_tls::kette::Vertrauensspeicher::aus_pem(&t).map_err(|e| e.to_string())),
        _ => narsil_tls::kette::Vertrauensspeicher::aus_system(),
    })
    .as_ref()
    .map_err(|e| e.clone())
}

fn connect(t: &Target) -> Result<Stream, String> {
    use std::net::ToSocketAddrs;
    let addrs: Vec<_> = (t.host.as_str(), t.port).to_socket_addrs().map_err(|e| e.to_string())?.collect();
    let mut last = String::from("no address");
    for a in addrs {
        match TcpStream::connect_timeout(&a, IO) {
            Ok(s) => {
                let _ = s.set_nodelay(true);
                let _ = s.set_read_timeout(Some(IO));
                let _ = s.set_write_timeout(Some(IO));
                let sock = s.try_clone().map_err(|e| e.to_string())?;
                let io: Box<dyn ReadWrite + Send> = if t.tls {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_secs() as i64);
                    let tls = narsil_tls::strom::TlsStrom::ueber_sockel(s, &t.host, &[b"http/1.1"], trust()?, now)
                        .map_err(|e| e.to_string())?;
                    Box::new(tls)
                } else {
                    Box::new(s)
                };
                return Ok(Stream { io, sock });
            }
            Err(e) => last = e.to_string(),
        }
    }
    Err(last)
}

impl Stream {
    /// Wait up to `PEEK` for incoming bytes; `Ok(false)` = none yet,
    /// `Err` = the connection ended.
    fn ready(&mut self) -> Result<bool, ()> {
        let _ = self.sock.set_read_timeout(Some(PEEK));
        let mut b = [0u8; 1];
        let r = self.sock.peek(&mut b);
        let _ = self.sock.set_read_timeout(Some(IO));
        match r {
            Ok(0) => Err(()),
            Ok(_) => Ok(true),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => Ok(false),
            Err(_) => Err(()),
        }
    }

    fn read_some(&mut self, buf: &mut Vec<u8>) -> Result<usize, ()> {
        let mut tmp = [0u8; 16384];
        match self.io.read(&mut tmp) {
            Ok(0) => Err(()),
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                Ok(n)
            }
            Err(_) => Err(()),
        }
    }
}

/// Read the HTTP response head; returns (status, headers lower-cased, rest
/// of the buffer after the head).
fn read_head(s: &mut Stream, buf: &mut Vec<u8>) -> Result<(u16, Vec<(String, String)>), String> {
    loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..i]).to_string();
            buf.drain(..i + 4);
            let mut lines = head.split("\r\n");
            let status_line = lines.next().unwrap_or("");
            let status = status_line.split_whitespace().nth(1).and_then(|c| c.parse::<u16>().ok()).ok_or("bad status line")?;
            let mut headers = Vec::new();
            for l in lines {
                if let Some((k, v)) = l.split_once(':') {
                    headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
                }
            }
            return Ok((status, headers));
        }
        if buf.len() > MAX_HEADER {
            return Err("response head too large".into());
        }
        s.read_some(buf).map_err(|_| "connection closed during the handshake".to_string())?;
    }
}

fn header<'a>(h: &'a [(String, String)], name: &str) -> Option<&'a str> {
    h.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    s
}

fn host_header(t: &Target) -> String {
    let h = if t.host.contains(':') { format!("[{}]", t.host) } else { t.host.clone() };
    let default = if t.tls { 443 } else { 80 };
    if t.port == default { h } else { format!("{h}:{}", t.port) }
}

// ---------------------------------------------------------------------------
// WebSocket thread
// ---------------------------------------------------------------------------

fn ws_frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(payload.len() + 14);
    f.push(0x80 | opcode);
    let n = payload.len();
    if n < 126 {
        f.push(0x80 | n as u8);
    } else if n < 65536 {
        f.push(0x80 | 126);
        f.extend((n as u16).to_be_bytes());
    } else {
        f.push(0x80 | 127);
        f.extend((n as u64).to_be_bytes());
    }
    let mask = narsil_tls::web_crypto::random(4).unwrap_or_else(|_| vec![0x5a, 0x17, 0xc3, 0x09]);
    f.extend_from_slice(&mask);
    f.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    f
}

struct Frame {
    fin: bool,
    opcode: u8,
    payload: Vec<u8>,
}

/// One frame from the buffer, or `Ok(None)` if incomplete; `Err(code)` =
/// protocol error with the close code to send.
fn parse_frame(buf: &mut Vec<u8>) -> Result<Option<Frame>, u16> {
    if buf.len() < 2 {
        return Ok(None);
    }
    let fin = buf[0] & 0x80 != 0;
    if buf[0] & 0x70 != 0 {
        return Err(1002);
    }
    let opcode = buf[0] & 0x0f;
    let masked = buf[1] & 0x80 != 0;
    let mut len = (buf[1] & 0x7f) as u64;
    let mut pos = 2;
    if len == 126 {
        if buf.len() < 4 {
            return Ok(None);
        }
        len = u16::from_be_bytes([buf[2], buf[3]]) as u64;
        pos = 4;
    } else if len == 127 {
        if buf.len() < 10 {
            return Ok(None);
        }
        len = u64::from_be_bytes(buf[2..10].try_into().unwrap_or([0; 8]));
        pos = 10;
    }
    if len as usize > MAX_MESSAGE {
        return Err(1009);
    }
    // A server must not mask (RFC 6455 section 5.1).
    if masked {
        return Err(1002);
    }
    if opcode >= 8 && (len > 125 || !fin) {
        return Err(1002);
    }
    let end = pos + len as usize;
    if buf.len() < end {
        return Ok(None);
    }
    let payload = buf[pos..end].to_vec();
    buf.drain(..end);
    Ok(Some(Frame { fin, opcode, payload }))
}

fn ws_run(url: String, protocols: Vec<String>, origin: String, cmds: Receiver<Cmd>, evs: Sender<Ev>) {
    let fail = |evs: &Sender<Ev>| {
        let _ = evs.send(Ev::Error(false));
        let _ = evs.send(Ev::Closed(1006, String::new(), false));
    };
    let Ok(t) = parse_target(&url) else { return fail(&evs) };
    let Ok(mut s) = connect(&t) else { return fail(&evs) };
    let key = b64(&narsil_tls::web_crypto::random(16).unwrap_or_else(|_| vec![7; 16]));
    let mut req = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n",
        t.path,
        host_header(&t)
    );
    if !origin.is_empty() && origin != "null" {
        req.push_str(&format!("Origin: {origin}\r\n"));
    }
    if !protocols.is_empty() {
        req.push_str(&format!("Sec-WebSocket-Protocol: {}\r\n", protocols.join(", ")));
    }
    req.push_str("\r\n");
    if s.io.write_all(req.as_bytes()).is_err() {
        return fail(&evs);
    }
    let mut buf = Vec::new();
    let Ok((status, h)) = read_head(&mut s, &mut buf) else { return fail(&evs) };
    let accept = b64(&narsil_tls::web_crypto::hash::digest(
        narsil_tls::web_crypto::hash::HashAlg::Sha1,
        format!("{key}{WS_GUID}").as_bytes(),
    ));
    let upgrade_ok = header(&h, "upgrade").is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    let conn_ok = header(&h, "connection").is_some_and(|v| v.split(',').any(|p| p.trim().eq_ignore_ascii_case("upgrade")));
    let accept_ok = header(&h, "sec-websocket-accept") == Some(accept.as_str());
    let protocol = header(&h, "sec-websocket-protocol").unwrap_or("").to_string();
    let protocol_ok = if protocol.is_empty() { true } else { protocols.iter().any(|p| *p == protocol) };
    let extensions = header(&h, "sec-websocket-extensions").unwrap_or("").to_string();
    if status != 101 || !upgrade_ok || !conn_ok || !accept_ok || !protocol_ok || !extensions.is_empty() {
        return fail(&evs);
    }
    if evs.send(Ev::Open(protocol, extensions)).is_err() {
        return;
    }
    let mut message: Vec<u8> = Vec::new();
    let mut message_op: Option<u8> = None;
    let mut closing_sent = false;
    let mut close_deadline: Option<std::time::Instant> = None;
    loop {
        // Outgoing commands.
        loop {
            match cmds.try_recv() {
                Ok(Cmd::Text(t)) => {
                    let n = t.len();
                    if s.io.write_all(&ws_frame(1, t.as_bytes())).is_err() {
                        let _ = evs.send(Ev::Closed(1006, String::new(), false));
                        return;
                    }
                    let _ = evs.send(Ev::Sent(n));
                }
                Ok(Cmd::Binary(b)) => {
                    let n = b.len();
                    if s.io.write_all(&ws_frame(2, &b)).is_err() {
                        let _ = evs.send(Ev::Closed(1006, String::new(), false));
                        return;
                    }
                    let _ = evs.send(Ev::Sent(n));
                }
                Ok(Cmd::Close(code, reason)) => {
                    if !closing_sent {
                        let mut p = Vec::new();
                        if let Some(c) = code {
                            p.extend(c.to_be_bytes());
                            p.extend(reason.as_bytes());
                        }
                        let _ = s.io.write_all(&ws_frame(8, &p));
                        closing_sent = true;
                        close_deadline = Some(std::time::Instant::now() + Duration::from_secs(5));
                    }
                }
                Ok(Cmd::Stop) | Err(TryRecvError::Disconnected) => return,
                Err(TryRecvError::Empty) => break,
            }
        }
        if let Some(d) = close_deadline
            && std::time::Instant::now() > d
        {
            let _ = evs.send(Ev::Closed(1006, String::new(), false));
            return;
        }
        // Incoming frames: handle what is complete, else wait for bytes.
        let mut progressed = false;
        loop {
            let f = match parse_frame(&mut buf) {
                Ok(Some(f)) => {
                    progressed = true;
                    f
                }
                Ok(None) => break,
                Err(code) => {
                    let _ = s.io.write_all(&ws_frame(8, &code.to_be_bytes()));
                    let _ = evs.send(Ev::Error(false));
                    let _ = evs.send(Ev::Closed(1006, String::new(), false));
                    return;
                }
            };
            match f.opcode {
                0 | 1 | 2 => {
                    if f.opcode == 0 && message_op.is_none() || f.opcode != 0 && message_op.is_some() {
                        let _ = evs.send(Ev::Error(false));
                        let _ = evs.send(Ev::Closed(1006, String::new(), false));
                        return;
                    }
                    if f.opcode != 0 {
                        message_op = Some(f.opcode);
                    }
                    message.extend_from_slice(&f.payload);
                    if message.len() > MAX_MESSAGE {
                        let _ = s.io.write_all(&ws_frame(8, &1009u16.to_be_bytes()));
                        let _ = evs.send(Ev::Closed(1009, String::new(), false));
                        return;
                    }
                    if f.fin {
                        let data = std::mem::take(&mut message);
                        let ev = if message_op == Some(1) {
                            match String::from_utf8(data) {
                                Ok(t) => Ev::Text(t),
                                Err(_) => {
                                    let _ = s.io.write_all(&ws_frame(8, &1007u16.to_be_bytes()));
                                    let _ = evs.send(Ev::Error(false));
                                    let _ = evs.send(Ev::Closed(1006, String::new(), false));
                                    return;
                                }
                            }
                        } else {
                            Ev::Binary(data)
                        };
                        message_op = None;
                        if closing_sent {
                            continue;
                        }
                        if evs.send(ev).is_err() {
                            return;
                        }
                    }
                }
                8 => {
                    let (code, reason) = if f.payload.len() >= 2 {
                        (
                            u16::from_be_bytes([f.payload[0], f.payload[1]]),
                            String::from_utf8_lossy(&f.payload[2..]).to_string(),
                        )
                    } else {
                        (1005, String::new())
                    };
                    if !closing_sent {
                        let echo = if f.payload.len() >= 2 { f.payload[..2].to_vec() } else { Vec::new() };
                        let _ = s.io.write_all(&ws_frame(8, &echo));
                    }
                    let _ = evs.send(Ev::Closed(code, reason, true));
                    return;
                }
                9 => {
                    let _ = s.io.write_all(&ws_frame(10, &f.payload));
                }
                10 => {}
                _ => {
                    let _ = s.io.write_all(&ws_frame(8, &1002u16.to_be_bytes()));
                    let _ = evs.send(Ev::Error(false));
                    let _ = evs.send(Ev::Closed(1006, String::new(), false));
                    return;
                }
            }
        }
        if !progressed {
            match s.ready() {
                Ok(true) => {
                    if s.read_some(&mut buf).is_err() {
                        let _ = evs.send(Ev::Closed(1006, String::new(), false));
                        return;
                    }
                }
                Ok(false) => {}
                Err(()) => {
                    let _ = evs.send(Ev::Closed(1006, String::new(), false));
                    return;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// EventSource thread
// ---------------------------------------------------------------------------

/// The event-stream parser (HTML "interpret the event stream").
#[derive(Default)]
struct EsParser {
    line: Vec<u8>,
    data: String,
    has_data: bool,
    event: String,
    last_id: String,
    pending_id: String,
    retry: Option<u64>,
    bom_checked: bool,
    cr: bool,
}

impl EsParser {
    fn feed(&mut self, bytes: &[u8], out: &mut Vec<(String, String, String)>) {
        let mut bytes = bytes;
        if !self.bom_checked {
            if bytes.is_empty() {
                return;
            }
            self.bom_checked = true;
            if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
                bytes = &bytes[3..];
            }
        }
        for &b in bytes {
            if self.cr {
                self.cr = false;
                if b == b'\n' {
                    continue;
                }
            }
            if b == b'\r' || b == b'\n' {
                if b == b'\r' {
                    self.cr = true;
                }
                let line = String::from_utf8_lossy(&std::mem::take(&mut self.line)).to_string();
                self.line_done(&line, out);
            } else {
                self.line.push(b);
            }
        }
    }

    fn line_done(&mut self, line: &str, out: &mut Vec<(String, String, String)>) {
        if line.is_empty() {
            // Dispatch.
            self.last_id = self.pending_id.clone();
            if self.has_data {
                let data = std::mem::take(&mut self.data);
                let data = data.strip_suffix('\n').unwrap_or(&data).to_string();
                let ev = if self.event.is_empty() { "message".to_string() } else { std::mem::take(&mut self.event) };
                out.push((ev, data, self.last_id.clone()));
            }
            self.data.clear();
            self.has_data = false;
            self.event.clear();
            return;
        }
        if line.starts_with(':') {
            return;
        }
        let (field, value) = match line.find(':') {
            Some(i) => {
                let v = &line[i + 1..];
                (&line[..i], v.strip_prefix(' ').unwrap_or(v))
            }
            None => (line, ""),
        };
        match field {
            "event" => self.event = value.to_string(),
            "data" => {
                self.data.push_str(value);
                self.data.push('\n');
                self.has_data = true;
            }
            "id" => {
                if !value.contains('\0') {
                    self.pending_id = value.to_string();
                }
            }
            "retry" => {
                if !value.is_empty() && value.bytes().all(|c| c.is_ascii_digit()) {
                    self.retry = value.parse().ok();
                }
            }
            _ => {}
        }
    }
}

/// Body reader with chunked transfer encoding support.
struct Body {
    chunked: bool,
    /// Remaining bytes of the current chunk (chunked) or of the body
    /// (with Content-Length); `None` = until close.
    remaining: Option<u64>,
    raw: Vec<u8>,
    done: bool,
}

impl Body {
    /// Turn raw bytes into body bytes as far as possible.
    fn take(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        if !self.chunked {
            match self.remaining {
                Some(r) => {
                    let n = (r as usize).min(self.raw.len());
                    out.extend(self.raw.drain(..n));
                    self.remaining = Some(r - n as u64);
                    if r - n as u64 == 0 {
                        self.done = true;
                    }
                }
                None => out.append(&mut self.raw),
            }
            return out;
        }
        loop {
            match self.remaining {
                Some(0) | None => {
                    // Expect "\r\n" after a chunk (except before the first), then a size line.
                    let Some(i) = self.raw.windows(2).position(|w| w == b"\r\n") else { break };
                    let line = String::from_utf8_lossy(&self.raw[..i]).to_string();
                    self.raw.drain(..i + 2);
                    if line.is_empty() {
                        continue;
                    }
                    let size = u64::from_str_radix(line.split(';').next().unwrap_or("").trim(), 16).unwrap_or(0);
                    if size == 0 {
                        self.done = true;
                        break;
                    }
                    self.remaining = Some(size);
                }
                Some(r) => {
                    if self.raw.is_empty() {
                        break;
                    }
                    let n = (r as usize).min(self.raw.len());
                    out.extend(self.raw.drain(..n));
                    self.remaining = Some(r - n as u64);
                }
            }
        }
        out
    }
}

fn es_run(url: String, origin: String, credentials: bool, cmds: Receiver<Cmd>, evs: Sender<Ev>) {
    let mut parser = EsParser::default();
    let mut retry_ms: u64 = 3000;
    let mut current = url;
    loop {
        if matches!(cmds.try_recv(), Ok(Cmd::Stop) | Err(TryRecvError::Disconnected)) {
            return;
        }
        // One connection attempt (following redirects).
        let mut redirects = 0;
        let result = loop {
            let Ok(t) = parse_target(&current) else { break Err(false) };
            let Ok(mut s) = connect(&t) else { break Err(true) };
            let mut req = format!(
                "GET {} HTTP/1.1\r\nHost: {}\r\nAccept: text/event-stream\r\nCache-Control: no-cache\r\n",
                t.path,
                host_header(&t)
            );
            if !parser.last_id.is_empty() {
                req.push_str(&format!("Last-Event-ID: {}\r\n", parser.last_id));
            }
            if !origin.is_empty() && origin != "null" {
                req.push_str(&format!("Origin: {origin}\r\n"));
            }
            let _ = credentials;
            req.push_str("\r\n");
            if s.io.write_all(req.as_bytes()).is_err() {
                break Err(true);
            }
            let mut raw = Vec::new();
            let Ok((status, h)) = read_head(&mut s, &mut raw) else { break Err(true) };
            if (300..400).contains(&status) && status != 304 {
                if let Some(loc) = header(&h, "location") {
                    redirects += 1;
                    if redirects > 20 {
                        break Err(false);
                    }
                    current = narsil_url::loese_auf(&current, loc).unwrap_or_else(|_| loc.to_string());
                    continue;
                }
            }
            let mime = header(&h, "content-type").unwrap_or("").split(';').next().unwrap_or("").trim().to_ascii_lowercase();
            if status != 200 || mime != "text/event-stream" {
                break Err(false);
            }
            let te = header(&h, "transfer-encoding").unwrap_or("").to_ascii_lowercase();
            let remaining = if te.contains("chunked") {
                None
            } else {
                header(&h, "content-length").and_then(|v| v.parse::<u64>().ok())
            };
            let mut body = Body { chunked: te.contains("chunked"), remaining, raw, done: false };
            if evs.send(Ev::Open(String::new(), String::new())).is_err() {
                return;
            }
            // Stream the body.
            loop {
                let bytes = body.take();
                if !bytes.is_empty() {
                    let mut out = Vec::new();
                    parser.feed(&bytes, &mut out);
                    for (ty, data, id) in out {
                        if evs.send(Ev::Message(ty, data, id)).is_err() {
                            return;
                        }
                    }
                    if let Some(r) = parser.retry.take() {
                        retry_ms = r;
                    }
                }
                if body.done {
                    break;
                }
                match cmds.try_recv() {
                    Ok(Cmd::Stop) | Err(TryRecvError::Disconnected) => return,
                    _ => {}
                }
                match s.ready() {
                    Ok(true) => {
                        if s.read_some(&mut body.raw).is_err() {
                            break;
                        }
                    }
                    Ok(false) => {}
                    Err(()) => break,
                }
            }
            break Err(true);
        };
        match result {
            Ok(()) => return,
            Err(reconnect) => {
                if evs.send(Ev::Error(reconnect)).is_err() || !reconnect {
                    return;
                }
                // Wait `retry_ms`, watching for a stop.
                let until = std::time::Instant::now() + Duration::from_millis(retry_ms);
                while std::time::Instant::now() < until {
                    match cmds.recv_timeout(Duration::from_millis(20)) {
                        Ok(Cmd::Stop) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                        _ => {}
                    }
                }
                // A new attempt starts a fresh line buffer, keeps lastEventId.
                let last = std::mem::take(&mut parser.last_id);
                parser = EsParser { last_id: last.clone(), pending_id: last, ..Default::default() };
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Engine side
// ---------------------------------------------------------------------------

impl Engine {
    pub(crate) fn net_glue_laden(&mut self) -> Result<(), Stoerung> {
        let list: Vec<(&str, PlatformOp, u32)> =
            NATIVES.iter().map(|(n, o, l)| (*n, PlatformOp::Net(*o), *l)).collect();
        let natives = self.platform_natives(&list)?;
        self.platform_run_glue(GLUE, natives)
    }

    fn net_spawn(&mut self, run: impl FnOnce(Receiver<Cmd>, Sender<Ev>) + Send + 'static) -> u32 {
        let (ctx, crx) = channel();
        let (etx, erx) = channel();
        let st = &mut self.platform.net;
        st.next += 1;
        let id = st.next;
        st.conns.insert(id, Conn { tx: ctx, rx: erx });
        let spawned = std::thread::Builder::new()
            .name(format!("narsil-net-{id}"))
            .spawn(move || run(crx, etx));
        if spawned.is_err() {
            // The thread could not start: the connection fails at once.
            if let Some(c) = self.platform.net.conns.get_mut(&id) {
                let (tx, rx) = channel();
                let _ = tx.send(Ev::Error(false));
                let _ = tx.send(Ev::Closed(1006, String::new(), false));
                c.rx = rx;
            }
        }
        id
    }

    pub(crate) fn net_native(&mut self, op: NetOp, args: &[GVal]) -> Result<GVal, Stoerung> {
        match op {
            NetOp::WsOpen => {
                let url = self.zu_text(self.arg(args, 0))?;
                let protos = self.iter_werte(self.arg(args, 1))?;
                let mut protocols = Vec::with_capacity(protos.len());
                for p in protos {
                    protocols.push(self.zu_text(p)?);
                }
                let origin = self.zu_text(self.arg(args, 2))?;
                let id = self.net_spawn(move |rx, tx| ws_run(url, protocols, origin, rx, tx));
                Ok(GVal::Zahl(id as f64))
            }
            NetOp::EsOpen => {
                let url = self.zu_text(self.arg(args, 0))?;
                let origin = self.zu_text(self.arg(args, 1))?;
                let cred = self.zu_bool(self.arg(args, 2));
                let id = self.net_spawn(move |rx, tx| es_run(url, origin, cred, rx, tx));
                Ok(GVal::Zahl(id as f64))
            }
            NetOp::WsSend => {
                let id = self.zu_zahl(self.arg(args, 0))? as u32;
                let cmd = if self.zu_bool(self.arg(args, 2)) {
                    Cmd::Binary(self.buffer_source_bytes(self.arg(args, 1))?)
                } else {
                    Cmd::Text(self.zu_text(self.arg(args, 1))?)
                };
                if let Some(c) = self.platform.net.conns.get(&id) {
                    let _ = c.tx.send(cmd);
                }
                Ok(GVal::Undefiniert)
            }
            NetOp::WsClose => {
                let id = self.zu_zahl(self.arg(args, 0))? as u32;
                let code = match self.arg(args, 1) {
                    GVal::Undefiniert => None,
                    v => Some(self.zu_zahl(v)? as u16),
                };
                let reason = match self.arg(args, 2) {
                    GVal::Undefiniert => String::new(),
                    v => self.zu_text(v)?,
                };
                if let Some(c) = self.platform.net.conns.get(&id) {
                    let _ = c.tx.send(Cmd::Close(code, reason));
                }
                Ok(GVal::Undefiniert)
            }
            NetOp::EsClose => {
                let id = self.zu_zahl(self.arg(args, 0))? as u32;
                if let Some(c) = self.platform.net.conns.remove(&id) {
                    let _ = c.tx.send(Cmd::Stop);
                }
                Ok(GVal::Undefiniert)
            }
            NetOp::Poll => self.net_poll(),
        }
    }

    /// Drain all connection events: `[id, kind, a, b, c, ...]` flat
    /// (kinds: 0 open, 1 text, 2 binary, 3 closed, 4 message, 5 error,
    /// 6 sent). Finished connections leave the table.
    fn net_poll(&mut self) -> Result<GVal, Stoerung> {
        let mut events: Vec<(u32, Ev)> = Vec::new();
        let mut gone = Vec::new();
        for (id, c) in &self.platform.net.conns {
            loop {
                match c.rx.try_recv() {
                    Ok(ev) => {
                        let closed = matches!(ev, Ev::Closed(..) | Ev::Error(false));
                        events.push((*id, ev));
                        if closed && matches!(events.last(), Some((_, Ev::Closed(..)))) {
                            gone.push(*id);
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        gone.push(*id);
                        break;
                    }
                }
            }
        }
        for id in gone {
            self.platform.net.conns.remove(&id);
        }
        events.sort_by_key(|(id, _)| *id);
        let marke = self.wurzel_merke();
        let mut out: Vec<GVal> = Vec::new();
        let text = |e: &mut Engine, s: &str| -> Result<GVal, Stoerung> {
            let v = e.text_wert(s).map_err(Stoerung::Fehler)?;
            e.wurzeln.push(v);
            Ok(v)
        };
        for (id, ev) in events {
            out.push(GVal::Zahl(id as f64));
            match ev {
                Ev::Open(p, x) => {
                    out.push(GVal::Zahl(0.0));
                    out.push(text(self, &p)?);
                    out.push(text(self, &x)?);
                    out.push(GVal::Undefiniert);
                }
                Ev::Text(t) => {
                    out.push(GVal::Zahl(1.0));
                    out.push(text(self, &t)?);
                    out.push(GVal::Undefiniert);
                    out.push(GVal::Undefiniert);
                }
                Ev::Binary(b) => {
                    out.push(GVal::Zahl(2.0));
                    let ab = self.new_array_buffer(b)?;
                    self.wurzeln.push(ab);
                    out.push(ab);
                    out.push(GVal::Undefiniert);
                    out.push(GVal::Undefiniert);
                }
                Ev::Closed(code, reason, clean) => {
                    out.push(GVal::Zahl(3.0));
                    out.push(GVal::Zahl(code as f64));
                    out.push(text(self, &reason)?);
                    out.push(GVal::Bool(clean));
                }
                Ev::Message(ty, data, id) => {
                    out.push(GVal::Zahl(4.0));
                    out.push(text(self, &ty)?);
                    out.push(text(self, &data)?);
                    out.push(text(self, &id)?);
                }
                Ev::Error(reconnect) => {
                    out.push(GVal::Zahl(5.0));
                    out.push(GVal::Bool(reconnect));
                    out.push(GVal::Undefiniert);
                    out.push(GVal::Undefiniert);
                }
                Ev::Sent(n) => {
                    out.push(GVal::Zahl(6.0));
                    out.push(GVal::Zahl(n as f64));
                    out.push(GVal::Undefiniert);
                    out.push(GVal::Undefiniert);
                }
            }
        }
        let r = self.array_of(&out);
        self.wurzel_zurueck(marke);
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_stream_parser() {
        let mut p = EsParser::default();
        let mut out = Vec::new();
        p.feed(b"\xef\xbb\xbfdata: a\r\ndata:b\n\nid: 7\nevent: x\ndata\n\n: c\nretry: 50\r\r", &mut out);
        assert_eq!(out, vec![("message".into(), "a\nb".into(), "".into()), ("x".into(), "".into(), "7".into())]);
        assert_eq!(p.retry, Some(50));
    }

    #[test]
    fn frames_round_trip_and_reject_masked_server_frames() {
        // An unmasked server text frame "hi".
        let mut buf = vec![0x81, 0x02, b'h', b'i'];
        let f = parse_frame(&mut buf).unwrap().unwrap();
        assert!(f.fin && f.opcode == 1 && f.payload == b"hi");
        let mut masked = ws_frame(1, b"x");
        assert_eq!(parse_frame(&mut masked).err(), Some(1002));
        let t = parse_target("wss://example.test:8443/chat?x=1#f").unwrap();
        assert!(t.tls && t.host == "example.test" && t.port == 8443 && t.path == "/chat?x=1");
    }
}
