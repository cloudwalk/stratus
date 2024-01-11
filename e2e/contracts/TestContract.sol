// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

contract TestContract {

    mapping(address => uint256) public balances;

    event Add(address indexed from, uint amount);
    event Sub(address indexed from, uint amount);

    function add(uint256 amount) public returns (uint256) {
        balances[msg.sender] += amount;
        emit Add(msg.sender, amount);
        return balances[msg.sender];
    }

    function sub(uint256 amount) public returns (uint256) {
        require(balances[msg.sender] >= amount, "Insufficient balance");
        balances[msg.sender] -= amount;
        emit Sub(msg.sender, amount);
        return balances[msg.sender];
    }

    function get(address account) public view returns (uint256) {
        return balances[account];
    }
}