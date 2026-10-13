// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

/// @notice Local WETH9-style wrapper used only for adversarial testing.
/// @dev Never deploy with real value and never treat this as Monad's WMON.
contract MockWMON {
    string public name = "Wrapped Monad Test";
    string public symbol = "WMON-TEST";
    uint8 public decimals = 18;
    uint256 public totalSupply;
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;
    /// @notice Adversarial switch: makes the native payout leg fail after burning.
    bool public failPayout;

    event Deposit(address indexed account, uint256 amount);
    event Withdrawal(address indexed account, uint256 amount);
    event Transfer(address indexed from, address indexed to, uint256 amount);
    event Approval(address indexed owner, address indexed spender, uint256 amount);

    receive() external payable {
        deposit();
    }

    function setFailPayout(bool value) external {
        failPayout = value;
    }

    function deposit() public payable {
        balanceOf[msg.sender] += msg.value;
        totalSupply += msg.value;
        emit Deposit(msg.sender, msg.value);
        emit Transfer(address(0), msg.sender, msg.value);
    }

    function withdraw(uint256 amount) external {
        require(balanceOf[msg.sender] >= amount, "insufficient wmon");
        balanceOf[msg.sender] -= amount;
        totalSupply -= amount;
        emit Withdrawal(msg.sender, amount);
        emit Transfer(msg.sender, address(0), amount);
        if (failPayout) {
            revert("native payout failed");
        }
        (bool ok,) = msg.sender.call{value: amount}("");
        require(ok, "native payout failed");
    }

    function transfer(address recipient, uint256 amount) external returns (bool) {
        _transfer(msg.sender, recipient, amount);
        return true;
    }

    function transferFrom(address sender, address recipient, uint256 amount) external returns (bool) {
        if (sender != msg.sender && allowance[sender][msg.sender] != type(uint256).max) {
            require(allowance[sender][msg.sender] >= amount, "insufficient allowance");
            allowance[sender][msg.sender] -= amount;
        }
        _transfer(sender, recipient, amount);
        return true;
    }

    function _transfer(address sender, address recipient, uint256 amount) private {
        require(balanceOf[sender] >= amount, "insufficient wmon");
        balanceOf[sender] -= amount;
        balanceOf[recipient] += amount;
        emit Transfer(sender, recipient, amount);
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        emit Approval(msg.sender, spender, amount);
        return true;
    }
}
