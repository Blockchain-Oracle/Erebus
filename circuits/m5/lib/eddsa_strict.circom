pragma circom 2.2.3;

// Strict EdDSA verifier for candidate v2 (M5A-2), included by transfer.circom.
//
// Adds to the circomlib EdDSAPoseidonVerifier the constraints that match the Rust
// acceptance rules: on-curve A and R8, prime-order subgroup membership for both, a
// non-zero S, and the circomlib scalar bound and identity rejection it already has.
//
// This is candidate source, not a release circuit: it is uncommitted, its artifacts
// use known-entropy test keys, and it has not been audited. Compile from circuits/m5
// through scripts/pool.mjs, which passes `-l lib`.

include "circomlib/circuits/eddsaposeidon.circom";
include "circomlib/circuits/babyjub.circom";
include "circomlib/circuits/escalarmulany.circom";
include "circomlib/circuits/bitify.circom";

// subOrder = 2736030358979909402780800718157159386076813972158567259200215660948447373041
// Its 254-bit expansion is written bit by bit below with constant assignments.

template SubgroupCheck() {
    signal input p[2];
    signal output ok;

    component mul = EscalarMulAny(254);
    var subOrderBits[254];
    var i;
    var value = 2736030358979909402780800718157159386076813972158567259200215660948447373041;
    for (i = 0; i < 254; i++) {
        subOrderBits[i] = value & 1;
        value = value >> 1;
    }
    for (i = 0; i < 254; i++) {
        if (subOrderBits[i] == 1) {
            mul.e[i] <== 1;
        } else {
            mul.e[i] <== 0;
        }
    }
    mul.p[0] <== p[0];
    mul.p[1] <== p[1];

    // The result must be the identity (0, 1).
    component isXZero = IsZero();
    isXZero.in <== mul.out[0];
    component isYOne = IsEqual();
    isYOne.in[0] <== mul.out[1];
    isYOne.in[1] <== 1;
    ok <== isXZero.out * isYOne.out;
    ok === 1;
}

// The reusable strict verifier: the exact signal interface of EdDSAPoseidonVerifier plus
// on-curve, prime-order subgroup, and non-zero-S constraints.
template EddsaStrictVerifier() {
    signal input enabled;
    signal input Ax;
    signal input Ay;
    signal input R8x;
    signal input R8y;
    signal input S;
    signal input M;

    // circomlib: S < subOrder, identity/torsion rejection via 8A.x, equation.
    component verifier = EdDSAPoseidonVerifier();
    verifier.enabled <== enabled;
    verifier.Ax <== Ax;
    verifier.Ay <== Ay;
    verifier.R8x <== R8x;
    verifier.R8y <== R8y;
    verifier.S <== S;
    verifier.M <== M;

    // On-curve A and R8.
    component checkA = BabyCheck();
    checkA.x <== Ax;
    checkA.y <== Ay;
    component checkR8 = BabyCheck();
    checkR8.x <== R8x;
    checkR8.y <== R8y;

    // Prime-order subgroup membership for both points. Identity passes this check (as it does
    // in Rust); identity A is separately rejected by the circomlib 8A.x constraint while
    // identity R8 is deliberately allowed, matching the Rust acceptance set.
    component subgroupA = SubgroupCheck();
    subgroupA.p[0] <== Ax;
    subgroupA.p[1] <== Ay;
    component subgroupR8 = SubgroupCheck();
    subgroupR8.p[0] <== R8x;
    subgroupR8.p[1] <== R8y;

    // Non-zero scalar; the circomlib bound already enforces S < subOrder.
    component sZero = IsZero();
    sZero.in <== S;
    sZero.out === 0;
}
