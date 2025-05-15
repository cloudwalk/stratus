// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

contract TestEvmInput {
    uint256 public executions;
    uint256 public result;

    // Define the event for logging
    event ComputationStarted(uint256 blockNumber, bool isSlow);

    function heavyComputation() public {
    // Emit event with current block number
        // Check if this is the first execution
        if (block.number < 5000) {
            emit ComputationStarted(block.number, true);

            // Perform a time-consuming loop
            uint256 temp = 0;
            for(uint i = 0; i < 10000; i++) {
                for(uint j = 0; j < 100; j++) {
                    temp += i * j;
                    temp = temp % 1000000; // Prevent overflow
                }
            }
            result = temp;
        } else {
            emit ComputationStarted(block.number, false);
            result = 42;
        }
        executions += 1;
    }

    // View function to get current block number (for testing)
    function getCurrentBlock() public view returns (uint256) {
        return block.number;
    }

    // View function to get result
    function getExecutions() public view returns (uint256) {
        return executions;
    }
}
