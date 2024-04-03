import { Addressable, BigNumberish, Signer, Wallet } from "ethers";

import { CHAIN_ID_DEC, ETHERJS } from "./rpc";

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

    async signWeiTransfer(
        counterParty: string,
        amount: BigNumberish,
        nonce: number = 0,
        gasLimit: BigNumberish = 1_000_000,
    ): Promise<string> {
        return await this.signer().signTransaction({
            to: counterParty,
            value: amount,
            chainId: CHAIN_ID_DEC,
            gasPrice: 0,
            gasLimit: gasLimit,
            nonce,
        });
    }
}

export const ALICE = new Account(
    "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
);

export const BOB = new Account(
    "0x70997970c51812dc3a010c7d01b50e0d17dc79c8",
    "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d",
);

export const CHARLIE = new Account(
    "0x3c44cdddb6a900fa2b585dd299e03d12fa4293bc",
    "0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a",
);

export const DAVE = new Account(
    "0x15d34AAf54267DB7D7c367839AAf71A00a2C6A65",
    "0x47e179ec197488593b187f80a00eb0da91f1b9d0b13f8733639f19c30a34926a",
);

export const EVE = new Account(
    "0x9965507D1a55bcC2695C58ba16FB37d819B0A4dc",
    "0x8b3a350cf5c34c9194ca85829a2df0ec3153be0318b5e2d3348e872092edffba",
);

export const FERDIE = new Account(
    "0x976EA74026E726554dB657fA54763abd0C3a0aa9",
    "0x92db14e403b83dfe3df233f83dfa3a0d7096f21ca9b0d6d6b8d88b2b4ec1564e",
);

export const TEST_ACCOUNTS = [ALICE, BOB, CHARLIE, DAVE, EVE, FERDIE];

/// Generates N random accounts.
export function randomAccounts(n: number): Account[] {
    const wallets: Account[] = [];
    for (let i = 0; i < n; i++) {
        let wallet = Wallet.createRandom();
        wallets.push(new Account(wallet.address, wallet.privateKey));
    }
    return wallets;
}
