// Adversarial probe for the exact circomlib EdDSAPoseidonVerifier used by transfer.circom.
//
// Compile first (from circuits/m5):
//   mkdir -p build/eddsa-probe && circom scripts/eddsa-probe.circom --r1cs --wasm \
//     -l node_modules -o build/eddsa-probe
// Then: node scripts/eddsa-probe.mjs
//
// The probe shows which key shapes the circuit accepts. It is evidence for the M5A-2
// proposal, not a proof system for the main circuits and not a release artifact.
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const root = fileURLToPath(new URL("../", import.meta.url));
const require = createRequire(import.meta.url);
const { buildEddsa, buildPoseidon } = require(resolve(root, "node_modules/circomlibjs/main.js"));
const snarkjs = require(resolve(root, "node_modules/snarkjs/build/main.cjs"));

const eddsa = await buildEddsa();
const poseidon = await buildPoseidon();
const F = eddsa.babyJub.F;
const p = eddsa.babyJub.p;
const l = eddsa.babyJub.subOrder;
const Base8 = eddsa.babyJub.Base8;
const mul = (point, scalar) => eddsa.babyJub.mulPointEscalar(point, scalar);
const add = (a, b) => eddsa.babyJub.addPoint(a, b);
const f = (x) => F.toObject(F.e(x)).toString();
const probe = process.env.EDDSA_PROBE ?? "eddsa-probe";
const wasm = resolve(root, `build/${probe}/${probe}_js/${probe}.wasm`);

async function witness(label, input) {
  try {
    await snarkjs.wtns.calculate(input, wasm, resolve(root, `build/${probe}/${label}.wtns`));
    console.log(label, "WITNESS_OK");
  } catch (error) {
    console.log(label, "WITNESS_REJECTED", String(error).split("\n")[0].slice(0, 100));
  }
}

const u = 7n;
const r = 11n;
const M = 12345n;
const A = mul(Base8, u);
const R8 = mul(Base8, r);
const h = F.toObject(poseidon([R8[0], R8[1], A[0], A[1], M]));
const S = (((r + 8n * h * u) % l) + l) % l;
await witness("valid", { Ax: f(A[0]), Ay: f(A[1]), R8x: f(R8[0]), R8y: f(R8[1]), S: S.toString(), M: M.toString() });

const T = [F.e(0n), F.e(p - 1n)];
const shifted = add(A, T);
const hs = F.toObject(poseidon([R8[0], R8[1], shifted[0], shifted[1], M]));
const Ss = (((r + 8n * hs * u) % l) + l) % l;
await witness("torsion_shifted", { Ax: f(shifted[0]), Ay: f(shifted[1]), R8x: f(R8[0]), R8y: f(R8[1]), S: Ss.toString(), M: M.toString() });

// Identity R8 with a signature the keyholder can produce: Rust accepts it (the dependency has
// no R8 identity check and erebus-core only rejects identity A), so the strict circuit must too.
await witness("identity_r8", { Ax: "20092560661213339045022877747484245238324772779820628739268223482659246842641", Ay: "12112450042127193446189577552007703839818242727902437791835414514847797088033", R8x: "0", R8y: "1", S: "790228551905763144678588445737794329074811803266296531245276789977379284457", M: "12345" });

await witness("identity_forgery", { Ax: "0", Ay: "1", R8x: f(Base8[0]), R8y: f(Base8[1]), S: "1", M: "0" });
await witness("order2_key", { Ax: "0", Ay: f(p - 1n), R8x: f(Base8[0]), R8y: f(Base8[1]), S: "1", M: "0" });
await witness("off_curve", { Ax: "1", Ay: "1", R8x: f(Base8[0]), R8y: f(Base8[1]), S: "1", M: "0" });
await witness("s_equal_suborder", { Ax: f(A[0]), Ay: f(A[1]), R8x: f(R8[0]), R8y: f(R8[1]), S: l.toString(), M: M.toString() });
await witness("s_zero", { Ax: f(A[0]), Ay: f(A[1]), R8x: f(R8[0]), R8y: f(R8[1]), S: "0", M: M.toString() });
