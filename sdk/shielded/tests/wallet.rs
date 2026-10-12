//! Local encrypted-wallet storage, restore, and state-transition checks.

use std::fs;

use erebus_shielded_prover::wallet::{
    NoteConsumption, NoteInclusion, OwnedNote, WalletDomain, WalletError, WalletStore,
};
use serde_json::Value;

mod support;

fn withdrawal_from_restored_change(root: &std::path::Path) -> erebus_shielded_prover::NoteWitness {
    use erebus_core::shielded::{note_tree_parent, NOTE_TREE_DEPTH};
    use erebus_shielded_prover::{
        build_withdraw_witness,
        indexer::{PoolBlock, PoolEvent, PoolIndex},
    };
    let vector: Value = serde_json::from_str(include_str!(
        "../../core/tests/fixtures/m5-note-vector.json"
    ))
    .unwrap();
    let data = &vector["change"];
    let change = OwnedNote::new(
        bytes(vector["assetHex"].as_str().unwrap()),
        80,
        bytes(data["ownerHex"].as_str().unwrap()),
        bytes(data["spendSecretHex"].as_str().unwrap()),
        bytes(data["saltHex"].as_str().unwrap()),
        domain().note_domain().unwrap(),
    )
    .unwrap();
    let change_commitment = change.commitment();
    assert_eq!(
        change_commitment,
        bytes(data["commitmentHex"].as_str().unwrap())
    );
    let input = fixture_note();
    let input_commitment = input.commitment();
    let input_nullifier = input.nullifier();
    let deposit_root = bytes(vector["input"]["rootHex"].as_str().unwrap());
    let path = root.join("original/wallet.enc");
    let store = WalletStore::new(&path, domain(), [7; 32]).unwrap();
    store
        .update(|wallet| {
            wallet.add(input)?;
            wallet.observe_insertion(
                &input_commitment,
                NoteInclusion {
                    index: 0,
                    block_number: 12,
                    block_hash: [3; 32],
                    root: deposit_root,
                },
            )?;
            wallet.reserve_transfer([4; 32], input_commitment, [5; 32], Some(change))
        })
        .unwrap();
    // This backup predates submission: it holds the opening, not an observed output.
    let backup_dir = root.join("restored");
    fs::create_dir(&backup_dir).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&backup_dir, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let backup = backup_dir.join("wallet.enc");
    fs::copy(&path, &backup).unwrap();
    drop(store);
    fs::remove_dir_all(root.join("original")).unwrap();
    let restored = WalletStore::new(&backup, domain(), [7; 32]).unwrap();
    assert!(restored
        .snapshot()
        .unwrap()
        .spendable(&change_commitment)
        .is_none());
    let payment: [u8; 32] = bytes(vector["payment"]["commitmentHex"].as_str().unwrap());
    let mut intermediate = note_tree_parent(&input_commitment, &payment).unwrap();
    let mut zero = [0; 32];
    for _ in 1..NOTE_TREE_DEPTH {
        zero = note_tree_parent(&zero, &zero).unwrap();
        intermediate = note_tree_parent(&intermediate, &zero).unwrap();
    }
    let final_root = bytes(vector["payment"]["rootHex"].as_str().unwrap());
    let mut index = PoolIndex::new(12).unwrap();
    index
        .apply_block(PoolBlock {
            number: 12,
            hash: [3; 32],
            parent_hash: [2; 32],
            events: vec![PoolEvent::Inserted {
                index: 0,
                commitment: input_commitment,
                root: deposit_root,
                tx_hash: [6; 32],
                log_index: 0,
            }],
        })
        .unwrap();
    index
        .apply_block(PoolBlock {
            number: 13,
            hash: [7; 32],
            parent_hash: [3; 32],
            events: vec![
                PoolEvent::Inserted {
                    index: 1,
                    commitment: payment,
                    root: intermediate,
                    tx_hash: [8; 32],
                    log_index: 0,
                },
                PoolEvent::Inserted {
                    index: 2,
                    commitment: change_commitment,
                    root: final_root,
                    tx_hash: [8; 32],
                    log_index: 1,
                },
                PoolEvent::Consumed {
                    nullifier: input_nullifier,
                    tx_hash: [8; 32],
                    log_index: 2,
                },
            ],
        })
        .unwrap();
    restored
        .update(|wallet| {
            index
                .replay_wallet(wallet)
                .map_err(|_| WalletError::Note("index replay"))
        })
        .unwrap();
    let snapshot = restored.snapshot().unwrap();
    assert!(snapshot.spendable(&input_commitment).is_none());
    assert_eq!(snapshot.spendable(&change_commitment).unwrap().amount(), 80);
    let witness = build_withdraw_witness(
        &snapshot,
        &index,
        &change_commitment,
        domain(),
        domain().verifier_version,
        [9; 20],
    )
    .unwrap();
    assert_eq!(witness.input["amount"], "80");
    assert_eq!(
        witness.input["spendSecret"],
        num_bigint::BigUint::from_bytes_be(&bytes::<32>(data["spendSecretHex"].as_str().unwrap()))
            .to_string()
    );
    witness
}

#[test]
fn positive_change_backup_builds_a_new_withdrawal_without_original_wallet_or_seller() {
    let root = tempfile::tempdir().unwrap();
    withdrawal_from_restored_change(root.path());
}

#[test]
#[ignore = "requires generated M5 local prototype artifacts; not a chain payout or release setup"]
fn positive_change_backup_generates_a_valid_local_withdrawal_proof() {
    use erebus_shielded_prover::ProvingArtifacts;
    let root = tempfile::tempdir().unwrap();
    let witness = withdrawal_from_restored_change(root.path());
    let build =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../circuits/m5/build");
    let manifest: Value =
        serde_json::from_slice(&fs::read(build.join("artifact-manifest.json")).unwrap()).unwrap();
    let hashes = &manifest["circuits"]["withdraw"];
    let artifacts = ProvingArtifacts {
        wasm: build.join("withdraw_js/withdraw.wasm"),
        r1cs: build.join("withdraw.r1cs"),
        zkey: build.join("withdraw.zkey"),
        wasm_sha256: hashes["wasmSha256"].as_str().unwrap().into(),
        r1cs_sha256: hashes["r1csSha256"].as_str().unwrap().into(),
        zkey_sha256: hashes["zkeySha256"].as_str().unwrap().into(),
    };
    let expected: Vec<&str> = witness.public_inputs.iter().map(String::as_str).collect();
    let proof = artifacts.prove(&witness.input, &expected).unwrap();
    assert_eq!(proof.public_inputs.len(), 8);
    assert_eq!(proof.public_inputs[7].to_string(), "80");
}

#[test]
fn every_wallet_write_boundary_preserves_a_restartable_note_reservation() {
    let mut boundaries = 0;
    let mut fail = 0;
    loop {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("private/wallet.enc");
        let (_, input) = confirmed_store(&path);
        let hook = std::sync::Arc::new(support::SweepHook::new(if fail == 0 {
            usize::MAX
        } else {
            fail
        }));
        let store = WalletStore::with_faults(&path, domain(), [7; 32], hook.clone()).unwrap();
        let result = store.update(|wallet| wallet.reserve(&input, [4; 32]));
        if fail == 0 {
            result.unwrap();
            boundaries = hook.count();
            assert!(boundaries > 0);
        } else {
            assert!(matches!(result, Err(WalletError::Io(_))));
        }
        drop(store);
        let restored = WalletStore::new(&path, domain(), [7; 32]).unwrap();
        assert_eq!(restored.snapshot().unwrap().notes().len(), 1);
        restored
            .update(|wallet| wallet.reserve(&input, [4; 32]))
            .unwrap();
        assert!(restored.snapshot().unwrap().spendable(&input).is_none());
        assert!(restored
            .update(|wallet| wallet.reserve(&input, [5; 32]))
            .is_err());
        assert!(restored
            .snapshot()
            .unwrap()
            .spendable_for(&input, [4; 32])
            .is_some());
        fail += 1;
        if fail > boundaries {
            break;
        }
    }
}

fn bytes<const N: usize>(hex_value: &str) -> [u8; N] {
    hex::decode(hex_value)
        .expect("fixture hex")
        .try_into()
        .expect("fixture width")
}

fn fixture_note() -> OwnedNote {
    let vector: Value = serde_json::from_str(include_str!(
        "../../core/tests/fixtures/m5-note-vector.json"
    ))
    .expect("pinned note vector");
    let note = &vector["input"];
    let value = |name: &str| note[name].as_str().expect("fixture string");
    let opening = OwnedNote::new(
        bytes(vector["assetHex"].as_str().expect("asset")),
        value("amount").parse().expect("amount"),
        bytes(value("ownerHex")),
        bytes(value("spendSecretHex")),
        bytes(value("saltHex")),
        bytes(vector["noteDomainHex"].as_str().expect("note domain")),
    )
    .expect("valid local note");
    assert_eq!(opening.commitment(), bytes(value("commitmentHex")));
    assert_eq!(opening.nullifier(), bytes(value("nullifierHex")));
    opening
}

fn domain() -> WalletDomain {
    WalletDomain {
        chain_id: 10143,
        pool: hex::decode("5fc8d32690cc91d4c39d9d3abcbd16989f875707")
            .expect("fixture pool")
            .try_into()
            .expect("fixture pool width"),
        verifier_version: 2,
    }
}

#[test]
fn deposit_and_withdraw_witnesses_refuse_foreign_note_domains_and_versions() {
    use erebus_shielded_prover::{
        build_deposit_witness, build_withdraw_witness,
        indexer::{PoolBlock, PoolEvent, PoolIndex},
        wallet::WalletSnapshot,
        ProverError,
    };
    let note = fixture_note();
    let commitment = note.commitment();
    let vector: Value = serde_json::from_str(include_str!(
        "../../core/tests/fixtures/m5-note-vector.json"
    ))
    .unwrap();
    let root = bytes(vector["input"]["rootHex"].as_str().unwrap());
    let mut wallet = WalletSnapshot::default();
    wallet.add(note.clone()).unwrap();
    let mut index = PoolIndex::new(12).unwrap();
    index
        .apply_block(PoolBlock {
            number: 12,
            hash: [3; 32],
            parent_hash: [2; 32],
            events: vec![PoolEvent::Inserted {
                index: 0,
                commitment,
                root,
                tx_hash: [4; 32],
                log_index: 0,
            }],
        })
        .unwrap();
    index.replay_wallet(&mut wallet).unwrap();
    for field in 0..3 {
        let mut foreign = domain();
        match field {
            0 => foreign.chain_id += 1,
            1 => foreign.pool[0] ^= 1,
            _ => foreign.verifier_version += 1,
        }
        assert!(matches!(
            build_deposit_witness(&note, foreign, foreign.verifier_version),
            Err(ProverError::Transfer("deposit note domain"))
        ));
        assert!(matches!(
            build_withdraw_witness(
                &wallet,
                &index,
                &commitment,
                foreign,
                foreign.verifier_version,
                [9; 20]
            ),
            Err(ProverError::Transfer("withdraw note domain"))
        ));
    }
    assert!(matches!(
        build_deposit_witness(&note, domain(), 3),
        Err(ProverError::Transfer("invalid pool domain"))
    ));
    assert!(matches!(
        build_withdraw_witness(&wallet, &index, &commitment, domain(), 3, [9; 20]),
        Err(ProverError::Transfer("invalid pool domain"))
    ));
    assert!(build_deposit_witness(&note, domain(), 2).is_ok());
    assert!(build_withdraw_witness(&wallet, &index, &commitment, domain(), 2, [9; 20]).is_ok());
}

#[test]
fn encrypted_backup_restores_note_and_replays_after_reorg() {
    let root = tempfile::tempdir().expect("private temp dir");
    let path = root.path().join("private/wallet.enc");
    let key = [7; 32];
    let store = WalletStore::new(&path, domain(), key).expect("wallet store");
    let note = fixture_note();
    let commitment = note.commitment();
    let nullifier = note.nullifier();
    store
        .update(|wallet| wallet.add(note))
        .expect("persist expected note");
    let ciphertext = fs::read(&path).expect("encrypted wallet");
    assert!(!ciphertext
        .windows(b"spend_secret".len())
        .any(|part| part == b"spend_secret"));
    assert_ne!(&ciphertext[..8], b"{\"notes\"");

    let backup = root.path().join("private/backup.enc");
    fs::copy(&path, &backup).expect("backup encrypted file");
    let restored = WalletStore::new(&backup, domain(), key).expect("restore store");
    assert_eq!(restored.snapshot().expect("restore").notes().len(), 1);
    let inclusion = NoteInclusion {
        index: 0,
        block_number: 12,
        block_hash: [3; 32],
        root: [4; 32],
    };
    restored
        .update(|wallet| {
            assert!(wallet.observe_insertion(&commitment, inclusion)?);
            assert!(wallet.observe_insertion(&commitment, inclusion)?);
            Ok(())
        })
        .expect("discover note");
    let snapshot = restored.snapshot().expect("confirmed note");
    assert_eq!(snapshot.notes()[0].inclusion(), Some(inclusion));
    let asset = bytes::<20>("5fbdb2315678afecb367f032d93f642f64180aa3");
    assert_eq!(
        snapshot.select(&asset, 70).expect("spendable").amount(),
        150
    );

    restored
        .update(|wallet| {
            wallet.reserve(&commitment, [5; 32])?;
            wallet.reserve(&commitment, [5; 32])?;
            assert!(wallet.select(&asset, 70).is_none());
            assert!(wallet.reserve(&commitment, [6; 32]).is_err());
            wallet.release(&commitment, [5; 32])?;
            wallet.observe_consumption(
                &nullifier,
                NoteConsumption {
                    block_number: 15,
                    block_hash: [8; 32],
                    tx_hash: [9; 32],
                },
            )?;
            assert!(wallet.select(&asset, 70).is_none());
            wallet.rewind_from(15);
            assert!(wallet.select(&asset, 70).is_some());
            wallet.rewind_from(12);
            assert!(wallet.select(&asset, 70).is_none());
            Ok(())
        })
        .expect("reorg replay");
    assert_eq!(
        restored.snapshot().expect("reopened").notes()[0].inclusion(),
        None
    );
}

#[test]
fn wrong_key_domain_and_corruption_never_reset_a_wallet() {
    let root = tempfile::tempdir().expect("temp dir");
    let path = root.path().join("private/wallet.enc");
    let store = WalletStore::new(&path, domain(), [7; 32]).expect("store");
    store
        .update(|wallet| wallet.add(fixture_note()))
        .expect("save");
    let before = fs::read(&path).expect("file");
    let wrong_key = WalletStore::new(&path, domain(), [8; 32]).expect("other key");
    assert!(matches!(
        wrong_key.snapshot(),
        Err(WalletError::Authentication)
    ));
    let wrong_domain = WalletStore::new(
        &path,
        WalletDomain {
            chain_id: 10144,
            ..domain()
        },
        [7; 32],
    )
    .expect("other domain");
    assert!(matches!(
        wrong_domain.snapshot(),
        Err(WalletError::Authentication)
    ));
    assert_eq!(fs::read(&path).expect("unchanged"), before);
    assert!(store
        .update(|wallet| {
            wallet.add(fixture_note())?;
            Ok(())
        })
        .is_err());
    assert_eq!(fs::read(&path).expect("still unchanged"), before);
    fs::write(&path, b"truncated").expect("simulate corruption");
    assert!(matches!(store.snapshot(), Err(WalletError::InvalidFile)));
}

fn confirmed_store(path: &std::path::Path) -> (WalletStore, [u8; 32]) {
    let store = WalletStore::new(path, domain(), [7; 32]).unwrap();
    let note = fixture_note();
    let input = note.commitment();
    store
        .update(|wallet| {
            wallet.add(note)?;
            wallet.observe_insertion(
                &input,
                NoteInclusion {
                    index: 0,
                    block_number: 12,
                    block_hash: [3; 32],
                    root: [4; 32],
                },
            )?;
            Ok(())
        })
        .unwrap();
    (store, input)
}

fn change_note() -> OwnedNote {
    let note = fixture_note();
    OwnedNote::new(
        note.asset(),
        80,
        note.owner(),
        [1; 32],
        [2; 32],
        note.note_domain(),
    )
    .unwrap()
}

#[test]
fn competing_stores_reserve_exactly_one_operation() {
    use std::sync::{Arc, Barrier};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private/wallet.enc");
    let (store, input) = confirmed_store(&path);
    let barrier = Arc::new(Barrier::new(4));
    let workers: Vec<_> = (1..=4)
        .map(|id| {
            let barrier = barrier.clone();
            let path = path.clone();
            std::thread::spawn(move || {
                let store = WalletStore::new(path, domain(), [7; 32]).unwrap();
                barrier.wait();
                (
                    id,
                    store
                        .update(|wallet| {
                            wallet.reserve_transfer([id; 32], input, [9; 32], Some(change_note()))
                        })
                        .is_ok(),
                )
            })
        })
        .collect();
    let winners: Vec<_> = workers
        .into_iter()
        .map(|w| w.join().unwrap())
        .filter(|(_, ok)| *ok)
        .collect();
    assert_eq!(winners.len(), 1);
    let operation = [winners[0].0; 32];
    let saved = store.snapshot().unwrap();
    assert_eq!(saved.notes().len(), 2);
    assert!(saved.spendable(&input).is_none());
    assert!(saved.spendable_for(&input, operation).is_some());
    store
        .update(|wallet| wallet.reserve_transfer(operation, input, [9; 32], Some(change_note())))
        .unwrap();
    assert_eq!(store.snapshot().unwrap().notes().len(), 2);
    let before = fs::read(&path).unwrap();
    assert!(store
        .update(|wallet| wallet.reserve_transfer(operation, input, [8; 32], Some(change_note())))
        .is_err());
    assert!(store
        .update(|wallet| wallet.release(&input, [10; 32]))
        .is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn failed_transition_and_failed_persistence_do_not_report_success() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private/wallet.enc");
    let (store, input) = confirmed_store(&path);
    let before = fs::read(&path).unwrap();
    let result: Result<(), WalletError> = store.update(|wallet| {
        wallet.reserve_transfer([5; 32], input, [9; 32], Some(change_note()))?;
        Err(WalletError::Note("injected failure before write"))
    });
    assert!(result.is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(store.snapshot().unwrap().spendable(&input).is_some());

    // Force rename failure after the new encrypted snapshot has been written.
    let backup = path.with_extension("backup");
    let result = store.update(|wallet| {
        wallet.reserve_transfer([5; 32], input, [9; 32], Some(change_note()))?;
        fs::rename(&path, &backup)?;
        fs::create_dir(&path)?;
        Ok(())
    });
    assert!(matches!(result, Err(WalletError::Io(_))));
    fs::remove_dir(&path).unwrap();
    fs::rename(&backup, &path).unwrap();
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(store.snapshot().unwrap().notes().len(), 1);
    assert!(store.snapshot().unwrap().spendable(&input).is_some());
    assert!(!fs::read_dir(path.parent().unwrap())
        .unwrap()
        .any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("tmp-")));
}

#[test]
fn legacy_snapshot_loads_and_released_operation_cannot_be_rebound() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private/wallet.enc");
    let (store, input) = confirmed_store(&path);
    let mut encoded = serde_json::to_value(store.snapshot().unwrap()).unwrap();
    encoded.as_object_mut().unwrap().remove("transfers");
    let legacy = serde_json::from_value(encoded).unwrap();
    store
        .update(|wallet| {
            *wallet = legacy;
            Ok(())
        })
        .unwrap();
    store
        .update(|wallet| wallet.reserve_transfer([5; 32], input, [9; 32], None))
        .unwrap();
    store
        .update(|wallet| wallet.release(&input, [5; 32]))
        .unwrap();
    assert!(store
        .update(|wallet| wallet.reserve_transfer([5; 32], input, [9; 32], None))
        .is_err());
    store
        .update(|wallet| wallet.reserve_transfer([6; 32], input, [9; 32], None))
        .unwrap();
}

#[test]
fn a_withdrawal_reservation_concludes_only_with_verified_consumption() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("private/wallet.enc");
    let (store, commitment) = confirmed_store(&path);
    let nullifier = store.snapshot().unwrap().notes()[0].nullifier();
    let binding = [9; 32];
    store
        .update(|wallet| wallet.reserve(&commitment, binding))
        .unwrap();
    // Without verified consumption, concluding is refused and the note stays reserved.
    assert!(store
        .update(|wallet| wallet.conclude_reservation(&commitment, binding))
        .is_err());
    assert!(store.snapshot().unwrap().spendable(&commitment).is_none());
    // Verified consumption concludes the reservation and cannot be undone by release.
    store
        .update(|wallet| {
            wallet.observe_consumption(
                &nullifier,
                NoteConsumption {
                    block_number: 13,
                    block_hash: [5; 32],
                    tx_hash: [6; 32],
                },
            )
        })
        .unwrap();
    store
        .update(|wallet| wallet.conclude_reservation(&commitment, binding))
        .unwrap();
    let snapshot = store.snapshot().unwrap();
    let note = snapshot
        .notes()
        .iter()
        .find(|note| note.commitment() == commitment)
        .unwrap();
    assert_eq!(note.reserved(), None);
    assert!(note.consumption().is_some());
    assert!(store
        .update(|wallet| wallet.release(&commitment, binding))
        .is_err());
}

#[test]
fn a_reserved_withdrawal_survives_backup_restore_and_cannot_be_replanned() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("private/wallet.enc");
    let (store, commitment) = confirmed_store(&path);
    let binding = [9; 32];
    store
        .update(|wallet| wallet.reserve(&commitment, binding))
        .unwrap();
    let backup = root.path().join("private/backup.enc");
    fs::copy(&path, &backup).unwrap();
    drop(store);
    let restored = WalletStore::new(&backup, domain(), [7; 32]).unwrap();
    let snapshot = restored.snapshot().unwrap();
    // The reservation survives the backup and still binds the same operation.
    assert!(snapshot.spendable(&commitment).is_none());
    assert!(snapshot.spendable_for(&commitment, binding).is_some());
    // A different operation cannot re-plan the note while the reservation holds.
    assert!(restored
        .update(|wallet| wallet.reserve(&commitment, [10; 32]))
        .is_err());
    // Only an explicit release, with no consumption, frees it.
    restored
        .update(|wallet| wallet.release(&commitment, binding))
        .unwrap();
    assert!(restored
        .snapshot()
        .unwrap()
        .spendable(&commitment)
        .is_some());
}
