// Whole-chain consistency only. Human independence and attestations are external evidence.
import { createHash } from "node:crypto";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { hashFile } from "./build-artifact-bundle.mjs";
import { contributionHistory, readBoundedJsonBytes, validatePlan as validateStep, verifyStep } from "./verify-phase2-step.mjs";

const kinds = ["deposit", "transfer", "withdraw"];
const hashPattern = /^[0-9a-f]{64}$/;

function fields(value, names) {
  if (value === null || typeof value !== "object" || Array.isArray(value) || Object.keys(value).sort().join(",") !== [...names].sort().join(",")) throw new Error("invalid chain fields");
}

export function validatePlan(plan) {
  fields(plan, ["version", "test_only", "contributors", "ptau_sha256", "circuits"]);
  if (plan.version !== 1 || typeof plan.test_only !== "boolean" || typeof plan.ptau_sha256 !== "string" || !hashPattern.test(plan.ptau_sha256)) throw new Error("invalid chain identity");
  if (!Array.isArray(plan.contributors) || plan.contributors.length < 2 || plan.contributors.length > 8
      || plan.contributors.some((name) => typeof name !== "string" || !/^[A-Za-z0-9_.@ -]{1,64}$/.test(name) || name !== name.trim())
      || new Set(plan.contributors.map((name) => name.toLowerCase())).size !== plan.contributors.length) throw new Error("requires distinct contributor labels");
  if (!Array.isArray(plan.circuits) || plan.circuits.length !== 3 || kinds.some((kind) => plan.circuits.filter((circuit) => circuit?.kind === kind).length !== 1)) throw new Error("requires all three circuit chains");
  for (const circuit of plan.circuits) {
    fields(circuit, ["kind", "r1cs_sha256", "steps"]);
    if (typeof circuit.r1cs_sha256 !== "string" || !hashPattern.test(circuit.r1cs_sha256)
        || !Array.isArray(circuit.steps) || circuit.steps.length !== plan.contributors.length) throw new Error("invalid circuit chain");
    for (const record of circuit.steps) {
      fields(record, ["path", "sha256"]);
      if (typeof record.path !== "string" || !record.path || typeof record.sha256 !== "string" || !hashPattern.test(record.sha256)) throw new Error("invalid step record");
    }
  }
  return plan;
}

export function validateSequence(plan, circuit, steps) {
  if (steps.length !== plan.contributors.length) throw new Error("incomplete circuit sequence");
  for (let index = 0; index < steps.length; index++) {
    const step = validateStep(steps[index]);
    if (step.test_only !== plan.test_only || step.contributor !== plan.contributors[index]
        || step.r1cs.sha256 !== circuit.r1cs_sha256 || step.ptau.sha256 !== plan.ptau_sha256) throw new Error("step does not match ceremony pins or contributor order");
    if (index > 0 && step.before.sha256 !== steps[index - 1].after.sha256) throw new Error("disconnected contribution chain");
  }
}

async function loadRecord(record) {
  const bytes = await readBoundedJsonBytes(record.path, "step record identity mismatch");
  if (createHash("sha256").update(bytes).digest("hex") !== record.sha256) throw new Error("step record identity mismatch");
  try { return JSON.parse(bytes.toString("utf8")); }
  catch (error) {
    if (error instanceof SyntaxError) throw new Error("invalid step record JSON");
    throw error;
  }
}

export async function verifyChain(rawPlan) {
  const plan = validatePlan(structuredClone(rawPlan));
  const loaded = [];
  for (const circuit of plan.circuits) {
    const steps = await Promise.all(circuit.steps.map(loadRecord));
    validateSequence(plan, circuit, steps);
    loaded.push({ circuit, steps });
  }
  const receipts = [];
  for (const { circuit, steps } of loaded) {
    const checked = [];
    for (let index = 0; index < steps.length; index++) {
      const receipt = await verifyStep(steps[index]);
      if (receipt.contribution_count !== index + 1) throw new Error("chain must begin at a zero-contribution initial key");
      checked.push(receipt);
    }
    const initial = await contributionHistory(steps[0].before.path);
    if (initial.contributions.length !== 0) throw new Error("initial key contains unreviewed contributions");
    receipts.push({ kind: circuit.kind, r1cs_sha256: circuit.r1cs_sha256, final_zkey_sha256: steps.at(-1).after.sha256, steps: checked });
  }
  // Keep all file pins valid across the entire three-circuit verification.
  for (const { circuit, steps } of loaded) {
    for (let index = 0; index < steps.length; index++) {
      if (await hashFile(circuit.steps[index].path) !== circuit.steps[index].sha256) throw new Error("step record changed during verification");
      for (const key of ["r1cs", "ptau", "before", "after"]) {
        if (await hashFile(steps[index][key].path) !== steps[index][key].sha256) throw new Error("chain input changed during verification");
      }
    }
  }
  return { version: 1, consistent: true, test_only_declared: plan.test_only, contributors: plan.contributors,
    ptau_sha256: plan.ptau_sha256, circuits: receipts, trust_verified: false,
    note: "Consistency only. Contributor labels do not authenticate people, independence, destruction, phase-1 provenance or an audit." };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    if (process.argv.length !== 3) throw new Error("usage: node verify-phase2-chain.mjs <hash-pinned-chain-plan.json>");
    const path = process.argv[2];
    const bytes = await readBoundedJsonBytes(path, "invalid chain plan file");
    let plan;
    try { plan = JSON.parse(bytes.toString("utf8")); }
    catch (error) {
      if (error instanceof SyntaxError) throw new Error("invalid chain plan JSON");
      throw error;
    }
    console.log(JSON.stringify(await verifyChain(plan)));
    process.exit(0);
  } catch (error) { console.error(error.message); process.exit(1); }
}
