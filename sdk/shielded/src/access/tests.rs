use super::*;
use erebus_core::{
    auth::{authorization_digest, Authorization, Role},
    commitment::{commit_agreement, CommitmentBlinding},
    ids::{BaseUnits, SignatureBytes},
    terms::{AgreementTerms, FeePolicy, GuaranteeSet},
};
use erebus_transport::identity::AuthorizationIdentity;
use std::sync::Mutex;

#[test]
fn retrieval_budget_leaves_an_authentication_margin() {
    assert_eq!(MAX_REQUEST_LIFETIME - client::RETRIEVAL_TIMEOUT_SECONDS, 60);
}

#[test]
fn late_issuance_preserves_the_signed_deadline_and_recovers_after_restart() {
    for suite in [1, 2] {
        let (policy, paid, mut request) = fixture(suite);
        let deadline = paid.evidence.terms.service.delivery_deadline;
        let now = deadline + 1;
        request.expires_at = now + MAX_REQUEST_LIFETIME;
        request.signature = signature(
            suite,
            [1; 32],
            &request_digest(
                &paid.evidence,
                policy.service_id,
                request.nonce,
                request.expires_at,
                None,
            )
            .unwrap(),
        );
        let root = tempfile::tempdir().unwrap();
        let issuer = AccessIssuer::open(root.path(), policy.clone()).unwrap();
        let first = issuer.issue(&paid, &request, now).unwrap();
        assert!(first.late);
        drop(issuer);
        let restarted = AccessIssuer::open(root.path(), policy).unwrap();
        let repeated = restarted.issue(&paid, &request, now + 1).unwrap();
        assert!(repeated.late && repeated.repeated);
        assert_eq!(first.issued_at, repeated.issued_at);
        assert_eq!(paid.evidence.terms.service.delivery_deadline, deadline);
        assert_eq!(restarted.store.records().unwrap().len(), 1);
    }
}

fn signature(suite: u16, seed: [u8; 32], digest: &[u8; 32]) -> Vec<u8> {
    if suite == 1 {
        AuthorizationIdentity::from_bytes(&seed)
            .unwrap()
            .sign_digest(digest)
            .to_vec()
    } else {
        erebus_core::shielded_auth::sign_message(&seed, digest)
            .unwrap()
            .1
            .to_vec()
    }
}

fn fixture(suite: u16) -> (AccessPolicy, PaidAccess, AccessRequest) {
    let value: serde_json::Value = serde_json::from_str(include_str!(
        "../../../core/tests/fixtures/agreement-v1-vectors.json"
    ))
    .unwrap();
    let mut terms = AgreementTerms::decode(
        &hex::decode(
            value["vectors"][0]["expected"]["canonicalHex"]
                .as_str()
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    terms.transcript_root = [0; 32];
    if suite == 2 {
        terms.suite_id = 2;
        terms.settlement_mode = SettlementMode::Shielded;
        terms.domain.pool = terms.domain.settlement_contract.clone();
        terms.required_guarantees = GuaranteeSet::from_bits(7).unwrap();
        terms.buyer_authorization_key = KeyBytes::new(
            erebus_core::shielded_auth::derive_key(&[1; 32])
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        terms.seller_authorization_key = KeyBytes::new(
            erebus_core::shielded_auth::derive_key(&[2; 32])
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        terms.payment_recipient = KeyBytes::new([0x0b; 32].to_vec()).unwrap();
        terms.fee_policy = FeePolicy::none();
    }
    let policy = AccessPolicy {
        service_id: [0x42; 32],
        seller: terms.seller_authorization_key.clone(),
        suite_id: suite,
        resource: "data.snapshot.v1".into(),
        payload: b"test dataset snapshot".to_vec(),
    };
    terms.service.resource = policy.resource.clone();
    terms.service.quantity = BaseUnits::new(1);
    terms.service.unit = "snapshot".into();
    terms.service.fulfillment_method = "http-access-v1".into();
    terms.service.fulfillment_digest = policy.resource_hash();
    terms.service.access_recipient = terms.buyer_authorization_key.clone();
    let blinding = CommitmentBlinding::from_bytes([0x0a; 32]);
    let commitment = commit_agreement(&terms, &blinding).unwrap();
    let authorization = |role, seed| Authorization {
        role,
        suite_id: suite,
        commitment,
        signature: SignatureBytes::new(signature(
            suite,
            seed,
            &authorization_digest(&terms.domain, role, &commitment, suite).unwrap(),
        ))
        .unwrap(),
    };
    let evidence = SelectedAgreement {
        buyer: authorization(Role::Buyer, [1; 32]),
        seller: authorization(Role::Seller, [2; 32]),
        terms,
        blinding,
        transcript_hash_version: 1,
        messages: Vec::new(),
    };
    let agreement = verify_selected_agreement(&evidence).unwrap();
    let request = AccessRequest {
        deal_commitment: commitment.to_hex(),
        nonce: [4; 32],
        expires_at: 1100,
        signature: signature(
            suite,
            [1; 32],
            &request_digest(&evidence, policy.service_id, [4; 32], 1100, None).unwrap(),
        ),
        payment: None,
    };
    (
        policy,
        PaidAccess {
            evidence,
            agreement,
        },
        request,
    )
}

#[test]
fn lost_response_and_restart_reuse_one_durable_issuance() {
    for suite in [1, 2] {
        let (policy, paid, request) = fixture(suite);
        let root = tempfile::tempdir().unwrap();
        let issuer = AccessIssuer::open(root.path(), policy.clone()).unwrap();
        let first = issuer.issue(&paid, &request, 1000).unwrap();
        assert!(!first.repeated);
        drop(issuer);
        let fresh = AccessIssuer::open(root.path(), policy).unwrap();
        let repeated = fresh.issue(&paid, &request, 1001).unwrap();
        assert!(repeated.repeated);
        assert_eq!(first.issuance_id, repeated.issuance_id);
        assert_eq!(first.issued_at, repeated.issued_at);
        assert_eq!(fresh.store.records().unwrap().len(), 1);
    }
}

#[test]
fn separately_authorized_neighboring_deals_cannot_share_access_or_issuance() {
    for suite in [1, 2] {
        let (policy, first, first_request) = fixture(suite);
        let mut evidence = first.evidence.clone();
        evidence.terms.deal_id[0] ^= 1;
        evidence.terms.settlement_nonce[0] ^= 1;
        let commitment = commit_agreement(&evidence.terms, &evidence.blinding).unwrap();
        for (authorization, role, seed) in [
            (&mut evidence.buyer, Role::Buyer, [1; 32]),
            (&mut evidence.seller, Role::Seller, [2; 32]),
        ] {
            authorization.commitment = commitment;
            authorization.signature = SignatureBytes::new(signature(
                suite,
                seed,
                &authorization_digest(&evidence.terms.domain, role, &commitment, suite).unwrap(),
            ))
            .unwrap();
        }
        let second = PaidAccess {
            agreement: verify_selected_agreement(&evidence).unwrap(),
            evidence,
        };
        assert_ne!(first.agreement.commitment, second.agreement.commitment);
        assert_ne!(first.agreement.nullifier, second.agreement.nullifier);
        let mut second_request = first_request.clone();
        second_request.deal_commitment = commitment.to_hex();
        second_request.signature = signature(
            suite,
            [1; 32],
            &request_digest(
                &second.evidence,
                policy.service_id,
                second_request.nonce,
                second_request.expires_at,
                None,
            )
            .unwrap(),
        );
        let root = tempfile::tempdir().unwrap();
        let issuer = AccessIssuer::open(root.path(), policy.clone()).unwrap();
        issuer
            .authenticate(&first.evidence, &first_request, 1000)
            .unwrap();
        issuer
            .authenticate(&second.evidence, &second_request, 1000)
            .unwrap();
        for (paid, request) in [(&first, &second_request), (&second, &first_request)] {
            assert_eq!(
                issuer.issue(paid, request, 1000),
                Err(AccessError::Authentication)
            );
        }
        // Swapping only the identifier still cannot transplant a valid signature.
        let mut transplanted = first_request.clone();
        transplanted.deal_commitment = second_request.deal_commitment.clone();
        assert_eq!(
            issuer.issue(&second, &transplanted, 1000),
            Err(AccessError::Authentication)
        );
        assert!(issuer.store.records().unwrap().is_empty());
        let issued_first = issuer.issue(&first, &first_request, 1000).unwrap();
        let issued_second = issuer.issue(&second, &second_request, 1000).unwrap();
        assert_ne!(issued_first.issuance_id, issued_second.issuance_id);
        assert_eq!(issuer.store.records().unwrap().len(), 2);
        drop(issuer);
        let restarted = AccessIssuer::open(root.path(), policy).unwrap();
        for (paid, request, issued) in [
            (&first, &first_request, &issued_first),
            (&second, &second_request, &issued_second),
        ] {
            let retry = restarted.issue(paid, request, 1001).unwrap();
            assert!(retry.repeated);
            assert_eq!(retry.issuance_id, issued.issuance_id);
            assert_eq!(retry.issued_at, issued.issued_at);
        }
    }
}

#[test]
fn wrong_buyer_expiry_commitment_nonce_and_service_never_issue() {
    let (policy, paid, request) = fixture(1);
    let root = tempfile::tempdir().unwrap();
    let issuer = AccessIssuer::open(root.path(), policy).unwrap();
    let mut wrong = request.clone();
    wrong.signature = signature(
        1,
        [3; 32],
        &request_digest(
            &paid.evidence,
            issuer.policy.service_id,
            wrong.nonce,
            wrong.expires_at,
            None,
        )
        .unwrap(),
    );
    assert_eq!(
        issuer.issue(&paid, &wrong, 1000),
        Err(AccessError::Authentication)
    );
    for mutate in [0, 1, 2, 3] {
        let mut wrong = request.clone();
        match mutate {
            0 => wrong.expires_at = 1000,
            1 => wrong.expires_at = 1301,
            2 => wrong.nonce = [0; 32],
            _ => wrong.deal_commitment = "00".repeat(32),
        }
        assert_eq!(
            issuer.issue(&paid, &wrong, 1000),
            Err(AccessError::Authentication)
        );
    }
    let digest = request_digest(
        &paid.evidence,
        [0x43; 32],
        request.nonce,
        request.expires_at,
        None,
    )
    .unwrap();
    let mut wrong = request.clone();
    wrong.signature = signature(1, [1; 32], &digest);
    assert_eq!(
        issuer.issue(&paid, &wrong, 1000),
        Err(AccessError::Authentication)
    );
    assert!(issuer.store.records().unwrap().is_empty());
}

#[test]
fn delegated_access_or_a_different_payload_fails_closed() {
    let (policy, mut paid, request) = fixture(1);
    let root = tempfile::tempdir().unwrap();
    let mut changed = policy.clone();
    changed.payload.push(0);
    let issuer = AccessIssuer::open(root.path(), changed).unwrap();
    assert_eq!(
        issuer.issue(&paid, &request, 1000),
        Err(AccessError::Agreement)
    );
    paid.evidence.terms.service.access_recipient = KeyBytes::new([4; 20].to_vec()).unwrap();
    assert_eq!(
        policy.check(&paid.evidence).unwrap_err(),
        AccessError::Agreement
    );
}

struct Fault {
    at: usize,
    count: Mutex<usize>,
}
impl FaultHook for Fault {
    fn after(&self, _: erebus_journal::Boundary<'_>) -> std::io::Result<()> {
        let mut count = self.count.lock().unwrap();
        *count += 1;
        if *count == self.at {
            Err(std::io::Error::other("test fault"))
        } else {
            Ok(())
        }
    }
}

#[test]
fn every_issuance_write_boundary_recovers_without_recharging() {
    let (policy, paid, request) = fixture(1);
    let probe = tempfile::tempdir().unwrap();
    let hook = Arc::new(Fault {
        at: usize::MAX,
        count: Mutex::new(0),
    });
    AccessIssuer::with_faults(probe.path(), policy.clone(), hook.clone())
        .unwrap()
        .issue(&paid, &request, 1000)
        .unwrap();
    let count = *hook.count.lock().unwrap();
    assert!(count > 0);
    for at in 1..=count {
        let root = tempfile::tempdir().unwrap();
        let hook = Arc::new(Fault {
            at,
            count: Mutex::new(0),
        });
        let issuer = AccessIssuer::with_faults(root.path(), policy.clone(), hook).unwrap();
        assert_eq!(
            issuer.issue(&paid, &request, 1000),
            Err(AccessError::Storage)
        );
        drop(issuer);
        let fresh = AccessIssuer::open(root.path(), policy.clone()).unwrap();
        let restored = fresh.issue(&paid, &request, 1001).unwrap();
        assert_eq!(fresh.store.records().unwrap().len(), 1);
        assert!(restored.issued_at == 1000 || restored.issued_at == 1001);
    }
}

#[test]
fn storage_corruption_and_time_rollback_never_return_a_payload() {
    let (policy, paid, request) = fixture(1);
    let root = tempfile::tempdir().unwrap();
    let issuer = AccessIssuer::open(root.path(), policy).unwrap();
    let first = issuer.issue(&paid, &request, 1000).unwrap();
    assert_eq!(
        issuer.issue(&paid, &request, 999),
        Err(AccessError::Storage)
    );
    std::fs::write(
        root.path().join(format!("{}.json", first.issuance_id)),
        b"{}",
    )
    .unwrap();
    assert_eq!(
        issuer.issue(&paid, &request, 1001),
        Err(AccessError::Storage)
    );
}

struct ResourceServer {
    endpoint: String,
    calls: Arc<std::sync::atomic::AtomicUsize>,
    response: Arc<Mutex<(u16, serde_json::Value)>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for ResourceServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn resource_server(policy: &AccessPolicy, paid: &PaidAccess) -> ResourceServer {
    resource_server_with_delay(policy, paid, std::time::Duration::ZERO).await
}

async fn resource_server_with_delay(
    policy: &AccessPolicy,
    paid: &PaidAccess,
    delay: std::time::Duration,
) -> ResourceServer {
    use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let calls = Arc::new(AtomicUsize::new(0));
    let response = Arc::new(Mutex::new((
        200,
        serde_json::json!({
            "status":"issued", "issuance": {
                "issuance_id":issuance_id(&policy.service_id, paid.agreement.commitment.as_bytes()),
                "resource_sha256":hex::encode(policy.resource_hash()), "issued_at":1000,
                "late":1000 > paid.evidence.terms.service.delivery_deadline, "repeated":false
            }, "payload_hex":hex::encode(&policy.payload), "payment_verified":true,"delivery_verified":false
        }),
    )));
    type Shared = (
        Arc<AtomicUsize>,
        Arc<Mutex<(u16, serde_json::Value)>>,
        std::time::Duration,
    );
    async fn handle(
        State(state): State<Shared>,
        Json(_request): Json<AccessRequest>,
    ) -> (StatusCode, Json<serde_json::Value>) {
        state.0.fetch_add(1, Ordering::SeqCst);
        let (status, body) = state.1.lock().unwrap().clone();
        tokio::time::sleep(state.2).await;
        (StatusCode::from_u16(status).unwrap(), Json(body))
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/access", listener.local_addr().unwrap());
    let router = Router::new().route("/v1/access", post(handle)).with_state((
        calls.clone(),
        response.clone(),
        delay,
    ));
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    ResourceServer {
        endpoint,
        calls,
        response,
        task,
    }
}

#[tokio::test]
async fn retrieval_waits_past_the_old_timeout_without_another_request() {
    let (policy, paid, _) = fixture(2);
    let server =
        resource_server_with_delay(&policy, &paid, std::time::Duration::from_secs(31)).await;
    let root = tempfile::tempdir().unwrap();
    let buyer =
        client::AccessClient::open(&server.endpoint, policy.service_id, root.path(), true).unwrap();
    let receipt = buyer
        .retrieve(&paid.evidence, &[1; 32], 1000)
        .await
        .unwrap();
    assert!(receipt.resource_verified && !receipt.payment_verified && !receipt.delivery_verified);
    assert_eq!(server.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn buyer_verifies_both_suites_and_recovers_local_content_without_network_or_keys() {
    use client::{AccessClient, RetrievalError};
    for suite in [1, 2] {
        let (policy, paid, _) = fixture(suite);
        let server = resource_server(&policy, &paid).await;
        let root = tempfile::tempdir().unwrap();
        let buyer =
            AccessClient::open(&server.endpoint, policy.service_id, root.path(), true).unwrap();
        assert!(buyer.cached(&paid.evidence).unwrap().is_none());
        assert_eq!(
            buyer.retrieve(&paid.evidence, &[7; 32], 1000).await,
            Err(RetrievalError::Authentication)
        );
        assert_eq!(server.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        let receipt = buyer
            .retrieve(&paid.evidence, &[1; 32], 1000)
            .await
            .unwrap();
        assert!(
            !receipt.cached
                && receipt.resource_verified
                && !receipt.payment_verified
                && !receipt.delivery_verified
        );
        assert!(receipt.seller_reported_payment_finalized);
        assert_eq!(
            std::fs::read(&receipt.resource_file).unwrap(),
            policy.payload
        );
        assert_eq!(server.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let endpoint = server.endpoint.clone();
        drop(server);
        drop(buyer);
        let restarted =
            AccessClient::open(&endpoint, policy.service_id, root.path(), true).unwrap();
        let local = restarted.cached(&paid.evidence).unwrap().unwrap();
        assert!(local.cached);
        assert_eq!(local.issuance_id, receipt.issuance_id);
        std::fs::write(&receipt.resource_file, b"tampered").unwrap();
        assert_eq!(
            restarted.cached(&paid.evidence),
            Err(RetrievalError::Storage)
        );
    }
}

#[tokio::test]
async fn buyer_rejects_malformed_or_wrong_content_and_never_caches_pending_or_failures() {
    use client::{AccessClient, RetrievalError};
    let (policy, paid, _) = fixture(1);
    let server = resource_server(&policy, &paid).await;
    let valid = server.response.lock().unwrap().1.clone();
    for (pointer, value) in [
        ("/payload_hex", serde_json::json!("00")),
        ("/issuance/issuance_id", serde_json::json!("00".repeat(32))),
        (
            "/issuance/resource_sha256",
            serde_json::json!("00".repeat(32)),
        ),
        ("/issuance/issued_at", serde_json::json!(2000)),
        ("/issuance/late", serde_json::json!(true)),
        ("/payment_verified", serde_json::json!(false)),
        ("/delivery_verified", serde_json::json!(true)),
        (
            "/payload_hex",
            serde_json::json!("00".repeat(MAX_RESOURCE_BYTES + 1)),
        ),
    ] {
        let mut body = valid.clone();
        *body.pointer_mut(pointer).unwrap() = value;
        *server.response.lock().unwrap() = (200, body);
        let root = tempfile::tempdir().unwrap();
        let buyer =
            AccessClient::open(&server.endpoint, policy.service_id, root.path(), true).unwrap();
        assert_eq!(
            buyer.retrieve(&paid.evidence, &[1; 32], 1000).await,
            Err(RetrievalError::Response),
            "{pointer}"
        );
        assert!(buyer.cached(&paid.evidence).unwrap().is_none());
    }
    for (status, expected) in [
        (202, RetrievalError::Pending),
        (302, RetrievalError::Unavailable),
        (503, RetrievalError::Unavailable),
    ] {
        *server.response.lock().unwrap() = (
            status,
            serde_json::json!({"error":"secret-provider-detail"}),
        );
        let root = tempfile::tempdir().unwrap();
        let buyer =
            AccessClient::open(&server.endpoint, policy.service_id, root.path(), true).unwrap();
        assert_eq!(
            buyer.retrieve(&paid.evidence, &[1; 32], 1000).await,
            Err(expected)
        );
        assert!(buyer.cached(&paid.evidence).unwrap().is_none());
    }
    *server.response.lock().unwrap() = (
        503,
        serde_json::json!({"status":"paid_but_undelivered","payment_verified":true,"delivery_verified":false,"retry_without_payment":true,"error":"private-provider-detail"}),
    );
    let root = tempfile::tempdir().unwrap();
    let buyer = AccessClient::open(&server.endpoint, policy.service_id, root.path(), true).unwrap();
    assert_eq!(
        buyer.retrieve(&paid.evidence, &[1; 32], 1000).await,
        Err(RetrievalError::SellerReportedUndelivered)
    );
    assert!(buyer.cached(&paid.evidence).unwrap().is_none());
}

#[tokio::test]
async fn buyer_cache_faults_never_publish_an_unverified_partial_resource() {
    use client::{AccessClient, RetrievalError};
    let (policy, paid, _) = fixture(1);
    let server = resource_server(&policy, &paid).await;
    let probe = tempfile::tempdir().unwrap();
    let hook = Arc::new(Fault {
        at: usize::MAX,
        count: Mutex::new(0),
    });
    AccessClient::with_faults(
        &server.endpoint,
        policy.service_id,
        probe.path(),
        true,
        hook.clone(),
    )
    .unwrap()
    .retrieve(&paid.evidence, &[1; 32], 1000)
    .await
    .unwrap();
    let count = *hook.count.lock().unwrap();
    assert!(count > 0);
    for at in 1..=count {
        let root = tempfile::tempdir().unwrap();
        let buyer = AccessClient::with_faults(
            &server.endpoint,
            policy.service_id,
            root.path(),
            true,
            Arc::new(Fault {
                at,
                count: Mutex::new(0),
            }),
        )
        .unwrap();
        assert_eq!(
            buyer.retrieve(&paid.evidence, &[1; 32], 1000).await,
            Err(RetrievalError::Storage)
        );
        drop(buyer);
        let restarted =
            AccessClient::open(&server.endpoint, policy.service_id, root.path(), true).unwrap();
        let receipt = restarted
            .retrieve(&paid.evidence, &[1; 32], 1001)
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(receipt.resource_file).unwrap(),
            policy.payload
        );
    }
}

#[tokio::test]
async fn buyer_endpoint_and_symlink_boundaries_fail_closed() {
    use client::{AccessClient, RetrievalError};
    let (policy, paid, _) = fixture(1);
    for url in [
        "http://example.com/v1/access",
        "http://localhost.evil/v1/access",
        "https://user:password@example.com/v1/access",
        "https://example.com/v1/access?key=secret",
        "https://example.com/v1/access#frag",
        "https://example.com/wrong",
    ] {
        let root = tempfile::tempdir().unwrap();
        assert!(matches!(
            AccessClient::open(url, policy.service_id, root.path(), true),
            Err(RetrievalError::Configuration)
        ));
    }
    let root = tempfile::tempdir().unwrap();
    assert!(matches!(
        AccessClient::open(
            "http://127.0.0.1/v1/access",
            policy.service_id,
            root.path(),
            false
        ),
        Err(RetrievalError::Configuration)
    ));
    #[cfg(unix)]
    {
        let server = resource_server(&policy, &paid).await;
        let buyer =
            AccessClient::open(&server.endpoint, policy.service_id, root.path(), true).unwrap();
        let receipt = buyer
            .retrieve(&paid.evidence, &[1; 32], 1000)
            .await
            .unwrap();
        std::fs::remove_file(&receipt.resource_file).unwrap();
        let external = root.path().join("external");
        std::fs::write(&external, &policy.payload).unwrap();
        std::os::unix::fs::symlink(external, &receipt.resource_file).unwrap();
        assert_eq!(buyer.cached(&paid.evidence), Err(RetrievalError::Storage));
    }
}

#[test]
fn x402_payment_block_is_bound_to_the_request_signature() {
    let (policy, paid, mut request) = fixture(1);
    let root = tempfile::tempdir().unwrap();
    let issuer = AccessIssuer::open(root.path(), policy).unwrap();
    request.payment = Some(X402Payment {
        token: [7; 20],
        amount: 70,
        deadline: 2000,
        to: [9; 20],
        valid_after: 900,
        signature: vec![1; 65],
    });
    // The old signature does not cover the payment block.
    assert_eq!(
        issuer.issue(&paid, &request, 1000),
        Err(AccessError::Authentication)
    );
    request.signature = signature(
        1,
        [1; 32],
        &request_digest(
            &paid.evidence,
            issuer.policy.service_id,
            request.nonce,
            request.expires_at,
            request.payment.as_ref(),
        )
        .unwrap(),
    );
    assert!(issuer.issue(&paid, &request, 1000).is_ok());
    // A mutated payment block no longer matches the signed digest.
    let mut swapped = request.clone();
    swapped.payment.as_mut().unwrap().amount = 71;
    assert_eq!(
        issuer.issue(&paid, &swapped, 1000),
        Err(AccessError::Authentication)
    );
    assert_eq!(issuer.store.records().unwrap().len(), 1);
}

#[test]
fn the_profile_check_is_separate_from_payment_verification() {
    for suite in [1, 2] {
        let (policy, paid, _) = fixture(suite);
        let checked = policy.check_agreement(paid.evidence.clone()).unwrap();
        assert_eq!(checked.agreement(), &policy.check(&paid.evidence).unwrap());
        assert_eq!(checked.agreement(), &paid.agreement);
        assert_eq!(
            checked.evidence().encode().unwrap(),
            paid.evidence.encode().unwrap()
        );
        assert_eq!(format!("{checked:?}"), "CheckedAgreement { <redacted> }");
        let mut changed = policy.clone();
        changed.payload.push(0);
        assert_eq!(
            changed.check_agreement(paid.evidence.clone()).unwrap_err(),
            AccessError::Agreement
        );
    }
}

#[test]
fn pending_finality_lag_is_pending_access_not_an_observation_failure() {
    use crate::{
        disclosure::ShieldedDisclosureError, observation::ObservationError, recovery::RecoveryError,
    };
    // A finalized anchor below the deployment block is progress, not a failed observation:
    // it must map to 202 pending, exactly like a bounded history scan.
    assert_eq!(
        shielded_access_error(ShieldedDisclosureError::Observation(
            ObservationError::FinalityPending
        )),
        AccessError::Pending
    );
    assert_eq!(
        shielded_access_error(ShieldedDisclosureError::Observation(
            ObservationError::Recovery(RecoveryError::HistoryPending {
                next_block: 10,
                through: 20,
            })
        )),
        AccessError::Pending
    );
    // Every other observation failure stays unavailable.
    assert_eq!(
        shielded_access_error(ShieldedDisclosureError::Observation(
            ObservationError::Inconsistent
        )),
        AccessError::Observation
    );
    assert_eq!(
        shielded_access_error(ShieldedDisclosureError::Payment),
        AccessError::Pending
    );
}

#[tokio::test]
async fn an_unreachable_observer_never_turns_a_checked_agreement_into_access() {
    let (policy, paid, _) = fixture(2);
    let directory = tempfile::tempdir().unwrap();
    let domain = crate::index_store::IndexDomain {
        chain_id: 31_337,
        pool: [3; 20],
        first_block: 1,
        first_hash: [1; 32],
    };
    let backend = AccessBackend::Shielded {
        rpc: Box::new(PoolRpc::new("http://127.0.0.1:1", 31_337, [3; 20]).unwrap()),
        index: IndexStore::new(directory.path().join("first/index.json"), domain).unwrap(),
        peer_rpc: Box::new(PoolRpc::new("http://127.0.0.1:2", 31_337, [3; 20]).unwrap()),
        peer_index: IndexStore::new(directory.path().join("peer/index.json"), domain).unwrap(),
        runtime: Box::new(crate::rpc::PoolRuntimePins {
            pool: [0x71; 32],
            poseidon: [0x72; 32],
            deposit: [0x73; 32],
            transfer: [0x74; 32],
            withdraw: [0x75; 32],
        }),
    };
    let checked = policy.check_agreement(paid.evidence.clone()).unwrap();
    assert!(backend.verify_checked(checked).await.is_err());
    assert!(backend.verify(&policy, paid.evidence).await.is_err());
}
