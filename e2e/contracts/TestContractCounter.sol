// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

contract TestContractCounter {
    uint256 counter;
    uint256 doubleCounter;

    function inc() public {
        counter += 1;
    }

    function double() public {
        doubleCounter += counter * 2;
    }

    function getCounter() public view returns (uint256) {
        return counter;
    }

    function getDoubleCounter() public view returns (uint256) {
        return doubleCounter;
    }
}
