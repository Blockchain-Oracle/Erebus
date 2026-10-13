//! Operator-sponsored shielded submission engine, using the shared gas-account nonce lane.
//! User fees remain zero. Network errors never permit a fresh nonce or buyer fallback.

use std::{path::PathBuf, time::Duration};

use erebus_core::{settlement::PreparedSettlement, suite::keccak256};
use erebus_evm::{
    chain::{
        Eip1559Fees, EvmChain, NonceError, SignerJournal, SigningPlan, TransactionKey, TxStatus,
    },
    deployment::EvmDeployment,
};

use crate::{
    relay_admission::{RelayAdmissionPolicy, TransferVerifier},
    relay_journal::{RelayJournal, RelayJournalError, RelayJournalStatus},
    relay_request::RelayRequest,
    rpc::PoolRpc,
};

/// Redacted relay failures; none proves that an earlier attempt cannot pay.
#[derive(Debug, thiserror::Error)]
pub enum SponsorError {
    /// Invalid owner configuration or request encoding.
    #[error("invalid shielded sponsorship configuration or request")]
    Configuration,
    /// External proof, deployment or pinned simulation refused the call.
    #[error("shielded relay admission unavailable or refused")]
    Admission,
    /// Paired evidence was missing, conflicting or nonfinal.
    #[error("shielded relay observation unavailable; retain the operation")]
    Observation,
    /// Native gas balance does not cover the complete fee-cap budget.
    #[error("shielded relay gas funding unavailable or insufficient")]
    Funding,
    /// Another retained operation or foreign pending nonce occupies the account.
    #[error("shielded relay sponsor account busy; retain the operation")]
    Busy,
    /// No retained transaction or signing policy was changed by this error.
    #[error("shielded relay transaction unavailable; retain the operation")]
    Transaction,
    /// Storage failed closed. Recovery must use the original request and journals.
    #[error(transparent)]
    Journal(#[from] RelayJournalError),
}

/// Fixed owner policy, shared storage and two independently configured providers.
pub struct SponsorConfig {
    /// Absolute service root, shared by every process for this service.
    pub root: PathBuf,
    /// Absolute account-wide nonce root, shared across all pools using the sponsor.
    pub signer_root: PathBuf,
    /// Authenticated deployment, sponsor, gas and proof context.
    pub admission: RelayAdmissionPolicy,
    /// Maximum EIP-1559 charges for the first retained transaction.
    pub fees: Eip1559Fees,
    /// Optional owner-fixed fee ceiling, reserved in full before admitting an operation.
    pub replacement_fees: Option<Eip1559Fees>,
    /// Primary read and broadcast endpoint.
    pub rpc_url: String,
    /// Independent verification endpoint, never used for broadcast.
    pub peer_rpc_url: String,
    /// Finite timeout for each RPC call.
    pub timeout: Duration,
}

/// Durable submission engine. An HTTP adapter must authenticate and rate-limit before entry.
pub struct SponsoredRelay {
    execution_root: PathBuf,
    policy: RelayAdmissionPolicy,
    verifier: TransferVerifier,
    journal: RelayJournal,
    signer: SignerJournal,
    key: TransactionKey,
    fees: Eip1559Fees,
    replacement_fees: Option<Eip1559Fees>,
    chain: EvmChain,
    peer_chain: EvmChain,
    rpc: PoolRpc,
    peer_rpc: PoolRpc,
}

impl SponsoredRelay {
    /// Validates the proof-only request before an adapter reserves its gas budget.
    /// This is not paired admission or evidence that a payment can be submitted.
    pub fn request_identity(&self, bytes: &[u8]) -> Result<[u8; 32], SponsorError> {
        Ok(self.decode(bytes)?.1.operation_ref)
    }

    /// Maximum native charge reserved for one operation under the fixed fee policy.
    pub fn gas_budget(&self) -> Result<u128, SponsorError> {
        (self.policy.gas_limit as u128)
            .checked_mul(self.replacement_fees.unwrap_or(self.fees).max_fee_per_gas())
            .ok_or(SponsorError::Configuration)
    }

    /// Configures proof verification and zero-user-fee sponsorship. Does not broadcast.
    pub async fn connect(
        config: SponsorConfig,
        verifier: TransferVerifier,
        key: &[u8; 32],
    ) -> Result<Self, SponsorError> {
        let policy = config.admission;
        let key = TransactionKey::from_bytes(key).map_err(|_| SponsorError::Configuration)?;
        if !config.root.is_absolute()
            || !config.signer_root.is_absolute()
            || config.timeout.is_zero()
            || key.address() != policy.sponsor
            || config
                .replacement_fees
                .is_some_and(|fees| !fees.can_replace(&config.fees))
            || policy
                .context
                .domain
                .pool
                .as_ref()
                .map(|pool| pool.as_bytes())
                != Some(policy.deployment.pool.as_slice())
        {
            return Err(SponsorError::Configuration);
        }
        let deployment = |url: &str| {
            EvmDeployment::new(
                policy.context.domain.namespace.clone(),
                policy.deployment.pool,
                policy.context.domain.verifier_version,
                url,
            )
        };
        let chain = EvmChain::connect(
            deployment(&config.rpc_url).map_err(|_| SponsorError::Configuration)?,
            config.timeout,
        )
        .await
        .map_err(|_| SponsorError::Observation)?;
        let peer_chain = EvmChain::connect(
            deployment(&config.peer_rpc_url).map_err(|_| SponsorError::Configuration)?,
            config.timeout,
        )
        .await
        .map_err(|_| SponsorError::Observation)?;
        chain
            .check_peer(&peer_chain)
            .map_err(|_| SponsorError::Configuration)?;
        let rpc = PoolRpc::with_timeout(
            &config.rpc_url,
            policy.deployment.chain_id,
            policy.deployment.pool,
            config.timeout,
        )
        .map_err(|_| SponsorError::Configuration)?;
        let peer_rpc = PoolRpc::with_timeout(
            &config.peer_rpc_url,
            policy.deployment.chain_id,
            policy.deployment.pool,
            config.timeout,
        )
        .map_err(|_| SponsorError::Configuration)?;
        let pins = policy.runtime_pins;
        let policy_pin = keccak256(&[
            b"erebus-shielded-sponsor-v1",
            &policy.deployment.chain_id.to_be_bytes(),
            &policy.deployment.pool,
            &policy.deployment.first_block.to_be_bytes(),
            &policy.deployment.first_hash,
            &policy.sponsor,
            &policy.gas_limit.to_be_bytes(),
            &config.fees.max_fee_per_gas().to_be_bytes(),
            &config.fees.max_priority_fee_per_gas().to_be_bytes(),
            &pins.pool,
            &pins.poseidon,
            &pins.deposit,
            &pins.transfer,
            &pins.withdraw,
        ]);
        let mut journal = RelayJournal::open(
            config.root.join("operations"),
            policy.context.clone(),
            policy_pin,
            policy.sponsor,
            policy.gas_limit,
            config.fees,
        )?;
        if let Some(ceiling) = config.replacement_fees {
            journal = journal.with_replacement_ceiling(ceiling)?;
        }
        let signer = SignerJournal::open(
            &config.signer_root,
            policy.deployment.chain_id,
            policy.sponsor,
        )
        .map_err(|_| SponsorError::Transaction)?;
        Ok(Self {
            execution_root: config.signer_root.join(format!(
                "relay-execution-{}-{}",
                policy.deployment.chain_id,
                hex::encode(policy.sponsor)
            )),
            policy,
            verifier,
            journal,
            signer,
            key,
            fees: config.fees,
            replacement_fees: config.replacement_fees,
            chain,
            peer_chain,
            rpc,
            peer_rpc,
        })
    }

    fn decode(&self, bytes: &[u8]) -> Result<(RelayRequest, PreparedSettlement), SponsorError> {
        let request = RelayRequest::decode(bytes).map_err(|_| SponsorError::Configuration)?;
        let prepared = request
            .prepared(&self.policy.context)
            .map_err(|_| SponsorError::Configuration)?;
        self.verifier
            .verify(&self.policy.context, &prepared, 0)
            .map_err(|_| SponsorError::Admission)?;
        Ok((request, prepared))
    }

    /// Submits only the first fenced call; repeats observe the exact retained attempt.
    pub async fn relay(&self, bytes: &[u8]) -> Result<RelayJournalStatus, SponsorError> {
        let _execution =
            RelayJournal::try_execution_lock(&self.execution_root)?.ok_or(SponsorError::Busy)?;
        let (request, prepared) = self.decode(bytes)?;
        if let Some(status) = self.journal.lookup(prepared.operation_ref)? {
            // Retain checks exact original calldata before any recovery RPC.
            self.journal.retain(&request)?;
            if status.broadcast_started || status.finalized.is_some() {
                return self.observe_prepared(&prepared).await;
            }
        }
        self.verifier
            .admit(&self.policy, &prepared, &self.rpc, &self.peer_rpc)
            .await
            .map_err(|_| SponsorError::Admission)?;
        let cost = self.gas_budget()?;
        for chain in [&self.chain, &self.peer_chain] {
            if chain
                .gas_payer_balance(self.policy.sponsor)
                .await
                .map_err(|_| SponsorError::Funding)?
                < cost
            {
                return Err(SponsorError::Funding);
            }
        }
        self.journal.retain(&request)?;
        let retained_plan = self.journal.retained_plan(prepared.operation_ref)?;
        let plan = if let Some(retained) = retained_plan {
            let claim = self
                .signer
                .resume_call(
                    self.chain.deployment(),
                    &prepared,
                    self.policy.deployment.pool,
                    &prepared.backend_evidence,
                )
                .map_err(|_| SponsorError::Transaction)?
                .ok_or(SponsorError::Transaction)?;
            if claim.encode() != retained.encode() {
                return Err(SponsorError::Transaction);
            }
            retained
        } else {
            self.chain
                .reserve_nonce_for_call(
                    &self.signer,
                    &prepared,
                    self.policy.deployment.pool,
                    &prepared.backend_evidence,
                    self.fees,
                    self.policy.gas_limit,
                )
                .await
                .map_err(|error| match error {
                    NonceError::Busy => SponsorError::Busy,
                    _ => SponsorError::Transaction,
                })?
        };
        // Refresh paired admission immediately before signing, after nonce/funding reads.
        // Failure retains the exact nonce claim; it never releases it on a simulation error.
        self.verifier
            .admit(&self.policy, &prepared, &self.rpc, &self.peer_rpc)
            .await
            .map_err(|_| SponsorError::Admission)?;
        self.journal
            .sign_once(prepared.operation_ref, &plan, |plan| {
                plan.sign_call(
                    self.policy.deployment.chain_id,
                    self.policy.deployment.pool,
                    prepared.backend_evidence.clone(),
                    &self.key,
                )
                .map_err(|_| RelayJournalError::Transaction)
            })?;
        if let Some(signed) = self
            .journal
            .begin_initial_broadcast(prepared.operation_ref)?
        {
            // Any network error is ambiguous after the durable fence: return retained status.
            let _outcome = self
                .chain
                .broadcast_call(
                    &plan,
                    signed.raw(),
                    self.policy.deployment.pool,
                    &prepared.backend_evidence,
                )
                .await;
        }
        self.journal
            .status(prepared.operation_ref)
            .map_err(Into::into)
    }

    /// Explicit owner recovery of retained bytes, not a client or automatic retry path.
    /// A finalized attempt is observed only. Nonfinal recovery uses its original nonce/call.
    pub async fn resume(&self, bytes: &[u8]) -> Result<RelayJournalStatus, SponsorError> {
        let _execution =
            RelayJournal::try_execution_lock(&self.execution_root)?.ok_or(SponsorError::Busy)?;
        let (request, prepared) = self.decode(bytes)?;
        let retained = self
            .journal
            .lookup(prepared.operation_ref)?
            .ok_or(SponsorError::Observation)?;
        self.journal.retain(&request)?;
        if retained.transaction_hash.is_none() {
            drop(_execution);
            return self.relay(bytes).await;
        }
        let status = self.observe_prepared(&prepared).await?;
        if status.finalized.is_some() {
            return Ok(status);
        }
        let plan = self
            .journal
            .retained_plan(prepared.operation_ref)?
            .ok_or(SponsorError::Transaction)?;
        let claim = self
            .signer
            .resume_call(
                self.chain.deployment(),
                &prepared,
                self.policy.deployment.pool,
                &prepared.backend_evidence,
            )
            .map_err(|_| SponsorError::Transaction)?
            .ok_or(SponsorError::Transaction)?;
        if claim.encode() != plan.encode() {
            return Err(SponsorError::Transaction);
        }
        self.verifier
            .admit(&self.policy, &prepared, &self.rpc, &self.peer_rpc)
            .await
            .map_err(|_| SponsorError::Admission)?;
        let cost = self.gas_budget()?;
        for chain in [&self.chain, &self.peer_chain] {
            if chain
                .gas_payer_balance(self.policy.sponsor)
                .await
                .map_err(|_| SponsorError::Funding)?
                < cost
            {
                return Err(SponsorError::Funding);
            }
        }
        if let Some(signed) = self.journal.begin_explicit_resume(prepared.operation_ref)? {
            let broadcast_plan = SigningPlan::new(self.policy.sponsor, signed.params())
                .map_err(|_| SponsorError::Transaction)?;
            let _outcome = self
                .chain
                .broadcast_call(
                    &broadcast_plan,
                    signed.raw(),
                    self.policy.deployment.pool,
                    &prepared.backend_evidence,
                )
                .await;
        }
        self.journal
            .status(prepared.operation_ref)
            .map_err(Into::into)
    }

    /// Explicit owner fee bump. No public HTTP route, new nonce, cancellation or user fee.
    pub async fn replace(&self, bytes: &[u8]) -> Result<RelayJournalStatus, SponsorError> {
        let _execution =
            RelayJournal::try_execution_lock(&self.execution_root)?.ok_or(SponsorError::Busy)?;
        let fees = self.replacement_fees.ok_or(SponsorError::Configuration)?;
        let (request, prepared) = self.decode(bytes)?;
        if self.journal.lookup(prepared.operation_ref)?.is_none() {
            return Err(SponsorError::Observation);
        }
        self.journal.retain(&request)?;
        let status = self.observe_prepared(&prepared).await?;
        if status.finalized.is_some() {
            return Ok(status);
        }
        if self
            .journal
            .replacement_broadcast_started(prepared.operation_ref)?
        {
            return Ok(status);
        }
        let original = self
            .journal
            .retained_plan(prepared.operation_ref)?
            .ok_or(SponsorError::Transaction)?;
        let claim = self
            .signer
            .resume_call(
                self.chain.deployment(),
                &prepared,
                self.policy.deployment.pool,
                &prepared.backend_evidence,
            )
            .map_err(|_| SponsorError::Transaction)?
            .ok_or(SponsorError::Transaction)?;
        if original.encode() != claim.encode() {
            return Err(SponsorError::Transaction);
        }
        self.verifier
            .admit(&self.policy, &prepared, &self.rpc, &self.peer_rpc)
            .await
            .map_err(|_| SponsorError::Admission)?;
        for chain in [&self.chain, &self.peer_chain] {
            if chain
                .gas_payer_balance(self.policy.sponsor)
                .await
                .map_err(|_| SponsorError::Funding)?
                < self.gas_budget()?
            {
                return Err(SponsorError::Funding);
            }
        }
        let signed = self
            .journal
            .sign_replacement(prepared.operation_ref, fees, |plan| {
                plan.sign_call(
                    self.policy.deployment.chain_id,
                    self.policy.deployment.pool,
                    prepared.backend_evidence.clone(),
                    &self.key,
                )
                .map_err(|_| RelayJournalError::Transaction)
            })?;
        let plan = SigningPlan::new(self.policy.sponsor, signed.params())
            .map_err(|_| SponsorError::Transaction)?;
        if let Some(signed) = self
            .journal
            .begin_replacement_broadcast(prepared.operation_ref, signed.hash())?
        {
            let _outcome = self
                .chain
                .broadcast_call(
                    &plan,
                    signed.raw(),
                    self.policy.deployment.pool,
                    &prepared.backend_evidence,
                )
                .await;
        }
        self.journal
            .status(prepared.operation_ref)
            .map_err(Into::into)
    }

    /// Read-only network recovery; never re-signs or broadcasts, including unknown attempts.
    pub async fn observe(&self, bytes: &[u8]) -> Result<RelayJournalStatus, SponsorError> {
        let _execution =
            RelayJournal::try_execution_lock(&self.execution_root)?.ok_or(SponsorError::Busy)?;
        let (request, prepared) = self.decode(bytes)?;
        if self.journal.lookup(prepared.operation_ref)?.is_none() {
            return Err(SponsorError::Observation);
        }
        self.journal.retain(&request)?;
        self.observe_prepared(&prepared).await
    }

    async fn observe_prepared(
        &self,
        prepared: &PreparedSettlement,
    ) -> Result<RelayJournalStatus, SponsorError> {
        let status = self.journal.status(prepared.operation_ref)?;
        for hash in self.journal.attempts(prepared.operation_ref)? {
            if let Some(finalized) = self.observe_attempt(prepared, status, hash).await? {
                return Ok(finalized);
            }
        }
        if status.finalized.is_some() {
            return Err(SponsorError::Observation);
        }
        Ok(status)
    }

    async fn observe_attempt(
        &self,
        prepared: &PreparedSettlement,
        status: RelayJournalStatus,
        hash: [u8; 32],
    ) -> Result<Option<RelayJournalStatus>, SponsorError> {
        let (first, second) = tokio::join!(self.chain.observe(hash), self.peer_chain.observe(hash));
        let first = first.map_err(|_| SponsorError::Observation)?;
        if first != second.map_err(|_| SponsorError::Observation)? {
            return Err(SponsorError::Observation);
        }
        let TxStatus::Included(receipt) = first else {
            return Ok(None);
        };
        if !receipt.success && status.transaction_hash != Some(hash) {
            return Ok(None);
        }
        let nonce = self
            .chain
            .verified_finalized_nonce_agreed(&self.peer_chain, self.policy.sponsor)
            .await
            .map_err(|_| SponsorError::Observation)?;
        if receipt.block.number > nonce.anchor().number {
            return Ok(None);
        }
        for rpc in [&self.rpc, &self.peer_rpc] {
            for anchor in [
                self.policy.deployment.first_hash,
                receipt.block.hash,
                nonce.anchor().hash,
            ] {
                rpc.authenticate_runtime_at(anchor, self.policy.runtime_pins)
                    .await
                    .map_err(|_| SponsorError::Observation)?;
            }
            if rpc
                .canonical_block_hash(self.policy.deployment.first_block)
                .await
                .map_err(|_| SponsorError::Observation)?
                != self.policy.deployment.first_hash
                || rpc
                    .canonical_block_hash(receipt.block.number)
                    .await
                    .map_err(|_| SponsorError::Observation)?
                    != receipt.block.hash
                || rpc
                    .canonical_block_hash(nonce.anchor().number)
                    .await
                    .map_err(|_| SponsorError::Observation)?
                    != nonce.anchor().hash
            {
                return Err(SponsorError::Observation);
            }
            if rpc
                .finalized_head()
                .await
                .map_err(|_| SponsorError::Observation)?
                .number
                < nonce.anchor().number
            {
                return Err(SponsorError::Observation);
            }
        }
        // Attribute only this signed attempt, never a consumed deal/nonce alone.
        self.journal
            .record_finalized(prepared.operation_ref, hash, &receipt, &nonce)?;
        match self.signer.release(&nonce) {
            Ok(()) | Err(NonceError::NotConsumed) => {}
            Err(_) => return Err(SponsorError::Transaction),
        }
        self.journal
            .status(prepared.operation_ref)
            .map(Some)
            .map_err(Into::into)
    }
}
