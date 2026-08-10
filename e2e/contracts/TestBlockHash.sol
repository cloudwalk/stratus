// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

contract TestBlockHash {
    event BlockHashRecorded(uint256 blockNumber, bytes32 blockHash);

    mapping(uint256 => bytes32) public recordedBlockHashes;

    /// @dev Reads the BLOCKHASH opcode for an arbitrary block.
    /// @return The hash of the block, or zero when it is outside the 256 block window
    function getBlockHash(uint256 blockNumber) external view returns (bytes32) {
        return blockhash(blockNumber);
    }

    /// @dev Reads the BLOCKHASH opcode for the block being executed, which the EVM always answers with zero.
    /// @return The number of the block being executed and its hash
    function getCurrentBlockHash() external view returns (uint256, bytes32) {
        return (block.number, blockhash(block.number));
    }

    /// @dev Reads the BLOCKHASH opcode for the parent of the block being executed. The parent number is returned
    ///      along with the hash so that callers can resolve it without racing against newly mined blocks.
    /// @return The number and the hash of the parent block
    function getParentBlockHash() external view returns (uint256, bytes32) {
        uint256 parentNumber = block.number - 1;
        return (parentNumber, blockhash(parentNumber));
    }

    /// @dev Same as `getBlockHash`, but executed while mining a block instead of during a call.
    /// @return The hash of the block
    function recordBlockHash(uint256 blockNumber) external returns (bytes32) {
        bytes32 blockHash = blockhash(blockNumber);
        recordedBlockHashes[blockNumber] = blockHash;
        emit BlockHashRecorded(blockNumber, blockHash);
        return blockHash;
    }

    /// @dev Records the hash of the parent of the block that mines this transaction.
    /// @return The number and the hash of the parent block
    function recordParentBlockHash() external returns (uint256, bytes32) {
        uint256 parentNumber = block.number - 1;
        bytes32 blockHash = blockhash(parentNumber);
        recordedBlockHashes[parentNumber] = blockHash;
        emit BlockHashRecorded(parentNumber, blockHash);
        return (parentNumber, blockHash);
    }
}
