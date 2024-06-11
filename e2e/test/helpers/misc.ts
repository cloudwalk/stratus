export function checkTimeEnough(begTimeInMs: number, timeoutInMs: number) {
    const currentTimeInMs = Date.now();
    if (currentTimeInMs - begTimeInMs >= timeoutInMs) {
        throw new Error(
            "The time to properly check the conditions has expired. " +
            "Try increasing the block interval of the blockchain you are testing",
        );
    }
}
