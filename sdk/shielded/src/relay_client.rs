//! Proof-only buyer transport. The caller must persist delegation before invoking it.
//! Remote status is diagnostic only; payment verification still uses paired chain evidence.

use std::{net::IpAddr, path::PathBuf, time::Duration};

use erebus_core::settlement::{PreparedSettlement, SettlementContext};
use serde::Deserialize;
use zeroize::Zeroizing;

use crate::{relay_journal::RelayFinalized, relay_request::RelayRequest};

/// Operator-pinned relay endpoint, never a model-supplied URL or credential.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelayEndpoint {
    /// Fixed full URL ending in `/v1/relay`.
    pub url: String,
    /// Nonzero service identity bound by the local coordinator fence.
    pub identity: String,
    /// Owner-only local raw authentication key; never sent except as a bearer credential over TLS.
    pub authentication_key_file: PathBuf,
    /// Public outer gas sender used by funding checks and exact call estimation.
    pub sponsor_address: String,
    /// Bounded HTTP timeout; a timeout never permits a direct fallback.
    pub timeout_seconds: u64,
    /// Explicit local rehearsal exception. Never permits non-loopback cleartext.
    #[serde(default)]
    pub allow_loopback_http: bool,
}

/// Remote retained-attempt diagnostics, not independently verified payment evidence.
pub struct RelayReply {
    /// Service operation identifier, derived locally from the proof's public inputs.
    pub operation_ref: String,
    /// Remote retained transaction hash, not a buyer signature or finality claim.
    pub transaction_hash: Option<String>,
    /// Remote pending/finalized/reverted classification.
    pub status: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireReply {
    protocol_version: u16,
    status: String,
    operation_ref: String,
    transaction_hash: Option<String>,
    broadcast_started: bool,
    finalized: Option<RelayFinalized>,
    retry_without_new_payment: bool,
    automatic_rebroadcast: bool,
}

impl RelayEndpoint {
    /// Rejects unsafe endpoint/path/timeout configuration before any network call.
    pub fn validate(&self) -> Result<(), &'static str> {
        let url = reqwest::Url::parse(&self.url).map_err(|_| "invalid relay endpoint")?;
        let loopback = url.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if !(url.scheme() == "https"
            || (url.scheme() == "http" && self.allow_loopback_http && loopback))
            || url.path() != "/v1/relay"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !(1..=900).contains(&self.timeout_seconds)
            || !self.authentication_key_file.is_absolute()
        {
            return Err("invalid relay endpoint");
        }
        Ok(())
    }

    /// Sends once. All HTTP/timeout/protocol failures return unknown, never nonpayment.
    /// The caller owns file permissions and must already have persisted delegation.
    pub async fn submit(
        &self,
        context: &SettlementContext,
        prepared: &PreparedSettlement,
        token: &[u8; 32],
    ) -> Result<Option<RelayReply>, &'static str> {
        self.validate()?;
        let request = RelayRequest::from_prepared(context, prepared)
            .map_err(|_| "invalid relay proof request")?;
        let identity = hex::encode(
            request
                .prepared(context)
                .map_err(|_| "invalid relay proof request")?
                .operation_ref,
        );
        let bytes = request
            .encode()
            .map_err(|_| "invalid relay proof request")?;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .timeout(Duration::from_secs(self.timeout_seconds))
            .build()
            .map_err(|_| "relay transport unavailable")?;
        let bearer = Zeroizing::new(hex::encode(token));
        let Ok(mut response) = client
            .post(&self.url)
            .bearer_auth(bearer.as_str())
            .header("content-type", "application/json")
            .body(bytes)
            .send()
            .await
        else {
            return Ok(None);
        };
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|length| length > 8192)
        {
            return Ok(None);
        }
        let mut body = Vec::new();
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) if body.len() + chunk.len() <= 8192 => {
                    body.extend_from_slice(&chunk)
                }
                Ok(None) => break,
                _ => return Ok(None),
            }
        }
        let Ok(reply) = serde_json::from_slice::<WireReply>(&body) else {
            return Ok(None);
        };
        if reply.protocol_version != 1
            || reply.operation_ref != identity
            || !reply.retry_without_new_payment
            || reply.automatic_rebroadcast
            || !matches!(reply.status.as_str(), "pending" | "finalized" | "reverted")
            || reply.transaction_hash.as_ref().is_some_and(|hash| {
                hash.len() != 66
                    || !hash.starts_with("0x")
                    || !hash[2..]
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
            || (reply.broadcast_started && reply.transaction_hash.is_none())
            || match reply.finalized {
                Some(done) => {
                    !reply.broadcast_started
                        || done.block_hash == [0; 32]
                        || (reply.status == "finalized") != done.success
                        || reply.status == "pending"
                }
                None => reply.status != "pending",
            }
        {
            return Ok(None);
        }
        Ok(Some(RelayReply {
            operation_ref: identity,
            transaction_hash: reply.transaction_hash,
            status: reply.status,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoints_require_tls_or_explicit_loopback_and_fixed_path() {
        let mut endpoint = RelayEndpoint {
            url: "https://relay.example/v1/relay".into(),
            identity: "01".repeat(32),
            authentication_key_file: "/tmp/token".into(),
            sponsor_address: "0x01".into(),
            timeout_seconds: 60,
            allow_loopback_http: false,
        };
        endpoint.validate().unwrap();
        for url in [
            "http://relay.example/v1/relay",
            "https://user:pass@relay.example/v1/relay",
            "https://relay.example/v1/relay?token=x",
            "https://relay.example/elsewhere",
            "file:///v1/relay",
        ] {
            endpoint.url = url.into();
            assert!(endpoint.validate().is_err());
        }
        endpoint.url = "http://127.0.0.1:8000/v1/relay".into();
        assert!(endpoint.validate().is_err());
        endpoint.allow_loopback_http = true;
        endpoint.validate().unwrap();
        endpoint.url = "http://192.0.2.1/v1/relay".into();
        assert!(endpoint.validate().is_err());
    }

    #[tokio::test]
    async fn public_transport_refuses_overclaims_and_never_follows_a_redirect() {
        use axum::{
            extract::State,
            http::{HeaderMap, StatusCode},
            routing::post,
            Json, Router,
        };
        use serde_json::{json, Value};
        use std::sync::{Arc, Mutex};
        let (context, prepared) = crate::relay_request::tests::fixture();
        let identity = hex::encode(
            RelayRequest::from_prepared(&context, &prepared)
                .unwrap()
                .prepared(&context)
                .unwrap()
                .operation_ref,
        );
        let reply = Arc::new(Mutex::new(json!({"protocol_version":1,"status":"pending",
            "operation_ref":identity,"transaction_hash":null,"broadcast_started":false,
            "finalized":null,"retry_without_new_payment":true,"automatic_rebroadcast":false})));
        let captured = Arc::new(Mutex::new(Vec::<Value>::new()));
        let handler =
            |State((reply, captured)): State<(Arc<Mutex<Value>>, Arc<Mutex<Vec<Value>>>)>,
             headers: HeaderMap,
             Json(request): Json<Value>| async move {
                assert_eq!(
                    headers["authorization"],
                    format!("Bearer {}", hex::encode([7; 32]))
                );
                assert_eq!(request.as_object().unwrap().len(), 2);
                assert!(request.get("calldata").is_some());
                captured.lock().unwrap().push(request);
                (StatusCode::OK, Json(reply.lock().unwrap().clone()))
            };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1/relay", listener.local_addr().unwrap());
        let router = Router::new()
            .route("/v1/relay", post(handler))
            .with_state((reply.clone(), captured.clone()));
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let mut endpoint = RelayEndpoint {
            url,
            identity: "01".repeat(32),
            authentication_key_file: "/tmp/token".into(),
            sponsor_address: "0x01".into(),
            timeout_seconds: 5,
            allow_loopback_http: true,
        };
        assert!(endpoint
            .submit(&context, &prepared, &[7; 32])
            .await
            .unwrap()
            .is_some());
        reply.lock().unwrap()["automatic_rebroadcast"] = json!(true);
        assert!(endpoint
            .submit(&context, &prepared, &[7; 32])
            .await
            .unwrap()
            .is_none());
        reply.lock().unwrap()["private_key"] = json!("not a secret: rejection marker");
        assert!(endpoint
            .submit(&context, &prepared, &[7; 32])
            .await
            .unwrap()
            .is_none());
        assert_eq!(captured.lock().unwrap().len(), 3);
        let redirect_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        endpoint.url = format!(
            "http://{}/v1/relay",
            redirect_listener.local_addr().unwrap()
        );
        let target = endpoint.url.clone();
        let redirects = Router::new().route(
            "/v1/relay",
            post(move || async move { (StatusCode::TEMPORARY_REDIRECT, [("location", target)]) }),
        );
        let redirect_server = tokio::spawn(async move {
            axum::serve(redirect_listener, redirects).await.unwrap();
        });
        assert!(endpoint
            .submit(&context, &prepared, &[7; 32])
            .await
            .unwrap()
            .is_none());
        server.abort();
        redirect_server.abort();
    }
}
