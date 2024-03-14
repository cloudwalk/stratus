import fs from 'fs';

const BALANCE_TRACKER_FILENAME : string = 'contracts/BalanceTracker.flattened.sol';

export function readTokenAddressFromSource() : string {
    let file = fs.readFileSync(BALANCE_TRACKER_FILENAME).toString();
    const regex = /address public constant TOKEN = address\((0x[0-9a-fA-F]{40})\);/
    const matches = file.match(regex) as RegExpMatchArray;
    const tokenAddress = matches[1];
    return tokenAddress;
}

export function replaceTokenAddress(oldAddress: string, newAddress: string) {
    let file = fs.readFileSync(BALANCE_TRACKER_FILENAME).toString();
    if (oldAddress === newAddress) {
        // nothing to do
        return;
    }
    fs.writeFileSync(BALANCE_TRACKER_FILENAME, file.replace(oldAddress, newAddress), {flag: 'w'});
}

export function recompile() {
    const execSync = require('child_process').execSync;
    execSync('npx hardhat compile');
}