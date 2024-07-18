// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

contract TestContractBalances {

    mapping(address => uint256) public balances;

    event Add(address indexed account, uint amount);
    event Sub(address indexed account, uint amount);
    event Set(address indexed account, uint amount);

    /// @dev Add amount to the balance of an account.
    /// @return The new balance.
    function add(address account, uint256 amount) public returns (uint256) {
        balances[account] += amount;
        emit Add(account, amount);

        return balances[msg.sender];
    }

    /// @dev Subtract amount from the balance of an account.
    /// @return The new balance.
    function sub(address account, uint256 amount) public returns (uint256) {
        require(balances[account] >= amount, "Insufficient balance");

        balances[account] -= amount;
        emit Sub(account, amount);

        return balances[msg.sender];
    }

    /// @dev Sets the exact balance of an account.
    /// @return The new balance.
    function set(address account, uint256 amount) public returns (uint256) {
        balances[account] = amount;
        emit Set(account, amount);

        return balances[msg.sender];
    }

    /// @dev Get the balance of an account.
    /// @return The balance of the account.
    function get(address account) public view returns (uint256) {
        return balances[account];
    }
}