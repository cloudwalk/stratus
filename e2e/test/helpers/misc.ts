const MINING_INTERVAL_PATTERN = /^(\d+)s$/;

export function checkTimeEnough(begTimeInMs: number, timeoutInMs: number) {
    const currentTimeInMs = Date.now();
    if (currentTimeInMs - begTimeInMs >= timeoutInMs) {
        throw new Error(
            "The time to properly check the conditions has expired. " +
            "Try increasing the block interval of the blockchain you are testing"
        );
    }
}

export function defineBlockMiningIntervalInMs(blockMintingModeTitle?: string): number | undefined {
    if (blockMintingModeTitle === "external") {
        return 0;
    } else {
        const regexpResults = MINING_INTERVAL_PATTERN.exec(blockMintingModeTitle ?? "");
        if (regexpResults && regexpResults.length > 1) {
            return parseInt(regexpResults[1]) * 1000;
        }
    }
    return undefined;
}
