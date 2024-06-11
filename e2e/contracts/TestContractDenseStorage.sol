// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

contract TestContractDenseStorage {
    struct Record {
        uint16 field16;
        uint16 reserve;
        uint32 field32;
        uint64 field64;
        uint128 field128;
    }

    mapping(address => Record) public records;

    /// @dev Changes fields of the record for an account.
    /// @param changeOfField16 The value to change of the appropriate fields of the record.
    /// @param changeOfField32 The value to change of the appropriate fields of the record.
    /// @param changeOfField64 The value to change of the appropriate fields of the record.
    /// @param changeOfField128 The value to change of the appropriate fields of the record.
    function change(
        address account,
        int256 changeOfField16,
        int256 changeOfField32,
        int256 changeOfField64,
        int256 changeOfField128
    ) public {
        Record storage record = records[account];

        record.field16 = uint16(uint256(int256(uint256(record.field16)) + changeOfField16));
        record.field32 = uint32(uint256(int256(uint256(record.field32)) + changeOfField32));
        record.field64 = uint64(uint256(int256(uint256(record.field64)) + changeOfField64));
        record.field128 = uint128(uint256(int256(uint256(record.field128)) + changeOfField128));
    }

    /// @dev Sets the exact field values of the record for an an account.
    /// @param newRecord The new content of the record for the account.
    function set(address account, Record calldata newRecord) public {
        records[account] = newRecord;
    }

    /// @dev Gets the record of an account.
    /// @return The record of the account.
    function get(address account) public view returns (Record memory) {
        return records[account];
    }
}
