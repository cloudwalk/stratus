// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.13;

import {Script} from "../lib/forge-std/src/Script.sol";
import {StdAssertions} from "../lib/forge-std/src/StdAssertions.sol";
import {Test} from "../lib/forge-std/src/Test.sol";
import {console} from "../lib/forge-std/src/console.sol";

import {TestContractBalances} from "../src/TestContractBalances.sol";

contract TestEof is Script, Test {
    function deploy() external {
        vm.startBroadcast();
        new TestContractBalances();
        vm.stopBroadcast();
    }

    function run() external {
        vm.startBroadcast();
        // for some reason foundry and stratus produce different contract addresses
        TestContractBalances c = TestContractBalances(0x5FbDB2315678afecb367f032d93F642f64180aa3);

        // verify EOF signature
        bytes4 sig = vm.getCode("TestContractBalances")[0] |
            (vm.getCode("TestContractBalances")[1] >> 8) |
            (vm.getCode("TestContractBalances")[2] >> 16) |
            (vm.getCode("TestContractBalances")[3] >> 24);
        assertEq(sig, '\xef\x00');

        c.add(0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266, 100);
        c.add(0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266, 100);
        c.sub(0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266, 50);
        uint256 value = c.get(0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266);
        assertEq(value, 150);

        vm.stopBroadcast();
    }
}
