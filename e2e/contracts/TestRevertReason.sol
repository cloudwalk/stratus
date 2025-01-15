// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

contract TestRevertReason {
    error KnownError();
    error UnknownError();

    function revertWithKnownError() public pure {
        revert KnownError();
    }

    function revertWithUnknownError() public pure {
        revert UnknownError();
    }

    function revertWithString() public pure {
        revert("Custom error message");
    }
}
