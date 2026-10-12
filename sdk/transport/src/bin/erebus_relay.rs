//! `erebus-relay`: a minimal ciphertext mailbox service (decision DM2-5).
//!
//! Erebus clients submit Noise ciphertext; the relay itself stores opaque bytes and cannot
//! validate that a client encrypted them. It never holds a session key. Hosted and self-hosted
//! deployments run this same binary with different
//! configuration (decision D05).
//!
//! Configuration is read from the environment:
//!
//! | Variable | Default | Meaning |
//! |---|---|---|
//! | `EREBUS_RELAY_ROOT` | `./relay-data` | Persistent storage directory |
//! | `EREBUS_RELAY_PORT` | `8080` | Listen port |
//! | `EREBUS_RELAY_TOKEN` | unset | Required bearer token; unset refuses to start unless `EREBUS_RELAY_INSECURE=1` |
//! | `EREBUS_RELAY_RETENTION` | 7 days | Retention in seconds |
//!
//! Endpoints: `GET /healthz`, `POST /v1/mailbox/{id}`, `GET /v1/mailbox/{id}?after=<cursor>`.
//! Operational logs never contain a blob or a mailbox id.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use erebus_transport::limits::{MAX_BLOB_BYTES, RELAY_RETENTION_SECONDS};
use erebus_transport::relay::{FileRelay, MailboxId, Relay, RelayError};
use serde::Deserialize;
use serde_json::json;

#[derive(Clone)]
struct AppState {
    relay: Arc<FileRelay>,
    token: Option<String>,
}

#[tokio::main]
async fn main() {
    let root = std::env::var("EREBUS_RELAY_ROOT").unwrap_or_else(|_| "./relay-data".to_owned());
    let port: u16 = std::env::var("EREBUS_RELAY_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8080);
    let retention = std::env::var("EREBUS_RELAY_RETENTION")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(RELAY_RETENTION_SECONDS);
    let token = std::env::var("EREBUS_RELAY_TOKEN").ok();
    let auth_enabled = token.is_some();
    let insecure = std::env::var("EREBUS_RELAY_INSECURE").is_ok_and(|value| value == "1");

    if validate_token(token.as_deref(), insecure).is_err() {
        eprintln!(
            "erebus-relay: refusing to start without a nonempty EREBUS_RELAY_TOKEN. \
             Set a token, or set EREBUS_RELAY_INSECURE=1 for a local-only run."
        );
        std::process::exit(2);
    }
    if token.is_none() {
        eprintln!("erebus-relay: WARNING running without authentication (EREBUS_RELAY_INSECURE=1)");
    }

    let relay = match FileRelay::with_retention(&root, retention) {
        Ok(relay) => Arc::new(relay),
        Err(error) => {
            eprintln!("erebus-relay: cannot open storage at {root}: {error}");
            std::process::exit(1);
        }
    };

    let state = AppState {
        relay: Arc::clone(&relay),
        token,
    };
    let app = app(state);

    let address = listen_address(auth_enabled, port);
    let listener = match tokio::net::TcpListener::bind(address).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("erebus-relay: cannot bind {address}: {error}");
            std::process::exit(1);
        }
    };
    eprintln!(
        "erebus-relay: listening on {address}, retention {retention}s, storage {root}, auth {}",
        if auth_enabled { "on" } else { "off" }
    );
    if let Err(error) = axum::serve(listener, app).await {
        eprintln!("erebus-relay: server error: {error}");
        std::process::exit(1);
    }
}

fn validate_token(token: Option<&str>, insecure: bool) -> Result<(), ()> {
    match token {
        Some(token) if token.trim().is_empty() => Err(()),
        None if !insecure => Err(()),
        _ => Ok(()),
    }
}

fn listen_address(auth_enabled: bool, port: u16) -> SocketAddr {
    SocketAddr::from((
        if auth_enabled {
            [0, 0, 0, 0]
        } else {
            [127, 0, 0, 1]
        },
        port,
    ))
}

fn app(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/v1/mailbox/{id}", post(put_blob).get(get_blobs))
        .route_layer(middleware::from_fn_with_state(state.clone(), authorize))
        .layer(DefaultBodyLimit::max(MAX_BLOB_BYTES))
        .with_state(state)
}

async fn authorize(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if let Some(token) = &state.token {
        let expected = format!("Bearer {token}");
        let presented = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok());
        if presented != Some(expected.as_str()) {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }
    Ok(next.run(request).await)
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "retention_seconds": state.relay.retention_seconds(),
    }))
}

async fn put_blob(
    State(state): State<AppState>,
    Path(mailbox): Path<String>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mailbox = MailboxId::from_hex(&mailbox)?;
    let receipt = state.relay.put(&mailbox, &body, now())?;
    Ok(Json(json!({
        "cursor": receipt.cursor,
        "expires_at": receipt.expires_at,
        "duplicate": receipt.duplicate,
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadQuery {
    #[serde(default)]
    after: u64,
}

async fn get_blobs(
    State(state): State<AppState>,
    Path(mailbox): Path<String>,
    query: Result<Query<ReadQuery>, axum::extract::rejection::QueryRejection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Query(query) = query.map_err(|_| ApiError {
        status: StatusCode::BAD_REQUEST,
        message: "mailbox cursor is invalid",
    })?;
    let mailbox = MailboxId::from_hex(&mailbox)?;
    let blobs = state.relay.get(&mailbox, query.after, now())?;
    let blobs: Vec<serde_json::Value> = blobs
        .into_iter()
        .map(|blob| {
            json!({
                "cursor": blob.cursor,
                "expires_at": blob.expires_at,
                "blob": hex::encode(blob.blob),
            })
        })
        .collect();
    Ok(Json(json!({ "blobs": blobs })))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

struct ApiError {
    status: StatusCode,
    message: &'static str,
}

impl From<RelayError> for ApiError {
    fn from(error: RelayError) -> Self {
        let (status, message) = match error {
            RelayError::BlobTooLarge { .. } => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "relay blob exceeds the size bound",
            ),
            RelayError::MailboxFull(_) => {
                (StatusCode::INSUFFICIENT_STORAGE, "relay mailbox is full")
            }
            RelayError::InvalidMailbox => (StatusCode::BAD_REQUEST, "mailbox id is invalid"),
            RelayError::Io(_) | RelayError::Corrupt(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "relay storage is unavailable",
            ),
        };
        Self { status, message }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn request(
        state: AppState,
        method: &str,
        path: &str,
        token: &str,
        body: &[u8],
    ) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = app(state);
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let result = async {
            let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
            let headers = format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n", body.len());
            socket.write_all(headers.as_bytes()).await.unwrap();
            socket.write_all(body).await.unwrap();
            let mut response = Vec::new();
            socket.read_to_end(&mut response).await.unwrap();
            String::from_utf8(response).unwrap()
        };
        let response = tokio::time::timeout(std::time::Duration::from_secs(3), result).await;
        task.abort();
        response.unwrap()
    }

    #[test]
    fn empty_token_is_not_an_authenticated_configuration() {
        assert!(validate_token(Some(""), false).is_err());
        assert!(validate_token(Some(""), true).is_err());
        assert!(validate_token(Some(" \t"), false).is_err());
        assert!(validate_token(None, false).is_err());
        assert!(validate_token(None, true).is_ok());
        assert!(validate_token(Some("test-token"), false).is_ok());
        assert!(listen_address(false, 8080).ip().is_loopback());
        assert!(listen_address(true, 8080).ip().is_unspecified());
    }

    #[tokio::test]
    async fn relay_http_rejects_private_shapes_and_redacts_storage_errors() {
        let root = tempfile::tempdir().unwrap();
        let relay = Arc::new(FileRelay::open(root.path()).unwrap());
        let state = AppState {
            relay,
            token: Some("test-token".into()),
        };
        let id = "01".repeat(32);
        let path = format!("/v1/mailbox/{id}");
        let response = request(
            state.clone(),
            "POST",
            &path,
            "private-marker",
            b"private-marker",
        )
        .await;
        assert!(response.starts_with("HTTP/1.1 401"));
        assert!(!response.contains("private-marker"));
        assert!(!root.path().join(&id).exists());
        let response = request(state.clone(), "POST", &path, "test-token", &[0xab; 32]).await;
        assert!(response.starts_with("HTTP/1.1 200"));
        for query in ["after=private-marker", "witness=private-marker"] {
            let response = request(
                state.clone(),
                "GET",
                &format!("{path}?{query}"),
                "test-token",
                b"",
            )
            .await;
            assert!(response.starts_with("HTTP/1.1 400"));
            assert!(!response.contains("private-marker"));
        }
        let response = request(state.clone(), "GET", &path, "test-token", b"").await;
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains(&hex::encode([0xab; 32])));
        // A parser/storage diagnostic must not become a remote response body.
        std::fs::write(
            root.path().join(&id).join("index.json"),
            br#"[{"cursor":"private-marker"}]"#,
        )
        .unwrap();
        let response = request(state, "GET", &path, "test-token", b"").await;
        assert!(response.starts_with("HTTP/1.1 500"));
        assert!(response.contains("relay storage is unavailable"));
        assert!(!response.contains("private-marker"));
        assert!(!response.contains("index.json"));
        let error = ApiError::from(RelayError::Io("private-marker".into())).into_response();
        let body = axum::body::to_bytes(error.into_body(), 1024).await.unwrap();
        assert!(!String::from_utf8_lossy(&body).contains("private-marker"));
    }
}
