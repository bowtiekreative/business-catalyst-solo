//! Fixed-endpoint blocking HTTPS. No redirects, environment proxy credentials,
//! body logging or propagation of reqwest errors containing secret session URLs.
use super::protocol::MAX_RESPONSE;
use crate::Result;
use std::{io::Read, time::Duration};

pub(super) enum Method {
    Post,
    Put,
}
pub(super) struct Request {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}
pub(super) struct Response {
    pub status: u16,
    pub location: Option<String>,
    pub range: Option<String>,
    pub body: Vec<u8>,
}
pub(super) trait Transport: Send + Sync {
    fn send(&self, request: Request) -> Result<Response>;
}
pub(super) struct GoogleHttp(reqwest::blocking::Client);
impl GoogleHttp {
    pub fn new() -> Result<Self> {
        Ok(Self(
            reqwest::blocking::Client::builder()
                .https_only(true)
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(45))
                .build()
                .map_err(|_| "Could not prepare the YouTube HTTPS client")?,
        ))
    }
}
impl Transport for GoogleHttp {
    fn send(&self, request: Request) -> Result<Response> {
        let mut builder = match request.method {
            Method::Post => self.0.post(&request.url),
            Method::Put => self.0.put(&request.url),
        };
        for (name, value) in request.headers {
            builder = builder.header(name, value);
        }
        let response = builder
            .body(request.body)
            .send()
            .map_err(|_| "YouTube request was interrupted; retry or resume the pending upload")?;
        let header = |name: &str| -> Result<Option<String>> {
            response
                .headers()
                .get(name)
                .map(|v| {
                    v.to_str()
                        .ok()
                        .filter(|v| v.len() <= 4096)
                        .map(str::to_owned)
                        .ok_or_else(|| "YouTube returned an invalid response header".into())
                })
                .transpose()
        };
        let status = response.status().as_u16();
        let location = header("location")?;
        let range = header("range")?;
        let mut body = Vec::new();
        response
            .take(MAX_RESPONSE + 1)
            .read_to_end(&mut body)
            .map_err(|_| "YouTube response was interrupted; retry or resume")?;
        if body.len() as u64 > MAX_RESPONSE {
            return Err("YouTube response exceeded its size bound".into());
        }
        Ok(Response {
            status,
            location,
            range,
            body,
        })
    }
}
