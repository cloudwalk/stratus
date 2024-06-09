import { expect } from "chai";
import { prepareSignedTx } from "./rpc";
import { Account } from "./account";
import { TestContractDenseStorage } from "../../typechain-types";

export interface AccountRecord {
    field16: bigint;
    reserve: bigint;
    field32: bigint;
    field64: bigint;
    field128: bigint;

    [key: string]: bigint;  // Index signature
}

export const initialRecord: AccountRecord = {
    field16: BigInt(2) ** BigInt(16 - 1), // 0x8000
    reserve: BigInt("0xABCD"),
    field32: BigInt(2) ** BigInt(32 - 1), // 0x8000_0000
    field64: BigInt(2) ** BigInt(64 - 1), // 0x8000_0000_0000_0000
    field128: BigInt(2) ** BigInt(128 - 1) // 0x8000_0000_0000_0000_0000_0000_0000_0000
};

export interface RecordChange {
    forField16: bigint;
    forField32: bigint;
    forField64: bigint;
    forField128: bigint;
}

export function defineRecordChange(index: number): RecordChange {
    let changeOfField16: bigint = BigInt(2) ** BigInt(16 - 6) - BigInt(index);
    if ((index & 1) !== 0) {
        changeOfField16 *= BigInt(-1);
    }
    let changeOfField32: bigint = BigInt(2) ** BigInt(32 - 6) - BigInt(33 * index);
    if ((index & 2) !== 0) {
        changeOfField32 *= BigInt(-1);
    }
    let changeOfField64: bigint = BigInt(2) ** BigInt(64 - 6) - BigInt(555 * index);
    if ((index & 4) !== 0) {
        changeOfField64 *= BigInt(-1);
    }
    let changeOfField128: bigint = BigInt(2) ** BigInt(128 - 6) - BigInt(7777 * index);
    if ((index & 8) !== 0) {
        changeOfField64 *= BigInt(-1);
    }

    return {
        forField16: changeOfField16,
        forField32: changeOfField32,
        forField64: changeOfField64,
        forField128: changeOfField128
    };
}

export function inverseRecordChange(change: RecordChange): RecordChange {
    return {
        forField16: -change.forField16,
        forField32: -change.forField32,
        forField64: -change.forField64,
        forField128: -change.forField128,
    };
}

export function applyRecordChange(
    record: AccountRecord,
    change: RecordChange,
) {
    record.field16 += change.forField16;
    record.field32 += change.forField32;
    record.field64 += change.forField64;
    record.field128 += change.forField128;
}

export function compareRecords(actualRecord: any, expectedRecord: AccountRecord) {
    Object.keys(expectedRecord).forEach(property => {
        expect(actualRecord[property]).to.eq(
            expectedRecord[property],
            `Mismatch in the "${property}" property of the storage record`
        );
    });
}

export async function prepareSignedTxOfRecordChange(
    contract: TestContractDenseStorage,
    sender: Account,
    targetAccount: Account,
    recordChange: RecordChange
): Promise<string> {
    return await prepareSignedTx({
        contract,
        account: sender,
        methodName: "change",
        methodParameters: [
            targetAccount.address,
            recordChange.forField16,
            recordChange.forField32,
            recordChange.forField64,
            recordChange.forField128
        ]
    });
}