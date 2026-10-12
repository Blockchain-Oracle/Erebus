"""Independent canonical-byte encoder for M1 fixtures, not a production SDK.

Uses only Python's standard library. Hashes and signatures remain covered by the
Rust suite's external primitive vectors; this check independently pins field order,
widths, optional tags, and length prefixes against the agreement specification.

It also recomputes the provisional metered access vectors (docs/metropolis-prepaid.md,
K2-K4) with a self-contained Keccak-256, because hashlib only offers padded SHA3-256.
"""

import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def uint(value: int | str, width: int) -> bytes:
    return int(value).to_bytes(width, "big")


def blob(value: bytes) -> bytes:
    return uint(len(value), 2) + value


def text(value: str) -> bytes:
    return blob(value.encode("utf-8"))


def key(value: str) -> bytes:
    return blob(bytes.fromhex(value))


def optional_key(value: str | None) -> bytes:
    return b"\x00" if value is None else b"\x01" + key(value)


def encode(terms: dict) -> bytes:
    domain = terms["domain"]
    service = terms["service"]
    bits = {"hidden-amount": 1, "hidden-recipient": 2,
            "agreement-bound-settlement": 4, "scoped-disclosure": 8}
    guarantees = 0
    for name in terms["requiredGuarantees"]:
        guarantees |= bits[name]
    return b"".join([
        uint(terms["protocolVersion"], 2), uint(terms["suiteId"], 2),
        text(domain["namespace"]), optional_key(domain["settlementContractHex"]),
        optional_key(domain["poolHex"]), uint(domain["verifierVersion"], 4),
        bytes.fromhex(terms["dealIdHex"]), uint(terms["revision"], 4),
        bytes.fromhex(terms["transcriptRootHex"]),
        key(terms["buyerAuthorizationKeyHex"]), key(terms["sellerAuthorizationKeyHex"]),
        key(terms["paymentRecipientHex"]), text(terms["asset"]),
        uint(terms["amount"], 16), uint(terms["expiry"], 8), uint(terms["fee"], 16),
        optional_key(terms["feeRecipientHex"]),
        uint({"public_bound": 1, "shielded": 2}[terms["settlementMode"]], 1),
        uint(guarantees, 4), bytes.fromhex(terms["settlementNonceHex"]),
        text(service["resource"]), uint(service["quantity"], 16), text(service["unit"]),
        key(service["accessRecipientHex"]), uint(service["deliveryDeadline"], 8),
        text(service["fulfillmentMethod"]), bytes.fromhex(service["fulfillmentDigestHex"]),
    ])


_ROUND_CONSTANTS = [
    0x0000000000000001, 0x0000000000008082, 0x800000000000808A, 0x8000000080008000,
    0x000000000000808B, 0x0000000080000001, 0x8000000080008081, 0x8000000000008009,
    0x000000000000008A, 0x0000000000000088, 0x0000000080008009, 0x000000008000000A,
    0x000000008000808B, 0x800000000000008B, 0x8000000000008089, 0x8000000000008003,
    0x8000000000008002, 0x8000000000000080, 0x000000000000800A, 0x800000008000000A,
    0x8000000080008081, 0x8000000000008080, 0x0000000080000001, 0x8000000080008008,
]
_ROTATIONS = [
    [0, 36, 3, 41, 18], [1, 44, 10, 45, 2], [62, 6, 43, 15, 61],
    [28, 55, 25, 21, 56], [27, 20, 39, 8, 14],
]
_MASK = (1 << 64) - 1


def _rotl(value: int, shift: int) -> int:
    return ((value << shift) | (value >> (64 - shift))) & _MASK if shift else value


def _keccak_f(lanes: list[list[int]]) -> None:
    for constant in _ROUND_CONSTANTS:
        parity = [lanes[x][0] ^ lanes[x][1] ^ lanes[x][2] ^ lanes[x][3] ^ lanes[x][4]
                  for x in range(5)]
        for x in range(5):
            mix = parity[(x - 1) % 5] ^ _rotl(parity[(x + 1) % 5], 1)
            for y in range(5):
                lanes[x][y] ^= mix
        moved = [[0] * 5 for _ in range(5)]
        for x in range(5):
            for y in range(5):
                moved[y][(2 * x + 3 * y) % 5] = _rotl(lanes[x][y], _ROTATIONS[x][y])
        for x in range(5):
            for y in range(5):
                lanes[x][y] = moved[x][y] ^ ((~moved[(x + 1) % 5][y]) & moved[(x + 2) % 5][y])
        lanes[0][0] ^= constant


def _sponge(data: bytes, suffix: int) -> bytes:
    rate = 136
    pad = rate - len(data) % rate
    tail = bytes([suffix | 0x80]) if pad == 1 else bytes([suffix]) + b"\x00" * (pad - 2) + b"\x80"
    padded = data + tail
    lanes = [[0] * 5 for _ in range(5)]
    for offset in range(0, len(padded), rate):
        block = padded[offset:offset + rate]
        for index in range(rate // 8):
            lanes[index % 5][index // 5] ^= int.from_bytes(block[8 * index:8 * index + 8], "little")
        _keccak_f(lanes)
    return b"".join(lanes[index % 5][index // 5].to_bytes(8, "little") for index in range(4))


def keccak256(data: bytes) -> bytes:
    """Original Keccak-256 (suffix 0x01), as used by Ethereum, not NIST SHA3-256 (0x06)."""
    return _sponge(data, 0x01)


def check_keccak() -> None:
    # The permutation and padding are checked against hashlib's SHA3-256, which shares
    # them and differs only in the domain suffix, across the one-byte and multi-block paths.
    for length in (0, 1, 134, 135, 136, 137, 271, 272, 300):
        message = bytes(range(256)) * 2
        if _sponge(message[:length], 0x06) != hashlib.sha3_256(message[:length]).digest():
            raise ValueError(f"keccak sponge mismatch against SHA3-256 at length {length}")
    known = {
        b"": "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470",
        b"abc": "4e03657aea45a94fc7d47ba826c8d667c0d1e6e33a64a036ec44f58fa12d6c45",
    }
    for message, digest in known.items():
        if keccak256(message).hex() != digest:
            raise ValueError("keccak256 self-test failed")


def encode_domain(domain: dict) -> bytes:
    return b"".join([
        text(domain["namespace"]), optional_key(domain["settlementContractHex"]),
        optional_key(domain["poolHex"]), uint(domain["verifierVersion"], 4),
    ])


def metered_profile(max_response_bytes: int, params_mode: int) -> bytes:
    return (b"EREBUS_HTTP_METERED_PROFILE_V1" + uint(1, 2)
            + uint(max_response_bytes, 4) + uint(params_mode, 1))


def metered_request_digest(request: dict) -> bytes:
    operation = {"call": 1, "status": 2}[request["operation"]]
    return keccak256(b"".join([
        b"EREBUS_HTTP_METERED_REQUEST_V1", bytes.fromhex(request["domainHex"]),
        bytes.fromhex(request["serviceIdHex"]), bytes.fromhex(request["commitmentHex"]),
        bytes.fromhex(request["nullifierHex"]), bytes.fromhex(request["fulfillmentDigestHex"]),
        uint(operation, 1), uint(request["callIndex"], 8),
        bytes.fromhex(request["paramsDigestHex"]), bytes.fromhex(request["nonceHex"]),
        uint(request["expiresAt"], 8),
    ]))


def entitlement_id(service_id: bytes, commitment: bytes) -> bytes:
    return keccak256(b"EREBUS_METERED_ENTITLEMENT_V1" + service_id + commitment)


def call_id(entitlement: bytes, index: int) -> bytes:
    return keccak256(b"EREBUS_METERED_CALL_V1" + entitlement + uint(index, 8))


def metered_terms(agreement_vectors: list[dict], source: dict) -> tuple[dict, bytes]:
    """Applies the fixture's service overlay to a suite-1 agreement vector."""
    vector = agreement_vectors[source["agreementVector"]]
    terms = json.loads(json.dumps(vector["terms"]))
    terms["transcriptRootHex"] = source["transcriptRootHex"]
    terms["service"].update(source["service"])
    return terms, bytes.fromhex(vector["blindingHex"])


def check_metered(agreement_vectors: list[dict]) -> int:
    check_keccak()
    path = ROOT / "sdk/shielded/tests/fixtures/metered-v1-vectors.json"
    fixture = json.loads(path.read_text())
    checked = 0
    for profile in fixture["profiles"]:
        encoded = metered_profile(profile["maxResponseBytes"], profile["paramsMode"])
        if (encoded.hex() != profile["profileHex"]
                or hashlib.sha256(encoded).hexdigest() != profile["profileDigestHex"]):
            raise ValueError(f"metered profile mismatch: {profile['maxResponseBytes']}")
        checked += 1
    terms, blinding = metered_terms(agreement_vectors, fixture["terms"])
    if terms["suiteId"] != 1:
        raise ValueError("metered request vectors use suite-1 terms")
    domain = encode_domain(terms["domain"])
    commitment = keccak256(b"EREBUS_DEAL_COMMITMENT_V1" + encode(terms) + blinding)
    nullifier = keccak256(b"EREBUS_DEAL_NULLIFIER_V1" + domain
                          + bytes.fromhex(terms["buyerAuthorizationKeyHex"])
                          + bytes.fromhex(terms["settlementNonceHex"]))
    for request in fixture["requests"]:
        if (domain.hex() != request["domainHex"]
                or commitment.hex() != request["commitmentHex"]
                or nullifier.hex() != request["nullifierHex"]
                or terms["service"]["fulfillmentDigestHex"] != request["fulfillmentDigestHex"]):
            raise ValueError(f"metered request agreement mismatch: {request['name']}")
        if metered_request_digest(request).hex() != request["digestHex"]:
            raise ValueError(f"metered request digest mismatch: {request['name']}")
        checked += 1
    for entitlement in fixture["entitlements"]:
        service_id = bytes.fromhex(entitlement["serviceIdHex"])
        identity = entitlement_id(service_id, bytes.fromhex(entitlement["commitmentHex"]))
        if identity.hex() != entitlement["entitlementIdHex"]:
            raise ValueError("metered entitlement id mismatch")
        checked += 1
        for call in entitlement["calls"]:
            if call_id(identity, call["index"]).hex() != call["callIdHex"]:
                raise ValueError(f"metered call id mismatch: {call['index']}")
            checked += 1
    return checked


def main() -> None:
    path = ROOT / "sdk/core/tests/fixtures/agreement-v1-vectors.json"
    vectors = json.loads(path.read_text())["vectors"]
    if len(vectors) != 4:
        raise ValueError("expected three public-bound vectors and one shielded rejection vector")
    for vector in vectors:
        if encode(vector["terms"]).hex() != vector["expected"]["canonicalHex"]:
            raise ValueError(f"canonical encoding mismatch: {vector['name']}")
    print("4 independent canonical encodings match (3 valid, 1 rejected by Rust mode validation)")
    print(f"{check_metered(vectors)} independent metered access vectors match")


if __name__ == "__main__":
    main()
