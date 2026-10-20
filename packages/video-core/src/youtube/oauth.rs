//! Explicit PKCE loopback connection. The browser URL is returned only through a
//! deliberate accessor; Debug never includes state, verifier or client secrets.
use super::{
    Authorization, ConnectionId,
    credentials::Tokens,
    http::{Method, Request, Transport},
    protocol::{self, ClientConfig, SCOPE, TOKEN_URL},
};
use crate::{Result, media::Cancellation};
use serde::Deserialize;
use std::{
    collections::HashSet,
    io::{Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use url::Url;

pub(super) fn now_ms() -> Result<u64> {
    u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())
        .map_err(|_| "System clock is outside the supported token range".into())
}
pub(super) fn begin(config: ClientConfig, generation: String) -> Result<Authorization> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let state = protocol::random_secret()?;
    let verifier = protocol::random_secret()?;
    let path = format!("/oauth2/callback/{}", protocol::random_secret()?);
    let redirect = format!("http://127.0.0.1:{port}{path}");
    let browser_url = protocol::authorization_url(&config, &redirect, &state, &verifier)?;
    Ok(Authorization {
        listener,
        state,
        verifier,
        redirect,
        path,
        port,
        config,
        generation,
        browser_url,
        started: Instant::now(),
    })
}
pub(super) fn finish(
    auth: Authorization,
    http: &dyn Transport,
    cancel: &Cancellation,
) -> Result<Tokens> {
    let code = receive(&auth, cancel)?;
    cancel.check()?;
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("client_id", auth.config.id.as_str()),
            ("client_secret", auth.config.secret.as_str()),
            ("code", code.as_str()),
            ("code_verifier", auth.verifier.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", auth.redirect.as_str()),
        ])
        .finish();
    let response = http.send(Request {
        method: Method::Post,
        url: TOKEN_URL.into(),
        headers: vec![("Content-Type", "application/x-www-form-urlencoded".into())],
        body: body.into_bytes(),
    })?;
    cancel.check()?;
    parse_tokens(
        response.status,
        &response.body,
        None,
        protocol::connection()?,
    )
}
pub(super) fn refresh(
    config: &ClientConfig,
    previous: &Tokens,
    http: &dyn Transport,
    cancel: &Cancellation,
) -> Result<Tokens> {
    cancel.check()?;
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("client_id", config.id.as_str()),
            ("client_secret", config.secret.as_str()),
            ("refresh_token", previous.refresh.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .finish();
    let response = http.send(Request {
        method: Method::Post,
        url: TOKEN_URL.into(),
        headers: vec![("Content-Type", "application/x-www-form-urlencoded".into())],
        body: body.into_bytes(),
    })?;
    cancel.check()?;
    parse_tokens(
        response.status,
        &response.body,
        Some(previous),
        previous.connection.clone(),
    )
}
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    token_type: String,
    expires_in: u64,
    refresh_token: Option<String>,
    scope: Option<String>,
}
pub(super) fn parse_tokens(
    status: u16,
    bytes: &[u8],
    previous: Option<&Tokens>,
    connection: ConnectionId,
) -> Result<Tokens> {
    if status != 200 {
        return Err("Google refused the YouTube permission or refresh; connect again".into());
    }
    let t: TokenResponse = serde_json::from_slice(bytes)
        .map_err(|_| "Google returned an invalid permission response; connect again")?;
    if !t.token_type.eq_ignore_ascii_case("Bearer")
        || !protocol::bounded_secret(&t.access_token, 4096)
        || t.expires_in == 0
        || t.expires_in > 86_400
        || t.scope.as_deref().map_or(previous.is_none(), |scope| {
            let scopes: Vec<_> = scope.split_whitespace().collect();
            scopes != [SCOPE]
        })
    {
        return Err("Google did not grant the expected YouTube upload permission".into());
    }
    let refresh = t
        .refresh_token
        .or_else(|| previous.map(|p| p.refresh.clone()))
        .filter(|v| protocol::bounded_secret(v, 4096))
        .ok_or("Google returned no refresh permission; reconnect and approve offline access")?;
    let expires_unix_ms = now_ms()?
        .checked_add(t.expires_in * 1000)
        .ok_or("Google token expiry overflows the system clock")?;
    Ok(Tokens {
        access: t.access_token,
        refresh,
        expires_unix_ms,
        connection,
    })
}
fn receive(auth: &Authorization, cancel: &Cancellation) -> Result<String> {
    let mut rejected = 0;
    loop {
        cancel.check()?;
        if auth.started.elapsed() > Duration::from_secs(180) {
            return Err("YouTube connection timed out; select Connect again".into());
        }
        match auth.listener.accept() {
            Ok((mut stream, peer)) => {
                if !peer.ip().is_loopback() {
                    continue;
                }
                stream.set_read_timeout(Some(Duration::from_millis(200)))?;
                stream.set_write_timeout(Some(Duration::from_millis(200)))?;
                match callback(&mut stream, auth) {
                    Ok(Callback::Code(code)) => {
                        respond(
                            &mut stream,
                            200,
                            "YouTube connection received. Return to Storyfold.",
                        );
                        return Ok(code);
                    }
                    Ok(Callback::Denied) => {
                        respond(
                            &mut stream,
                            200,
                            "YouTube permission was declined. Return to Storyfold.",
                        );
                        return Err(
                            "Google permission was declined; no account was connected".into()
                        );
                    }
                    Err(_) => {
                        respond(&mut stream, 400, "Invalid connection response.");
                        rejected += 1;
                        if rejected >= 32 {
                            return Err(
                                "Too many invalid connection responses; select Connect again"
                                    .into(),
                            );
                        }
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20))
            }
            Err(_) => return Err("Local YouTube callback failed; select Connect again".into()),
        }
    }
}
pub(super) enum Callback {
    Code(String),
    Denied,
}
fn callback(stream: &mut TcpStream, auth: &Authorization) -> Result<Callback> {
    let mut header = Vec::new();
    let mut buffer = [0_u8; 512];
    loop {
        let n = stream.read(&mut buffer)?;
        if n == 0 {
            return Err("Incomplete callback".into());
        }
        header.extend_from_slice(&buffer[..n]);
        if header.len() > 8192 {
            return Err("Callback too large".into());
        }
        if header.windows(4).any(|v| v == b"\r\n\r\n") {
            break;
        }
    }
    parse_callback(&header, &auth.path, auth.port, &auth.state)
}
pub(super) fn parse_callback(
    header: &[u8],
    path: &str,
    port: u16,
    state: &str,
) -> Result<Callback> {
    let request = std::str::from_utf8(header).map_err(|_| "Invalid callback encoding")?;
    let (headers, body) = request
        .split_once("\r\n\r\n")
        .ok_or("Incomplete callback")?;
    if !body.is_empty() {
        return Err("Callback body is not supported".into());
    }
    let mut lines = headers.split("\r\n");
    let parts: Vec<_> = lines
        .next()
        .ok_or("Missing callback method")?
        .split(' ')
        .collect();
    if parts.len() != 3 || parts[0] != "GET" || parts[2] != "HTTP/1.1" || !parts[1].starts_with('/')
    {
        return Err("Unsupported callback request".into());
    }
    let mut names = HashSet::new();
    let mut host = None;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or("Invalid callback header")?;
        let name = name.to_ascii_lowercase();
        if !names.insert(name.clone())
            || name == "transfer-encoding"
            || (name == "content-length" && value.trim() != "0")
        {
            return Err("Invalid callback headers".into());
        }
        if name == "host" {
            host = Some(value.trim());
        }
    }
    if host != Some(format!("127.0.0.1:{port}").as_str()) {
        return Err("Unexpected callback host".into());
    }
    let url = Url::parse(&format!("http://127.0.0.1:{port}{}", parts[1]))?;
    if url.path() != path || url.fragment().is_some() {
        return Err("Unexpected callback path".into());
    }
    let mut query = std::collections::HashMap::new();
    for (key, value) in url.query_pairs() {
        if query.insert(key.to_string(), value.to_string()).is_some() {
            return Err("Duplicate callback query field".into());
        }
    }
    let received = query.get("state").ok_or("Missing callback state")?;
    let equal = received.len() == state.len()
        && received
            .bytes()
            .zip(state.bytes())
            .fold(0, |diff, (a, b)| diff | (a ^ b))
            == 0;
    if !equal {
        return Err("Callback state does not match".into());
    }
    if query.contains_key("error") {
        if query.contains_key("code") {
            return Err("Ambiguous callback".into());
        }
        return Ok(Callback::Denied);
    }
    let code = query
        .remove("code")
        .filter(|v| protocol::bounded_secret(v, 4096))
        .ok_or("Missing callback code")?;
    Ok(Callback::Code(code))
}
fn respond(stream: &mut TcpStream, status: u16, text: &str) {
    let reason = if status == 200 { "OK" } else { "Bad Request" };
    let _ = write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{text}",
        text.len()
    );
}
