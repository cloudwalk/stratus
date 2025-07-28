// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

contract TestConcurrentStorageIsolation {
    mapping(uint256 => uint256) public sharedMapping;
    uint256 public readCount;

    // Event for logging reads
    event MappingRead(uint256[] keys, uint256[] values, uint256 readIndex);

    // Event for logging writes
    event ValueWritten(uint256 key, uint256 newValue);

    // Long-running function that reads one key per iteration
    // This will be called via eth_call
    function multipleReads(uint256 iterations) public view returns (uint256[] memory) {
        uint256[] memory allReads = new uint256[](iterations);

        for(uint256 i = 0; i < iterations; i++) {
            allReads[i] = sharedMapping[i];
        }

        return allReads;
    }

    // Function that writes to a specific key
    // This will be called via transaction
    function writeValue(uint256 key, uint256 newValue) public {
        sharedMapping[key] = newValue;
        emit ValueWritten(key, newValue);
    }

    // View function to get all values from fixed keys
    function getAllValues() public view returns (uint256[] memory) {
        uint256[] memory values = new uint256[](10);
        for (uint256 i = 0; i < 10; i++) {
            values[i] = sharedMapping[10];
        }
        return values;
    }
}
