//! Circomlib/Anvil-generated M5 note and tree vectors, checked independently in Rust.

use erebus_core::shielded::{
    note_commitment, note_domain, note_nullifier, note_root_from_path, note_spend_tag,
    note_tree_parent, NOTE_TREE_DEPTH,
};
use serde_json::Value;

const VECTOR: &str = include_str!("fixtures/m5-note-vector.json");

fn fixed<const N: usize>(hex_value: &str) -> [u8; N] {
    hex::decode(hex_value)
        .expect("pinned hex")
        .try_into()
        .expect("pinned width")
}

fn field<'a>(value: &'a Value, name: &str) -> &'a str {
    value[name].as_str().expect("pinned string")
}

fn zero_nodes() -> [[u8; 32]; NOTE_TREE_DEPTH] {
    let mut nodes = [[0u8; 32]; NOTE_TREE_DEPTH];
    for level in 1..NOTE_TREE_DEPTH {
        nodes[level] = note_tree_parent(&nodes[level - 1], &nodes[level - 1]).expect("zero hash");
    }
    nodes
}

#[test]
fn note_openings_and_paths_match_the_m5_circuit_runner() {
    let vector: Value = serde_json::from_str(VECTOR).expect("pinned JSON");
    let asset = fixed::<20>(field(&vector, "assetHex"));
    let domain = note_domain(
        field(&vector, "chainId").parse().expect("chain"),
        &fixed::<20>(field(&vector, "poolHex")),
        field(&vector, "verifierVersion").parse().expect("version"),
    )
    .expect("note domain");
    assert_eq!(hex::encode(domain), field(&vector, "noteDomainHex"));
    let mut opened = Vec::new();

    for name in ["input", "payment", "change"] {
        let note = &vector[name];
        let secret = fixed::<32>(field(note, "spendSecretHex"));
        let tag = note_spend_tag(&secret).expect("tag");
        assert_eq!(hex::encode(tag), field(note, "spendTagHex"));

        let commitment = note_commitment(
            &domain,
            &asset,
            field(note, "amount").parse().expect("amount"),
            &fixed(field(note, "ownerHex")),
            &tag,
            &fixed(field(note, "saltHex")),
        )
        .expect("note");
        assert_eq!(hex::encode(commitment), field(note, "commitmentHex"));
        assert_eq!(
            hex::encode(note_nullifier(&domain, &secret, &commitment).expect("nullifier")),
            field(note, "nullifierHex")
        );
        opened.push(commitment);
    }

    let zeros = zero_nodes();
    let input_path = zeros;
    assert_eq!(
        hex::encode(note_root_from_path(&opened[0], 0, &input_path).expect("deposit root")),
        field(&vector["input"], "rootHex")
    );

    let mut payment_path = zeros;
    payment_path[0] = opened[0];
    payment_path[1] = note_tree_parent(&opened[2], &[0u8; 32]).expect("change subtree");
    assert_eq!(
        hex::encode(note_root_from_path(&opened[1], 1, &payment_path).expect("transfer root")),
        field(&vector["payment"], "rootHex")
    );
    assert_ne!(
        note_root_from_path(&opened[1], 0, &payment_path).expect("wrong index"),
        fixed(field(&vector["payment"], "rootHex"))
    );
    assert!(note_root_from_path(&opened[1], 1 << NOTE_TREE_DEPTH, &payment_path).is_err());
}

#[test]
fn invalid_note_material_is_rejected() {
    let vector: Value = serde_json::from_str(VECTOR).expect("pinned JSON");
    let input = &vector["input"];
    let asset = fixed(field(&vector, "assetHex"));
    let domain = fixed(field(&vector, "noteDomainHex"));
    let owner = fixed(field(input, "ownerHex"));
    let tag = fixed(field(input, "spendTagHex"));
    let salt = fixed(field(input, "saltHex"));
    let note = fixed(field(input, "commitmentHex"));
    assert!(note_spend_tag(&[0; 32]).is_err());
    assert!(note_nullifier(&domain, &[0; 32], &note).is_err());
    assert!(note_commitment(&domain, &[0; 20], 150, &owner, &tag, &salt).is_err());
    assert!(note_commitment(&domain, &asset, 150, &owner, &[0; 32], &salt).is_err());
    assert!(note_commitment(&[0; 32], &asset, 150, &owner, &tag, &salt).is_err());

    let mut changed_salt = salt;
    changed_salt[31] ^= 1;
    assert_ne!(
        note_commitment(&domain, &asset, 150, &owner, &tag, &changed_salt).expect("other note"),
        note
    );
    let mut invalid_field = [0xff; 32];
    assert!(note_nullifier(
        &domain,
        &fixed(field(input, "spendSecretHex")),
        &invalid_field
    )
    .is_err());
    invalid_field[0] = 0;
    assert!(note_tree_parent(&[0xff; 32], &invalid_field).is_err());
}

/// Domain binding: the same opening in another deployment domain must give a different
/// commitment and nullifier, so identical openings are not recognizable across domains.
#[test]
fn cross_domain_openings_are_not_recognizable() {
    let vector: Value = serde_json::from_str(VECTOR).expect("pinned JSON");
    let input = &vector["input"];
    let asset = fixed(field(&vector, "assetHex"));
    let domain = fixed(field(&vector, "noteDomainHex"));
    let owner = fixed(field(input, "ownerHex"));
    let tag = fixed(field(input, "spendTagHex"));
    let salt = fixed(field(input, "saltHex"));
    let secret = fixed(field(input, "spendSecretHex"));
    let commitment = note_commitment(&domain, &asset, 150, &owner, &tag, &salt).expect("note");
    let nullifier = note_nullifier(&domain, &secret, &commitment).expect("nullifier");

    for (chain, pool, version) in [
        (10143u64, fixed::<20>(field(&vector, "poolHex")), 3u32),
        (10144, fixed::<20>(field(&vector, "poolHex")), 2),
        (10143, [9u8; 20], 2),
    ] {
        let other = note_domain(chain, &pool, version).expect("other domain");
        let other_commitment =
            note_commitment(&other, &asset, 150, &owner, &tag, &salt).expect("other note");
        assert_ne!(other_commitment, commitment);
        assert_ne!(
            note_nullifier(&other, &secret, &other_commitment).expect("other nullifier"),
            nullifier
        );
    }
}
