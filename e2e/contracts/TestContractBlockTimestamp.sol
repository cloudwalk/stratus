// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

contract TestContractBlockTimestamp {
    event TimestampRecorded(uint256 timestamp, uint256 blockNumber);
    
    struct TimeRecord {
        uint256 timestamp;
        uint256 blockNumber;
    }
    
    TimeRecord[] public records;

    /// @dev Records current block timestamp and number
    /// @return The recorded timestamp
    function recordTimestamp() public returns (uint256) {
        uint256 timestamp = block.timestamp;
        records.push(TimeRecord(timestamp, block.number));
        emit TimestampRecorded(timestamp, block.number);
        return timestamp;
    }
}