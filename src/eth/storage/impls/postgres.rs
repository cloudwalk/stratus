use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;

use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;

use crate::eth::primitives::TransactionMined;
use crate::eth::storage::BlockNumberStorage;
use crate::eth::storage::EthStorage;
use crate::eth::EthError;

use crate::infra::postgres::Postgres;

impl EthStorage for Postgres {
    fn read_account(&self, address: &Address) -> Result<Account, EthError> {
        tracing::debug!(%address, "reading account");

        let rt = tokio::runtime::Handle::current();

        let account = rt
            .block_on(async {
                sqlx::query_as!(
                    Account,
                    r#"
                        SELECT 
                            address as "address: _", 
                            nonce as "nonce: _", 
                            balance as "balance: _",
                            bytecode as "bytecode: _"
                        FROM accounts
                    "#
                )
                .fetch_one(&self.connection_pool)
                .await
            })
            .map_err(|_| EthError::UnexpectedStorageError);

        account
    }
    fn read_slot(&self, address: &Address, slot_index: &SlotIndex) -> Result<Slot, EthError> {
        tracing::debug!(%address, %slot_index, "reading slot");

        let rt = tokio::runtime::Handle::current();

        let slot = rt
            .block_on(async {
                sqlx::query_as!(
                    Slot,
                    r#"
                        SELECT 
                            idx as "index: _", 
                            value as "value: _"
                        FROM account_slots
                    "#
                )
                .fetch_one(&self.connection_pool)
                .await
            })
            .map_err(|_| EthError::UnexpectedStorageError);

        slot
    }
    fn read_block(&self, number: &BlockNumber) -> Result<Option<Block>, EthError> {
        tracing::debug!(%number, "reading block");

        // let rt = Runtime::new().unwrap();
        // let row = rt
        //     .block_on(async {
        //         sqlx::query!(
        //             r#"
        //                 SELECT b.number, b.hash, b.transactions_root, b.created_at, b.gas as block_gas,
        //                     t.signer_address, t.gas as transaction_gas, t.address_from, t.address_to, t.input, t.idx_in_block
        //                 FROM blocks b
        //                 JOIN transactions t on b.number = t.block_number
        //             "#
        //         )
        //         .fetch_one(&self.connection_pool)
        //         .await
        //     })
        //     .unwrap();

        // let block_header = BlockHeader {
        //     number: row.number.into(),
        //     hash: row.hash.try_into().unwrap(),
        //     transactions_root: row.transactions_root.try_into().unwrap(),
        //     gas: row.gas.into(),
        //     bloom: Bloom::default(),
        //     created_at: DateTime::default(), //row.created_at,
        // };

        // let transaction_mined = TransactionMined {
        //     signer: row.signer_address.try_into().unwrap(),
        //     input: TransactionInput::default(), // row.input.try_into().unwrap(),
        //     execution: TransactionExecution { result: , output: , logs: , gas: , changes:  },
        //     index_in_block: row.idx_in_block.try_into().unwrap(),
        //     block_number: row.number.into(),
        //     block_hash: row.hash.try_into().unwrap(),
        // };

        // let block = Block {
        //     header: block_header,
        //     transactions: vec![transaction_mined],
        // };

        // Ok(Some(block))

        todo!()
    }
    fn read_mined_transaction(&self, hash: &Hash) -> Result<Option<TransactionMined>, EthError> {
        tracing::debug!(%hash, "reading transaction");
        todo!()
    }
    fn save_block(&self, _block: Block) -> Result<(), EthError> {
        todo!()
    }
}

impl BlockNumberStorage for Postgres {
    fn current_block_number(&self) -> Result<BlockNumber, EthError> {
        tracing::debug!("reading current block number");

        let rt = tokio::runtime::Handle::current();

        let row = rt
            .block_on(async {
                sqlx::query!(
                    r#"
                        SELECT CURRVAL('block_number_seq')
                    "#
                )
                .fetch_one(&self.connection_pool)
                .await
            })
            .map_err(|_| EthError::UnexpectedStorageError)?;

        if row.currval.is_none() {
            return Err(EthError::MissingStorageItem("block_number_seq"));
        }

        // safe unwrap, check above
        let block_number = BlockNumber(row.currval.unwrap().into());

        Ok(block_number)
    }

    fn increment_block_number(&self) -> Result<BlockNumber, EthError> {
        tracing::debug!("incrementing block number");

        let rt = tokio::runtime::Handle::current();

        let row = rt
            .block_on(async {
                sqlx::query!(
                    r#"
                        SELECT NEXTVAL('block_number_seq')
                    "#
                )
                .fetch_one(&self.connection_pool)
                .await
            })
            .map_err(|_| EthError::UnexpectedStorageError)?;

        if row.nextval.is_none() {
            return Err(EthError::MissingStorageItem("block_number_seq"));
        }

        // safe unwrap, check above
        let block_number = BlockNumber(row.nextval.unwrap().into());

        Ok(block_number)
    }
}
