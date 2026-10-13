//! The independent M4 Circom/JS runner pins these suite-2 agreement hashes.

use erebus_core::auth::{Authorization, Role};
use erebus_core::commitment::CommitmentBlinding;
use erebus_core::domain::DeploymentDomain;
use erebus_core::ids::{
    AddressBytes, AssetId, BaseUnits, ChainNamespace, KeyBytes, SignatureBytes,
};
use erebus_core::service::ServiceRecord;
use erebus_core::shielded::ShieldedDeal;
use erebus_core::shielded_auth::{derive_key, sign_message, verify_agreement, verify_message};
use erebus_core::terms::{AgreementTerms, FeePolicy, Guarantee, GuaranteeSet, SettlementMode};
use serde_json::Value;

const FIXTURE: &str = include_str!("fixtures/agreement-suite2-vector.json");

#[test]
fn access_signatures_preserve_digest_limbs_and_cannot_authorize_payment_or_disclosure() {
    use erebus_core::shielded::{access_message, disclosure_message};
    let mut high = [0; 32];
    high[0] = 1;
    let mut low = [0; 32];
    low[31] = 1;
    assert_ne!(
        access_message(&high).unwrap(),
        access_message(&low).unwrap()
    );
    let (terms, blinding, _) = fixture_terms();
    let deal = ShieldedDeal::from_terms(&terms).unwrap();
    let commitment = deal.commitment(&blinding).unwrap();
    let digest = commitment.as_bytes();
    let access = access_message(digest).unwrap();
    let (key, signature) = sign_message(&[1; 32], &access).unwrap();
    verify_message(&key, &access, &signature).unwrap();
    let disclosure = disclosure_message(digest).unwrap();
    assert_ne!(access, disclosure);
    assert!(verify_message(&key, &disclosure, &signature).is_err());
    for role in [Role::Buyer, Role::Seller] {
        let payment = deal.authorization_message(role, &commitment).unwrap();
        assert_ne!(access, payment);
        assert!(verify_message(&key, &payment, &signature).is_err());
    }
}

#[test]
fn disclosure_messages_match_circomlib_and_cannot_authorize_payment() {
    use erebus_core::shielded::disclosure_message;
    // circomlibjs 0.1.7: Poseidon([3001, high128(digest), low128(digest)]).
    for (digest, expected) in [
        (
            [0; 32],
            "2920fd34a3f1366d8fdf72b216134c0fba4ea675cdbc96ef21158929d772fe98",
        ),
        (
            [0xff; 32],
            "00ec53d46422bcccb62bf09b2cd3e4cdcd719dfa125c3f6e5102c4ca12b8b3ab",
        ),
    ] {
        assert_eq!(hex::encode(disclosure_message(&digest).unwrap()), expected);
    }
    let mut high = [0; 32];
    high[0] = 1;
    let mut low = [0; 32];
    low[31] = 1;
    assert_ne!(
        disclosure_message(&high).unwrap(),
        disclosure_message(&low).unwrap()
    );
    let (terms, blinding, _) = fixture_terms();
    let deal = ShieldedDeal::from_terms(&terms).unwrap();
    let commitment = deal.commitment(&blinding).unwrap();
    let message = disclosure_message(commitment.as_bytes()).unwrap();
    let seed = [1; 32];
    let (key, signature) = sign_message(&seed, &message).unwrap();
    verify_message(&key, &message, &signature).unwrap();
    for role in [Role::Buyer, Role::Seller] {
        let payment = deal.authorization_message(role, &commitment).unwrap();
        assert_ne!(message, payment);
        assert!(verify_message(&key, &payment, &signature).is_err());
    }
}

fn bytes(value: &str) -> Vec<u8> {
    hex::decode(value).expect("pinned hex")
}

fn fixed<const N: usize>(value: &str) -> [u8; N] {
    bytes(value).try_into().expect("pinned width")
}

fn field<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().expect("pinned string")
}

#[test]
fn private_agreement_objects_redact_normal_and_pretty_debug() {
    let (terms, _, _) = fixture_terms();
    let mapped = ShieldedDeal::from_terms(&terms).unwrap();
    for (normal, pretty, expected) in [
        (
            format!("{terms:?}"),
            format!("{terms:#?}"),
            "AgreementTerms { <redacted> }",
        ),
        (
            format!("{:?}", terms.service),
            format!("{:#?}", terms.service),
            "ServiceRecord { <redacted> }",
        ),
        (
            format!("{:?}", terms.fee_policy),
            format!("{:#?}", terms.fee_policy),
            "FeePolicy { <redacted> }",
        ),
        (
            format!("{mapped:?}"),
            format!("{mapped:#?}"),
            "ShieldedDeal { <redacted> }",
        ),
    ] {
        assert_eq!(normal, expected);
        assert_eq!(pretty, expected);
    }
}

fn fixture_terms() -> (AgreementTerms, CommitmentBlinding, Value) {
    let vector: Value = serde_json::from_str(FIXTURE).expect("pinned JSON");
    let input = &vector["terms"];
    let domain = &input["domain"];
    let service = &input["service"];
    let mut guarantees = GuaranteeSet::empty();
    for guarantee in [
        Guarantee::HiddenAmount,
        Guarantee::HiddenRecipient,
        Guarantee::AgreementBoundSettlement,
    ] {
        guarantees.insert(guarantee);
    }
    let terms = AgreementTerms {
        protocol_version: input["protocolVersion"].as_u64().expect("version") as u16,
        suite_id: input["suiteId"].as_u64().expect("suite") as u16,
        domain: DeploymentDomain {
            namespace: ChainNamespace::parse(field(domain, "namespace")).expect("namespace"),
            settlement_contract: Some(
                AddressBytes::new(bytes(field(domain, "settlementContractHex"))).expect("contract"),
            ),
            pool: Some(AddressBytes::new(bytes(field(domain, "poolHex"))).expect("pool")),
            verifier_version: domain["verifierVersion"].as_u64().expect("version") as u32,
        },
        deal_id: fixed(field(input, "dealIdHex")),
        revision: input["revision"].as_u64().expect("revision") as u32,
        transcript_root: fixed(field(input, "transcriptRootHex")),
        buyer_authorization_key: KeyBytes::new(bytes(field(input, "buyerAuthorizationKeyHex")))
            .expect("buyer"),
        seller_authorization_key: KeyBytes::new(bytes(field(input, "sellerAuthorizationKeyHex")))
            .expect("seller"),
        payment_recipient: KeyBytes::new(bytes(field(input, "paymentRecipientHex")))
            .expect("recipient"),
        asset: AssetId::parse(field(input, "asset")).expect("asset"),
        amount: BaseUnits::new(field(input, "amount").parse().expect("amount")),
        expiry: input["expiry"].as_u64().expect("expiry"),
        fee_policy: FeePolicy::none(),
        settlement_mode: SettlementMode::Shielded,
        required_guarantees: guarantees,
        settlement_nonce: fixed(field(input, "settlementNonceHex")),
        service: ServiceRecord {
            resource: field(service, "resource").to_owned(),
            quantity: BaseUnits::new(field(service, "quantity").parse().expect("quantity")),
            unit: field(service, "unit").to_owned(),
            access_recipient: KeyBytes::new(bytes(field(service, "accessRecipientHex")))
                .expect("access key"),
            delivery_deadline: service["deliveryDeadline"].as_u64().expect("deadline"),
            fulfillment_method: field(service, "fulfillmentMethod").to_owned(),
            fulfillment_digest: fixed(field(service, "fulfillmentDigestHex")),
        },
    };
    let blinding = CommitmentBlinding::from_bytes(fixed(field(&vector, "blindingHex")));
    (terms, blinding, vector)
}

#[test]
fn rust_matches_circomlib_commitment_and_messages() {
    let (terms, blinding, vector) = fixture_terms();
    let deal = ShieldedDeal::from_terms(&terms).expect("fixed suite-2 shape");
    let commitment = deal.commitment(&blinding).expect("Poseidon commitment");
    let expected = &vector["expected"];
    assert_eq!(commitment.to_hex(), field(expected, "commitmentHex"));
    assert_eq!(
        deal.deal_nullifier().expect("nullifier").to_hex(),
        field(expected, "dealNullifierHex")
    );
    assert_eq!(
        hex::encode(
            deal.authorization_message(Role::Buyer, &commitment)
                .expect("buyer")
        ),
        field(expected, "buyerMessageHex")
    );
    assert_eq!(
        hex::encode(
            deal.authorization_message(Role::Seller, &commitment)
                .expect("seller")
        ),
        field(expected, "sellerMessageHex")
    );
    assert_eq!(
        hex::encode(deal.payment_note_commitment().expect("payment note")),
        field(expected, "paymentCommitmentHex")
    );
}

#[test]
fn shared_agreement_api_matches_the_shielded_circuit() {
    use erebus_core::auth::{
        authorization_digest, verify_authorization, verify_authorization_signature,
    };
    use erebus_core::commitment::{commit_agreement, deal_nullifier};
    let (terms, blinding, vector) = fixture_terms();
    let encoded = terms.encode().expect("suite-2 terms encode");
    assert_eq!(AgreementTerms::decode(&encoded).unwrap(), terms);
    use erebus_core::settlement::{check_capabilities, BackendCapabilities, SettlementContext};
    let capabilities = BackendCapabilities {
        suites: [2].into_iter().collect(),
        modes: [SettlementMode::Shielded].into_iter().collect(),
        guarantees: terms.required_guarantees,
        local_proving: true,
    };
    let context = SettlementContext {
        require_local_proving: true,
        mode: terms.settlement_mode,
        domain: terms.domain.clone(),
        suite_id: 2,
        asset: terms.asset.clone(),
        required_guarantees: terms.required_guarantees,
    };
    check_capabilities(&context, &capabilities).expect("shielded selection");
    let mut invalid_context = context.clone();
    invalid_context.domain.pool = None;
    assert!(check_capabilities(&invalid_context, &capabilities).is_err());
    let commitment = commit_agreement(&terms, &blinding).expect("shared commitment");
    assert_eq!(
        commitment.to_hex(),
        field(&vector["expected"], "commitmentHex")
    );
    assert_eq!(
        deal_nullifier(&terms).unwrap().to_hex(),
        field(&vector["expected"], "dealNullifierHex")
    );
    for (role, name) in [(Role::Buyer, "buyer"), (Role::Seller, "seller")] {
        let message = authorization_digest(&terms.domain, role, &commitment, 2).unwrap();
        assert_eq!(
            hex::encode(message),
            field(&vector["expected"], &format!("{name}MessageHex"))
        );
        let auth = Authorization {
            role,
            suite_id: 2,
            commitment,
            signature: SignatureBytes::new(bytes(field(
                &vector["expected"],
                &format!("{name}SignatureHex"),
            )))
            .unwrap(),
        };
        verify_authorization(&terms, &commitment, &blinding, &auth, terms.expiry - 1).unwrap();
        assert!(verify_authorization(&terms, &commitment, &blinding, &auth, terms.expiry).is_err());
        verify_authorization_signature(&terms, &commitment, &blinding, &auth).unwrap();
        let mut changed = terms.clone();
        changed.transcript_root[31] ^= 1;
        assert!(verify_authorization_signature(&changed, &commitment, &blinding, &auth).is_err());
        let mut changed = terms.clone();
        changed.domain.verifier_version += 1;
        assert!(verify_authorization_signature(&changed, &commitment, &blinding, &auth).is_err());
    }
    let mut invalid = terms;
    invalid.domain.verifier_version = 0;
    assert!(invalid.validate().is_err());
}

#[test]
fn shielded_shape_and_opening_reject_mutations() {
    let (terms, blinding, _) = fixture_terms();
    let original = ShieldedDeal::from_terms(&terms).expect("shape");
    let original_commitment = original.commitment(&blinding).expect("commitment");
    let mut changed = terms.clone();
    changed.amount = BaseUnits::new(71);
    let revised = ShieldedDeal::from_terms(&changed).expect("shape");
    assert_ne!(
        revised.commitment(&blinding).expect("commitment"),
        original_commitment
    );
    assert_ne!(
        revised.payment_note_commitment().expect("other payment"),
        original.payment_note_commitment().expect("payment")
    );
    assert_eq!(
        revised.deal_nullifier().expect("nullifier"),
        original.deal_nullifier().expect("nullifier")
    );

    let mut changed = terms.clone();
    changed.fee_policy.fee = BaseUnits::new(1);
    assert!(ShieldedDeal::from_terms(&changed).is_err());
    let mut changed = terms.clone();
    changed.payment_recipient = KeyBytes::new(vec![1; 20]).expect("key");
    assert!(ShieldedDeal::from_terms(&changed).is_err());
    let mut changed = terms;
    changed.domain.verifier_version += 1;
    assert_ne!(
        ShieldedDeal::from_terms(&changed)
            .expect("shape")
            .commitment(&blinding)
            .expect("commitment"),
        original_commitment
    );
    assert_ne!(
        ShieldedDeal::from_terms(&changed)
            .expect("shape")
            .payment_note_commitment()
            .expect("other payment"),
        original.payment_note_commitment().expect("payment")
    );
    assert!(original
        .commitment(&CommitmentBlinding::from_bytes([0; 32]))
        .is_err());
}

#[test]
fn rust_signatures_match_circomlib_for_both_roles() {
    let (terms, blinding, vector) = fixture_terms();
    let deal = ShieldedDeal::from_terms(&terms).expect("shape");
    let commitment = deal.commitment(&blinding).expect("commitment");
    for (role, label) in [(Role::Buyer, "buyer"), (Role::Seller, "seller")] {
        let seed = fixed(
            vector["testOnlySeeds"][format!("{label}Hex")]
                .as_str()
                .expect("seed"),
        );
        let key = derive_key(&seed).expect("derive key");
        let agreed_key = match role {
            Role::Buyer => terms.buyer_authorization_key.as_bytes(),
            Role::Seller => terms.seller_authorization_key.as_bytes(),
        };
        assert_eq!(key.as_slice(), agreed_key);
        let message = deal
            .authorization_message(role, &commitment)
            .expect("message");
        let (signed_key, signature) = sign_message(&seed, &message).expect("sign");
        assert_eq!(signed_key, key);
        assert_eq!(
            hex::encode(signature),
            field(&vector["expected"], &format!("{label}SignatureHex"))
        );
        verify_message(&key, &message, &signature).expect("matching signature");
        let wrong_role = if role == Role::Buyer {
            Role::Seller
        } else {
            Role::Buyer
        };
        let other_message = deal
            .authorization_message(wrong_role, &commitment)
            .expect("message");
        assert!(verify_message(&key, &other_message, &signature).is_err());
        let mut changed = signature;
        changed[95] ^= 1;
        assert!(verify_message(&key, &message, &changed).is_err());
    }
}

#[test]
fn shielded_agreement_requires_both_roles_and_the_exact_opening() {
    let (terms, blinding, vector) = fixture_terms();
    let commitment = ShieldedDeal::from_terms(&terms)
        .expect("shape")
        .commitment(&blinding)
        .expect("commitment");
    let make_auth = |role, name: &str| Authorization {
        role,
        suite_id: 2,
        commitment,
        signature: SignatureBytes::new(bytes(field(
            &vector["expected"],
            &format!("{name}SignatureHex"),
        )))
        .expect("signature"),
    };
    let buyer = make_auth(Role::Buyer, "buyer");
    let seller = make_auth(Role::Seller, "seller");
    let verified = verify_agreement(&terms, &blinding, &buyer, &seller, terms.expiry - 1)
        .expect("agreed and unexpired");
    assert_eq!(verified.commitment, commitment);
    assert_eq!(
        hex::encode(verified.payment_note),
        field(&vector["expected"], "paymentCommitmentHex")
    );
    assert!(verify_agreement(&terms, &blinding, &buyer, &seller, terms.expiry).is_err());
    assert!(verify_agreement(&terms, &blinding, &seller, &buyer, 1).is_err());
    let mut altered = terms.clone();
    altered.amount = BaseUnits::new(71);
    assert!(verify_agreement(&altered, &blinding, &buyer, &seller, 1).is_err());
    let mut bad = seller.clone();
    bad.signature = SignatureBytes::new(vec![0; 96]).expect("length");
    assert!(verify_agreement(&terms, &blinding, &buyer, &bad, 1).is_err());
}

#[test]
fn identity_authorization_keys_cannot_authorize_any_agreement() {
    // `(0, 1)` is on-curve and in the prime-order subgroup, so the EdDSA check accepts it,
    // but with `A` the identity the verification equation reduces to `S*Base8 == R8`.
    // The publicly known `(R8 = Base8, S = 1)` therefore forges any message.
    let mut identity = [0u8; 64];
    identity[63] = 1;
    let message = [0x11; 32];
    let mut forgery = [0u8; 96];
    forgery[..32].copy_from_slice(&fixed::<32>(
        "0bb77a6ad63e739b4eacb2e09d6277c12ab8d8010534e0b62893f3f6bb957051",
    ));
    forgery[32..64].copy_from_slice(&fixed::<32>(
        "25797203f7a0b24925572e1cd16bf9edfce0051fb9e133774b3c257a872d7d8b",
    ));
    forgery[95] = 1;
    assert!(
        verify_message(&identity, &message, &forgery).is_err(),
        "identity key must never verify a signature"
    );

    let (terms, blinding, _) = fixture_terms();
    for role in [Role::Buyer, Role::Seller] {
        let mut candidate = terms.clone();
        match role {
            Role::Buyer => {
                candidate.buyer_authorization_key =
                    KeyBytes::new(identity.to_vec()).expect("bounded identity key")
            }
            Role::Seller => {
                candidate.seller_authorization_key =
                    KeyBytes::new(identity.to_vec()).expect("bounded identity key")
            }
        }
        assert!(ShieldedDeal::from_terms(&candidate).is_err());
        assert!(erebus_core::commitment::commit_agreement(&candidate, &blinding).is_err());
    }
}

/// The compiled circomlib `EdDSAPoseidonVerifier` was probed with these exact vectors
/// (scalar u=7, r=11, message 0x3039): the valid and torsion-shifted witnesses were accepted
/// by the circuit, the identity and order-2 keys were rejected by the `8A.x != 0` check, and
/// `S = subOrder` was rejected by the scalar bound. Rust verification must accept the control
/// and reject the torsion-shifted key, the off-curve key, and the out-of-range scalars.
#[test]
fn circuit_probe_vectors_show_the_exact_circuit_rust_divergence() {
    let message = fixed("0000000000000000000000000000000000000000000000000000000000003039");
    let build = |ax: &str, ay: &str, r8x: &str, r8y: &str, s: &str| {
        let mut key = [0u8; 64];
        key[..32].copy_from_slice(&fixed::<32>(ax));
        key[32..].copy_from_slice(&fixed::<32>(ay));
        let mut signature = [0u8; 96];
        signature[..32].copy_from_slice(&fixed::<32>(r8x));
        signature[32..64].copy_from_slice(&fixed::<32>(r8y));
        signature[64..].copy_from_slice(&fixed::<32>(s));
        (key, signature)
    };
    let (valid_key, valid_signature) = build(
        "2c6bfc7fe056ed38e26ec136ec8aec5f63ecc4b52c44afba967649cf1e6e2311",
        "1ac7675df6265f6e12d1c79a2b3b6658a0d46a320fba497ad0b817f9b19e0f21",
        "2fc449e2dd0c5349d06092adc126e213858984584e429842d87bcc705837c765",
        "29b060ee13f1da3bde25e0e11c23a145656ede61710ed78593d69b8bdbc1a581",
        "026d7d9c96207d442cfb81d682e7a20e0ba5596b77e6593cc6e412df0ae75c0e",
    );
    assert!(
        verify_message(&valid_key, &message, &valid_signature).is_ok(),
        "prime-order control must verify in Rust"
    );

    let (torsion_key, torsion_signature) = build(
        "03f851f300dab2f0d5e1847f94f66bfdc44723934d74c0d6ad6babc4d191dcf0",
        "159ce714eb0b40bba57e7e1c5645f204875f7e1669ff27167329dd9a3e61f0e0",
        "2fc449e2dd0c5349d06092adc126e213858984584e429842d87bcc705837c765",
        "29b060ee13f1da3bde25e0e11c23a145656ede61710ed78593d69b8bdbc1a581",
        "000d56b420978461b973793e381de98bc30ec281fa5a6282825376d21ddd5a6c",
    );
    assert!(
        verify_message(&torsion_key, &message, &torsion_signature).is_err(),
        "Rust must reject a torsion-shifted key the circuit probe accepted"
    );

    let one = "0000000000000000000000000000000000000000000000000000000000000001";
    let (off_curve_key, _) = build(one, one, one, one, one);
    assert!(verify_message(&off_curve_key, &message, &valid_signature).is_err());

    let mut zero_s = valid_signature;
    zero_s[64..].fill(0);
    assert!(verify_message(&valid_key, &message, &zero_s).is_err());

    let (_, suborder_s) = build(
        "2c6bfc7fe056ed38e26ec136ec8aec5f63ecc4b52c44afba967649cf1e6e2311",
        "1ac7675df6265f6e12d1c79a2b3b6658a0d46a320fba497ad0b817f9b19e0f21",
        "2fc449e2dd0c5349d06092adc126e213858984584e429842d87bcc705837c765",
        "29b060ee13f1da3bde25e0e11c23a145656ede61710ed78593d69b8bdbc1a581",
        "060c89ce5c263405370a08b6d0302b0bab3eedb83920ee0a677297dc392126f1",
    );
    assert!(verify_message(&valid_key, &message, &suborder_s).is_err());
}

/// Rust's exact identity handling: `erebus-core` rejects only the identity authorization key
/// (`A = (0,1)`); the dependency has no R8 identity check, and the circomlib verifier's
/// `8A.x != 0` likewise rejects only A. Both sides therefore accept an identity R8 signature
/// that a keyholder can produce, and both reject off-curve points. The circuit probe accepts
/// the same identity-R8 vector (`identity_r8` → WITNESS_OK for the plain and strict verifiers).
#[test]
fn identity_r8_is_accepted_by_both_sides_and_off_curve_r8_is_rejected() {
    let message = fixed("0000000000000000000000000000000000000000000000000000000000003039");
    let mut key = [0u8; 64];
    key[..32].copy_from_slice(&fixed::<32>(
        "2c6bfc7fe056ed38e26ec136ec8aec5f63ecc4b52c44afba967649cf1e6e2311",
    ));
    key[32..].copy_from_slice(&fixed::<32>(
        "1ac7675df6265f6e12d1c79a2b3b6658a0d46a320fba497ad0b817f9b19e0f21",
    ));
    let mut identity_r8 = [0u8; 96];
    identity_r8[63] = 1;
    identity_r8[64..].copy_from_slice(&fixed::<32>(
        "01bf40e60d413d071d3fb7c89716a969b4b2f619b57b40f6383e740087a8a5e9",
    ));
    assert!(
        verify_message(&key, &message, &identity_r8).is_ok(),
        "identity R8 with a matching signature is accepted, matching the circuit"
    );

    // Off-curve R8 = (1,1) is rejected by the on-curve check.
    let mut off_curve_r8 = identity_r8;
    off_curve_r8[31] = 1;
    assert!(verify_message(&key, &message, &off_curve_r8).is_err());

    // Identity A remains rejected by the core boundary.
    let mut identity_a = [0u8; 64];
    identity_a[63] = 1;
    assert!(verify_message(&identity_a, &message, &identity_r8).is_err());
}
