pragma circom 2.2.3;
include "eddsa-strict.circom";

template EddsaStrictProbe() {
    signal input Ax;
    signal input Ay;
    signal input R8x;
    signal input R8y;
    signal input S;
    signal input M;

    component strict = EddsaStrictVerifier();
    strict.enabled <== 1;
    strict.Ax <== Ax;
    strict.Ay <== Ay;
    strict.R8x <== R8x;
    strict.R8y <== R8y;
    strict.S <== S;
    strict.M <== M;
}

component main = EddsaStrictProbe();
