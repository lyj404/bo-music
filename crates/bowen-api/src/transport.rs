use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::{Client, Method, Url, header};
use serde::de::DeserializeOwned;
use serde_json::Value;
use thiserror::Error;

use crate::{
    session::{Session, SessionSnapshot},
    signer,
};

const DEFAULT_BASE: &str = "https://bd-api.kuwo.cn/api/";
const DEFAULT_MAX_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("invalid transport configuration or request path")]
    InvalidRequest,
    #[error("request timed out")]
    Timeout,
    #[error("network request failed")]
    Network,
    #[error("server returned HTTP {0}")]
    Http(u16),
    #[error("response exceeded the configured size limit")]
    ResponseTooLarge,
    #[error("invalid response envelope or data")]
    MalformedResponse,
    #[error("service returned business code {0}")]
    Business(i32),
    #[error("session changed while request was in flight")]
    SessionChanged,
}

#[derive(Clone, Debug)]
pub struct TransportOptions {
    pub base_url: String,
    pub timeout: Duration,
    pub max_response_bytes: usize,
}

impl Default for TransportOptions {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE.into(),
            timeout: Duration::from_secs(30),
            max_response_bytes: DEFAULT_MAX_BYTES,
        }
    }
}

/// Request body is already serialized: signing and transmission use identical bytes.
pub struct ApiRequest {
    pub path: String,
    pub method: Method,
    pub query: Vec<(String, String)>,
    pub json_body: Option<String>,
    pub file: Option<FormFile>,
    pub platform: Option<String>,
    pub signed: bool,
    pub accepted_codes: Vec<i32>,
}

pub struct FormFile {
    pub field_name: String,
    pub file_name: String,
    pub content_type: String,
    pub content: Vec<u8>,
}

impl ApiRequest {
    pub fn get(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            method: Method::GET,
            query: Vec::new(),
            json_body: None,
            file: None,
            platform: None,
            signed: false,
            accepted_codes: Vec::new(),
        }
    }
}

pub struct Envelope<T> {
    pub code: i32,
    pub message: Option<String>,
    pub request_id: Option<String>,
    pub data: Option<T>,
    pub lrcx: Option<i32>,
    pub session_revision: u64,
}

pub struct Transport {
    client: Client,
    base: Url,
    options: TransportOptions,
    session: Session,
    device_id: String,
}

impl Transport {
    pub fn new(
        options: TransportOptions,
        session: Session,
        device_id: String,
    ) -> Result<Self, TransportError> {
        let base = Url::parse(&options.base_url).map_err(|_| TransportError::InvalidRequest)?;
        if !matches!(base.scheme(), "http" | "https")
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || !base.path().ends_with('/')
            || device_id.is_empty()
            || options.max_response_bytes == 0
            || options.timeout.is_zero()
        {
            return Err(TransportError::InvalidRequest);
        }
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .gzip(true)
            .timeout(options.timeout)
            .build()
            .map_err(|_| TransportError::InvalidRequest)?;
        Ok(Self {
            client,
            base,
            options,
            session,
            device_id,
        })
    }

    fn prepare(
        &self,
        request: &ApiRequest,
        snapshot: &SessionSnapshot,
        timestamp_ms: u128,
    ) -> Result<reqwest::Request, TransportError> {
        // Only fixed relative API paths are accepted; never let a caller replace the host.
        if request.path.is_empty()
            || request.path.starts_with('/')
            || request
                .path
                .split('/')
                .any(|p| p == "." || p == ".." || p.is_empty())
            || !request
                .path
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'/' | b'_' | b'-'))
        {
            return Err(TransportError::InvalidRequest);
        }
        if request.json_body.is_some() && request.file.is_some() {
            return Err(TransportError::InvalidRequest);
        }
        let mut pairs = request.query.clone();
        pairs.push(("uid".into(), snapshot.uid.clone()));
        pairs.push(("token".into(), snapshot.token.clone()));
        if request.signed {
            pairs.push(("timestamp".into(), timestamp_ms.to_string()));
            pairs.push(("sign".into(), String::new()));
            let signature = signer::sign_raw(
                &request.path,
                &signer::form_url_encode(&pairs),
                request.json_body.as_deref(),
            );
            pairs.last_mut().expect("sign was appended").1 = signature;
        }
        let mut url = self
            .base
            .join(&request.path)
            .map_err(|_| TransportError::InvalidRequest)?;
        url.set_query(Some(&signer::form_url_encode(&pairs)));
        let mut builder = self
            .client
            .request(request.method.clone(), url)
            .header(header::USER_AGENT, "Dart/3.3 (dart:io)")
            .header("plat", request.platform.as_deref().unwrap_or("win"))
            .header("channel", "W1")
            .header("ver", "1.1.7")
            .header("svrver", "13")
            .header("api-ver", "application/json")
            .header("brand", "Windows")
            .header("net", "wifi")
            .header("devid", &self.device_id)
            .header("qimei36", &self.device_id)
            .header(header::ACCEPT_ENCODING, "gzip");
        if snapshot.is_authenticated() {
            builder = builder
                .header("uid", &snapshot.uid)
                .header("token", &snapshot.token);
        }
        if let Some(body) = &request.json_body {
            builder = builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(body.clone());
        } else if let Some(file) = &request.file {
            if file.field_name.trim().is_empty()
                || file.file_name.trim().is_empty()
                || file.content_type.trim().is_empty()
            {
                return Err(TransportError::InvalidRequest);
            }
            let part = reqwest::multipart::Part::bytes(file.content.clone())
                .file_name(file.file_name.clone())
                .mime_str(&file.content_type)
                .map_err(|_| TransportError::InvalidRequest)?;
            builder = builder
                .multipart(reqwest::multipart::Form::new().part(file.field_name.clone(), part));
        }
        builder.build().map_err(|_| TransportError::InvalidRequest)
    }

    /// Dropping this future cancels the request. No request or response content is logged.
    pub async fn send<T: DeserializeOwned>(
        &self,
        request: &ApiRequest,
    ) -> Result<Envelope<T>, TransportError> {
        self.send_bound(request, None).await
    }

    pub(crate) fn session_snapshot(&self) -> SessionSnapshot {
        self.session.snapshot()
    }
    /// Shared identity used by this transport, for coordinating account-bound work.
    pub fn session(&self) -> Session {
        self.session.clone()
    }
    pub(crate) fn device_id(&self) -> &str {
        &self.device_id
    }

    /// Bind multi-request operations to the identity that started them.
    pub(crate) async fn send_bound<T: DeserializeOwned>(
        &self,
        request: &ApiRequest,
        expected_revision: Option<u64>,
    ) -> Result<Envelope<T>, TransportError> {
        let snapshot = self.session.snapshot();
        if expected_revision.is_some_and(|revision| revision != snapshot.revision) {
            return Err(TransportError::SessionChanged);
        }
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| TransportError::InvalidRequest)?
            .as_millis();
        let prepared = self.prepare(request, &snapshot, timestamp)?;
        self.execute_bound(prepared, snapshot.revision, &request.accepted_codes, true)
            .await
    }

    /// Lyrics are hosted outside the API origin. The endpoint is constructed by
    /// the lyric module and never receives account credentials.
    pub(crate) async fn send_lyric<T: DeserializeOwned>(
        &self,
        mut endpoint: Url,
        encoded_payload: &str,
        expected_revision: u64,
    ) -> Result<Envelope<T>, TransportError> {
        let allowed = endpoint.scheme() == "https" && endpoint.host_str() == Some("mlyric.kuwo.cn");
        #[cfg(any(test, feature = "test-support"))]
        let allowed =
            allowed || (endpoint.scheme() == "http" && endpoint.host_str() == Some("127.0.0.1"));
        if !allowed
            || endpoint.path() != "/mobi.s"
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
        {
            return Err(TransportError::InvalidRequest);
        }
        if self.session.snapshot().revision != expected_revision {
            return Err(TransportError::SessionChanged);
        }
        endpoint
            .query_pairs_mut()
            .append_pair("f", "bodian")
            .append_pair("q", encoded_payload);
        let prepared = self
            .client
            .get(endpoint)
            .header(header::USER_AGENT, "Dart/3.3 (dart:io)")
            .header("plat", "win")
            .header("channel", "W1")
            .header("ver", "1.1.7")
            .header("svrver", "13")
            .header("api-ver", "application/json")
            .header("brand", "Windows")
            .header("net", "wifi")
            .header("devid", &self.device_id)
            .header("qimei36", &self.device_id)
            .header(header::ACCEPT_ENCODING, "gzip")
            .build()
            .map_err(|_| TransportError::InvalidRequest)?;
        self.execute_bound(prepared, expected_revision, &[], false)
            .await
    }

    async fn execute_bound<T: DeserializeOwned>(
        &self,
        prepared: reqwest::Request,
        revision: u64,
        accepted_codes: &[i32],
        clear_unauthorized: bool,
    ) -> Result<Envelope<T>, TransportError> {
        let mut response = self.client.execute(prepared).await.map_err(network_error)?;
        if !response.status().is_success() {
            return Err(TransportError::Http(response.status().as_u16()));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(network_error)? {
            if chunk.len() > self.options.max_response_bytes.saturating_sub(bytes.len()) {
                return Err(TransportError::ResponseTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        // Compare before interpreting any result, including unauthorized responses.
        if self.session.snapshot().revision != revision {
            return Err(TransportError::SessionChanged);
        }
        let envelope = parse_envelope::<T>(&bytes, revision)?;
        if envelope.code != 200 && !accepted_codes.contains(&envelope.code) {
            if clear_unauthorized && envelope.code == 11012 {
                self.session.clear_if_revision(Some(revision));
            }
            return Err(TransportError::Business(envelope.code));
        }
        Ok(envelope)
    }
}

fn network_error(error: reqwest::Error) -> TransportError {
    if error.is_timeout() {
        TransportError::Timeout
    } else {
        TransportError::Network
    }
}

fn parse_envelope<T: DeserializeOwned>(
    bytes: &[u8],
    revision: u64,
) -> Result<Envelope<T>, TransportError> {
    let root: Value =
        serde_json::from_slice(bytes).map_err(|_| TransportError::MalformedResponse)?;
    let obj = root.as_object().ok_or(TransportError::MalformedResponse)?;
    let code = obj
        .get("code")
        .and_then(Value::as_i64)
        .and_then(|n| i32::try_from(n).ok())
        .unwrap_or(-2);
    let data = match obj.get("data") {
        Some(value) if !value.is_null() => Some(
            serde_json::from_value(value.clone()).map_err(|_| TransportError::MalformedResponse)?,
        ),
        _ => None,
    };
    Ok(Envelope {
        code,
        message: obj.get("msg").and_then(Value::as_str).map(str::to_owned),
        request_id: obj.get("reqId").and_then(Value::as_str).map(str::to_owned),
        lrcx: obj
            .get("lrcx")
            .and_then(Value::as_i64)
            .and_then(|n| i32::try_from(n).ok()),
        data,
        session_revision: revision,
    })
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
