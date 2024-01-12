import { Addressable, Signer, Wallet } from "ethers";

import { ETHERJS } from "./rpc";

export class Account implements Addressable {
    constructor(
        readonly address: string,
        readonly privateKey: string,
    ) {}

    getAddress(): Promise<string> {
        return Promise.resolve(this.address);
    }

    signer(): Signer {
        return new Wallet(this.privateKey, ETHERJS);
    }
}

// Used in RPC/native tests.
export const ALICE = new Account(
    "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
);

// Used in RPC/native tests.
export const BOB = new Account(
    "0x70997970c51812dc3a010c7d01b50e0d17dc79c8",
    "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d",
);

// Used in contracts tests.
export const CHARLIE = new Account(
    "0x3c44cdddb6a900fa2b585dd299e03d12fa4293bc",
    "0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a",
);
