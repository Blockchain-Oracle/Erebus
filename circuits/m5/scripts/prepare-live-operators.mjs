// Off-chain preparation only. Uses installed product commands and authenticated deployment pins.
import { mkdir, readFile, writeFile, open } from "node:fs/promises";
import { resolve } from "node:path";
import { createHash, randomBytes } from "node:crypto";
import { spawnSync } from "node:child_process";
import { Wallet } from "ethers";
import { liveAssetAmounts } from "./live-amounts.mjs";

const hash = (b) => createHash("sha256").update(b).digest("hex");
async function privateFile(path, bytes) {
  const file = await open(path, "wx", 0o600);
  try { await file.writeFile(bytes); await file.sync(); } finally { await file.close(); }
}
async function json(path, data) { await privateFile(path, `${JSON.stringify(data, null, 2)}\n`); }
function command(bin, cwd, request) {
  const result = spawnSync(bin, [], { cwd, input: JSON.stringify(request), encoding: "utf8", timeout: 60_000 });
  if (result.status !== 0) throw new Error("installed operator preparation failed; inspect retained state");
  const reply = JSON.parse(result.stdout);
  if (reply.status !== "ok") throw new Error("operator preparation refused");
  return reply;
}

try {
  if (process.argv.length !== 8) throw new Error("requires deployment, bundle, manifest pin, pool name, new workdir and installed bin directory");
  const [deploymentFile, bundle, pin, poolName, workdirArg, binArg] = process.argv.slice(2);
  const amounts = liveAssetAmounts(poolName);
  const directory = resolve(workdirArg);
  const bins = resolve(binArg);
  const deployment = JSON.parse(await readFile(deploymentFile, "utf8"));
  const pool = deployment.pools.find((p) => p.name === poolName);
  const manifestBytes = await readFile(resolve(bundle, "manifest.json"));
  const manifest = JSON.parse(manifestBytes);
  if (!/^[0-9a-f]{64}$/.test(pin) || hash(manifestBytes) !== pin || deployment.chain_id !== 10143
      || deployment.test_only !== true || !pool || manifest.chain_id !== 10143 || !manifest.test_only
      || manifest.settlement_contract !== pool.pool.toLowerCase() || manifest.verifier_version !== 2
      || manifest.circuits.some((c) => c.identity.verifier_runtime_keccak256 !== pool.runtime_pins[c.kind])) {
    throw new Error("pool-bound manifest and deployment mismatch");
  }
  await mkdir(directory, { mode: 0o700 });
  const namespace = "eip155:10143";
  const asset = `${namespace}/erc20:${pool.asset.toLowerCase()}`;
  const contract = pool.pool.toLowerCase();
  const endpoint = "127.0.0.1:18731";
  const expiry = Math.floor(Date.now() / 1000) + 21600;
  const rpc = "https://testnet-rpc.monad.xyz";
  const peerRpc = "https://rpc.ankr.com/monad_testnet";
  const deposit = amounts.deposit;
  const price = amounts.price;
  const payload = Buffer.from(JSON.stringify({ service: "erebus.testnet.data-snapshot.v1", chain_id: 10143,
    verified_deployment_anchor: deployment.anchor, asset: pool.asset, pool: pool.pool, test_only: true }));
  const fulfillment = hash(payload);
  const operation = randomBytes(32).toString("hex");
  const serviceId = randomBytes(32);
  const common = { version: 1, namespace, pool: contract, verifier_version: 2, asset: pool.asset.toLowerCase(),
    wrapper: poolName === "wmon" ? pool.asset.toLowerCase() : null,
    rpc_url: rpc, peer_rpc_url: peerRpc, first_block: pool.deployment_block, first_hash: pool.deployment_hash,
    artifact_manifest_file: resolve(bundle, "manifest.json"), artifact_manifest_sha256: pin,
    runtime_pins: pool.runtime_pins, allow_test_artifacts: true, allow_loopback_artifact_http: false,
    gas_limit: 3000000, max_fee_per_gas: "300000000000", max_priority_fee_per_gas: "2000000000",
    timeout_seconds: 30, max_scan_blocks: 1000, artifact_timeout_seconds: 300 };
  const operators = {};
  for (const role of ["buyer", "seller"]) {
    const dir = resolve(directory, role);
    const setup = command(resolve(bins, "erebus-negotiate"), directory, { method: "prepare_operator", suite_id: 2,
      directory: dir, role, endpoint, namespace, assets: [asset], descriptor_lifetime_seconds: 86400 });
    command(resolve(bins, "erebus-negotiate"), directory, { method: "prepare_binding", output: resolve(dir, "identity.binding"),
      descriptor_file: setup.descriptor_file, discovery_key_file: setup.discovery_key_file, agreement_key_file: setup.agreement_key_file,
      settlement_contract: contract, verifier_version: 2, expires_at: expiry });
    const gas = Wallet.createRandom();
    await privateFile(resolve(dir, "transaction.key"), Buffer.from(gas.privateKey.slice(2), "hex"));
    const state = resolve(dir, "state");
    const cache = resolve(state, "artifacts");
    await mkdir(cache, { mode: 0o700, recursive: true });
    for (const circuit of manifest.circuits) {
      for (const [field, name] of [["wasm", `${circuit.kind}_js/${circuit.kind}.wasm`], ["r1cs", `${circuit.kind}.r1cs`], ["zkey", `${circuit.kind}.zkey`]]) {
        const bytes = await readFile(resolve(bundle, name));
        if (bytes.length !== circuit[field].bytes || hash(bytes) !== circuit[field].sha256) throw new Error("artifact staging mismatch");
        await privateFile(resolve(cache, `${circuit[field].sha256}.artifact`), bytes);
      }
    }
    const walletConfig = { ...common, state_root: state, wallet_file: resolve(state, "wallet/notes.enc"),
      wallet_key_file: resolve(dir, "wallet.key"), agreement_key_file: setup.wallet_agreement_key_file,
      signer_address: gas.address.toLowerCase(), signer_journal_root: resolve(state, "signer-journal"),
      transaction_key_file: resolve(dir, "transaction.key"), artifact_cache_root: cache };
    await json(resolve(dir, "wallet.json"), walletConfig);
    operators[role] = { directory: dir, signer: gas.address.toLowerCase(), setup };
  }
  for (const [role, other] of [["buyer", "seller"], ["seller", "buyer"]]) {
    const dir = operators[role].directory;
    const otherDir = operators[other].directory;
    command(resolve(bins, "erebus-negotiate"), directory, { method: "prepare_terms", suite_id: 2, output: resolve(dir, "service.terms"), role,
      local_descriptor_file: operators[role].setup.descriptor_file, peer_descriptor_file: operators[other].setup.descriptor_file,
      local_binding_file: resolve(dir, "identity.binding"), peer_binding_file: resolve(otherDir, "identity.binding"),
      settlement_contract: contract, verifier_version: 2, asset, amount: role === "buyer" ? amounts.buyerOffer : price,
      resource: "dataset.snapshot.v1", unit: "snapshot", quantity: "1", fulfillment_method: "http-access-v1",
      fulfillment_digest: fulfillment, delivery_deadline: expiry });
    await json(resolve(dir, "negotiate.json"), { version: 1, role, state_root: resolve(dir, "state"),
      transport_key_file: resolve(dir, "transport.key"), agreement_key_file: resolve(dir, "agreement.key"),
      discovery_key_file: resolve(dir, "discovery.key"), local_descriptor_file: operators[role].setup.descriptor_file,
      peer_descriptor_file: operators[other].setup.descriptor_file, terms_template_file: resolve(dir, "service.terms"), endpoint,
      maximum_price: amounts.maximumPrice, minimum_price: amounts.minimumPrice, max_deal_lifetime_seconds: 21600, timeout_seconds: 30,
      ...(role === "seller" ? { seller_spend_secret_file: resolve(dir, "spend.key"), seller_wallet_file: resolve(dir, "state/wallet/notes.enc"),
        seller_wallet_key_file: resolve(dir, "wallet.key"), access_evidence_root: resolve(dir, "access-evidence") } : {}) });
  }
  const buyerDir = operators.buyer.directory;
  const buyerWallet = JSON.parse(await readFile(resolve(buyerDir, "wallet.json"), "utf8"));
  await json(resolve(buyerDir, "payment.json"), { version: 1, mode: "shielded", state_root: buyerWallet.state_root, namespace,
    pool: contract, verifier_version: 2, runtime_keccak256: pool.runtime_pins.pool, poseidon_keccak256: pool.runtime_pins.poseidon,
    deposit_verifier_keccak256: pool.runtime_pins.deposit, transfer_verifier_keccak256: pool.runtime_pins.transfer,
    withdraw_verifier_keccak256: pool.runtime_pins.withdraw, first_block: pool.deployment_block, first_hash: pool.deployment_hash,
    rpc_url: rpc, peer_rpc_url: peerRpc, buyer_key_hex: (await readFile(resolve(buyerDir, "agreement.public"))).toString("hex"), asset,
    signer_address: operators.buyer.signer, signer_journal_root: buyerWallet.signer_journal_root,
    transaction_key_file: buyerWallet.transaction_key_file, wallet_file: buyerWallet.wallet_file, wallet_key_file: buyerWallet.wallet_key_file,
    manifest_file: buyerWallet.artifact_manifest_file, manifest_sha256: pin, artifact_cache: buyerWallet.artifact_cache_root,
    allow_test_artifacts: true, allow_loopback_http: false, maximum_price: amounts.maximumPrice, gas_limit: common.gas_limit,
    max_fee_per_gas: common.max_fee_per_gas, max_priority_fee_per_gas: common.max_priority_fee_per_gas, timeout_seconds: 30, max_scan_blocks: 1000 });
  const sellerDir = operators.seller.directory;
  await mkdir(resolve(sellerDir, "access-evidence"), { mode: 0o700 });
  await privateFile(resolve(sellerDir, "payload.json"), payload);
  await json(resolve(sellerDir, "access.json"), { service_id: [...serviceId], seller_key: [...await readFile(resolve(sellerDir, "agreement.public"))],
    suite_id: 2, resource: "dataset.snapshot.v1", payload_file: resolve(sellerDir, "payload.json"),
    evidence_root: resolve(sellerDir, "access-evidence"), state_root: resolve(sellerDir, "access-state"), port: 18732,
    backend: { mode: "shielded", chain_id: 10143, pool: contract, rpc_url: rpc, peer_rpc_url: peerRpc,
      first_block: pool.deployment_block, first_hash: [...Buffer.from(pool.deployment_hash.slice(2), "hex")],
      runtime_keccak256: pool.runtime_pins.pool, poseidon_keccak256: pool.runtime_pins.poseidon,
      deposit_verifier_keccak256: pool.runtime_pins.deposit, transfer_verifier_keccak256: pool.runtime_pins.transfer,
      withdraw_verifier_keccak256: pool.runtime_pins.withdraw, max_concurrent_queries: 4 } });
  const auditor = resolve(directory, "auditor");
  await mkdir(auditor, { mode: 0o700 });
  const auditorKey = command(resolve(bins, "erebus-shielded-disclosure"), directory,
    { method: "keygen", key_file: resolve(auditor, "auditor.key") });
  await json(resolve(directory, "onboarding.json"), { operators: Object.fromEntries(Object.entries(operators).map(([role, op]) => [role, { signer: op.signer, setup: op.setup }])), auditor: auditorKey });
  await json(resolve(directory, "run.json"), { version: 1, chain_id: 10143, test_only: true, audited: false, operation_ref: operation,
    wrap_operation_ref: randomBytes(32).toString("hex"), service_id: serviceId.toString("hex"), pool: contract,
    asset: pool.asset.toLowerCase(), asset_decimals: amounts.decimals, deposit_amount: deposit, price, fulfillment_digest: fulfillment,
    manifest_sha256: pin, evidence_scope: "team-operated-installed-local-wheel-warm-artifacts", binaries: bins,
    gas_addresses: Object.fromEntries(Object.entries(operators).map(([role, op]) => [role, op.signer])) });
  console.log(JSON.stringify({ status: "prepared", chain_id: 10143, pool: contract, gas_addresses:
    Object.fromEntries(Object.entries(operators).map(([role, op]) => [role, op.signer])), live_transactions_sent: 0 }));
} catch { console.error("live operator preparation failed; retain partial state; nothing broadcast"); process.exitCode = 1; }
