//! Buyer client for prepaid metered access (`docs/metropolis-operations.md`).
//!
//! One durable cursor per agreement at an independently configured service holds the next
//! index to consume, the index in flight, and N. The in-flight index is durable before any
//! HTTP request, so a timeout, crash, or lost response retries the same index; the seller's
//! call index is the idempotency key, and a retry of a committed index returns its stored
//! bytes without consuming another unit. Each completed call is cached as a record naming its
//! response hash, with the body as the record's blob, before the cursor advances.
//!
//! Recovery compares the cursor with the seller's ledger. A seller ahead of the cursor means
//! replies were lost after commit: earlier indices are fetched again as repeated reads, which
//! consume nothing. A seller behind the cursor, or behind any call still cached, lost state:
//! the client fails closed and never consumes an index it already holds a second time. A
//! cached call counts as complete only with its intact body; a missing or damaged body is
//! refetched as a repeated read. A known index never accepts different bytes.
//!
//! Lock order: cursor-store identity lock, then the cursor record lock, held for the whole
//! operation including HTTP; then call-store identity lock, then the call record lock. This
//! serializes every metered operation that shares one cache. Nothing here signs or submits a
//! payment, and no failure is answered by paying again.

use std::{
    fmt,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use ark_std::rand::{rngs::OsRng, RngCore};
use erebus_journal::{
    FaultHook, IdentityLock, JournalRecord, NoFaults, RecordId, RecordLock, Store, StoreError,
};
use erebus_transport::disclosure::{verify_selected_agreement, SelectedAgreement};
use reqwest::{redirect::Policy, Client, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

use super::{
    call_id, entitlement_id, http::METERED_PATH, sign_metered_request, EntitlementId,
    MeteredOperation, MeteredRequest, MAX_METERED_QUANTITY, MAX_METERED_RESPONSE_BYTES,
    METERED_FULFILLMENT_METHOD, METERED_UNIT,
};
use crate::access::AccessError;

const CURSOR_DIR: &str = "metered";
const CALL_DIR: &str = "calls";
/// The single cached response of a call record is attempt 0 of its blob.
const RESPONSE_ATTEMPT: usize = 0;
/// Lifetime of each signed request; every attempt signs afresh with a new nonce.
const REQUEST_LIFETIME: u64 = 120;
const MAX_RESPONSE: usize = MAX_METERED_RESPONSE_BYTES as usize;
/// A call reply: hex body plus fixed fields.
const MAX_CALL_REPLY_BYTES: usize = MAX_RESPONSE * 2 + 4096;
/// A status reply lists at most 4096 calls of bounded size.
const MAX_STATUS_REPLY_BYTES: usize = 2 * 1024 * 1024;
const RECORD_LIMIT: u64 = 4096;

/// Metered access failures. None of them authorizes another payment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MeteredClientError {
    /// Endpoint or local cache configuration is invalid.
    #[error("configure a trusted HTTPS metered endpoint and private local cache")]
    Configuration,
    /// Invalid agreement or not a metered profile.
    #[error("agreement does not support metered access")]
    Agreement,
    /// The local key does not match the agreement buyer.
    #[error("configure the existing buyer agreement key for access")]
    Authentication,
    /// The seller is still observing finalized payment. The in-flight index is kept.
    #[error("access is pending; retry the same call without another payment")]
    Pending,
    /// Service unavailable, rejected, or timed out. The in-flight index is kept.
    #[error(
        "metered service unavailable; retain state and retry the same call without another payment"
    )]
    Unavailable,
    /// The seller reply failed strict checks, or names different bytes for a known index.
    #[error("metered response failed verification; do not pay again")]
    Response,
    /// Local durable storage is unavailable or corrupt.
    #[error("local metered cache unavailable; retain state and retry without another payment")]
    Storage,
    /// The seller's ledger is behind calls this buyer already holds. Nothing is reconsumed.
    #[error("seller ledger is behind the local cursor; stop and report it, do not pay again")]
    SellerLedgerBehind,
    /// Every prepaid call has been consumed. Earlier indices remain readable.
    #[error("prepaid allowance is exhausted")]
    AllowanceExhausted,
    /// The requested index is ahead of the next unconsumed index.
    #[error("call index is ahead of the next unconsumed call")]
    CallIndexAhead,
}

impl MeteredClientError {
    /// Stable machine-readable code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Configuration => "configuration",
            Self::Agreement => "agreement",
            Self::Authentication => "authentication",
            Self::Pending => "pending",
            Self::Unavailable => "unavailable",
            Self::Response => "response_invalid",
            Self::Storage => "storage",
            Self::SellerLedgerBehind => "seller_ledger_behind",
            Self::AllowanceExhausted => "allowance_exhausted",
            Self::CallIndexAhead => "call_index_ahead",
        }
    }
}

impl From<StoreError<ClientId>> for MeteredClientError {
    fn from(_: StoreError<ClientId>) -> Self {
        Self::Storage
    }
}

/// One call's response, cached locally. This does not independently verify payment, the
/// resource's truth, or delivery: the bytes are checked only against the seller-reported hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MeteredCallReceipt {
    /// Commitment of the signed agreement.
    pub deal_commitment: String,
    /// Entitlement id at this service.
    pub entitlement_id: String,
    /// Zero-based index of this call.
    pub call_index: u64,
    /// Prepaid calls N from the verified agreement.
    pub quantity: u64,
    /// Local cursor after this call: the next index a new call consumes.
    pub next_call_index: u64,
    /// `quantity - next_call_index`.
    pub remaining: u64,
    /// SHA-256 independently computed from the cached bytes.
    pub response_sha256: String,
    /// Cached response length.
    pub response_bytes: u64,
    /// Owner-only durable file, never response bytes in model-visible output.
    pub response_file: PathBuf,
    /// The seller had committed this index before this request, or it came from the cache.
    pub repeated: bool,
    /// Served from verified local storage without HTTP.
    pub cached: bool,
    /// The seller reported committing this call after the signed delivery deadline.
    pub late: bool,
    /// The seller claimed finalized payment; this client does not verify that claim.
    pub seller_reported_payment_finalized: bool,
    /// The bytes match the hash the seller reported for this index.
    pub response_hash_consistent: bool,
    /// Always false: a feed's bytes are not pinned in the agreement.
    pub resource_verified: bool,
    /// Always false: this client does not observe the chain.
    pub payment_verified: bool,
    /// Always false: a cached response is not an independent delivery audit.
    pub delivery_verified: bool,
}

/// The seller's allowance, reconciled with the local cursor. Consumes nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MeteredStatusReceipt {
    /// Commitment of the signed agreement.
    pub deal_commitment: String,
    /// Entitlement id at this service.
    pub entitlement_id: String,
    /// Prepaid calls N.
    pub quantity: u64,
    /// The seller's next unconsumed index.
    pub seller_next_call_index: u64,
    /// `quantity - seller_next_call_index`.
    pub remaining: u64,
    /// The local cursor after reconciliation.
    pub next_call_index: u64,
    /// An index sent but not yet committed by the seller, if any.
    pub in_flight: Option<u64>,
    /// No prepaid call remains.
    pub exhausted: bool,
    /// Earlier responses fetched again as repeated reads during this status.
    pub recovered_calls: u64,
    /// The seller claimed finalized payment; this client does not verify that claim.
    pub seller_reported_payment_finalized: bool,
    /// Always false: this client does not observe the chain.
    pub payment_verified: bool,
    /// Always false.
    pub delivery_verified: bool,
}

/// Record id: lowercase hex of an entitlement or call id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientId(String);

impl fmt::Display for ClientId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl RecordId for ClientId {
    fn as_file_stem(&self) -> &str {
        &self.0
    }
    fn from_file_stem(stem: &str) -> Option<Self> {
        is_hex32(stem).then(|| Self(stem.into()))
    }
}

fn is_hex32(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Durable cursor of one entitlement.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    version: u32,
    id: ClientId,
    service_id: [u8; 32],
    commitment: [u8; 32],
    quantity: u64,
    next_index: u64,
    in_flight: Option<u64>,
    exhausted: bool,
}

impl JournalRecord for Cursor {
    type Id = ClientId;
    const CURRENT_VERSION: u32 = 1;
    const OLDEST_READABLE_VERSION: u32 = 1;
    fn version(&self) -> u32 {
        self.version
    }
    fn record_id(&self) -> &Self::Id {
        &self.id
    }
    fn attempt_count(&self) -> usize {
        1
    }
}

impl Cursor {
    /// Moves the cursor to `next`, clearing an in-flight index below it.
    fn advance(&mut self, next: u64) {
        self.next_index = next;
        if self.in_flight.is_some_and(|index| index < next) {
            self.in_flight = None;
        }
        self.exhausted = next >= self.quantity;
    }
}

/// One cached call. Its blob is the response body.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CachedCall {
    version: u32,
    id: ClientId,
    entitlement: [u8; 32],
    index: u64,
    response_len: u64,
    response_sha256: [u8; 32],
    late: bool,
}

impl JournalRecord for CachedCall {
    type Id = ClientId;
    const CURRENT_VERSION: u32 = 1;
    const OLDEST_READABLE_VERSION: u32 = 1;
    fn version(&self) -> u32 {
        self.version
    }
    fn record_id(&self) -> &Self::Id {
        &self.id
    }
    fn attempt_count(&self) -> usize {
        1
    }
}

/// Trusted endpoint, service identity, and durable buyer cursor and response cache.
/// Parent directories must remain under operator control.
#[derive(Clone)]
pub struct MeteredClient {
    client: Client,
    endpoint: Url,
    service_id: [u8; 32],
    cursors: Store<Cursor>,
    calls: Store<CachedCall>,
}

impl fmt::Debug for MeteredClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MeteredClient { <redacted> }")
    }
}

/// The verified agreement and the store handles one operation works on.
#[derive(Clone)]
struct Ledger {
    cursors: Store<Cursor>,
    calls: Store<CachedCall>,
    service_id: [u8; 32],
    commitment: [u8; 32],
    entitlement: EntitlementId,
    quantity: u64,
    delivery_deadline: u64,
}

/// Locks held for one whole operation.
struct Held {
    _identity: IdentityLock,
    _record: RecordLock,
}

/// A seller call reply that passed every check.
struct VerifiedCall {
    body: Zeroizing<Vec<u8>>,
    response_sha256: [u8; 32],
    repeated: bool,
    late: bool,
    seller_next: u64,
}

/// A seller status reply that passed every check.
struct VerifiedStatus {
    next: u64,
    /// Response length and hash of every committed index below `next`, in order.
    calls: Vec<(u64, [u8; 32])>,
}

/// One operation's signing context.
struct Context<'a> {
    ledger: &'a Ledger,
    evidence: &'a SelectedAgreement,
    seed: &'a Zeroizing<[u8; 32]>,
    now: u64,
    started: Instant,
}

impl Context<'_> {
    /// Expiry of a request signed now, measured from the caller's clock.
    fn expires_at(&self) -> Result<u64, MeteredClientError> {
        self.now
            .checked_add(self.started.elapsed().as_secs())
            .and_then(|now| now.checked_add(REQUEST_LIFETIME))
            .ok_or(MeteredClientError::Authentication)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CallReply {
    status: String,
    call_index: u64,
    quantity: u64,
    remaining: u64,
    repeated: bool,
    late: bool,
    response_hex: String,
    response_sha256: String,
    payment_verified: bool,
    delivery_verified: bool,
}

impl Drop for CallReply {
    fn drop(&mut self) {
        self.response_hex.zeroize();
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusEntry {
    call_index: u64,
    response_len: u64,
    response_sha256: String,
    committed_at: u64,
    late: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusReply {
    status: String,
    quantity: u64,
    next_call_index: u64,
    remaining: u64,
    calls: Vec<StatusEntry>,
    payment_verified: bool,
    delivery_verified: bool,
}

#[derive(Deserialize)]
struct ErrorReply {
    error: String,
}

impl MeteredClient {
    /// Open the client. HTTP is permitted only for explicitly selected loopback development.
    pub fn open(
        endpoint: &str,
        service_id: [u8; 32],
        cache: impl Into<PathBuf>,
        allow_loopback_http: bool,
    ) -> Result<Self, MeteredClientError> {
        Self::with_faults(
            endpoint,
            service_id,
            cache,
            allow_loopback_http,
            std::sync::Arc::new(NoFaults),
        )
    }

    /// Open with injected durable-write faults for crash tests.
    pub fn with_faults(
        endpoint: &str,
        service_id: [u8; 32],
        cache: impl Into<PathBuf>,
        allow_loopback_http: bool,
        faults: std::sync::Arc<dyn FaultHook>,
    ) -> Result<Self, MeteredClientError> {
        let endpoint = Url::parse(endpoint).map_err(|_| MeteredClientError::Configuration)?;
        let local = endpoint.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
                || host == "[::1]"
        });
        if service_id == [0; 32]
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || endpoint.path() != METERED_PATH
            || !(endpoint.scheme() == "https"
                || (allow_loopback_http && endpoint.scheme() == "http" && local))
        {
            return Err(MeteredClientError::Configuration);
        }
        let cache = cache.into();
        match std::fs::symlink_metadata(&cache) {
            Ok(metadata) if !metadata.is_dir() => return Err(MeteredClientError::Storage),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut builder = std::fs::DirBuilder::new();
                builder.recursive(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                builder
                    .create(&cache)
                    .map_err(|_| MeteredClientError::Storage)?;
            }
            Err(_) => return Err(MeteredClientError::Storage),
        }
        let root = cache.join(CURSOR_DIR);
        let cursors = Store::open_with_fault_hook(root.clone(), faults.clone())
            .map_err(|_| MeteredClientError::Storage)?;
        let calls = Store::open_with_fault_hook(root.join(CALL_DIR), faults)
            .map_err(|_| MeteredClientError::Storage)?;
        let client = Client::builder()
            .redirect(Policy::none())
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|_| MeteredClientError::Configuration)?;
        Ok(Self {
            client,
            endpoint,
            service_id,
            cursors,
            calls,
        })
    }

    /// Return an already completed call from verified local storage. Requires no key, chain
    /// provider, or seller. `None` when the index is not complete locally.
    pub fn cached(
        &self,
        evidence: &SelectedAgreement,
        call_index: u64,
    ) -> Result<Option<MeteredCallReceipt>, MeteredClientError> {
        let ledger = self.ledger(evidence)?;
        let _held = ledger.lock()?;
        let Some(cursor) = ledger.load()? else {
            return Ok(None);
        };
        if call_index >= cursor.next_index {
            return Ok(None);
        }
        let Some(record) = ledger.read_call(call_index)? else {
            return Ok(None);
        };
        // The record can be a rename a previous attempt never confirmed durable.
        ledger.sync_calls()?;
        if !ledger.body_intact(&record)? {
            return Ok(None);
        }
        ledger.read_body(&record)?;
        Ok(Some(ledger.receipt(&record, &cursor, true, true)))
    }

    /// Consume the next call, finish the call in flight, or fetch an earlier call.
    ///
    /// `call_index` absent consumes the next index or finishes the one in flight. An index
    /// below the cursor returns cached bytes, fetching them again as a repeated read if the
    /// cache lost them; an index above it is rejected locally. Callers must keep the seed
    /// private and erase their copy after use.
    pub async fn call(
        &self,
        evidence: &SelectedAgreement,
        seed: &[u8; 32],
        call_index: Option<u64>,
        now: u64,
    ) -> Result<MeteredCallReceipt, MeteredClientError> {
        let ledger = self.ledger(evidence)?;
        let seed = Zeroizing::new(*seed);
        let context = Context {
            ledger: &ledger,
            evidence,
            seed: &seed,
            now,
            started: Instant::now(),
        };
        let _held = blocking({
            let ledger = ledger.clone();
            move || ledger.lock()
        })
        .await?;
        let mut cursor = match blocking({
            let ledger = ledger.clone();
            move || ledger.load()
        })
        .await?
        {
            Some(cursor) => cursor,
            // No local state: learn the seller's position before consuming anything, so a
            // wiped cache never consumes an index it could have replayed.
            None => self.sync(&context, None).await?.0,
        };
        let target = call_index.unwrap_or(cursor.next_index);
        if target < cursor.next_index {
            return self.earlier(&context, cursor, target).await;
        }
        if target > cursor.next_index {
            return Err(if cursor.exhausted {
                MeteredClientError::AllowanceExhausted
            } else {
                MeteredClientError::CallIndexAhead
            });
        }
        if cursor.exhausted {
            return Err(MeteredClientError::AllowanceExhausted);
        }
        // Already stored locally (a crash after storing, before the cursor moved): finish it
        // without HTTP. Never request an index that is complete locally, and never report it
        // complete before its rename is durable. A record exists only for an index the seller
        // answered, so one whose body is damaged is repaired by reconciliation as a repeated
        // read against the seller's listed hash; it never consumes a new index.
        let local = blocking({
            let ledger = ledger.clone();
            move || {
                let Some(record) = ledger.read_call(target)? else {
                    return Ok(None);
                };
                ledger.sync_calls()?;
                if !ledger.body_intact(&record)? {
                    return Ok(Some(None));
                }
                ledger.read_body(&record)?;
                Ok(Some(Some(record)))
            }
        })
        .await?;
        if let Some(local) = local {
            let Some(record) = local else {
                return self.earlier(&context, cursor, target).await;
            };
            cursor.advance(target + 1);
            let saved = cursor.clone();
            let ledger_copy = ledger.clone();
            blocking(move || ledger_copy.save(&saved)).await?;
            return Ok(ledger.receipt(&record, &cursor, true, true));
        }
        if cursor.in_flight != Some(target) {
            cursor.in_flight = Some(target);
            let saved = cursor.clone();
            let ledger_copy = ledger.clone();
            // Durable before HTTP: every retry after this point reuses this index.
            blocking(move || ledger_copy.save(&saved)).await?;
        }
        let reply = self.request_call(&context, target, None).await?;
        let seller_next = reply.seller_next;
        let (record, repeated) = {
            let ledger = ledger.clone();
            blocking(move || {
                let record = ledger.store_call(target, &reply)?;
                Ok((record, reply.repeated))
            })
            .await?
        };
        cursor.advance(target + 1);
        {
            let saved = cursor.clone();
            let ledger = ledger.clone();
            blocking(move || ledger.save(&saved)).await?;
        }
        if seller_next > target + 1 {
            // The seller committed later indices this cursor never saw: replies were lost
            // after commit. Rebuild them as repeated reads; consume nothing new.
            cursor = self.sync(&context, Some(cursor)).await?.0;
        }
        Ok(ledger.receipt(&record, &cursor, repeated, false))
    }

    /// Report the seller's allowance and reconcile the local cursor with it. A seller ahead
    /// of the cursor is rebuilt by repeated reads; a seller behind it fails closed.
    pub async fn status(
        &self,
        evidence: &SelectedAgreement,
        seed: &[u8; 32],
        now: u64,
    ) -> Result<MeteredStatusReceipt, MeteredClientError> {
        let ledger = self.ledger(evidence)?;
        let seed = Zeroizing::new(*seed);
        let context = Context {
            ledger: &ledger,
            evidence,
            seed: &seed,
            now,
            started: Instant::now(),
        };
        let _held = blocking({
            let ledger = ledger.clone();
            move || ledger.lock()
        })
        .await?;
        let cursor = blocking({
            let ledger = ledger.clone();
            move || ledger.load()
        })
        .await?;
        let (cursor, status, recovered) = self.sync(&context, cursor).await?;
        Ok(MeteredStatusReceipt {
            deal_commitment: hex::encode(ledger.commitment),
            entitlement_id: ledger.entitlement.to_hex(),
            quantity: ledger.quantity,
            seller_next_call_index: status.next,
            remaining: ledger.quantity - status.next,
            next_call_index: cursor.next_index,
            in_flight: cursor.in_flight,
            exhausted: cursor.exhausted,
            recovered_calls: recovered,
            seller_reported_payment_finalized: true,
            payment_verified: false,
            delivery_verified: false,
        })
    }

    /// An index below the cursor, or one at it whose record is stored: the cached bytes, or a
    /// repeated read after reconciling.
    async fn earlier(
        &self,
        context: &Context<'_>,
        cursor: Cursor,
        index: u64,
    ) -> Result<MeteredCallReceipt, MeteredClientError> {
        let ledger = context.ledger;
        let local = blocking({
            let ledger = ledger.clone();
            move || {
                let Some(record) = ledger.read_call(index)? else {
                    return Ok(None);
                };
                ledger.sync_calls()?;
                // A record without its intact body is not complete: reconcile to refetch it.
                if !ledger.body_intact(&record)? {
                    return Ok(None);
                }
                ledger.read_body(&record)?;
                Ok(Some(record))
            }
        })
        .await?;
        if let Some(record) = local {
            return Ok(ledger.receipt(&record, &cursor, true, true));
        }
        // Status first, so a seller that lost this index fails closed instead of consuming it.
        let (cursor, _, _) = self.sync(context, Some(cursor)).await?;
        let record = blocking({
            let ledger = ledger.clone();
            move || {
                let record = ledger
                    .read_call(index)?
                    .ok_or(MeteredClientError::Storage)?;
                ledger.read_body(&record)?;
                Ok(record)
            }
        })
        .await?;
        Ok(ledger.receipt(&record, &cursor, true, false))
    }

    /// Reconciles with the seller's status: fails closed if the seller is behind the cursor or
    /// any surviving cached call, checks every cached hash against the seller's list, fetches
    /// every index below the seller's next that lacks a record or an intact body as a repeated
    /// read, then moves the cursor to the seller's next.
    async fn sync(
        &self,
        context: &Context<'_>,
        cursor: Option<Cursor>,
    ) -> Result<(Cursor, VerifiedStatus, u64), MeteredClientError> {
        let ledger = context.ledger;
        let status = self.request_status(context).await?;
        let local_next = cursor.as_ref().map_or(0, |cursor| cursor.next_index);
        if status.next < local_next {
            return Err(MeteredClientError::SellerLedgerBehind);
        }
        let missing = blocking({
            let ledger = ledger.clone();
            let listed = status.calls.clone();
            move || ledger.missing(&listed)
        })
        .await?;
        let mut recovered = 0;
        for index in missing {
            let expected = status
                .calls
                .get(usize::try_from(index).map_err(|_| MeteredClientError::Response)?)
                .copied()
                .ok_or(MeteredClientError::Response)?;
            let reply = self.request_call(context, index, Some(expected)).await?;
            let ledger = ledger.clone();
            blocking(move || ledger.store_call(index, &reply).map(|_| ())).await?;
            recovered += 1;
        }
        let mut cursor = cursor.unwrap_or(Cursor {
            version: Cursor::CURRENT_VERSION,
            id: ledger.cursor_id(),
            service_id: ledger.service_id,
            commitment: ledger.commitment,
            quantity: ledger.quantity,
            next_index: 0,
            in_flight: None,
            exhausted: false,
        });
        cursor.advance(status.next);
        let saved = cursor.clone();
        let ledger = ledger.clone();
        blocking(move || ledger.save(&saved)).await?;
        Ok((cursor, status, recovered))
    }

    /// Signs one request with a fresh nonce and posts it.
    async fn post(
        &self,
        context: &Context<'_>,
        operation: MeteredOperation,
        index: u64,
        limit: usize,
    ) -> Result<(u16, Zeroizing<Vec<u8>>), MeteredClientError> {
        let expires_at = context.expires_at()?;
        let evidence = context.evidence.clone();
        let seed = context.seed.clone();
        let service_id = self.service_id;
        let request: MeteredRequest = tokio::task::spawn_blocking(move || {
            let mut nonce = [0; 32];
            OsRng
                .try_fill_bytes(&mut nonce)
                .map_err(|_| MeteredClientError::Authentication)?;
            sign_metered_request(
                &evidence, service_id, operation, index, nonce, expires_at, &seed,
            )
            .map_err(|error| match error {
                AccessError::Agreement => MeteredClientError::Agreement,
                _ => MeteredClientError::Authentication,
            })
        })
        .await
        .map_err(|_| MeteredClientError::Authentication)??;
        let mut response = self
            .client
            .post(self.endpoint.clone())
            .json(&request)
            .send()
            .await
            .map_err(|_| MeteredClientError::Unavailable)?;
        let status = response.status().as_u16();
        let mut body = Zeroizing::new(Vec::new());
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| MeteredClientError::Unavailable)?
        {
            if body.len().saturating_add(chunk.len()) > limit {
                return Err(MeteredClientError::Response);
            }
            body.extend_from_slice(&chunk);
        }
        Ok((status, body))
    }

    /// Requests call `index`. With `expected`, this is a replay of an index the seller listed
    /// as committed: the reply must be a repeat with exactly the listed bytes.
    async fn request_call(
        &self,
        context: &Context<'_>,
        index: u64,
        expected: Option<(u64, [u8; 32])>,
    ) -> Result<VerifiedCall, MeteredClientError> {
        let quantity = context.ledger.quantity;
        let (status, body) = self
            .post(context, MeteredOperation::Call, index, MAX_CALL_REPLY_BYTES)
            .await?;
        match status {
            200 => {}
            202 => return Err(MeteredClientError::Pending),
            409 if error_code(&body) == Some("out_of_order") => {
                // The seller's next is below an index this buyer is entitled to request.
                return Err(MeteredClientError::SellerLedgerBehind);
            }
            // Every requested index is below N, so exhaustion is a seller inconsistency.
            403 => return Err(MeteredClientError::Response),
            _ => return Err(MeteredClientError::Unavailable),
        }
        let reply: CallReply =
            serde_json::from_slice(&body).map_err(|_| MeteredClientError::Response)?;
        let committed = index.checked_add(1).ok_or(MeteredClientError::Response)?;
        if reply.status != "issued"
            || reply.call_index != index
            || reply.quantity != quantity
            || reply.remaining > quantity.saturating_sub(committed)
            || (!reply.repeated && reply.remaining != quantity - committed)
            || !reply.payment_verified
            || reply.delivery_verified
            || reply.response_hex.len() > MAX_RESPONSE * 2
            || !is_hex32(&reply.response_sha256)
        {
            return Err(MeteredClientError::Response);
        }
        let bytes = Zeroizing::new(
            hex::decode(&reply.response_hex).map_err(|_| MeteredClientError::Response)?,
        );
        let response_sha256: [u8; 32] = Sha256::digest(&bytes).into();
        if hex::encode(response_sha256) != reply.response_sha256 {
            return Err(MeteredClientError::Response);
        }
        if let Some((length, hash)) = expected {
            if !reply.repeated {
                // The seller listed this index as committed, then consumed it fresh.
                return Err(MeteredClientError::SellerLedgerBehind);
            }
            if u64::try_from(bytes.len()).ok() != Some(length) || response_sha256 != hash {
                return Err(MeteredClientError::Response);
            }
        }
        Ok(VerifiedCall {
            body: bytes,
            response_sha256,
            repeated: reply.repeated,
            late: reply.late,
            seller_next: quantity - reply.remaining,
        })
    }

    async fn request_status(
        &self,
        context: &Context<'_>,
    ) -> Result<VerifiedStatus, MeteredClientError> {
        let ledger = context.ledger;
        let (status, body) = self
            .post(context, MeteredOperation::Status, 0, MAX_STATUS_REPLY_BYTES)
            .await?;
        match status {
            200 => {}
            202 => return Err(MeteredClientError::Pending),
            _ => return Err(MeteredClientError::Unavailable),
        }
        let reply: StatusReply =
            serde_json::from_slice(&body).map_err(|_| MeteredClientError::Response)?;
        if reply.status != "allowance"
            || reply.quantity != ledger.quantity
            || reply.next_call_index > ledger.quantity
            || reply.remaining != ledger.quantity - reply.next_call_index
            || u64::try_from(reply.calls.len()).ok() != Some(reply.next_call_index)
            || !reply.payment_verified
            || reply.delivery_verified
        {
            return Err(MeteredClientError::Response);
        }
        let calls = reply
            .calls
            .iter()
            .enumerate()
            .map(|(position, entry)| {
                let hash: [u8; 32] = (is_hex32(&entry.response_sha256))
                    .then(|| hex::decode(&entry.response_sha256).ok())
                    .flatten()
                    .and_then(|bytes| bytes.try_into().ok())
                    .ok_or(MeteredClientError::Response)?;
                if u64::try_from(position).ok() != Some(entry.call_index)
                    || entry.response_len > u64::from(MAX_METERED_RESPONSE_BYTES)
                    || entry.late != (entry.committed_at > ledger.delivery_deadline)
                {
                    return Err(MeteredClientError::Response);
                }
                Ok((entry.response_len, hash))
            })
            .collect::<Result<_, _>>()?;
        Ok(VerifiedStatus {
            next: reply.next_call_index,
            calls,
        })
    }

    fn ledger(&self, evidence: &SelectedAgreement) -> Result<Ledger, MeteredClientError> {
        let agreement =
            verify_selected_agreement(evidence).map_err(|_| MeteredClientError::Agreement)?;
        let terms = &evidence.terms;
        let service = &terms.service;
        if service.access_recipient != terms.buyer_authorization_key
            || service.unit != METERED_UNIT
            || service.fulfillment_method != METERED_FULFILLMENT_METHOD
            || !(1..=MAX_METERED_QUANTITY).contains(&service.quantity.get())
        {
            return Err(MeteredClientError::Agreement);
        }
        let commitment = *agreement.commitment.as_bytes();
        Ok(Ledger {
            cursors: self.cursors.clone(),
            calls: self.calls.clone(),
            service_id: self.service_id,
            commitment,
            entitlement: entitlement_id(&self.service_id, &commitment),
            quantity: u64::try_from(service.quantity.get())
                .map_err(|_| MeteredClientError::Agreement)?,
            delivery_deadline: service.delivery_deadline,
        })
    }
}

impl Ledger {
    fn cursor_id(&self) -> ClientId {
        ClientId(self.entitlement.to_hex())
    }

    fn call_id(&self, index: u64) -> ClientId {
        ClientId(call_id(&self.entitlement, index).to_hex())
    }

    fn lock(&self) -> Result<Held, MeteredClientError> {
        let identity = self.cursors.lock_identity()?;
        let record = self.cursors.lock_record(&self.cursor_id())?;
        Ok(Held {
            _identity: identity,
            _record: record,
        })
    }

    /// The cursor, if any. A cursor that contradicts the agreement or itself is corrupt.
    fn load(&self) -> Result<Option<Cursor>, MeteredClientError> {
        let id = self.cursor_id();
        let path = self.cursors.record_path(&id);
        match std::fs::symlink_metadata(&path) {
            Ok(info) if !info.is_file() || info.len() > RECORD_LIMIT => {
                return Err(MeteredClientError::Storage)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(MeteredClientError::Storage),
            _ => {}
        }
        private_read(&path, RECORD_LIMIT as usize)?;
        let cursor = self.cursors.read(&id)?.ok_or(MeteredClientError::Storage)?;
        if cursor.service_id != self.service_id
            || cursor.commitment != self.commitment
            || cursor.quantity != self.quantity
            || cursor.next_index > self.quantity
            || cursor.exhausted != (cursor.next_index >= self.quantity)
            || cursor
                .in_flight
                .is_some_and(|index| index != cursor.next_index)
        {
            return Err(MeteredClientError::Storage);
        }
        Ok(Some(cursor))
    }

    fn save(&self, cursor: &Cursor) -> Result<(), MeteredClientError> {
        Ok(self.cursors.write(cursor)?)
    }

    /// Makes every cached-call rename durable before a read treats one as complete.
    ///
    /// A previous attempt can have renamed a call record and then failed its directory sync,
    /// leaving the record visible but unconfirmed. Returning it or advancing the cursor past it
    /// without this could lose it in a power failure after the caller already has a receipt.
    fn sync_calls(&self) -> Result<(), MeteredClientError> {
        Ok(self.calls.sync()?)
    }

    fn read_call(&self, index: u64) -> Result<Option<CachedCall>, MeteredClientError> {
        let id = self.call_id(index);
        let path = self.calls.record_path(&id);
        match std::fs::symlink_metadata(&path) {
            Ok(info) if !info.is_file() || info.len() > RECORD_LIMIT => {
                return Err(MeteredClientError::Storage)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(MeteredClientError::Storage),
            _ => {}
        }
        private_read(&path, RECORD_LIMIT as usize)?;
        let record = self.calls.read(&id)?.ok_or(MeteredClientError::Storage)?;
        if record.entitlement != *self.entitlement.as_bytes()
            || record.index != index
            || record.response_len > u64::from(MAX_METERED_RESPONSE_BYTES)
        {
            return Err(MeteredClientError::Storage);
        }
        Ok(Some(record))
    }

    /// The cached body, checked against its record.
    fn read_body(&self, record: &CachedCall) -> Result<Zeroizing<Vec<u8>>, MeteredClientError> {
        let bytes = Zeroizing::new(private_read(
            &self.calls.blob_path(&record.id, RESPONSE_ATTEMPT),
            MAX_RESPONSE,
        )?);
        if u64::try_from(bytes.len()).ok() != Some(record.response_len)
            || <[u8; 32]>::from(Sha256::digest(&*bytes)) != record.response_sha256
        {
            return Err(MeteredClientError::Storage);
        }
        Ok(bytes)
    }

    /// Whether the cached body is present with exactly the record's length and hash. A
    /// missing, truncated, or different body means the call is not complete locally and may
    /// be refetched with the record's hash; an unsafe file (not a regular file, or readable by
    /// others) is a storage failure, never repaired.
    fn body_intact(&self, record: &CachedCall) -> Result<bool, MeteredClientError> {
        let path = self.calls.blob_path(&record.id, RESPONSE_ATTEMPT);
        let info = match std::fs::symlink_metadata(&path) {
            Ok(info) => info,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err(MeteredClientError::Storage),
        };
        if !info.is_file() {
            return Err(MeteredClientError::Storage);
        }
        if info.len() != record.response_len {
            return Ok(false);
        }
        let bytes = Zeroizing::new(private_read(&path, MAX_RESPONSE)?);
        Ok(u64::try_from(bytes.len()).ok() == Some(record.response_len)
            && <[u8; 32]>::from(Sha256::digest(&*bytes)) == record.response_sha256)
    }

    /// Stores a verified reply as the record of `index`, body first. A record that already
    /// exists must name exactly these bytes; different bytes for a known index are rejected.
    /// An existing record whose body is missing or damaged gets these bytes, which match its
    /// recorded hash and length, written back under it.
    fn store_call(
        &self,
        index: u64,
        reply: &VerifiedCall,
    ) -> Result<CachedCall, MeteredClientError> {
        let id = self.call_id(index);
        let response_len =
            u64::try_from(reply.body.len()).map_err(|_| MeteredClientError::Response)?;
        let _identity = self.calls.lock_identity()?;
        let _record = self.calls.lock_record(&id)?;
        if let Some(existing) = self.read_call(index)? {
            // A prior attempt's rename may be visible without a confirmed directory sync.
            self.sync_calls()?;
            if existing.response_sha256 != reply.response_sha256
                || existing.response_len != response_len
            {
                return Err(MeteredClientError::Response);
            }
            if !self.body_intact(&existing)? {
                let mut repaired = existing.clone();
                self.calls.write_blob_then_record(
                    &mut repaired,
                    RESPONSE_ATTEMPT,
                    &reply.body,
                    |_| Ok::<(), StoreError<ClientId>>(()),
                )?;
            }
            self.read_body(&existing)?;
            return Ok(existing);
        }
        let mut record = CachedCall {
            version: CachedCall::CURRENT_VERSION,
            id,
            entitlement: *self.entitlement.as_bytes(),
            index,
            response_len,
            response_sha256: reply.response_sha256,
            late: reply.late,
        };
        self.calls
            .write_blob_then_record(&mut record, RESPONSE_ATTEMPT, &reply.body, |_| {
                Ok::<(), StoreError<ClientId>>(())
            })?;
        self.read_body(&record)?;
        Ok(record)
    }

    /// Indices below the seller's next that are not complete locally: no record, or a record
    /// whose body is missing or damaged. Every cached record below it must match the seller's
    /// list, and none may exist at or above the seller's next.
    fn missing(&self, listed: &[(u64, [u8; 32])]) -> Result<Vec<u64>, MeteredClientError> {
        let next = u64::try_from(listed.len()).map_err(|_| MeteredClientError::Response)?;
        // A record exists only for an index the seller answered with 200, so one at or above
        // the seller's next proves the seller lost committed calls, even if the cursor and
        // the records between were lost too. Checked before anything is fetched or saved.
        for index in next..self.quantity {
            if self.read_call(index)?.is_some() {
                return Err(MeteredClientError::SellerLedgerBehind);
            }
        }
        let mut missing = Vec::new();
        let mut cached = false;
        for (index, (length, hash)) in (0..).zip(listed) {
            match self.read_call(index)? {
                Some(record) => {
                    cached = true;
                    if record.response_sha256 != *hash || record.response_len != *length {
                        return Err(MeteredClientError::Response);
                    }
                    if !self.body_intact(&record)? {
                        missing.push(index);
                    }
                }
                None => missing.push(index),
            }
        }
        if cached {
            // Reconciliation must not treat a visible but unconfirmed rename as a completed call.
            self.sync_calls()?;
        }
        Ok(missing)
    }

    fn receipt(
        &self,
        record: &CachedCall,
        cursor: &Cursor,
        repeated: bool,
        cached: bool,
    ) -> MeteredCallReceipt {
        MeteredCallReceipt {
            deal_commitment: hex::encode(self.commitment),
            entitlement_id: self.entitlement.to_hex(),
            call_index: record.index,
            quantity: self.quantity,
            next_call_index: cursor.next_index,
            remaining: self.quantity - cursor.next_index,
            response_sha256: hex::encode(record.response_sha256),
            response_bytes: record.response_len,
            response_file: self.calls.blob_path(&record.id, RESPONSE_ATTEMPT),
            repeated,
            cached,
            late: record.late,
            seller_reported_payment_finalized: true,
            response_hash_consistent: true,
            resource_verified: false,
            payment_verified: false,
            delivery_verified: false,
        }
    }
}

/// Runs one blocking cache step off the async runtime. A panicked step is a storage failure.
async fn blocking<T: Send + 'static>(
    step: impl FnOnce() -> Result<T, MeteredClientError> + Send + 'static,
) -> Result<T, MeteredClientError> {
    tokio::task::spawn_blocking(step)
        .await
        .unwrap_or(Err(MeteredClientError::Storage))
}

fn error_code(body: &[u8]) -> Option<&'static str> {
    let reply: ErrorReply = serde_json::from_slice(body).ok()?;
    (reply.error == "out_of_order").then_some("out_of_order")
}

fn private_read(path: &Path, limit: usize) -> Result<Vec<u8>, MeteredClientError> {
    let info = std::fs::symlink_metadata(path).map_err(|_| MeteredClientError::Storage)?;
    if !info.is_file() || info.len() > limit as u64 {
        return Err(MeteredClientError::Storage);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if info.permissions().mode() & 0o077 != 0 {
            return Err(MeteredClientError::Storage);
        }
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| MeteredClientError::Storage)?
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| MeteredClientError::Storage)?;
    if bytes.len() > limit {
        return Err(MeteredClientError::Storage);
    }
    Ok(bytes)
}
