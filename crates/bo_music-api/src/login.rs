//! Authentication protocol primitives. Credential adoption and persistence are not implemented yet.
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::transport::{ApiRequest, Transport, TransportError};

#[derive(Debug, Error)]
pub enum LoginError {
    #[error("input cannot be blank")]
    BlankInput,
    #[error("service returned an invalid QR challenge")]
    InvalidChallenge,
    #[error("login response contains invalid credentials")]
    InvalidCredential,
    #[error("session changed while login was in progress")]
    SessionChanged,
    #[error(transparent)]
    Transport(#[from] TransportError),
}

/// The key and URL are login secrets; do not print or log this value.
pub struct QrChallenge {
    pub key: String,
    pub landing_url: String,
    session: crate::session::Session,
    session_revision: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChallengeDto {
    qr_code: Option<String>,
}

pub async fn create_challenge(transport: &Transport) -> Result<QrChallenge, LoginError> {
    let mut request = ApiRequest::get("ucenter/login/qrCode");
    request.signed = true;
    let response = transport.send::<ChallengeDto>(&request).await?;
    let key = response
        .data
        .and_then(|data| data.qr_code)
        .filter(|key| !key.is_empty())
        .ok_or(LoginError::InvalidChallenge)?;
    let mut url = reqwest::Url::parse(
        "https://bodian-oia.kuwo.cn/bodian/download.html?pageName=login_pc&pt=3",
    )
    .map_err(|_| LoginError::InvalidChallenge)?;
    url.query_pairs_mut().append_pair("id", &key);
    Ok(QrChallenge {
        key,
        landing_url: url.into(),
        session: transport.session(),
        session_revision: response.session_revision,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QrScanStatus {
    Waiting,
    Expired,
    Confirmed,
    Unknown,
}

#[derive(Deserialize)]
struct QrStatusDto {
    #[serde(default)]
    status: i32,
}

pub async fn poll(
    transport: &Transport,
    challenge: &QrChallenge,
) -> Result<QrScanStatus, LoginError> {
    let mut request = ApiRequest::get("ucenter/login/qrCodeStatus");
    request.signed = true;
    request.query.push(("qrCode".into(), challenge.key.clone()));
    request.accepted_codes.push(11027);
    if !challenge.session.same_instance(&transport.session()) {
        return Err(LoginError::SessionChanged);
    }
    let response = transport
        .send_bound::<QrStatusDto>(&request, Some(challenge.session_revision))
        .await?;
    Ok(match response.data.map(|data| data.status) {
        Some(1) => QrScanStatus::Waiting,
        Some(2) => QrScanStatus::Expired,
        Some(3) => QrScanStatus::Confirmed,
        _ => QrScanStatus::Unknown,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SmsOutcome {
    Sent,
    Failed { code: i32 },
}

/// Sends a real SMS if called against the service. The caller decides when to invoke it.
pub async fn send_sms(transport: &Transport, mobile: &str) -> Result<SmsOutcome, LoginError> {
    if mobile.trim().is_empty() {
        return Err(LoginError::BlankInput);
    }
    let mut request = ApiRequest::get("ucenter/code/sendsms");
    request.signed = true;
    request.query = vec![
        ("type".into(), "2".into()),
        ("mobile".into(), mobile.into()),
    ];
    request.accepted_codes.push(11003);
    let response = transport.send::<serde_json::Value>(&request).await?;
    Ok(if response.code == 200 {
        SmsOutcome::Sent
    } else {
        SmsOutcome::Failed {
            code: response.code,
        }
    })
}

/// A validated credential is returned to the caller; persistence remains the caller's responsibility.
/// No Debug implementation is provided because it contains an authentication token.
pub struct ValidatedCredential {
    uid: i64,
    token: String,
    pub nickname: Option<String>,
    pub avatar_url: Option<String>,
    session_revision: u64,
    session: crate::session::Session,
}

impl ValidatedCredential {
    pub fn uid(&self) -> i64 {
        self.uid
    }

    /// Activate an exchange only if the session has not changed since it started.
    pub fn adopt(self) -> Result<(), LoginError> {
        if self.uid <= 0 || self.token.is_empty() {
            return Err(LoginError::InvalidCredential);
        }
        if !self
            .session
            .set_if_revision(self.session_revision, self.uid.to_string(), self.token)
        {
            return Err(LoginError::SessionChanged);
        }
        Ok(())
    }

    /// Save a validated credential before installing it, while excluding a
    /// concurrent session change. Run on a dedicated blocking thread because
    /// the provided system-secret-store operation can block.
    pub fn adopt_with<E>(
        self,
        persist: impl FnOnce(&str, &str, Option<&str>) -> Result<(), E>,
    ) -> Result<Result<(), E>, LoginError> {
        if self.uid <= 0 || self.token.is_empty() {
            return Err(LoginError::InvalidCredential);
        }
        let nickname = self.nickname;
        self.session
            .set_if_revision_after(
                self.session_revision,
                self.uid.to_string(),
                self.token,
                |uid, token| persist(uid, token, nickname.as_deref()),
            )
            .ok_or(LoginError::SessionChanged)
    }
}

pub enum ExchangeOutcome {
    Credential(ValidatedCredential),
    Pending,
    WrongCode,
    InvalidResponse,
    IdentityMismatch,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct QrExchangeBody<'a> {
    auth_type: u8,
    qr_code: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PhoneExchangeBody<'a> {
    auth_type: u8,
    mobile: &'a str,
    verify_code: &'a str,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct LoginDto {
    id: Option<serde_json::Value>,
    bid: Option<serde_json::Value>,
    uid: Option<serde_json::Value>,
    token: Option<String>,
    user_info: Option<LoginUserDto>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct LoginUserDto {
    id: Option<serde_json::Value>,
    nickname: Option<String>,
    head_img: Option<String>,
}

fn identity_number(value: Option<serde_json::Value>) -> Option<i64> {
    match value? {
        serde_json::Value::Number(n) => n.as_i64(),
        serde_json::Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn validate_login(
    data: LoginDto,
    revision: u64,
    session: crate::session::Session,
) -> ExchangeOutcome {
    let Some(token) = data.token.filter(|token| !token.is_empty()) else {
        return ExchangeOutcome::InvalidResponse;
    };
    let user_id = data
        .user_info
        .as_ref()
        .and_then(|user| identity_number(user.id.clone()));
    let identities = [
        identity_number(data.id),
        identity_number(data.bid),
        identity_number(data.uid),
        user_id,
    ];
    let values: Vec<_> = identities.into_iter().flatten().collect();
    if values.len() < 2 || values.iter().any(|id| *id != values[0]) {
        return ExchangeOutcome::IdentityMismatch;
    }
    let (nickname, avatar_url) = data
        .user_info
        .map(|user| {
            (
                user.nickname,
                user.head_img.as_deref().and_then(crate::search::http_url),
            )
        })
        .unwrap_or_default();
    ExchangeOutcome::Credential(ValidatedCredential {
        uid: values[0],
        token,
        nickname,
        avatar_url,
        session_revision: revision,
        session,
    })
}

/// Performs one QR exchange attempt. Pending may be retried by a caller with a bounded policy.
pub async fn exchange_qr(
    transport: &Transport,
    challenge: &QrChallenge,
) -> Result<ExchangeOutcome, LoginError> {
    if !challenge.session.same_instance(&transport.session()) {
        return Err(LoginError::SessionChanged);
    }
    let body = serde_json::to_string(&QrExchangeBody {
        auth_type: 10,
        qr_code: &challenge.key,
    })
    .map_err(|_| LoginError::InvalidChallenge)?;
    let mut request = ApiRequest::get("ucenter/users/login");
    request.method = reqwest::Method::POST;
    request.signed = true;
    request.json_body = Some(body);
    request.accepted_codes.push(11027);
    let response = transport
        .send_bound::<LoginDto>(&request, Some(challenge.session_revision))
        .await?;
    if response.code == 11027 {
        return Ok(ExchangeOutcome::Pending);
    }
    Ok(response
        .data
        .map(|data| validate_login(data, response.session_revision, transport.session()))
        .unwrap_or(ExchangeOutcome::InvalidResponse))
}

/// Retry only the QR exchange's explicitly retryable pending response.
/// Dropping this future cancels the remaining attempts.
pub async fn complete_qr(
    transport: &Transport,
    challenge: &QrChallenge,
) -> Result<ExchangeOutcome, LoginError> {
    complete_qr_with_policy(transport, challenge, 5, std::time::Duration::from_secs(2)).await
}

async fn complete_qr_with_policy(
    transport: &Transport,
    challenge: &QrChallenge,
    attempts: usize,
    interval: std::time::Duration,
) -> Result<ExchangeOutcome, LoginError> {
    for attempt in 0..attempts {
        match exchange_qr(transport, challenge).await? {
            ExchangeOutcome::Pending if attempt + 1 < attempts => {
                tokio::time::sleep(interval).await
            }
            outcome => return Ok(outcome),
        }
    }
    Ok(ExchangeOutcome::Pending)
}

pub async fn exchange_phone(
    transport: &Transport,
    mobile: &str,
    code: &str,
) -> Result<ExchangeOutcome, LoginError> {
    if mobile.trim().is_empty() || code.trim().is_empty() {
        return Err(LoginError::BlankInput);
    }
    let body = serde_json::to_string(&PhoneExchangeBody {
        auth_type: 1,
        mobile,
        verify_code: code,
    })
    .map_err(|_| LoginError::BlankInput)?;
    let mut request = ApiRequest::get("ucenter/users/login");
    request.method = reqwest::Method::POST;
    request.signed = true;
    request.json_body = Some(body);
    request.accepted_codes.push(11004);
    let response = transport.send::<LoginDto>(&request).await?;
    if response.code == 11004 {
        return Ok(ExchangeOutcome::WrongCode);
    }
    Ok(response
        .data
        .map(|data| validate_login(data, response.session_revision, transport.session()))
        .unwrap_or(ExchangeOutcome::InvalidResponse))
}

#[cfg(test)]
#[path = "login_tests.rs"]
mod tests;
