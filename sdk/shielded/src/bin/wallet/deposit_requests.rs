//! Caller deposit references are retained before proving and before any broadcast.

use std::{fmt, path::Path};

use erebus_journal::{IdentityLock, JournalRecord, RecordId, Store};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Id(String);
impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl RecordId for Id {
    fn as_file_stem(&self) -> &str {
        &self.0
    }
    fn from_file_stem(stem: &str) -> Option<Self> {
        (stem.len() == 64
            && stem
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && stem != "0".repeat(64))
        .then(|| Self(stem.into()))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    version: u32,
    id: Id,
    binding: [u8; 32],
    commitment: Option<[u8; 32]>,
}
impl JournalRecord for Request {
    type Id = Id;
    const CURRENT_VERSION: u32 = 1;
    const OLDEST_READABLE_VERSION: u32 = 1;
    fn version(&self) -> u32 {
        self.version
    }
    fn record_id(&self) -> &Id {
        &self.id
    }
    fn attempt_count(&self) -> usize {
        1
    }
}

pub(super) struct DepositRequest {
    store: Store<Request>,
    record: Request,
    _lock: IdentityLock,
}
impl DepositRequest {
    #[cfg(test)]
    pub(super) fn retain(
        root: &Path,
        reference: [u8; 32],
        binding: [u8; 32],
    ) -> Result<Self, &'static str> {
        Self::retain_if_new(root, reference, binding, || Ok(()))
    }

    pub(super) fn lock_legacy(root: &Path) -> Result<IdentityLock, &'static str> {
        let store: Store<Request> =
            Store::open(root).map_err(|_| "deposit request journal unavailable")?;
        let lock = store
            .lock_identity()
            .map_err(|_| "deposit request journal unavailable")?;
        if store
            .records()
            .map_err(|_| "deposit request journal unavailable")?
            .iter()
            .any(|record| record.commitment.is_none())
        {
            return Err("a referenced deposit preparation is retained");
        }
        Ok(lock)
    }

    pub(super) fn retain_if_new(
        root: &Path,
        reference: [u8; 32],
        binding: [u8; 32],
        before_create: impl FnOnce() -> Result<(), &'static str>,
    ) -> Result<Self, &'static str> {
        if reference == [0; 32] || binding == [0; 32] {
            return Err("invalid deposit reference");
        }
        let store: Store<Request> =
            Store::open(root).map_err(|_| "deposit request journal unavailable")?;
        let lock = store
            .lock_identity()
            .map_err(|_| "deposit request journal unavailable")?;
        let id = Id(hex::encode(reference));
        let saved = store
            .read(&id)
            .map_err(|_| "deposit request journal unavailable")?;
        let record = match saved {
            Some(record) => {
                if record.binding != binding
                    || store
                        .read_blob(&id, 0)
                        .map_err(|_| "deposit request journal unavailable")?
                        != binding
                {
                    return Err("deposit reference conflicts with retained policy");
                }
                record
            }
            None => {
                if store
                    .blob_path(&id, 0)
                    .try_exists()
                    .map_err(|_| "deposit request journal unavailable")?
                {
                    return Err("deposit request record missing; retain original state");
                }
                if store
                    .records()
                    .map_err(|_| "deposit request journal unavailable")?
                    .iter()
                    .any(|record| record.commitment.is_none())
                {
                    return Err("another deposit preparation is retained");
                }
                before_create()?;
                let mut record = Request {
                    version: 1,
                    id,
                    binding,
                    commitment: None,
                };
                store
                    .write_blob_then_record::<erebus_journal::StoreError<Id>>(
                        &mut record,
                        0,
                        &binding,
                        |_| Ok(()),
                    )
                    .map_err(|_| "deposit request journal unavailable")?;
                record
            }
        };
        Ok(Self {
            store,
            record,
            _lock: lock,
        })
    }

    pub(super) fn commitment(&self) -> Option<[u8; 32]> {
        self.record.commitment
    }

    pub(super) fn bind(&mut self, commitment: [u8; 32]) -> Result<(), &'static str> {
        if commitment == [0; 32]
            || self
                .record
                .commitment
                .is_some_and(|saved| saved != commitment)
        {
            return Err("deposit reference conflicts with retained note");
        }
        if self
            .store
            .records()
            .map_err(|_| "deposit request journal unavailable")?
            .iter()
            .any(|record| record.id != self.record.id && record.commitment == Some(commitment))
        {
            return Err("deposit note belongs to another reference");
        }
        self.record.commitment = Some(commitment);
        self.store
            .write(&self.record)
            .map_err(|_| "deposit request journal unavailable")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn references_survive_restart_and_bind_one_policy_and_note() {
        let root = tempfile::tempdir().unwrap();
        let mut retained = DepositRequest::retain(root.path(), [1; 32], [2; 32]).unwrap();
        assert_eq!(retained.commitment(), None);
        retained.bind([3; 32]).unwrap();
        drop(retained);
        let mut restored = DepositRequest::retain(root.path(), [1; 32], [2; 32]).unwrap();
        assert_eq!(restored.commitment(), Some([3; 32]));
        assert!(restored.bind([4; 32]).is_err());
        drop(restored);
        assert!(DepositRequest::retain(root.path(), [1; 32], [4; 32]).is_err());
        let mut other = DepositRequest::retain(root.path(), [5; 32], [2; 32]).unwrap();
        assert!(other.bind([3; 32]).is_err());
    }

    #[test]
    fn interrupted_preparation_and_deleted_or_corrupt_records_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        drop(DepositRequest::retain(root.path(), [1; 32], [2; 32]).unwrap());
        assert!(DepositRequest::retain(root.path(), [3; 32], [2; 32]).is_err());
        let record = root.path().join(format!("{}.json", hex::encode([1; 32])));
        std::fs::write(&record, b"{").unwrap();
        assert!(DepositRequest::retain(root.path(), [1; 32], [2; 32]).is_err());
        std::fs::remove_file(&record).unwrap();
        assert!(DepositRequest::retain(root.path(), [1; 32], [2; 32]).is_err());
    }

    #[test]
    fn initial_refusal_does_not_trap_the_reference_and_restart_reuses_its_intent() {
        let root = tempfile::tempdir().unwrap();
        assert!(
            DepositRequest::retain_if_new(root.path(), [1; 32], [2; 32], || Err("busy")).is_err()
        );
        let retained =
            DepositRequest::retain_if_new(root.path(), [1; 32], [2; 32], || Ok(())).unwrap();
        assert_eq!(retained.commitment(), None);
        drop(retained);
        assert!(DepositRequest::lock_legacy(root.path()).is_err());
        let mut restored = DepositRequest::retain_if_new(root.path(), [1; 32], [2; 32], || {
            panic!("existing intent must not be replaced")
        })
        .unwrap();
        restored.bind([3; 32]).unwrap();
        drop(restored);
        assert!(DepositRequest::lock_legacy(root.path()).is_ok());
    }
}
