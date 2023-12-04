// SPDX-License-Identifier: MIT
pragma solidity >=0.6.0;

contract test {

    mapping(address => uint) private counter;
    uint private total;

    constructor() {
        counter[msg.sender] = 999;
        total = 1001;
    }

    function inc() public returns (uint) {
        counter[msg.sender] += 1;
        total += 1;
        return 3;
    }

    function read() public pure returns (uint) {
        return 16;
    }
}