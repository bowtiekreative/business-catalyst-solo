//! Fakes contain synthetic credentials only. Never read/write the host keychain,
//! open Google/browser, download media or send an external HTTP request.
use super::*;
use credentials::{Keychain, Record, Tokens, Vault};
use http::{Request, Response, Transport};
use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    net::TcpStream,
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::Duration,
};

#[derive(Default)]
pub(super) struct MemoryKeychain {
    pub values: Mutex<HashMap<String, Vec<u8>>>,
    pub reads: AtomicUsize,
    pub writes: AtomicUsize,
    pub fail_write: Mutex<Option<usize>>,
}
impl Keychain for MemoryKeychain {
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(self.values.lock().unwrap().get(name).cloned())
    }
    fn set(&self, name: &str, bytes: &[u8]) -> Result<()> {
        let count = self.writes.fetch_add(1, Ordering::SeqCst);
        if *self.fail_write.lock().unwrap() == Some(count) {
            return Err("Synthetic locked keychain".into());
        }
        assert!(bytes.len() <= 2000);
        assert!(
            std::str::from_utf8(bytes).is_ok(),
            "each bounded chunk stays UTF-8 safe"
        );
        self.values
            .lock()
            .unwrap()
            .insert(name.into(), bytes.to_vec());
        Ok(())
    }
    fn delete(&self, name: &str) -> Result<()> {
        self.values.lock().unwrap().remove(name);
        Ok(())
    }
}
#[derive(Default)]
pub(super) struct FakeHttp {
    pub requests: Mutex<Vec<Request>>,
    pub replies: Mutex<VecDeque<std::result::Result<Response, &'static str>>>,
}
impl FakeHttp {
    pub fn push(&self, status: u16, range: Option<&str>, body: &str) {
        self.replies.lock().unwrap().push_back(Ok(Response {
            status,
            range: range.map(str::to_owned),
            location: None,
            body: body.as_bytes().to_vec(),
        }));
    }
}
impl Transport for FakeHttp {
    fn send(&self, request: Request) -> Result<Response> {
        self.requests.lock().unwrap().push(request);
        self.replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected synthetic HTTP request")
            .map_err(Into::into)
    }
}
pub(super) fn configured() -> Record {
    Record {
        schema: 1,
        client: Some(ClientConfig {
            id: "synthetic.apps.googleusercontent.com".into(),
            secret: "synthetic-client-secret".into(),
        }),
        client_generation: "synthetic-client-generation".into(),
        tokens: Some(Tokens {
            access: "synthetic-access".into(),
            refresh: "synthetic-refresh".into(),
            expires_unix_ms: oauth::now_ms().unwrap() + 3_600_000,
            connection: ConnectionId("synthetic-reviewed-connection".into()),
        }),
        pending: None,
    }
}
pub(super) fn services(
    keychain: Arc<MemoryKeychain>,
    http: Arc<FakeHttp>,
    root: &Path,
) -> Services {
    Services {
        vault: Vault::new(keychain),
        http,
        state_root: Some(root.into()),
    }
}
fn client_json() -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({"installed":{
        "client_id":"synthetic.apps.googleusercontent.com","client_secret":"synthetic-client-secret",
        "auth_uri":protocol::AUTH_URL,"token_uri":protocol::TOKEN_URL,"redirect_uris":["http://localhost"],"project_id":"synthetic-project",
        "auth_provider_x509_cert_url":"https://www.googleapis.com/oauth2/v1/certs"
    }})).unwrap()
}
#[test]
fn pkce_known_vector_google_client_shape_and_private_debug() {
    assert_eq!(
        protocol::challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    let config = protocol::parse_client(&client_json()).unwrap();
    let mut bad: serde_json::Value = serde_json::from_slice(&client_json()).unwrap();
    for endpoint in ["token_uri", "auth_uri"] {
        let old = bad["installed"][endpoint].clone();
        bad["installed"][endpoint] = "https://127.0.0.1/token".into();
        assert!(protocol::parse_client(&serde_json::to_vec(&bad).unwrap()).is_err());
        bad["installed"][endpoint] = old;
    }
    assert!(protocol::parse_client(br#"{"web":{}}"#).is_err());
    assert!(!format!("{config:?}").contains("synthetic-client-secret"));
    let auth = oauth::begin(config, "generation".into()).unwrap();
    assert_eq!(format!("{auth:?}"), "Authorization([redacted])");
    let url = url::Url::parse(auth.browser_url()).unwrap();
    let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(query["scope"], protocol::SCOPE);
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(query["code_challenge"], protocol::challenge(&auth.verifier));
    assert_eq!(query["redirect_uri"], auth.redirect);
}
#[test]
fn keychain_chunks_roundtrip_unicode_and_failed_stages_preserve_old_record() {
    let memory = Arc::new(MemoryKeychain::default());
    let vault = Vault::new(memory.clone());
    let old = configured();
    vault.save(&old).unwrap();
    let mut large = old.clone();
    large.client.as_mut().unwrap().secret = "x".repeat(1024);
    large.tokens.as_mut().unwrap().refresh = "r".repeat(4096);
    large.tokens.as_mut().unwrap().access = "a".repeat(4096);
    // Large multi-byte metadata must be reconstructed before JSON parsing.
    large.pending = Some(credentials::ResumeRecord {
        request: PublishRequest {
            relative_path: "exports/测试.mp4".into(),
            title: "标题".repeat(40),
            description: "é".repeat(2400),
            privacy: Privacy::Unlisted,
            reviewed_connection: old.tokens.as_ref().unwrap().connection.clone(),
        },
        project_binding: "b".repeat(43),
        session: "https://www.googleapis.com/upload/youtube/v3/videos?upload_id=synthetic".into(),
        source: credentials::SourceFingerprint {
            bytes: 10,
            modified_ns: 0,
            sha256: "s".repeat(43),
            #[cfg(unix)]
            dev: 1,
            #[cfg(unix)]
            ino: 2,
        },
    });
    let baseline = memory.values.lock().unwrap().clone();
    let before = memory.writes.load(Ordering::SeqCst);
    vault.save(&large).unwrap();
    let staged_writes = memory.writes.load(Ordering::SeqCst) - before;
    assert!(staged_writes > 5);
    assert_eq!(
        serde_json::to_value(vault.load().unwrap()).unwrap(),
        serde_json::to_value(&large).unwrap()
    );
    for fail in 0..staged_writes {
        *memory.values.lock().unwrap() = baseline.clone();
        let next = memory.writes.load(Ordering::SeqCst);
        *memory.fail_write.lock().unwrap() = Some(next + fail);
        assert!(vault.save(&large).is_err());
        assert_eq!(
            serde_json::to_value(vault.load().unwrap()).unwrap(),
            serde_json::to_value(&old).unwrap()
        );
    }
    *memory.fail_write.lock().unwrap() = None;
    vault.save(&large).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&memory.values.lock().unwrap()["manifest"]).unwrap();
    let name = format!("slot-{}-00", manifest["slot"].as_u64().unwrap());
    memory.values.lock().unwrap().get_mut(&name).unwrap()[50] ^= 1;
    assert!(vault.load().is_err());
    vault.clear().unwrap();
    assert!(memory.values.lock().unwrap().is_empty());
}
#[test]
fn callback_refuses_wrong_state_path_method_host_duplicates_and_body() {
    let request =
        |target: &str, host: &str| format!("GET {target} HTTP/1.1\r\nHost: {host}\r\n\r\n");
    let good = request(
        "/callback?state=expected&code=synthetic-code",
        "127.0.0.1:12345",
    );
    assert!(matches!(
        oauth::parse_callback(good.as_bytes(), "/callback", 12345, "expected").unwrap(),
        oauth::Callback::Code(_)
    ));
    for bad in [
        good.replace("state=expected", "state=wrong"),
        good.replace("/callback?", "/other?"),
        good.replace("GET ", "POST "),
        good.replace("Host: 127.0.0.1:12345", "Host: localhost:12345"),
        good.replace("&code=", "&state=expected&code="),
        format!("{good}unexpected"),
        good.replace("\r\n\r\n", "\r\nTransfer-Encoding: chunked\r\n\r\n"),
    ] {
        assert!(oauth::parse_callback(bad.as_bytes(), "/callback", 12345, "expected").is_err());
    }
    let denied = request(
        "/callback?state=expected&error=access_denied",
        "127.0.0.1:12345",
    );
    assert!(matches!(
        oauth::parse_callback(denied.as_bytes(), "/callback", 12345, "expected").unwrap(),
        oauth::Callback::Denied
    ));
}
#[test]
fn loopback_receives_only_matching_callback_and_exchange_uses_pkce() {
    let http = FakeHttp::default();
    http.push(200, None, &format!(r#"{{"access_token":"synthetic-access","refresh_token":"synthetic-refresh","expires_in":3600,"token_type":"Bearer","scope":"{}"}}"#, protocol::SCOPE));
    let auth = oauth::begin(
        protocol::parse_client(&client_json()).unwrap(),
        "generation".into(),
    )
    .unwrap();
    let port = auth.port;
    let path = auth.path.clone();
    let state = auth.state.clone();
    let verifier = auth.verifier.clone();
    let worker = thread::spawn(move || {
        for value in ["wrong-state", state.as_str()] {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            write!(stream,"GET {path}?state={value}&code=synthetic-code HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n").unwrap();
            let mut reply = String::new();
            stream.read_to_string(&mut reply).unwrap();
            assert!(reply.starts_with(if value == "wrong-state" {
                "HTTP/1.1 400"
            } else {
                "HTTP/1.1 200"
            }));
        }
    });
    let tokens = oauth::finish(auth, &http, &Cancellation::default()).unwrap();
    worker.join().unwrap();
    assert_eq!(tokens.access, "synthetic-access");
    let requests = http.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].url, protocol::TOKEN_URL);
    let fields: HashMap<_, _> = url::form_urlencoded::parse(&requests[0].body)
        .into_owned()
        .collect();
    assert_eq!(fields["code_verifier"], verifier);
    assert_eq!(fields["code"], "synthetic-code");
}
#[test]
fn expired_tokens_refresh_without_extra_scope_and_bad_responses_fail_closed() {
    let memory = Arc::new(MemoryKeychain::default());
    let http = Arc::new(FakeHttp::default());
    let temp = tempfile::tempdir().unwrap();
    let s = services(memory, http.clone(), temp.path());
    let mut record = configured();
    record.tokens.as_mut().unwrap().expires_unix_ms = 0;
    http.push(
        200,
        None,
        r#"{"access_token":"synthetic-new","expires_in":3600,"token_type":"Bearer"}"#,
    );
    assert_eq!(
        s.access(&mut record, &Cancellation::default()).unwrap(),
        "synthetic-new"
    );
    assert_eq!(record.tokens.as_ref().unwrap().refresh, "synthetic-refresh");
    assert_eq!(http.requests.lock().unwrap().len(), 1);
    for (status, body) in [
        (400, r#"{"error":"invalid_grant","secret":"never surface"}"#),
        (
            200,
            r#"{"access_token":"x","expires_in":3600,"token_type":"Other","scope":"https://www.googleapis.com/auth/youtube.upload"}"#,
        ),
        (
            200,
            r#"{"access_token":"x","expires_in":3600,"token_type":"Bearer","scope":"https://www.googleapis.com/auth/youtube"}"#,
        ),
        (
            200,
            r#"{"access_token":"x","expires_in":0,"token_type":"Bearer"}"#,
        ),
    ] {
        let error = oauth::parse_tokens(
            status,
            body.as_bytes(),
            record.tokens.as_ref(),
            ConnectionId("synthetic".into()),
        )
        .unwrap_err();
        assert!(!error.to_string().contains("never surface"));
    }
}
#[test]
fn construction_is_idle_and_parallel_process_lock_is_held_until_drop() {
    let _system = YoutubeClient::system().unwrap(); // builds HTTPS client only
    let keychain = Arc::new(MemoryKeychain::default());
    let http = Arc::new(FakeHttp::default());
    let temp = tempfile::tempdir().unwrap();
    let client = YoutubeClient {
        services: Arc::new(Mutex::new(services(
            keychain.clone(),
            http.clone(),
            temp.path(),
        ))),
    };
    assert_eq!(keychain.reads.load(Ordering::SeqCst), 0);
    assert!(http.requests.lock().unwrap().is_empty());
    assert!(!temp.path().join(".sidebar").exists());
    assert!(!client.status().unwrap().configured);
    let s = client.services().unwrap();
    let lock = s.process_lock().unwrap();
    // A distinct service object models a second process' independent memory.
    let second = services(keychain.clone(), http.clone(), temp.path());
    assert!(second.process_lock().is_err());
    drop(lock);
    assert!(second.process_lock().is_ok());
    assert!(http.requests.lock().unwrap().is_empty());
}

#[test]
fn older_connect_handle_cannot_replace_newer_attempt_or_reconnect_after_disconnect() {
    let keychain = Arc::new(MemoryKeychain::default());
    let http = Arc::new(FakeHttp::default());
    let temp = tempfile::tempdir().unwrap();
    let s = services(keychain, http.clone(), temp.path());
    s.vault.save(&configured()).unwrap();
    let client = YoutubeClient {
        services: Arc::new(Mutex::new(s)),
    };
    let first = client.begin_connect(&Cancellation::default()).unwrap();
    let second = client.begin_connect(&Cancellation::default()).unwrap();
    assert_ne!(first.generation, second.generation);
    assert!(
        client
            .finish_connect(first, &Cancellation::default())
            .is_err()
    );
    assert!(http.requests.lock().unwrap().is_empty());
    assert!(client.disconnect().unwrap().connection.is_none());
    assert!(
        client
            .finish_connect(second, &Cancellation::default())
            .is_err()
    );
    assert!(client.status().unwrap().connection.is_none());
    assert!(http.requests.lock().unwrap().is_empty());
}
