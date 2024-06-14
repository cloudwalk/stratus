/// Holds the log entries that are stored in the Raft log.
/// The log entries are either a BlockEntryData or a TransactionExecution.
/// The LogEntry struct is used to store the index and term of the log entry.
use prost::bytes;
use prost::Message;

use super::append_entry::BlockEntry;
use super::append_entry::TransactionExecutionEntry;

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Default)]
pub enum LogEntryData {
    BlockEntryData(BlockEntry),
    TransactionExecutionEntriesData(Vec<TransactionExecutionEntry>),
    #[default]
    EmptyData,
}

#[derive(Debug, Clone, Default)]
pub struct LogEntry {
    pub index: u64,
    pub term: u64,
    pub data: LogEntryData,
}

impl Message for LogEntryData {
    fn encode_raw<B>(&self, buf: &mut B)
    where
        B: bytes::BufMut,
    {
        match self {
            LogEntryData::BlockEntryData(header) => header.encode_raw(buf),
            LogEntryData::TransactionExecutionEntriesData(executions) =>
                for execution in executions {
                    execution.encode_raw(buf);
                },
            LogEntryData::EmptyData => {}
        }
    }

    fn merge_field<B>(
        &mut self,
        tag: u32,
        wire_type: prost::encoding::WireType,
        buf: &mut B,
        ctx: prost::encoding::DecodeContext,
    ) -> Result<(), prost::DecodeError>
    where
        B: bytes::Buf,
    {
        match self {
            LogEntryData::BlockEntryData(header) => header.merge_field(tag, wire_type, buf, ctx),
            LogEntryData::TransactionExecutionEntriesData(executions) => {
                for execution in executions {
                    execution.merge_field(tag, wire_type, buf, ctx.clone())?;
                }
                Ok(())
            }
            LogEntryData::EmptyData => Ok(()),
        }
    }

    fn encoded_len(&self) -> usize {
        match self {
            LogEntryData::BlockEntryData(header) => header.encoded_len(),
            LogEntryData::TransactionExecutionEntriesData(executions) => executions.iter().map(|execution| execution.encoded_len()).sum(),
            LogEntryData::EmptyData => 0,
        }
    }

    fn clear(&mut self) {
        match self {
            LogEntryData::BlockEntryData(header) => header.clear(),
            LogEntryData::TransactionExecutionEntriesData(executions) => {
                executions.clear();
            }
            LogEntryData::EmptyData => {}
        }
    }
}

impl Message for LogEntry {
    fn encode_raw<B>(&self, buf: &mut B)
    where
        B: bytes::BufMut,
    {
        prost::encoding::uint64::encode(1, &self.index, buf);
        prost::encoding::uint64::encode(2, &self.term, buf);
        self.data.encode_raw(buf);
    }

    fn merge_field<B>(
        &mut self,
        tag: u32,
        wire_type: prost::encoding::WireType,
        buf: &mut B,
        ctx: prost::encoding::DecodeContext,
    ) -> Result<(), prost::DecodeError>
    where
        B: bytes::Buf,
    {
        match tag {
            1 => prost::encoding::uint64::merge(wire_type, &mut self.index, buf, ctx),
            2 => prost::encoding::uint64::merge(wire_type, &mut self.term, buf, ctx),
            _ => self.data.merge_field(tag, wire_type, buf, ctx),
        }
    }

    fn encoded_len(&self) -> usize {
        prost::encoding::uint64::encoded_len(1, &self.index) + prost::encoding::uint64::encoded_len(2, &self.term) + self.data.encoded_len()
    }

    fn clear(&mut self) {
        self.index = 0;
        self.term = 0;
        self.data.clear();
    }
}
