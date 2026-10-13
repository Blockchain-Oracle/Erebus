// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {TestBase} from "./TestBase.sol";
import {MockWMON} from "../src/MockWMON.sol";

/// @notice Allowance and backing checks for the local wrapper fixture, not Monad WMON qualification.
contract MockWMONTest is TestBase {
    MockWMON internal token;
    address internal spender = address(0xBEEF);
    address internal recipient = address(0xCAFE);

    receive() external payable {}

    function setUp() public {
        token = new MockWMON();
        vm.deal(address(this), 100);
        token.deposit{value: 100}();
    }

    function test_transferFrom_debits_allowance_without_changing_backing() public {
        token.approve(spender, 40);
        vm.prank(spender);
        assertTrue(token.transferFrom(address(this), recipient, 25));
        assertEq(token.allowance(address(this), spender), 15);
        assertEq(token.balanceOf(address(this)), 75);
        assertEq(token.balanceOf(recipient), 25);
        assertEq(token.totalSupply(), 100);
        assertEq(address(token).balance, 100);
    }

    function test_transferFrom_refuses_short_allowance_and_rolls_back_on_short_balance() public {
        token.approve(spender, 10);
        vm.prank(spender);
        vm.expectRevert();
        token.transferFrom(address(this), recipient, 11);
        assertEq(token.allowance(address(this), spender), 10);
        assertEq(token.balanceOf(address(this)), 100);
        assertEq(token.balanceOf(recipient), 0);
        token.approve(spender, 200);
        vm.prank(spender);
        vm.expectRevert();
        token.transferFrom(address(this), recipient, 101);
        assertEq(token.allowance(address(this), spender), 200);
        assertEq(token.balanceOf(address(this)), 100);
        assertEq(token.balanceOf(recipient), 0);
    }

    function test_transferFrom_preserves_unlimited_allowance_and_allows_owner_transfer() public {
        token.approve(spender, type(uint256).max);
        vm.prank(spender);
        assertTrue(token.transferFrom(address(this), recipient, 25));
        assertEq(token.allowance(address(this), spender), type(uint256).max);
        assertTrue(token.transferFrom(address(this), recipient, 25));
        assertEq(token.balanceOf(address(this)), 50);
        assertEq(token.balanceOf(recipient), 50);
        assertEq(token.totalSupply(), 100);
        assertEq(address(token).balance, 100);
    }

    /// @dev Mixed transitions and payout failures on the fixture, not deployed WMON evidence.
    function testFuzz_wrapper_sequences_preserve_backing_and_failed_burns(bytes32 entropy) public {
        vm.deal(address(this), 10_000);
        for (uint256 i; i < 64; ++i) {
            uint256 random = uint256(keccak256(abi.encode(entropy, i)));
            uint256 own = token.balanceOf(address(this));
            uint256 other = token.balanceOf(recipient);
            uint256 amount = ((random >> 8) % 32) + 1;
            uint256 action = random % 8;
            if (action == 0) {
                token.deposit{value: amount}();
            } else if (action == 1 && own > 0) {
                token.transfer(recipient, amount % own + 1);
            } else if (action == 2 && other > 0) {
                vm.prank(recipient);
                token.transfer(address(this), amount % other + 1);
            } else if (action == 3 && own > 0) {
                token.withdraw(amount % own + 1);
            } else if (action == 4 && own > 0) {
                uint256 moved = amount % own + 1;
                token.approve(spender, moved);
                vm.prank(spender);
                token.transferFrom(address(this), recipient, moved);
                assertEq(token.allowance(address(this), spender), 0);
            } else if (action == 5) {
                uint256 nativeBefore = address(this).balance;
                uint256 backingBefore = address(token).balance;
                token.setFailPayout(true);
                vm.expectRevert();
                token.withdraw(own == 0 ? 0 : amount % own + 1);
                token.setFailPayout(false);
                assertEq(token.balanceOf(address(this)), own);
                assertEq(address(this).balance, nativeBefore);
                assertEq(address(token).balance, backingBefore);
            } else if (action == 6) {
                vm.expectRevert();
                token.withdraw(own + 1);
            } else if (action == 7) {
                token.approve(spender, 0);
                vm.prank(spender);
                vm.expectRevert();
                token.transferFrom(address(this), recipient, 1);
                assertEq(token.allowance(address(this), spender), 0);
                assertEq(token.balanceOf(address(this)), own);
                assertEq(token.balanceOf(recipient), other);
            }
            assertEq(token.balanceOf(address(this)) + token.balanceOf(recipient), token.totalSupply());
            assertEq(token.totalSupply(), address(token).balance);
            assertEq(address(this).balance + address(token).balance, 10_100);
        }
    }
}
