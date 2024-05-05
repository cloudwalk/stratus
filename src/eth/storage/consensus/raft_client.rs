use raftpb::raft_pb_client::RaftPbClient;
use raftpb::AppendEntriesRequest;
use raftpb::InstallSnapshotRequest;
use raftpb::LogEntry;
use raftpb::VoteRequest;
use tonic::transport::Channel;

pub mod raftpb {
    tonic::include_proto!("raftpb");
}

pub struct RaftClient {
    client: RaftPbClient<Channel>,
}


impl RaftClient {
    pub async fn connect(addr: String) -> Result<Self, tonic::transport::Error> {
        let client = RaftPbClient::connect(addr).await?;
        Ok(RaftClient { client })
    }

    pub async fn vote(&mut self, term: i64, candidate_id: i64, last_log_index: i64, last_log_term: i64) -> Result<String, tonic::Status> {
        let request = VoteRequest {
            term,
            candidate_id,
            last_log_index,
            last_log_term,
        };
        let response = self.client.vote(tonic::Request::new(request)).await?;
        Ok(response.into_inner().message)
    }

    pub async fn append_entries(
        &mut self,
        term: i64,
        leader_id: i64,
        entries: Vec<LogEntry>,
        prev_log_index: i64,
        prev_log_term: i64,
        leader_commit: i64,
    ) -> Result<String, tonic::Status> {
        let request = AppendEntriesRequest {
            term,
            leader_id,
            entries,
            prev_log_index,
            prev_log_term,
            leader_commit,
        };
        let response = self.client.append_entries(tonic::Request::new(request)).await?;
        Ok(response.into_inner().message)
    }

    pub async fn install_snapshot(
        &mut self,
        term: i64,
        leader_id: i64,
        last_included_index: i64,
        last_included_term: i64,
        data: Vec<u8>,
    ) -> Result<String, tonic::Status> {
        let request = InstallSnapshotRequest {
            term,
            leader_id,
            last_included_index,
            last_included_term,
            data,
        };
        let response = self.client.install_snapshot(tonic::Request::new(request)).await?;
        Ok(response.into_inner().message)
    }
}
