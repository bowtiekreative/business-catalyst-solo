//! Real public search and bounded public-only browser egress. No model-invented
//! search results, browser login reuse, plaintext proxying or private targets.
use crate::{Result, media::Cancellation};
use quick_xml::{Reader, events::Event};
use std::{
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub description: String,
}
pub fn search(query: &str, count: u8, cancel: &Cancellation) -> Result<Vec<SearchHit>> {
    if query.trim().is_empty() || query.chars().count() > 300 || !(1..=12).contains(&count) {
        return Err("Enter a search up to 300 characters and choose 1–12 results".into());
    }
    catalyst_core::check_secrets(query)?;
    cancel.check()?;
    // shortcut: Bing's public RSS adapter requires no API secret, but is not a
    // versioned search API. Failure is explicit; manual URL/screenshot remain.
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .user_agent("Storyfold/0.1")
        .build()?;
    let response = client
        .get("https://www.bing.com/search")
        .query(&[("format", "rss"), ("q", query)])
        .send()?
        .error_for_status()?;
    let mut bytes = Vec::new();
    response.take(262145).read_to_end(&mut bytes)?;
    cancel.check()?;
    if bytes.len() > 262144 {
        return Err("Search response exceeds 256 KiB".into());
    }
    parse(&bytes, count)
}
fn parse(bytes: &[u8], count: u8) -> Result<Vec<SearchHit>> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(false);
    let (mut hits, mut current, mut field) = (Vec::new(), None::<SearchHit>, String::new());
    loop {
        match reader.read_event()? {
            Event::Start(e) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                if name == "item" {
                    current = Some(SearchHit {
                        title: String::new(),
                        url: String::new(),
                        description: String::new(),
                    });
                }
                field = name;
            }
            Event::Text(e) => {
                if let Some(hit) = &mut current {
                    let decoded = e.decode()?;
                    let text = quick_xml::escape::unescape(&decoded)?;
                    match field.as_str() {
                        "title" => hit.title.push_str(&text),
                        "link" => hit.url.push_str(&text),
                        "description" => hit.description.push_str(&text),
                        _ => {}
                    }
                }
            }
            Event::GeneralRef(e) => {
                if let Some(hit) = &mut current {
                    let entity = format!("&{};", e.decode()?);
                    let text = quick_xml::escape::unescape(&entity)?;
                    match field.as_str() {
                        "title" => hit.title.push_str(&text),
                        "link" => hit.url.push_str(&text),
                        "description" => hit.description.push_str(&text),
                        _ => {}
                    }
                }
            }
            Event::End(e) => {
                if e.name().as_ref() == b"item"
                    && let Some(hit) = current.take()
                    && !hit.title.trim().is_empty()
                    && hit.title.len() <= 1024
                    && hit.description.len() <= 4096
                    && url_syntax(&hit.url).is_ok()
                    && !hits.iter().any(|h: &SearchHit| h.url == hit.url)
                {
                    hits.push(hit);
                    if hits.len() == usize::from(count) {
                        break;
                    }
                }
                field.clear();
            }
            Event::DocType(_) => return Err("Search returned an unsupported document".into()),
            Event::Eof => break,
            _ => {}
        }
    }
    if hits.is_empty() {
        return Err(
            "No usable public HTTPS results. Try another search or paste a URL/drop a screenshot."
                .into(),
        );
    }
    Ok(hits)
}
fn url_syntax(value: &str) -> Result<url::Url> {
    if value.len() > 2048 || value.chars().any(char::is_control) {
        return Err("Invalid public page URL".into());
    }
    let u = url::Url::parse(value)?;
    if u.scheme() != "https"
        || !u.username().is_empty()
        || u.password().is_some()
        || u.port_or_known_default() != Some(443)
    {
        return Err("Use a public HTTPS page on the standard port, without credentials".into());
    }
    let host = u.host_str().ok_or("Missing page host")?;
    if host == "localhost"
        || !host.contains('.')
        || [".local", ".localhost", ".internal"]
            .iter()
            .any(|suffix| host.ends_with(suffix))
    {
        return Err("Local network pages cannot be captured".into());
    }
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>()
        && !public_ip(ip)
    {
        return Err("Private network pages cannot be captured".into());
    }
    catalyst_core::check_secrets(value)?;
    Ok(u)
}
pub(super) fn public_url(value: &str) -> Result<url::Url> {
    let u = url_syntax(value)?;
    resolve(u.host_str().ok_or("Missing page host")?)?;
    Ok(u)
}
fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0
        || a == 10
        || a == 127
        || a >= 224
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && (b == 168 || b == 0 || (b == 88 && c == 99)))
        || (a == 198 && (b == 18 || b == 19 || b == 51 && c == 100))
        || (a == 203 && b == 0 && c == 113))
}
fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => public_v4(v),
        IpAddr::V6(v) => {
            let s = v.segments();
            s[0] & 0xe000 == 0x2000
                && !(s[0] == 0x2001 && (s[1] == 0xdb8 || s[1] < 0x200))
                && s[0] != 0x2002
        }
    }
}
fn resolve(host: &str) -> Result<Vec<SocketAddr>> {
    let ips: Vec<_> = (host, 443).to_socket_addrs()?.collect();
    if ips.is_empty() || ips.iter().any(|a| !public_ip(a.ip())) {
        return Err("Page resolves to an unsupported or private network address".into());
    }
    Ok(ips)
}
pub(crate) struct PublicProxy {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
}
impl PublicProxy {
    pub fn address(&self) -> SocketAddr {
        self.address
    }
    pub fn start() -> Result<Self> {
        Self::start_with_limits(Duration::from_secs(35), 64 * 1024 * 1024, 128)
    }
    pub fn start_with_limits(
        timeout: Duration,
        max_bytes: u64,
        max_connections: u32,
    ) -> Result<Self> {
        let limits = ProxyLimits::new(timeout, max_bytes, max_connections)?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let token = stop.clone();
        let bytes = Arc::new(AtomicU64::new(0));
        thread::Builder::new()
            .name("storyfold-public-proxy".into())
            .spawn(move || {
                let started = Instant::now();
                let mut accepted = 0;
                while !token.load(Ordering::Relaxed) && started.elapsed() < limits.timeout {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            accepted += 1;
                            if accepted > limits.connections {
                                drop(stream);
                                continue;
                            }
                            let stop = token.clone();
                            let bytes = bytes.clone();
                            let _ = thread::Builder::new()
                                .name("storyfold-public-tunnel".into())
                                .spawn(move || {
                                    let _ = tunnel(stream, stop, bytes, started, limits);
                                });
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10))
                        }
                        Err(_) => break,
                    }
                }
            })?;
        Ok(Self { address, stop })
    }
}
#[derive(Clone, Copy)]
struct ProxyLimits {
    timeout: Duration,
    bytes: u64,
    connections: u32,
}
impl ProxyLimits {
    fn new(timeout: Duration, bytes: u64, connections: u32) -> Result<Self> {
        if timeout.is_zero()
            || timeout > Duration::from_secs(1800)
            || bytes == 0
            || bytes > 4 * 1024 * 1024 * 1024
            || connections == 0
            || connections > 512
        {
            return Err("Public transfer limits are outside the supported bounds".into());
        }
        Ok(Self {
            timeout,
            bytes,
            connections,
        })
    }
}
#[allow(deprecated)] // fetch_update remains available on the declared Rust1.88 minimum.
fn reserve_bytes(total: &AtomicU64, amount: u64, limit: u64) -> bool {
    total
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |old| {
            old.checked_add(amount).filter(|next| *next <= limit)
        })
        .is_ok()
}
impl Drop for PublicProxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
fn tunnel(
    mut client: TcpStream,
    stop: Arc<AtomicBool>,
    bytes: Arc<AtomicU64>,
    started: Instant,
    limits: ProxyLimits,
) -> Result<()> {
    client.set_read_timeout(Some(Duration::from_secs(2)))?;
    client.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut header = Vec::new();
    let deadline = Instant::now();
    while !header.ends_with(b"\r\n\r\n") {
        if header.len() >= 8192
            || stop.load(Ordering::Relaxed)
            || deadline.elapsed() > Duration::from_secs(5)
        {
            return Err("Invalid proxy request".into());
        }
        let mut b = [0];
        client.read_exact(&mut b)?;
        header.push(b[0]);
    }
    let header = std::str::from_utf8(&header)?;
    let line = header.lines().next().ok_or("Missing proxy request")?;
    let parts: Vec<_> = line.split_whitespace().collect();
    if parts.len() != 3 || parts[0] != "CONNECT" || !parts[1].ends_with(":443") {
        client.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")?;
        return Err("Only public HTTPS tunnels are allowed".into());
    }
    let url = url_syntax(&format!("https://{}/", parts[1]))?;
    let addresses = resolve(url.host_str().ok_or("Missing tunnel host")?)?;
    if stop.load(Ordering::Relaxed) {
        return Ok(());
    }
    let mut remote = None;
    for address in addresses.into_iter().take(4) {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        if let Ok(stream) = TcpStream::connect_timeout(&address, Duration::from_secs(3)) {
            remote = Some(stream);
            break;
        }
    }
    let remote = remote.ok_or("Could not connect to public page")?;
    client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")?;
    remote.set_read_timeout(Some(Duration::from_millis(250)))?;
    remote.set_write_timeout(Some(Duration::from_secs(2)))?;
    client.set_read_timeout(Some(Duration::from_millis(250)))?;
    let upstream = client.try_clone()?;
    let downstream = remote.try_clone()?;
    let up_stop = stop.clone();
    let up_bytes = bytes.clone();
    let worker =
        thread::spawn(move || relay(upstream, remote, &up_stop, &up_bytes, started, limits));
    relay(downstream, client, &stop, &bytes, started, limits);
    let _ = worker.join();
    Ok(())
}
fn relay(
    mut from: TcpStream,
    mut to: TcpStream,
    stop: &AtomicBool,
    total: &AtomicU64,
    started: Instant,
    limits: ProxyLimits,
) {
    let mut buffer = [0_u8; 16384];
    while !stop.load(Ordering::Relaxed) && started.elapsed() < limits.timeout {
        match from.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                if !reserve_bytes(total, n as u64, limits.bytes)
                    || to.write_all(&buffer[..n]).is_err()
                {
                    break;
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => break,
        }
    }
    let _ = to.shutdown(Shutdown::Both);
    let _ = from.shutdown(Shutdown::Both);
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transfer_limits_are_bounded_and_shared_bytes_never_overshoot() {
        assert!(ProxyLimits::new(Duration::ZERO, 1, 1).is_err());
        assert!(ProxyLimits::new(Duration::from_secs(1801), 1, 1).is_err());
        assert!(ProxyLimits::new(Duration::from_secs(30), 0, 1).is_err());
        assert!(ProxyLimits::new(Duration::from_secs(30), u64::MAX, 1).is_err());
        assert!(ProxyLimits::new(Duration::from_secs(30), 1, 513).is_err());
        let total = AtomicU64::new(0);
        assert!(reserve_bytes(&total, 6, 10));
        assert!(!reserve_bytes(&total, 5, 10));
        assert_eq!(total.load(Ordering::Relaxed), 6);
        assert!(reserve_bytes(&total, 4, 10));
        assert!(!reserve_bytes(&total, u64::MAX, 10));
        assert_eq!(total.load(Ordering::Relaxed), 10);
    }
    #[test]
    fn private_addresses_and_non_https_never_reach_dns_or_browser() {
        for u in [
            "http://example.com",
            "https://localhost",
            "https://127.0.0.1",
            "https://169.254.169.254",
            "https://user:pass@example.com",
            "https://example.com:8443",
            "file:///tmp/page",
            "https://192.168.1.1",
            "https://[::1]",
        ] {
            assert!(url_syntax(u).is_err(), "{u}");
        }
        assert!(url_syntax("https://example.org/hello").is_ok());
        for v in [
            "10.0.0.1",
            "172.16.0.1",
            "100.100.100.100",
            "192.0.2.1",
            "198.18.0.1",
            "::ffff:127.0.0.1",
            "2001:db8::1",
        ] {
            assert!(!public_ip(v.parse().unwrap()), "{v}");
        }
        assert!(public_ip("1.1.1.1".parse().unwrap()));
    }
    #[test]
    fn rss_parser_returns_real_deduplicated_safe_results() {
        let rss=b"<rss><channel><item><title>A &amp; B</title><link>https://example.org/a</link><description>Evidence</description></item><item><title>Same</title><link>https://example.org/a</link></item><item><title>Private</title><link>https://127.0.0.1</link></item></channel></rss>";
        let hits = parse(rss, 4).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "A & B");
    }
}
