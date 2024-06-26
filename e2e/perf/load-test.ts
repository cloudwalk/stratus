import { ALICE, randomAccounts } from "../test/helpers/account";
import { TX_PARAMS, deployTestContractBalances, sendRawTransaction } from "../test/helpers/rpc";

async function main(): Promise<void> {
    const contract = await deployTestContractBalances();

    var counter = 0;
    while (true) {
        if (counter % 1000 == 0) {
            console.log(counter);
        }

        const accounts = randomAccounts(10);
        for (const account of accounts) {
            counter++;
            let tx = await contract.add.populateTransaction(account.address, 1, { nonce: 0, ...TX_PARAMS });
            let signedTx = await account.signer().signTransaction(tx);
            await sendRawTransaction(signedTx);
        }
    }
}
main().catch((error) => {
    console.error("Unhandled error:", error);
    process.exit(1);
});
