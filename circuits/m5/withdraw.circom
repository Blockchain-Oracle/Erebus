pragma circom 2.2.3;

include "circomlib/circuits/poseidon.circom";
include "circomlib/circuits/bitify.circom";

template WithdrawNote(depth) {
    signal input chainId;
    signal input contractAddress;
    signal input verifierVersion;
    signal input asset;
    signal input root;
    signal input noteNullifier;
    signal input recipient;
    signal input amount;

    signal input ownerAx;
    signal input ownerAy;
    signal input spendSecret;
    signal input salt;
    signal input pathElements[depth];
    signal input pathIndices[depth];
    signal input privateChainId;
    signal input privateContractAddress;
    signal input privateVerifierVersion;
    signal input privateRecipient;

    component chainBits = Num2Bits(64);
    chainBits.in <== chainId;
    component contractBits = Num2Bits(160);
    contractBits.in <== contractAddress;
    component versionBits = Num2Bits(32);
    versionBits.in <== verifierVersion;
    component assetBits = Num2Bits(160);
    assetBits.in <== asset;
    component recipientBits = Num2Bits(160);
    recipientBits.in <== recipient;
    component amountBits = Num2Bits(128);
    amountBits.in <== amount;
    component amountZero = IsZero();
    amountZero.in <== amount;
    amountZero.out === 0;
    component spendZero = IsZero();
    spendZero.in <== spendSecret;
    spendZero.out === 0;
    privateChainId === chainId;
    privateContractAddress === contractAddress;
    privateVerifierVersion === verifierVersion;
    privateRecipient === recipient;

    component spendTag = Poseidon(2);
    spendTag.inputs[0] <== 2006;
    spendTag.inputs[1] <== spendSecret;
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
    note.inputs[6] <== spendTag.out;
    note.inputs[7] <== salt;
    component nullifier = Poseidon(4);
    nullifier.inputs[0] <== 2011;
    nullifier.inputs[1] <== noteDomain.out;
    nullifier.inputs[2] <== spendSecret;
    nullifier.inputs[3] <== note.out;
    nullifier.out === noteNullifier;

    signal pathNode[depth + 1];
    component branch[depth];
    pathNode[0] <== note.out;
    for (var i = 0; i < depth; i++) {
        pathIndices[i] * (pathIndices[i] - 1) === 0;
        branch[i] = Poseidon(2);
        branch[i].inputs[0] <== pathNode[i] + pathIndices[i] * (pathElements[i] - pathNode[i]);
        branch[i].inputs[1] <== pathElements[i] + pathIndices[i] * (pathNode[i] - pathElements[i]);
        pathNode[i + 1] <== branch[i].out;
    }
    pathNode[depth] === root;
}

component main {public [chainId, contractAddress, verifierVersion, asset, root, noteNullifier, recipient, amount]} = WithdrawNote(20);
