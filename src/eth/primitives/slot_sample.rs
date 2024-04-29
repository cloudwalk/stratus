use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotValue;

#[derive(Debug, sqlx::Decode)]
pub struct SlotSample {
    pub address: Address,
    pub block_number: BlockNumber,
    pub index: SlotIndex,
    pub value: SlotValue,
}
