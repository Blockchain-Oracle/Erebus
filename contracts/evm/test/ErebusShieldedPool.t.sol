// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {ErebusShieldedPool} from "../src/ErebusShieldedPool.sol";
import {MockERC20} from "../src/MockERC20.sol";
import {MockReentrantERC20} from "../src/MockReentrantERC20.sol";
import {TestBase} from "./TestBase.sol";

/// @dev State-machine tests only. A permissive verifier cannot establish circuit soundness.
contract StateOnlyVerifier {
    bool public accepts = true;

    function setAccepts(bool value) external {
        accepts = value;
    }

    function verifyProof(uint256[2] calldata, uint256[2][2] calldata, uint256[2] calldata, uint256[6] calldata)
        external
        view
        returns (bool)
    {
        return accepts;
    }

    function verifyProof(uint256[2] calldata, uint256[2][2] calldata, uint256[2] calldata, uint256[11] calldata)
        external
        view
        returns (bool)
    {
        return accepts;
    }

    function verifyProof(uint256[2] calldata, uint256[2][2] calldata, uint256[2] calldata, uint256[8] calldata)
        external
        view
        returns (bool)
    {
        return accepts;
    }
}

/// @dev A deterministic field hash is enough to exercise the pool's tree bookkeeping.
contract StateOnlyPoseidon {
    uint256 private constant FIELD = 21888242871839275222246405745257275088548364400416034343698204186575808495617;

    function poseidon(uint256[2] calldata input) external pure returns (uint256) {
        return uint256(keccak256(abi.encode(input[0], input[1]))) % FIELD;
    }
}

contract ErebusShieldedPoolTest is TestBase {
    MockERC20 private token;
    ErebusShieldedPool private pool;
    StateOnlyVerifier private verifier;

    function setUp() public {
        token = new MockERC20("Test", "TEST");
        verifier = new StateOnlyVerifier();
        StateOnlyPoseidon hash = new StateOnlyPoseidon();
        pool = new ErebusShieldedPool(
            address(token), address(hash), address(verifier), address(verifier), address(verifier), 2
        );
        token.mint(address(this), 1_000_000);
        token.approve(address(pool), type(uint256).max);
    }

    function _deposit(uint256 amount, uint256 commitment) private {
        uint256[2] memory a;
        uint256[2][2] memory b;
        uint256[2] memory c;
        uint256[6] memory input;
        input[0] = block.chainid;
        input[1] = uint160(address(pool));
        input[2] = 2;
        input[3] = uint160(address(token));
        input[4] = amount;
        input[5] = commitment;
        pool.deposit(a, b, c, input);
    }

    function _depositInput(uint256 amount, uint256 commitment) private view returns (uint256[6] memory input) {
        input[0] = block.chainid;
        input[1] = uint160(address(pool));
        input[2] = 2;
        input[3] = uint160(address(token));
        input[4] = amount;
        input[5] = commitment;
    }

    function _transferInput(uint256 deal, uint256 noteNullifier, uint256 payment, uint256 change, uint256 root)
        private
        view
        returns (uint256[11] memory input)
    {
        input[0] = block.chainid;
        input[1] = uint160(address(pool));
        input[2] = 2;
        input[3] = uint160(address(token));
        input[4] = deal;
        input[5] = deal + 100;
        input[6] = root;
        input[7] = noteNullifier;
        input[8] = payment;
        input[9] = change;
        input[10] = block.timestamp + 100;
    }

    function _withdrawInput(uint256 nullifier, address recipient, uint256 amount)
        private
        view
        returns (uint256[8] memory input)
    {
        input[0] = block.chainid;
        input[1] = uint160(address(pool));
        input[2] = 2;
        input[3] = uint160(address(token));
        input[4] = pool.currentRoot();
        input[5] = nullifier;
        input[6] = uint160(recipient);
        input[7] = amount;
    }

    function _transferAtRoot(uint256 deal, uint256 noteNullifier, uint256 payment, uint256 change, uint256 root)
        private
    {
        uint256[2] memory a;
        uint256[2][2] memory b;
        uint256[2] memory c;
        uint256[11] memory input;
        input[0] = block.chainid;
        input[1] = uint160(address(pool));
        input[2] = 2;
        input[3] = uint160(address(token));
        input[4] = deal;
        input[5] = deal + 100;
        input[6] = root;
        input[7] = noteNullifier;
        input[8] = payment;
        input[9] = change;
        input[10] = block.timestamp + 100;
        pool.transferPrivate(a, b, c, input);
    }

    function _transfer(uint256 deal, uint256 noteNullifier, uint256 payment, uint256 change) private {
        _transferAtRoot(deal, noteNullifier, payment, change, pool.currentRoot());
    }

    function _withdraw(uint256 nullifier, address recipient, uint256 amount) private {
        uint256[2] memory a;
        uint256[2][2] memory b;
        uint256[2] memory c;
        uint256[8] memory input;
        input[0] = block.chainid;
        input[1] = uint160(address(pool));
        input[2] = 2;
        input[3] = uint160(address(token));
        input[4] = pool.currentRoot();
        input[5] = nullifier;
        input[6] = uint160(recipient);
        input[7] = amount;
        pool.withdraw(a, b, c, input);
    }

    function attemptDeposit(uint256 amount, uint256 commitment) external {
        _deposit(amount, commitment);
    }

    function attemptTransfer(uint256 deal, uint256 noteNullifier, uint256 payment, uint256 change) external {
        _transfer(deal, noteNullifier, payment, change);
    }

    function attemptWithdraw(uint256 nullifier, address recipient, uint256 amount) external {
        _withdraw(nullifier, recipient, amount);
    }

    function test_deposit_transfer_and_withdraw_update_pool_state_once() public {
        _deposit(150, 11);
        uint256 depositedRoot = pool.currentRoot();
        assertTrue(pool.knownRoots(depositedRoot));
        assertEq(pool.nextLeafIndex(), 1);
        assertEq(token.balanceOf(address(pool)), 150);

        _transfer(21, 31, 41, 51);
        assertEq(pool.nextLeafIndex(), 3);
        assertTrue(pool.consumedDeals(121));
        assertTrue(pool.consumedNotes(31));
        assertTrue(pool.insertedCommitments(41));
        assertTrue(pool.insertedCommitments(51));
        assertTrue(pool.knownRoots(pool.currentRoot()));
        assertEq(token.balanceOf(address(pool)), 150);
        vm.expectRevert();
        this.attemptTransfer(21, 32, 61, 71);
        vm.expectRevert();
        this.attemptTransfer(22, 31, 61, 71);
        vm.expectRevert();
        this.attemptDeposit(1, 41);

        address recipient = address(0xBEEF);
        _withdraw(42, recipient, 70);
        assertEq(token.balanceOf(recipient), 70);
        assertEq(token.balanceOf(address(pool)), 80);
        assertTrue(pool.consumedNotes(42));
        vm.expectRevert();
        this.attemptWithdraw(42, recipient, 1);
    }

    function test_rejected_proof_or_inexact_token_delta_never_consumes_state() public {
        verifier.setAccepts(false);
        vm.expectRevert();
        this.attemptDeposit(150, 11);
        assertEq(pool.nextLeafIndex(), 0);
        verifier.setAccepts(true);
        token.setFeeOnTransfer(true);
        vm.expectRevert();
        this.attemptDeposit(150, 11);
        assertEq(pool.nextLeafIndex(), 0);
        token.setFeeOnTransfer(false);
        _deposit(150, 11);

        verifier.setAccepts(false);
        vm.expectRevert();
        this.attemptTransfer(21, 31, 41, 51);
        assertFalse(pool.consumedDeals(121));
        assertEq(pool.nextLeafIndex(), 1);
        verifier.setAccepts(true);
        _transfer(21, 31, 41, 51);
        token.setFeeOnTransfer(true);
        vm.expectRevert();
        this.attemptWithdraw(42, address(0xBEEF), 70);
        assertFalse(pool.consumedNotes(42));
        assertEq(token.balanceOf(address(pool)), 150);
    }

    function _callbackPool() private returns (MockReentrantERC20 callbackToken) {
        callbackToken = new MockReentrantERC20();
        token = MockERC20(address(callbackToken));
        StateOnlyPoseidon hash = new StateOnlyPoseidon();
        pool = new ErebusShieldedPool(
            address(token), address(hash), address(verifier), address(verifier), address(verifier), 2
        );
        token.mint(address(this), 1_000_000);
        token.approve(address(pool), type(uint256).max);
    }

    function _reentrantTransfer() private view returns (bytes memory) {
        uint256[2] memory a;
        uint256[2][2] memory b;
        uint256[2] memory c;
        uint256[11] memory input = _transferInput(21, 31, 41, 51, pool.currentRoot());
        return abi.encodeCall(ErebusShieldedPool.transferPrivate, (a, b, c, input));
    }

    function test_deposit_callback_cannot_create_private_outputs_and_rolls_back_all_state() public {
        MockReentrantERC20 callbackToken = _callbackPool();
        uint256 root = pool.currentRoot();
        callbackToken.arm(address(pool), _reentrantTransfer());
        vm.expectRevert();
        this.attemptDeposit(150, 11);
        assertEq(pool.currentRoot(), root);
        assertEq(pool.nextLeafIndex(), 0);
        assertFalse(pool.insertedCommitments(11));
        assertFalse(pool.insertedCommitments(41));
        assertFalse(pool.insertedCommitments(51));
        assertFalse(pool.consumedNotes(31));
        assertFalse(pool.consumedDeals(121));
        assertEq(token.balanceOf(address(pool)), 0);
        assertEq(token.balanceOf(address(this)), 1_000_000);
        callbackToken.arm(address(0), "");
        _deposit(150, 11);
        assertEq(pool.nextLeafIndex(), 1);
        assertEq(token.balanceOf(address(pool)), 150);
    }

    function test_withdraw_callback_preserves_funds_nullifiers_and_tree_until_safe_retry() public {
        MockReentrantERC20 callbackToken = _callbackPool();
        _deposit(150, 11);
        uint256 root = pool.currentRoot();
        callbackToken.armTransfer(address(pool), _reentrantTransfer());
        vm.expectRevert();
        this.attemptWithdraw(42, address(0xBEEF), 70);
        assertEq(pool.currentRoot(), root);
        assertEq(pool.nextLeafIndex(), 1);
        assertFalse(pool.consumedNotes(42));
        assertFalse(pool.consumedNotes(31));
        assertFalse(pool.consumedDeals(121));
        assertFalse(pool.insertedCommitments(41));
        assertFalse(pool.insertedCommitments(51));
        assertEq(token.balanceOf(address(pool)), 150);
        assertEq(token.balanceOf(address(0xBEEF)), 0);
        callbackToken.armTransfer(address(0), "");
        _withdraw(42, address(0xBEEF), 70);
        assertTrue(pool.consumedNotes(42));
        assertEq(token.balanceOf(address(pool)), 80);
        assertEq(token.balanceOf(address(0xBEEF)), 70);
    }

    function test_multiple_deposits_keep_earlier_root_available_for_one_spend() public {
        _deposit(100, 11);
        uint256 firstRoot = pool.currentRoot();
        _deposit(80, 12);
        uint256 secondRoot = pool.currentRoot();
        assertTrue(firstRoot != secondRoot);
        assertTrue(pool.knownRoots(firstRoot));
        assertEq(pool.nextLeafIndex(), 2);
        assertEq(token.balanceOf(address(pool)), 180);

        _transferAtRoot(21, 31, 41, 51, firstRoot);
        assertEq(pool.nextLeafIndex(), 4);
        assertTrue(pool.consumedDeals(121));
        assertTrue(pool.consumedNotes(31));
        assertEq(token.balanceOf(address(pool)), 180);
        vm.expectRevert();
        this.attemptTransfer(21, 32, 61, 71);
    }

    function testFuzz_exact_vault_delta_for_supported_amounts(uint128 rawDeposit, uint128 rawWithdrawal) public {
        uint256 depositAmount = uint256(rawDeposit % 10_000) + 1;
        uint256 withdrawalAmount = uint256(rawWithdrawal % uint128(depositAmount)) + 1;
        _deposit(depositAmount, 11);
        _withdraw(42, address(0xBEEF), withdrawalAmount);
        assertEq(token.balanceOf(address(pool)), depositAmount - withdrawalAmount);
        assertEq(token.balanceOf(address(0xBEEF)), withdrawalAmount);
        assertEq(pool.nextLeafIndex(), 1);
    }

    /// @dev Stateful bookkeeping fuzzing with a permissive verifier, not proof soundness.
    function testFuzz_state_machine_conserves_vault_and_rejects_replays(bytes32 entropy) public {
        uint256 deposited;
        uint256 withdrawn;
        uint256 leaves;
        uint256[] memory commitments = new uint256[](128);
        assertEq(pool.currentRoot(), _referenceRoot(commitments, 0));
        for (uint256 i; i < 64; ++i) {
            uint256 random = uint8(uint256(keccak256(abi.encode(entropy, i))));
            uint256 rootBefore = pool.currentRoot();
            uint256 vault = deposited - withdrawn;
            uint256 action = random % 3;
            if (action == 0 || vault == 0) {
                uint256 amount = random + 1;
                uint256 commitment = 1000 + i * 10;
                _deposit(amount, commitment);
                deposited += amount;
                commitments[leaves++] = commitment;
                assertTrue(pool.insertedCommitments(commitment));
                vm.expectRevert();
                this.attemptDeposit(amount, commitment);
            } else if (action == 1) {
                uint256 deal = 2000 + i * 10;
                uint256 nullifier = 3000 + i * 10;
                uint256 payment = 4000 + i * 10;
                _transfer(deal, nullifier, payment, payment + 1);
                commitments[leaves++] = payment;
                commitments[leaves++] = payment + 1;
                assertTrue(pool.consumedDeals(deal + 100));
                assertTrue(pool.consumedNotes(nullifier));
                assertTrue(pool.insertedCommitments(payment));
                assertTrue(pool.insertedCommitments(payment + 1));
                vm.expectRevert();
                this.attemptTransfer(deal, nullifier, payment, payment + 1);
                vm.expectRevert();
                this.attemptTransfer(deal + 1, nullifier, payment + 2, payment + 3);
                assertFalse(pool.consumedDeals(deal + 101));
            } else {
                uint256 amount = (random % vault) + 1;
                uint256 nullifier = 5000 + i * 10;
                _withdraw(nullifier, address(0xBEEF), amount);
                withdrawn += amount;
                assertTrue(pool.consumedNotes(nullifier));
                vm.expectRevert();
                this.attemptWithdraw(nullifier, address(0xBEEF), amount);
            }
            assertEq(pool.nextLeafIndex(), leaves);
            assertEq(pool.currentRoot(), _referenceRoot(commitments, leaves));
            assertTrue(pool.knownRoots(rootBefore));
            assertTrue(pool.knownRoots(pool.currentRoot()));
            assertEq(token.balanceOf(address(pool)), deposited - withdrawn);
            assertEq(token.balanceOf(address(0xBEEF)), withdrawn);
            assertEq(token.balanceOf(address(this)) + token.balanceOf(address(pool)) + withdrawn, 1_000_000);
            if (i % 4 == 0) {
                uint256 retainedRoot = pool.currentRoot();
                verifier.setAccepts(false);
                vm.expectRevert();
                this.attemptTransfer(6000 + i * 10, 7000 + i * 10, 8000 + i * 10, 8001 + i * 10);
                verifier.setAccepts(true);
                assertEq(pool.currentRoot(), retainedRoot);
                assertEq(pool.nextLeafIndex(), leaves);
                assertFalse(pool.consumedDeals(6100 + i * 10));
                assertFalse(pool.consumedNotes(7000 + i * 10));
                assertEq(token.balanceOf(address(pool)), deposited - withdrawn);
                // Failed exact-delta calls must also roll back token supply and allowances.
                token.approve(address(pool), 17);
                token.setFeeOnTransfer(true);
                vm.expectRevert();
                this.attemptDeposit(17, 9000 + i * 10);
                assertFalse(pool.insertedCommitments(9000 + i * 10));
                assertEq(token.allowance(address(this), address(pool)), 17);
                uint256 failedNullifier = 10000 + i * 10;
                vm.expectRevert();
                this.attemptWithdraw(failedNullifier, address(0xBEEF), 1);
                assertFalse(pool.consumedNotes(failedNullifier));
                token.setFeeOnTransfer(false);
                token.approve(address(pool), type(uint256).max);
                assertEq(pool.currentRoot(), retainedRoot);
                assertEq(pool.nextLeafIndex(), leaves);
                assertEq(token.totalSupply(), 1_000_000);
                assertEq(token.balanceOf(address(pool)), deposited - withdrawn);
                assertEq(token.balanceOf(address(0xBEEF)), withdrawn);
                assertEq(token.balanceOf(address(this)), 1_000_000 - deposited);
            }
        }
    }

    /// @dev Rebuild from ordered leaves, without using the pool's frontier or zero hashes.
    function _referenceRoot(uint256[] memory commitments, uint256 count) private pure returns (uint256) {
        uint256[] memory level = new uint256[](commitments.length);
        for (uint256 i; i < count; ++i) {
            level[i] = commitments[i];
        }
        uint256 width = count == 0 ? 1 : count;
        uint256 zero;
        for (uint256 depth; depth < 20; ++depth) {
            uint256 parents = (width + 1) / 2;
            for (uint256 i; i < parents; ++i) {
                uint256 right = 2 * i + 1 < width ? level[2 * i + 1] : zero;
                level[i] = _referenceParent(level[2 * i], right);
            }
            zero = _referenceParent(zero, zero);
            width = parents;
        }
        return level[0];
    }

    function _referenceParent(uint256 left, uint256 right) private pure returns (uint256) {
        return uint256(keccak256(abi.encode(left, right)))
            % 21888242871839275222246405745257275088548364400416034343698204186575808495617;
    }

    function test_expired_or_wrong_deployment_transfer_never_consumes_state() public {
        _deposit(150, 11);
        uint256 root = pool.currentRoot();
        vm.warp(1_000_000);

        uint256[11] memory input = _transferInput(21, 31, 41, 51, root);
        input[10] = block.timestamp;
        (uint256[2] memory a, uint256[2][2] memory b, uint256[2] memory c) = _emptyProof();
        vm.expectRevert();
        pool.transferPrivate(a, b, c, input);
        assertFalse(pool.consumedDeals(121));
        assertFalse(pool.consumedNotes(31));

        input[10] = block.timestamp + 100;
        input[0] = block.chainid + 1;
        vm.expectRevert();
        pool.transferPrivate(a, b, c, input);
        input[0] = block.chainid;
        input[1] = 999;
        vm.expectRevert();
        pool.transferPrivate(a, b, c, input);
        input[1] = uint160(address(pool));
        input[2] = 3;
        vm.expectRevert();
        pool.transferPrivate(a, b, c, input);
        input[2] = 2;
        input[3] = 999;
        vm.expectRevert();
        pool.transferPrivate(a, b, c, input);
        input[3] = uint160(address(token));
        input[6] = 0xdead;
        vm.expectRevert();
        pool.transferPrivate(a, b, c, input);
        assertFalse(pool.consumedDeals(121));
        assertFalse(pool.consumedNotes(31));
        assertEq(pool.nextLeafIndex(), 1);

        input[6] = root;
        pool.transferPrivate(a, b, c, input);
        assertTrue(pool.consumedDeals(121));
        assertTrue(pool.consumedNotes(31));
    }

    function test_public_input_bounds_and_output_shape_never_consume_state() public {
        uint256[2] memory a;
        uint256[2][2] memory b;
        uint256[2] memory c;

        uint256[6] memory depositInput = _depositInput(0, 11);
        vm.expectRevert();
        pool.deposit(a, b, c, depositInput);
        depositInput = _depositInput(uint256(type(uint128).max) + 1, 11);
        vm.expectRevert();
        pool.deposit(a, b, c, depositInput);
        depositInput = _depositInput(150, 0);
        vm.expectRevert();
        pool.deposit(a, b, c, depositInput);
        depositInput = _depositInput(150, pool.FIELD_MODULUS());
        vm.expectRevert();
        pool.deposit(a, b, c, depositInput);
        assertEq(pool.nextLeafIndex(), 0);

        _deposit(150, 11);
        uint256 root = pool.currentRoot();
        uint256[11] memory transferInput = _transferInput(21, 31, 41, 41, root);
        vm.expectRevert();
        pool.transferPrivate(a, b, c, transferInput);
        transferInput = _transferInput(21, 31, 0, 51, root);
        vm.expectRevert();
        pool.transferPrivate(a, b, c, transferInput);
        transferInput = _transferInput(21, 31, 41, 0, root);
        vm.expectRevert();
        pool.transferPrivate(a, b, c, transferInput);
        assertFalse(pool.consumedDeals(121));
        assertFalse(pool.consumedNotes(31));

        uint256[8] memory withdrawInput = _withdrawInput(42, address(0xBEEF), 0);
        vm.expectRevert();
        pool.withdraw(a, b, c, withdrawInput);
        withdrawInput = _withdrawInput(42, address(0), 1);
        vm.expectRevert();
        pool.withdraw(a, b, c, withdrawInput);
        withdrawInput = _withdrawInput(42, address(0xBEEF), 1);
        withdrawInput[6] = uint256(1) << 160;
        vm.expectRevert();
        pool.withdraw(a, b, c, withdrawInput);
        withdrawInput = _withdrawInput(42, address(0xBEEF), 1);
        withdrawInput[5] = pool.FIELD_MODULUS();
        vm.expectRevert();
        pool.withdraw(a, b, c, withdrawInput);
        assertFalse(pool.consumedNotes(42));
        assertEq(token.balanceOf(address(pool)), 150);
    }

    function _emptyProof() private pure returns (uint256[2] memory a, uint256[2][2] memory b, uint256[2] memory c) {
        return (a, b, c);
    }
}
