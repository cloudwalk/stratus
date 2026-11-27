//! Single source of truth for every JSON-RPC method name exposed by the server.
//!
//! The serialized value of each variant is the exact wire name, so parsing a request method and
//! registering a handler can never drift apart: both are driven by this enum and guarded by the
//! `register_methods_matches_enum` test in `server.rs`.

use strum::EnumIter;
use strum::EnumString;
use strum::IntoStaticStr;

/// Every RPC method handled by the Stratus JSON-RPC server.
///
/// Registration in `server::register_methods` loops over all variants of this enum and the compiler
/// enforces an arm for each one, so a new variant cannot be introduced without being registered
/// (or explicitly marked as a skip). The wire name is declared once, on the variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, EnumIter, IntoStaticStr)]
pub enum RpcMethod {
    // middleware-intercepted methods
    #[strum(serialize = "eth_sendRawTransaction")]
    EthSendRawTransaction,
    #[strum(serialize = "stratus_sendRawTransactionAndGetReceipt")]
    StratusSendRawTransactionAndGetReceipt,

    // stratus health
    #[strum(serialize = "stratus_health")]
    StratusHealth,

    // dev mode methods
    #[cfg(feature = "dev")]
    #[strum(serialize = "evm_setNextBlockTimestamp")]
    EvmSetNextBlockTimestamp,
    #[cfg(feature = "dev")]
    #[strum(serialize = "evm_mine")]
    EvmMine,
    #[cfg(feature = "dev")]
    #[strum(serialize = "hardhat_reset")]
    HardhatReset,
    #[cfg(feature = "dev")]
    #[strum(serialize = "stratus_reset")]
    StratusReset,
    #[cfg(feature = "dev")]
    #[strum(serialize = "hardhat_setStorageAt")]
    HardhatSetStorageAt,
    #[cfg(feature = "dev")]
    #[strum(serialize = "stratus_setStorageAt")]
    StratusSetStorageAt,
    #[cfg(feature = "dev")]
    #[strum(serialize = "hardhat_setNonce")]
    HardhatSetNonce,
    #[cfg(feature = "dev")]
    #[strum(serialize = "stratus_setNonce")]
    StratusSetNonce,
    #[cfg(feature = "dev")]
    #[strum(serialize = "hardhat_setBalance")]
    HardhatSetBalance,
    #[cfg(feature = "dev")]
    #[strum(serialize = "stratus_setBalance")]
    StratusSetBalance,
    #[cfg(feature = "dev")]
    #[strum(serialize = "hardhat_setCode")]
    HardhatSetCode,
    #[cfg(feature = "dev")]
    #[strum(serialize = "stratus_setCode")]
    StratusSetCode,
    #[cfg(feature = "dev")]
    #[strum(serialize = "stratus_clearCache")]
    StratusClearCache,

    // stratus admin
    #[strum(serialize = "stratus_enableTransactions")]
    StratusEnableTransactions,
    #[strum(serialize = "stratus_disableTransactions")]
    StratusDisableTransactions,
    #[strum(serialize = "stratus_enableMiner")]
    StratusEnableMiner,
    #[strum(serialize = "stratus_disableMiner")]
    StratusDisableMiner,
    #[strum(serialize = "stratus_enableUnknownClients")]
    StratusEnableUnknownClients,
    #[strum(serialize = "stratus_disableUnknownClients")]
    StratusDisableUnknownClients,
    #[strum(serialize = "stratus_blockClient")]
    StratusBlockClient,
    #[strum(serialize = "stratus_unblockClient")]
    StratusUnblockClient,
    #[strum(serialize = "stratus_clearBlockedClients")]
    StratusClearBlockedClients,
    #[strum(serialize = "stratus_enableRestartOnUnhealthy")]
    StratusEnableRestartOnUnhealthy,
    #[strum(serialize = "stratus_disableRestartOnUnhealthy")]
    StratusDisableRestartOnUnhealthy,
    #[strum(serialize = "stratus_changeToLeader")]
    StratusChangeToLeader,
    #[strum(serialize = "stratus_changeToFollower")]
    StratusChangeToFollower,
    #[strum(serialize = "stratus_initImporter")]
    StratusInitImporter,
    #[strum(serialize = "stratus_shutdownImporter")]
    StratusShutdownImporter,
    #[strum(serialize = "stratus_changeMinerMode")]
    StratusChangeMinerMode,
    #[strum(serialize = "stratus_emitBlockEvents")]
    StratusEmitBlockEvents,

    // stratus state
    #[strum(serialize = "stratus_version")]
    StratusVersion,
    #[strum(serialize = "stratus_config")]
    StratusConfig,
    #[strum(serialize = "stratus_state")]
    StratusState,

    #[strum(serialize = "stratus_getSubscriptions")]
    StratusGetSubscriptions,
    #[strum(serialize = "stratus_pendingTransactionsCount")]
    StratusPendingTransactionsCount,

    // blockchain
    #[strum(serialize = "net_version")]
    NetVersion,
    #[strum(serialize = "net_listening")]
    NetListening,
    #[strum(serialize = "eth_chainId")]
    EthChainId,
    #[strum(serialize = "web3_clientVersion")]
    Web3ClientVersion,

    // gas
    #[strum(serialize = "eth_gasPrice")]
    EthGasPrice,

    // stratus importing helpers
    // dispatched to the dedicated exporter runtime, so importer requests cannot stall behind a
    // saturated main blocking pool (which makes followers fall behind)
    #[strum(serialize = "stratus_getBlockAndReceipts")]
    StratusGetBlockAndReceipts,
    #[strum(serialize = "stratus_getBlockWithChanges")]
    StratusGetBlockWithChanges,

    // block
    // `eth_blockNumber` is on the follower sync critical path (polled every sync interval), so it
    // is also dispatched to the dedicated exporter runtime
    #[strum(serialize = "eth_blockNumber")]
    EthBlockNumber,
    #[strum(serialize = "eth_getBlockByNumber")]
    EthGetBlockByNumber,
    #[strum(serialize = "eth_getBlockByHash")]
    EthGetBlockByHash,
    #[strum(serialize = "stratus_getBlockByTimestamp")]
    StratusGetBlockByTimestamp,
    #[strum(serialize = "eth_getUncleByBlockHashAndIndex")]
    EthGetUncleByBlockHashAndIndex,

    // transactions
    #[strum(serialize = "eth_getTransactionByHash")]
    EthGetTransactionByHash,
    #[strum(serialize = "eth_getTransactionReceipt")]
    EthGetTransactionReceipt,
    #[strum(serialize = "eth_estimateGas")]
    EthEstimateGas,
    #[strum(serialize = "eth_call")]
    EthCall,
    #[strum(serialize = "stratus_call")]
    StratusCall,
    #[strum(serialize = "stratus_accessList")]
    StratusAccessList,
    #[strum(serialize = "stratus_getTransactionResult")]
    StratusGetTransactionResult,
    #[strum(serialize = "debug_traceTransaction")]
    DebugTraceTransaction,

    // logs
    #[strum(serialize = "eth_getLogs")]
    EthGetLogs,

    // account
    #[strum(serialize = "eth_accounts")]
    EthAccounts,
    #[strum(serialize = "eth_getTransactionCount")]
    EthGetTransactionCount,
    #[strum(serialize = "eth_getBalance")]
    EthGetBalance,
    #[strum(serialize = "eth_getCode")]
    EthGetCode,

    // storage
    #[strum(serialize = "eth_getStorageAt")]
    EthGetStorageAt,

    // subscriptions
    #[strum(serialize = "eth_subscribe")]
    EthSubscribe,
    #[strum(serialize = "eth_unsubscribe")]
    EthUnsubscribe,
}

impl RpcMethod {
    /// Exact JSON-RPC wire name of the method.
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    /// Whether `server::register_methods` registers this method on the `RpcModule`.
    ///
    /// `false` marks the middleware-intercepted transaction submission pair: both variants share a
    /// single handler running on the dedicated blocking pool, distinguished by a `with_receipt`
    /// flag, so they never reach module registration.
    pub const fn is_module_registered(self) -> bool {
        !matches!(self, Self::EthSendRawTransaction | Self::StratusSendRawTransactionAndGetReceipt)
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use strum::IntoEnumIterator;

    use super::RpcMethod;

    /// Every variant parses back from its own wire name, and no wire name fell back to the
    /// variant identifier, which would mean a missing `#[strum(serialize)]` attribute.
    #[test]
    fn rpc_method_wire_names_round_trip() {
        for method in RpcMethod::iter() {
            let name = method.as_str();
            assert_ne!(name, format!("{method:?}"), "wire name fell back to the variant identifier");
            assert_eq!(name.parse::<RpcMethod>(), Ok(method), "wire name does not round-trip: {name}");
        }
    }
}
