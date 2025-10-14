// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

contract TestRWOperations {
    uint256 public value;

    // Allow contract to receive ETH
    receive() external payable {}

    function setValue(uint256 newValue) public {
        value = newValue;
    }

    function readValue() public view returns (uint256) {
        return value;
    }

    function readAndModifyOther(uint256 otherValue) public returns (uint256) {
        // Read the value (loads it into EVM)
        uint256 currentValue = value;

        // But don't modify value, modify something else
        // This simulates a transaction that reads a slot but doesn't write to it
        assembly {
            // Write to a different slot
            sstore(1, otherValue)
        }

        return currentValue;
    }
}
