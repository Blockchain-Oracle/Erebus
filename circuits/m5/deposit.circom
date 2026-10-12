pragma circom 2.2.3;

include "circomlib/circuits/poseidon.circom";
include "circomlib/circuits/bitify.circom";

template DepositNote() {
    signal input chainId;
    signal input contractAddress;
    signal input verifierVersion;
    signal input asset;
    signal input amount;
    signal input noteCommitment;

    signal input ownerAx;
    signal input ownerAy;
    signal input spendTag;
    signal input salt;
    signal input privateChainId;
    signal input privateContractAddress;
    signal input privateVerifierVersion;

    component chainBits = Num2Bits(64);
    chainBits.in <== chainId;
    component contractBits = Num2Bits(160);
    contractBits.in <== contractAddress;
    component versionBits = Num2Bits(32);
    versionBits.in <== verifierVersion;
    component assetBits = Num2Bits(160);
    assetBits.in <== asset;
    component amountBits = Num2Bits(128);
    amountBits.in <== amount;
    component amountZero = IsZero();
    amountZero.in <== amount;
    amountZero.out === 0;
    component spendTagZero = IsZero();
    spendTagZero.in <== spendTag;
    spendTagZero.out === 0;

    // Notes are bound to one deployment domain so identical openings cannot produce
    // identical commitments across chains, pools, or verifier versions.
    component noteDomain = Poseidon(3);
    noteDomain.inputs[0] <== chainId;
    noteDomain.inputs[1] <== contractAddress;
    noteDomain.inputs[2] <== verifierVersion;
    component note = Poseidon(8);
    note.inputs[0] <== 2010;
    note.inputs[1] <== noteDomain.out;
    note.inputs[2] <== asset;
    note.inputs[3] <== amount;
    note.inputs[4] <== ownerAx;
    note.inputs[5] <== ownerAy;
    note.inputs[6] <== spendTag;
    note.inputs[7] <== salt;
    note.out === noteCommitment;

    // The pool checks these against its deployment. These constraints also ensure the
    // public values cannot be changed while reusing an existing proof.
    privateChainId === chainId;
    privateContractAddress === contractAddress;
    privateVerifierVersion === verifierVersion;
}

component main {public [chainId, contractAddress, verifierVersion, asset, amount, noteCommitment]} = DepositNote();
