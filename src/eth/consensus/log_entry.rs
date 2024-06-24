/// Holds the log entries that are stored in the Raft log.
/// The log entries are either a BlockEntry or a TransactionExecution.
/// The LogEntry struct is used to store the index and term of the log entry.
use prost::bytes;
use prost::Message;

use super::append_entry::BlockEntry;
use super::append_entry::TransactionExecutionEntry;

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Default)]
pub enum LogEntryData {
    BlockEntry(BlockEntry),
    TransactionExecutionEntries(Vec<TransactionExecutionEntry>),
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
            LogEntryData::BlockEntry(header) => {
                prost::encoding::message::encode(1, header, buf);
            }
            LogEntryData::TransactionExecutionEntries(executions) => {
                for execution in executions {
                    prost::encoding::message::encode(2, execution, buf);
                }
            }
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
        match tag {
            1 => {
                if let LogEntryData::BlockEntry(ref mut header) = self {
                    prost::encoding::message::merge(wire_type, header, buf, ctx)
                } else {
                    let mut header = BlockEntry::default();
                    prost::encoding::message::merge(wire_type, &mut header, buf, ctx)?;
                    *self = LogEntryData::BlockEntry(header);
                    Ok(())
                }
            }
            2 => {
                let mut execution = TransactionExecutionEntry::default();
                prost::encoding::message::merge(wire_type, &mut execution, buf, ctx)?;
                if let LogEntryData::TransactionExecutionEntries(ref mut executions) = self {
                    executions.push(execution);
                } else {
                    *self = LogEntryData::TransactionExecutionEntries(vec![execution]);
                }
                Ok(())
            }
            _ => prost::encoding::skip_field(wire_type, tag, buf, ctx),
        }
    }

    fn encoded_len(&self) -> usize {
        match self {
            LogEntryData::BlockEntry(header) => prost::encoding::message::encoded_len(1, header),
            LogEntryData::TransactionExecutionEntries(executions) => {
                executions.iter().map(|execution| prost::encoding::message::encoded_len(2, execution)).sum()
            }
            LogEntryData::EmptyData => 0,
        }
    }

    fn clear(&mut self) {
        match self {
            LogEntryData::BlockEntry(header) => header.clear(),
            LogEntryData::TransactionExecutionEntries(executions) => executions.clear(),
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
        prost::encoding::message::encode(3, &self.data, buf); // Encode LogEntryData with the correct tag
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
            3 => prost::encoding::message::merge(wire_type, &mut self.data, buf, ctx),
            _ => prost::encoding::skip_field(wire_type, tag, buf, ctx),
        }
    }

    fn encoded_len(&self) -> usize {
        prost::encoding::uint64::encoded_len(1, &self.index) +
        prost::encoding::uint64::encoded_len(2, &self.term) +
        prost::encoding::message::encoded_len(3, &self.data)
    }

    fn clear(&mut self) {
        self.index = 0;
        self.term = 0;
        self.data.clear();
    }
}
