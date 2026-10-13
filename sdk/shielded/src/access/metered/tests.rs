use super::*;
use crate::access::{
    request_digest, AccessIssuer, AccessPolicy, AccessRequest, ACCESS_ISSUANCE_TAG,
    ACCESS_REQUEST_TAG, ACCESS_REQUEST_X402_TAG,
};
use erebus_core::{
    auth::{authorization_digest, Authorization, Role},
    commitment::{commit_agreement, CommitmentBlinding},
    ids::{BaseUnits, SignatureBytes},
    terms::{AgreementTerms, FeePolicy, GuaranteeSet, SettlementMode},
};
use serde_json::Value;

const SERVICE_ID: [u8; 32] = [0x42; 32];
const NOW: u64 = 1000;
const EXPIRES: u64 = 1100;
const NONCE: [u8; 32] = [4; 32];
const BUYER: [u8; 32] = [1; 32];
const SELLER: [u8; 32] = [2; 32];

fn vectors() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/metered-v1-vectors.json"
    ))
    .unwrap()
}

fn bytes32(value: &Value) -> [u8; 32] {
    hex::decode(value.as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap()
}

fn sign(suite: u16, seed: [u8; 32], digest: &[u8; 32]) -> Vec<u8> {
    if suite == 1 {
        AuthorizationIdentity::from_bytes(&seed)
            .unwrap()
            .sign_digest(digest)
            .to_vec()
    } else {
        shielded_auth::sign_message(&seed, digest)
            .unwrap()
            .1
            .to_vec()
    }
}

/// Metered agreement built from agreement vector 0 with the fixture's service overlay.
/// Suite 1 reproduces exactly the terms the Python vectors were computed over.
fn fixture(suite: u16) -> (MeteredPolicy, SelectedAgreement) {
    let agreement: Value = serde_json::from_str(include_str!(
        "../../../../core/tests/fixtures/agreement-v1-vectors.json"
    ))
    .unwrap();
    let overlay = &vectors()["terms"];
    let source = &agreement["vectors"][overlay["agreementVector"].as_u64().unwrap() as usize];
    let mut terms = AgreementTerms::decode(
        &hex::decode(source["expected"]["canonicalHex"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    terms.transcript_root = bytes32(&overlay["transcriptRootHex"]);
    if suite == 2 {
        terms.suite_id = 2;
        terms.settlement_mode = SettlementMode::Shielded;
        terms.domain.pool = terms.domain.settlement_contract.clone();
        terms.required_guarantees = GuaranteeSet::from_bits(7).unwrap();
        terms.buyer_authorization_key =
            KeyBytes::new(shielded_auth::derive_key(&BUYER).unwrap().to_vec()).unwrap();
        terms.seller_authorization_key =
            KeyBytes::new(shielded_auth::derive_key(&SELLER).unwrap().to_vec()).unwrap();
        terms.payment_recipient = KeyBytes::new([0x0b; 32].to_vec()).unwrap();
        terms.fee_policy = FeePolicy::none();
    }
    let service = &overlay["service"];
    let policy = MeteredPolicy {
        service_id: SERVICE_ID,
        seller: terms.seller_authorization_key.clone(),
        suite_id: suite,
        resource: service["resource"].as_str().unwrap().into(),
        profile: MeteredProfile::new(65536).unwrap(),
    };
    terms.service.resource = policy.resource.clone();
    terms.service.quantity = BaseUnits::new(service["quantity"].as_str().unwrap().parse().unwrap());
    terms.service.unit = service["unit"].as_str().unwrap().into();
    terms.service.fulfillment_method = service["fulfillmentMethod"].as_str().unwrap().into();
    terms.service.fulfillment_digest = bytes32(&service["fulfillmentDigestHex"]);
    terms.service.access_recipient = terms.buyer_authorization_key.clone();
    let blinding = CommitmentBlinding::from_bytes(bytes32(&source["blindingHex"]));
    (policy, select(terms, blinding, suite))
}

fn select(terms: AgreementTerms, blinding: CommitmentBlinding, suite: u16) -> SelectedAgreement {
    let commitment = commit_agreement(&terms, &blinding).unwrap();
    let authorization = |role, seed| Authorization {
        role,
        suite_id: suite,
        commitment,
        signature: SignatureBytes::new(sign(
            suite,
            seed,
            &authorization_digest(&terms.domain, role, &commitment, suite).unwrap(),
        ))
        .unwrap(),
    };
    SelectedAgreement {
        buyer: authorization(Role::Buyer, BUYER),
        seller: authorization(Role::Seller, SELLER),
        terms,
        blinding,
        transcript_hash_version: 1,
        messages: Vec::new(),
    }
}

fn request(
    evidence: &SelectedAgreement,
    operation: MeteredOperation,
    call_index: u64,
) -> MeteredRequest {
    sign_metered_request(
        evidence, SERVICE_ID, operation, call_index, NONCE, EXPIRES, &BUYER,
    )
    .unwrap()
}

// K3: profile bytes and fulfillment digest.
#[test]
fn profile_bytes_and_digest_match_independent_vectors() {
    let vectors = vectors();
    let profiles = vectors["profiles"].as_array().unwrap();
    assert_eq!(profiles.len(), 2);
    for vector in profiles {
        let max = u32::try_from(vector["maxResponseBytes"].as_u64().unwrap()).unwrap();
        let profile = MeteredProfile::new(max).unwrap();
        assert_eq!(vector["paramsMode"].as_u64().unwrap(), 0);
        assert_eq!(profile.params_mode(), ParamsMode::None);
        assert_eq!(
            hex::encode(profile.encode()),
            vector["profileHex"].as_str().unwrap()
        );
        assert_eq!(
            profile.profile_digest(),
            bytes32(&vector["profileDigestHex"])
        );
    }
}

#[test]
fn profile_rejects_zero_and_oversize_response_caps() {
    assert_eq!(MeteredProfile::new(0), Err(AccessError::Agreement));
    assert_eq!(MeteredProfile::new(65537), Err(AccessError::Agreement));
    assert!(MeteredProfile::new(1).is_ok());
}

// K2: suite-1 request digest K, through the full evidence path.
#[test]
fn suite_one_request_digest_matches_independent_vectors() {
    let (_, evidence) = fixture(1);
    let agreement = verify_selected_agreement(&evidence).unwrap();
    let vectors = vectors();
    let requests = vectors["requests"].as_array().unwrap();
    assert_eq!(requests.len(), 3);
    for vector in requests {
        assert_eq!(
            hex::encode(evidence.terms.domain.encode().unwrap()),
            vector["domainHex"].as_str().unwrap()
        );
        assert_eq!(
            agreement.commitment.to_hex(),
            vector["commitmentHex"].as_str().unwrap()
        );
        assert_eq!(
            agreement.nullifier.to_hex(),
            vector["nullifierHex"].as_str().unwrap()
        );
        assert_eq!(bytes32(&vector["serviceIdHex"]), SERVICE_ID);
        assert_eq!(bytes32(&vector["paramsDigestHex"]), PARAMS_DIGEST_V1);
        let operation: MeteredOperation =
            serde_json::from_value(vector["operation"].clone()).unwrap();
        let digest = metered_request_digest(
            &evidence,
            SERVICE_ID,
            operation,
            vector["callIndex"].as_u64().unwrap(),
            bytes32(&vector["nonceHex"]),
            vector["expiresAt"].as_u64().unwrap(),
        )
        .unwrap();
        assert_eq!(digest, bytes32(&vector["digestHex"]), "{}", vector["name"]);
    }
}

// K4: entitlement and call record ids.
#[test]
fn entitlement_and_call_ids_match_independent_vectors() {
    let vectors = vectors();
    let entitlements = vectors["entitlements"].as_array().unwrap();
    assert_eq!(entitlements.len(), 1);
    for vector in entitlements {
        let entitlement = entitlement_id(
            &bytes32(&vector["serviceIdHex"]),
            &bytes32(&vector["commitmentHex"]),
        );
        assert_eq!(
            entitlement.to_hex(),
            vector["entitlementIdHex"].as_str().unwrap()
        );
        let calls = vector["calls"].as_array().unwrap();
        assert_eq!(calls.len(), 4);
        for call in calls {
            assert_eq!(
                call_id(&entitlement, call["index"].as_u64().unwrap()).to_hex(),
                call["callIdHex"].as_str().unwrap()
            );
        }
    }
}

#[test]
fn suite_two_signs_the_tag_3003_message_of_the_suite_one_digest() {
    let (_, evidence) = fixture(2);
    let agreement = verify_selected_agreement(&evidence).unwrap();
    let domain = evidence.terms.domain.encode().unwrap();
    let keccak = RequestBinding {
        domain: &domain,
        service_id: &SERVICE_ID,
        commitment: agreement.commitment.as_bytes(),
        nullifier: agreement.nullifier.as_bytes(),
        fulfillment_digest: &evidence.terms.service.fulfillment_digest,
        operation: MeteredOperation::Call,
        call_index: 7,
        params_digest: &PARAMS_DIGEST_V1,
        nonce: &NONCE,
        expires_at: EXPIRES,
    }
    .keccak();
    let message = metered_request_digest(
        &evidence,
        SERVICE_ID,
        MeteredOperation::Call,
        7,
        NONCE,
        EXPIRES,
    )
    .unwrap();
    assert_eq!(
        message,
        erebus_core::shielded::metered_access_message(&keccak).unwrap()
    );
    assert_ne!(
        message,
        erebus_core::shielded::access_message(&keccak).unwrap()
    );
}

#[test]
fn signed_call_and_status_authenticate_in_both_suites() {
    for suite in [1, 2] {
        let (policy, evidence) = fixture(suite);
        for (operation, index) in [
            (MeteredOperation::Call, 0),
            (MeteredOperation::Call, 2),
            (MeteredOperation::Status, 0),
        ] {
            let signed = request(&evidence, operation, index);
            assert_eq!(policy.authenticate(&evidence, &signed, NOW), Ok(()));
            assert_eq!(
                policy.authenticate(&evidence, &signed, EXPIRES - 300),
                Ok(())
            );
        }
        // The allowance is a ledger answer: an index past N still authenticates.
        let past = request(&evidence, MeteredOperation::Call, 3);
        assert_eq!(policy.authenticate(&evidence, &past, NOW), Ok(()));
    }
}

#[test]
fn status_requires_call_index_zero() {
    for suite in [1, 2] {
        let (policy, evidence) = fixture(suite);
        assert_eq!(
            metered_request_digest(
                &evidence,
                SERVICE_ID,
                MeteredOperation::Status,
                1,
                NONCE,
                EXPIRES
            ),
            Err(AccessError::Authentication)
        );
        assert!(sign_metered_request(
            &evidence,
            SERVICE_ID,
            MeteredOperation::Status,
            1,
            NONCE,
            EXPIRES,
            &BUYER
        )
        .is_err());
        // A call signature at index 1 relabelled as status at index 1 still fails.
        let mut relabelled = request(&evidence, MeteredOperation::Call, 1);
        relabelled.operation = MeteredOperation::Status;
        assert_eq!(
            policy.authenticate(&evidence, &relabelled, NOW),
            Err(AccessError::Authentication)
        );
    }
}

#[test]
fn nonzero_params_digest_is_rejected() {
    for suite in [1, 2] {
        let (policy, evidence) = fixture(suite);
        let digest = digest_with_params(
            &evidence,
            SERVICE_ID,
            MeteredOperation::Call,
            0,
            &[9; 32],
            NONCE,
            EXPIRES,
        )
        .unwrap();
        let mut signed = request(&evidence, MeteredOperation::Call, 0);
        signed.signature = sign(suite, BUYER, &digest);
        assert_eq!(
            policy.authenticate(&evidence, &signed, NOW),
            Err(AccessError::Authentication)
        );
    }
    // v1 has no wire field for parameters.
    let (_, evidence) = fixture(1);
    let mut body = serde_json::to_value(request(&evidence, MeteredOperation::Call, 0)).unwrap();
    body["params_digest"] = serde_json::to_value([0u8; 32]).unwrap();
    assert!(serde_json::from_value::<MeteredRequest>(body).is_err());
}

#[test]
fn unknown_fields_and_operations_are_rejected() {
    let (_, evidence) = fixture(1);
    let body = serde_json::to_value(request(&evidence, MeteredOperation::Status, 0)).unwrap();
    assert_eq!(body["operation"], "status");
    assert!(serde_json::from_value::<MeteredRequest>(body.clone()).is_ok());
    let mut extra = body.clone();
    extra["payment"] = Value::Null;
    assert!(serde_json::from_value::<MeteredRequest>(extra).is_err());
    let mut unknown = body;
    unknown["operation"] = "refund".into();
    assert!(serde_json::from_value::<MeteredRequest>(unknown).is_err());
}

#[test]
fn freshness_nonce_commitment_and_buyer_are_enforced() {
    for suite in [1, 2] {
        let (policy, evidence) = fixture(suite);
        let signed = request(&evidence, MeteredOperation::Call, 0);
        // Expired, and expiry too far in the future.
        assert_eq!(
            policy.authenticate(&evidence, &signed, EXPIRES),
            Err(AccessError::Authentication)
        );
        assert_eq!(
            policy.authenticate(&evidence, &signed, EXPIRES - 301),
            Err(AccessError::Authentication)
        );
        // Zero nonce, signed correctly, still fails; the signer refuses it outright.
        let mut zero = signed.clone();
        zero.nonce = [0; 32];
        zero.signature = sign(
            suite,
            BUYER,
            &metered_request_digest(
                &evidence,
                SERVICE_ID,
                MeteredOperation::Call,
                0,
                [0; 32],
                EXPIRES,
            )
            .unwrap(),
        );
        assert_eq!(
            policy.authenticate(&evidence, &zero, NOW),
            Err(AccessError::Authentication)
        );
        assert!(sign_metered_request(
            &evidence,
            SERVICE_ID,
            MeteredOperation::Call,
            0,
            [0; 32],
            EXPIRES,
            &BUYER
        )
        .is_err());
        // Wrong buyer key: a seller-signed request, and a signer that is not the buyer.
        let mut wrong = signed.clone();
        wrong.signature = sign(
            suite,
            SELLER,
            &metered_request_digest(
                &evidence,
                SERVICE_ID,
                MeteredOperation::Call,
                0,
                NONCE,
                EXPIRES,
            )
            .unwrap(),
        );
        assert_eq!(
            policy.authenticate(&evidence, &wrong, NOW),
            Err(AccessError::Authentication)
        );
        assert_eq!(
            sign_metered_request(
                &evidence,
                SERVICE_ID,
                MeteredOperation::Call,
                0,
                NONCE,
                EXPIRES,
                &SELLER
            )
            .unwrap_err(),
            AccessError::Authentication
        );
        // Wrong commitment string.
        let mut other = signed.clone();
        other.deal_commitment = "00".repeat(32);
        assert_eq!(
            policy.authenticate(&evidence, &other, NOW),
            Err(AccessError::Authentication)
        );
    }
}

#[test]
fn every_signed_field_is_bound() {
    for suite in [1, 2] {
        let (policy, evidence) = fixture(suite);
        let signed = request(&evidence, MeteredOperation::Call, 1);
        for mutate in 0..4 {
            let mut changed = signed.clone();
            match mutate {
                0 => changed.call_index = 2,
                1 => changed.nonce = [5; 32],
                2 => changed.expires_at = EXPIRES - 1,
                _ => changed.operation = MeteredOperation::Status,
            }
            assert_eq!(
                policy.authenticate(&evidence, &changed, NOW),
                Err(AccessError::Authentication),
                "suite {suite} mutation {mutate}"
            );
        }
        let mut other_service = policy.clone();
        other_service.service_id = [0x43; 32];
        assert_eq!(
            other_service.authenticate(&evidence, &signed, NOW),
            Err(AccessError::Authentication)
        );
    }
}

#[test]
fn snapshot_and_metered_signatures_do_not_cross_verify() {
    for suite in [1, 2] {
        let (policy, evidence) = fixture(suite);
        let verifier = suite::suite(suite).unwrap();
        let buyer = evidence.terms.buyer_authorization_key.as_bytes();
        // A snapshot request signature, over the same agreement, nonce, and expiry.
        let snapshot_digest = request_digest(&evidence, SERVICE_ID, NONCE, EXPIRES, None).unwrap();
        let mut forged = request(&evidence, MeteredOperation::Call, 0);
        forged.signature = sign(suite, BUYER, &snapshot_digest);
        assert_eq!(
            policy.authenticate(&evidence, &forged, NOW),
            Err(AccessError::Authentication)
        );
        let mut status = forged.clone();
        status.operation = MeteredOperation::Status;
        assert_eq!(
            policy.authenticate(&evidence, &status, NOW),
            Err(AccessError::Authentication)
        );
        // A metered signature never verifies as snapshot access.
        let metered = request(&evidence, MeteredOperation::Call, 0);
        assert!(verifier
            .verify_authorization(buyer, &snapshot_digest, &metered.signature)
            .is_err());
    }
}

#[test]
fn snapshot_terms_fail_as_metered_and_metered_terms_fail_as_snapshot() {
    for suite in [1, 2] {
        let (policy, evidence) = fixture(suite);
        // The snapshot issuer refuses metered terms before checking any signature.
        let snapshot = AccessPolicy {
            service_id: SERVICE_ID,
            seller: policy.seller.clone(),
            suite_id: suite,
            resource: policy.resource.clone(),
            payload: b"feed".to_vec(),
        };
        let root = tempfile::tempdir().unwrap();
        let issuer = AccessIssuer::open(root.path(), snapshot.clone()).unwrap();
        let snapshot_request = AccessRequest {
            deal_commitment: verify_selected_agreement(&evidence)
                .unwrap()
                .commitment
                .to_hex(),
            nonce: NONCE,
            expires_at: EXPIRES,
            signature: sign(
                suite,
                BUYER,
                &request_digest(&evidence, SERVICE_ID, NONCE, EXPIRES, None).unwrap(),
            ),
            payment: None,
        };
        assert_eq!(
            issuer.authenticate(&evidence, &snapshot_request, NOW),
            Err(AccessError::Agreement)
        );
        // Snapshot terms for the same resource never pass the metered check.
        let mut terms = evidence.terms.clone();
        terms.service.quantity = BaseUnits::new(1);
        terms.service.unit = "snapshot".into();
        terms.service.fulfillment_method = "http-access-v1".into();
        terms.service.fulfillment_digest = snapshot.resource_hash();
        let snapshot_evidence = select(terms, evidence.blinding.clone(), suite);
        assert!(issuer
            .authenticate(
                &snapshot_evidence,
                &snapshot_request_for(&snapshot_evidence, suite),
                NOW
            )
            .is_ok());
        let metered = sign_metered_request(
            &snapshot_evidence,
            SERVICE_ID,
            MeteredOperation::Call,
            0,
            NONCE,
            EXPIRES,
            &BUYER,
        )
        .unwrap();
        assert_eq!(
            policy.authenticate(&snapshot_evidence, &metered, NOW),
            Err(AccessError::Agreement)
        );
    }
}

fn snapshot_request_for(evidence: &SelectedAgreement, suite: u16) -> AccessRequest {
    AccessRequest {
        deal_commitment: verify_selected_agreement(evidence)
            .unwrap()
            .commitment
            .to_hex(),
        nonce: NONCE,
        expires_at: EXPIRES,
        signature: sign(
            suite,
            BUYER,
            &request_digest(evidence, SERVICE_ID, NONCE, EXPIRES, None).unwrap(),
        ),
        payment: None,
    }
}

#[test]
fn metered_terms_outside_the_profile_are_rejected() {
    for suite in [1, 2] {
        let (policy, evidence) = fixture(suite);
        for mutate in 1..7 {
            let mut terms = evidence.terms.clone();
            match mutate {
                1 => terms.service.quantity = BaseUnits::new(4097),
                2 => terms.service.unit = "snapshot".into(),
                3 => terms.service.fulfillment_method = "http-access-v1".into(),
                4 => {
                    terms.service.fulfillment_digest =
                        MeteredProfile::new(1024).unwrap().profile_digest()
                }
                5 => terms.service.resource = "data.feed.v2".into(),
                _ => terms.service.access_recipient = terms.seller_authorization_key.clone(),
            }
            let changed = select(terms, evidence.blinding.clone(), suite);
            assert_eq!(
                policy.check(&changed).unwrap_err(),
                AccessError::Agreement,
                "suite {suite} mutation {mutate}"
            );
        }
        // N = 0 is already unrepresentable: core terms validation rejects it.
        let mut zero = evidence.terms.clone();
        zero.service.quantity = BaseUnits::new(0);
        assert!(zero.encode().is_err());
        for quantity in [1, 4096] {
            let mut terms = evidence.terms.clone();
            terms.service.quantity = BaseUnits::new(quantity);
            assert!(policy
                .check(&select(terms, evidence.blinding.clone(), suite))
                .is_ok());
        }
    }
}

#[test]
fn tags_are_distinct_and_not_prefixes_of_one_another() {
    let tags = [
        METERED_PROFILE_TAG,
        METERED_REQUEST_TAG,
        METERED_ENTITLEMENT_TAG,
        METERED_CALL_TAG,
        ACCESS_REQUEST_TAG,
        ACCESS_REQUEST_X402_TAG,
        ACCESS_ISSUANCE_TAG,
    ];
    for (i, left) in tags.iter().enumerate() {
        for (j, right) in tags.iter().enumerate() {
            if i != j {
                assert!(
                    !right.starts_with(left),
                    "{} prefixes {}",
                    String::from_utf8_lossy(left),
                    String::from_utf8_lossy(right)
                );
            }
        }
    }
}

#[test]
fn record_ids_separate_services_commitments_and_indices() {
    let entitlement = entitlement_id(&SERVICE_ID, &[7; 32]);
    assert_ne!(entitlement, entitlement_id(&[0x43; 32], &[7; 32]));
    assert_ne!(entitlement, entitlement_id(&SERVICE_ID, &[8; 32]));
    assert_ne!(call_id(&entitlement, 0), call_id(&entitlement, 1));
    assert_ne!(
        entitlement.to_hex(),
        crate::access::issuance_id(&SERVICE_ID, &[7; 32])
    );
}
