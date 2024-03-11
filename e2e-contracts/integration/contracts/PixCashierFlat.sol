// SPDX-License-Identifier: MIT

pragma solidity 0.8.16;

/**
 * @title PixCashier types interface
 */
interface IPixCashierTypes {
    /**
     * @dev Possible statuses of a cash-in operation as an enum.
     *
     * The possible values:
     * - Nonexistent - The operation does not exist (the default value).
     * - Executed ---- The operation was executed.
     */
    enum CashInStatus {
        Nonexistent, // 0
        Executed     // 1
    }

    /**
     * @dev Possible statuses of a cash-in batch operation as an enum.
     *
     * The possible values:
     * - Nonexistent - The operation does not exist (the default value).
     * - Executed ---- The operation was executed.
     */
    enum CashInBatchStatus {
        Nonexistent, // 0
        Executed     // 1
    }

    /**
     * @dev Possible result statuses of a cash-in operation as an enum.
     *
     * The possible values:
     * - Success --------- The operation was executed successfully.
     * - AlreadyExecuted - The operation was already executed.
     */
    enum CashInExecutionResult {
        Success,        // 0
        AlreadyExecuted // 1
    }

    /**
     * @dev Possible execution policies of a cash-in operation as an enum.
     *
     * The possible values:
     * - Revert - In case of failure the operation will be reverted.
     * - Skip --- In case of failure the operation will be skipped.
     */
    enum CashInExecutionPolicy {
        Revert, // 0
        Skip    // 1
    }

    /**
     * @dev Possible statuses of a cash-out operation as an enum.
     *
     * The possible values:
     * - Nonexistent - The operation does not exist (the default value).
     * - Pending ----- The status immediately after the operation requesting.
     * - Reversed ---- The operation was reversed.
     * - Confirmed --- The operation was confirmed.
     */
    enum CashOutStatus {
        Nonexistent, // 0
        Pending,     // 1
        Reversed,    // 2
        Confirmed    // 3
    }

    /// @dev Structure with data of a single cash-in operation.
    struct CashInOperation {
        CashInStatus status;  // The status of the cash-in operation according to the {CashInStatus} enum.
        address account;      // The owner of tokens to cash-in.
        uint256 amount;       // The amount of tokens to cash-in.
    }

    /// @dev Structure with data of a batch cash-in operation.
    struct CashInBatchOperation {
        CashInBatchStatus status;  // The status of the cash-in batch operation according to the {CashInBatchStatus}.
    }

    /// @dev Structure with data of a single cash-in operation.
    struct CashOut {
        address account;      // The owner of tokens to cash-out.
        uint256 amount;       // The amount of tokens to cash-out.
        CashOutStatus status; // The status of the cash-out operation according to the {CashOutStatus} enum.
    }
}

/**
 * @title PixCashier interface
 * @dev The interface of the wrapper contract for PIX cash-in and cash-out operations.
 */
interface IPixCashier is IPixCashierTypes {
    /// @dev Emitted when a new cash-in operation is executed.
    event CashIn(
        address indexed account, // The account that receives tokens.
        uint256 amount,          // The amount of tokens to receive.
        bytes32 indexed txId     // The off-chain transaction identifier.
    );

    /// @dev Emitted when a new batch of cash-in operations is executed.
    event CashInBatch(
        bytes32 indexed batchId,                 // The off-chain batch identifier.
        bytes32[] txIds,                         // The array of off-chain identifiers for each operation in the batch.
        CashInExecutionResult[] executionResults // The array of execution results for each operation in the batch.
    );

    /// @dev Emitted when a new cash-out operation is initiated.
    event RequestCashOut(
        address indexed account, // The account that owns the tokens to cash-out.
        uint256 amount,          // The amount of tokens to cash-out.
        uint256 balance,         // The new pending cash-out balance of the account.
        bytes32 indexed txId,    // The off-chain transaction identifier.
        address indexed sender   // The account that initiated the cash-out.
    );

    /// @dev Emitted when a cash-out operation is confirmed.
    event ConfirmCashOut(
        address indexed account, // The account that owns the tokens to cash-out.
        uint256 amount,          // The amount of tokens to cash-out.
        uint256 balance,         // The new pending cash-out balance of the account.
        bytes32 indexed txId     // The off-chain transaction identifier.
    );

    /// @dev Emitted when a cash-out operation is reversed.
    event ReverseCashOut(
        address indexed account, // The account that owns the tokens to cash-out.
        uint256 amount,          // The amount of tokens to cash-out.
        uint256 balance,         // The new pending cash-out balance of the account.
        bytes32 indexed txId     // The off-chain transaction identifier.
    );

    /**
     * @dev Returns the address of the underlying token.
     */
    function underlyingToken() external view returns (address);

    /**
     * @dev Returns the data of a single cash-in operation.
     * @param txId The off-chain transaction identifier of the operation.
     */
    function getCashIn(bytes32 txId) external view returns (CashInOperation memory);

    /**
     * @dev Returns the data of multiple cash-in operations.
     * @param txIds The off-chain transaction identifiers of the operations.
     */
    function getCashIns(bytes32[] memory txIds) external view returns (CashInOperation[] memory cashIns);

    /**
     * @dev Returns the data of a cash-in batch operation.
     * @param batchId The off-chain identifier of the cash-in batch operation.
     */
    function getCashInBatch(bytes32 batchId) external view returns (CashInBatchOperation memory);

    /**
     * @dev Returns the data of multiple cash-in batch operations.
     * @param batchIds The off-chain identifiers of the cash-in batch operations.
     */
    function getCashInBatches(
        bytes32[] memory batchIds
    ) external view returns (CashInBatchOperation[] memory cashInBatches);

    /**
     * @dev Returns the pending cash-out balance for an account.
     * @param account The address of the account.
     */
    function cashOutBalanceOf(address account) external view returns (uint256);

    /**
     * @dev Returns the pending cash-out operation counter.
     */
    function pendingCashOutCounter() external view returns (uint256);

    /**
     * @dev Returns the processed cash-out operation counter (reversed and confirmed operations included).
     */
    function processedCashOutCounter() external view returns (uint256);

    /**
     * @dev Returns the off-chain transaction identifiers of pending cash-out operations.
     *
     * No guarantees are made on the ordering of the identifiers in the returned array.
     * When you can't prevent confirming and reversing of cash-out operations during calling this function several
     * times to sequentially read of all available identifiers the following procedure is recommended:
     *
     * - 1. Call the `processedCashOutCounter()` function and remember the returned value as C1.
     * - 2. Call this function several times with needed values of `index` and `limit` like (0,5), (5,5), (10,5), ...
     * - 3. Execute step 2 until the length of the returned array becomes less than the `limit` value.
     * - 4. Call the `processedCashOutCounter()` function and remember the returned value as C2.
     * - 5. If C1 == C2 the result of function calls is consistent. Else repeat the procedure from step 1.
     * @param index The first index in the internal array of pending identifiers to fetch.
     * @param limit The maximum number of returned identifiers.
     * @return txIds The array of requested identifiers.
     */
    function getPendingCashOutTxIds(uint256 index, uint256 limit) external view returns (bytes32[] memory txIds);

    /**
     * @dev Returns the data of a single cash-out operation.
     * @param txId The off-chain transaction identifier of the operation.
     */
    function getCashOut(bytes32 txId) external view returns (CashOut memory);

    /**
     * @dev Returns the data of multiple cash-out operations.
     * @param txIds The off-chain transaction identifiers of the operations.
     */
    function getCashOuts(bytes32[] memory txIds) external view returns (CashOut[] memory cashOuts);

    /**
     * @dev Executes a cash-in operation.
     *
     * This function is expected to be called by a limited number of accounts
     * that are allowed to execute cash-in operations.
     *
     * Emits a {CashIn} event.
     *
     * @param account The address of the tokens recipient.
     * @param amount The amount of tokens to be received.
     * @param txId The off-chain transaction identifier of the operation.
     */
    function cashIn(
        address account,
        uint256 amount,
        bytes32 txId
    ) external;

    /**
     * @dev Executes a batch of cash-in operations.
     *
     * This function is expected to be called by a limited number of accounts
     * that are allowed to execute cash-in operations.
     *
     * Emits a {CashInBatch} event.
     * Emits a {CashIn} events.
     *
     * @param accounts The array of the addresses of the tokens recipient.
     * @param amounts The array of the token amounts to be received.
     * @param txIds The array of the off-chain transaction identifiers of the operation.
     * @param batchId The off-chain batch identifier.
     */
    function cashInBatch(
        address[] memory accounts,
        uint256[] memory amounts,
        bytes32[] memory txIds,
        bytes32 batchId
    ) external;

    /**
     * @dev Initiates a cash-out operation from some other account.
     *
     * Transfers tokens from the account to the contract.
     * This function is expected to be called by a limited number of accounts
     * that are allowed to process cash-out operations.
     *
     * Emits a {CashOut} event.
     *
     * @param account The account on that behalf the operation is made.
     * @param amount The amount of tokens to be cash-outed.
     * @param txId The off-chain transaction identifier of the operation.
     */
    function requestCashOutFrom(
        address account,
        uint256 amount,
        bytes32 txId
    ) external;

    /**
     * @dev Initiates a batch of cash-out operations from some other accounts.
     *
     * Transfers tokens from the accounts to the contract.
     * This function is expected to be called by a limited number of accounts
     * that are allowed to process cash-out operations.
     *
     * Emits {CashOut} events.
     *
     * @param accounts The array of accounts on that behalf the operation is made.
     * @param amounts The array of amounts of tokens to be cash-outed.
     * @param txIds The array of off-chain transaction identifiers of the operation.
     */
    function requestCashOutFromBatch(
        address[] memory accounts,
        uint256[] memory amounts,
        bytes32[] memory txIds
    ) external;

    /**
     * @dev Confirms a single cash-out operation.
     *
     * Burns tokens previously transferred to the contract.
     * This function is expected to be called by a limited number of accounts
     * that are allowed to process cash-out operations.
     *
     * Emits a {CashOutConfirm} event for the operation.
     *
     * @param txId The off-chain transaction identifier of the operation.
     */
    function confirmCashOut(bytes32 txId) external;

    /**
     * @dev Confirms multiple cash-out operations.
     *
     * Burns tokens previously transferred to the contract.
     * This function is expected to be called by a limited number of accounts
     * that are allowed to process cash-out operations.
     *
     * Emits a {CashOutConfirm} event for each operation.
     *
     * @param txIds The off-chain transaction identifiers of the operations.
     */
    function confirmCashOutBatch(bytes32[] memory txIds) external;

    /**
     * @dev Reverts a single cash-out operation.
     *
     * Transfers tokens back from the contract to the account that requested the operation.
     * This function is expected to be called by a limited number of accounts
     * that are allowed to process cash-out operations.
     *
     * Emits a {CashOutReverse} event for the operation.
     *
     * @param txId The off-chain transaction identifier of the operation.
     */
    function reverseCashOut(bytes32 txId) external;

    /**
     * @dev Reverts multiple cash-out operation.
     *
     * Transfers tokens back from the contract to the accounts that requested the operations.
     * This function is expected to be called by a limited number of accounts
     * that are allowed to process cash-out operations.
     *
     * Emits a {CashOutReverse} event for each operation.
     *
     * @param txIds The off-chain transaction identifiers of the operations.
     */
    function reverseCashOutBatch(bytes32[] memory txIds) external;
}

/**
 * @dev Library for managing
 * https://en.wikipedia.org/wiki/Set_(abstract_data_type)[sets] of primitive
 * types.
 *
 * Sets have the following properties:
 *
 * - Elements are added, removed, and checked for existence in constant time
 * (O(1)).
 * - Elements are enumerated in O(n). No guarantees are made on the ordering.
 *
 * ```
 * contract Example {
 *     // Add the library methods
 *     using EnumerableSet for EnumerableSet.AddressSet;
 *
 *     // Declare a set state variable
 *     EnumerableSet.AddressSet private mySet;
 * }
 * ```
 *
 * As of v3.3.0, sets of type `bytes32` (`Bytes32Set`), `address` (`AddressSet`)
 * and `uint256` (`UintSet`) are supported.
 *
 * [WARNING]
 * ====
 *  Trying to delete such a structure from storage will likely result in data corruption, rendering the structure unusable.
 *  See https://github.com/ethereum/solidity/pull/11843[ethereum/solidity#11843] for more info.
 *
 *  In order to clean an EnumerableSet, you can either remove all elements one by one or create a fresh instance using an array of EnumerableSet.
 * ====
 */
library EnumerableSetUpgradeable {
    // To implement this library for multiple types with as little code
    // repetition as possible, we write it in terms of a generic Set type with
    // bytes32 values.
    // The Set implementation uses private functions, and user-facing
    // implementations (such as AddressSet) are just wrappers around the
    // underlying Set.
    // This means that we can only create new EnumerableSets for types that fit
    // in bytes32.

    struct Set {
        // Storage of set values
        bytes32[] _values;
        // Position of the value in the `values` array, plus 1 because index 0
        // means a value is not in the set.
        mapping(bytes32 => uint256) _indexes;
    }

    /**
     * @dev Add a value to a set. O(1).
     *
     * Returns true if the value was added to the set, that is if it was not
     * already present.
     */
    function _add(Set storage set, bytes32 value) private returns (bool) {
        if (!_contains(set, value)) {
            set._values.push(value);
            // The value is stored at length-1, but we add 1 to all indexes
            // and use 0 as a sentinel value
            set._indexes[value] = set._values.length;
            return true;
        } else {
            return false;
        }
    }

    /**
     * @dev Removes a value from a set. O(1).
     *
     * Returns true if the value was removed from the set, that is if it was
     * present.
     */
    function _remove(Set storage set, bytes32 value) private returns (bool) {
        // We read and store the value's index to prevent multiple reads from the same storage slot
        uint256 valueIndex = set._indexes[value];

        if (valueIndex != 0) {
            // Equivalent to contains(set, value)
            // To delete an element from the _values array in O(1), we swap the element to delete with the last one in
            // the array, and then remove the last element (sometimes called as 'swap and pop').
            // This modifies the order of the array, as noted in {at}.

            uint256 toDeleteIndex = valueIndex - 1;
            uint256 lastIndex = set._values.length - 1;

            if (lastIndex != toDeleteIndex) {
                bytes32 lastValue = set._values[lastIndex];

                // Move the last value to the index where the value to delete is
                set._values[toDeleteIndex] = lastValue;
                // Update the index for the moved value
                set._indexes[lastValue] = valueIndex; // Replace lastValue's index to valueIndex
            }

            // Delete the slot where the moved value was stored
            set._values.pop();

            // Delete the index for the deleted slot
            delete set._indexes[value];

            return true;
        } else {
            return false;
        }
    }

    /**
     * @dev Returns true if the value is in the set. O(1).
     */
    function _contains(Set storage set, bytes32 value) private view returns (bool) {
        return set._indexes[value] != 0;
    }

    /**
     * @dev Returns the number of values on the set. O(1).
     */
    function _length(Set storage set) private view returns (uint256) {
        return set._values.length;
    }

    /**
     * @dev Returns the value stored at position `index` in the set. O(1).
     *
     * Note that there are no guarantees on the ordering of values inside the
     * array, and it may change when more values are added or removed.
     *
     * Requirements:
     *
     * - `index` must be strictly less than {length}.
     */
    function _at(Set storage set, uint256 index) private view returns (bytes32) {
        return set._values[index];
    }

    /**
     * @dev Return the entire set in an array
     *
     * WARNING: This operation will copy the entire storage to memory, which can be quite expensive. This is designed
     * to mostly be used by view accessors that are queried without any gas fees. Developers should keep in mind that
     * this function has an unbounded cost, and using it as part of a state-changing function may render the function
     * uncallable if the set grows to a point where copying to memory consumes too much gas to fit in a block.
     */
    function _values(Set storage set) private view returns (bytes32[] memory) {
        return set._values;
    }

    // Bytes32Set

    struct Bytes32Set {
        Set _inner;
    }

    /**
     * @dev Add a value to a set. O(1).
     *
     * Returns true if the value was added to the set, that is if it was not
     * already present.
     */
    function add(Bytes32Set storage set, bytes32 value) internal returns (bool) {
        return _add(set._inner, value);
    }

    /**
     * @dev Removes a value from a set. O(1).
     *
     * Returns true if the value was removed from the set, that is if it was
     * present.
     */
    function remove(Bytes32Set storage set, bytes32 value) internal returns (bool) {
        return _remove(set._inner, value);
    }

    /**
     * @dev Returns true if the value is in the set. O(1).
     */
    function contains(Bytes32Set storage set, bytes32 value) internal view returns (bool) {
        return _contains(set._inner, value);
    }

    /**
     * @dev Returns the number of values in the set. O(1).
     */
    function length(Bytes32Set storage set) internal view returns (uint256) {
        return _length(set._inner);
    }

    /**
     * @dev Returns the value stored at position `index` in the set. O(1).
     *
     * Note that there are no guarantees on the ordering of values inside the
     * array, and it may change when more values are added or removed.
     *
     * Requirements:
     *
     * - `index` must be strictly less than {length}.
     */
    function at(Bytes32Set storage set, uint256 index) internal view returns (bytes32) {
        return _at(set._inner, index);
    }

    /**
     * @dev Return the entire set in an array
     *
     * WARNING: This operation will copy the entire storage to memory, which can be quite expensive. This is designed
     * to mostly be used by view accessors that are queried without any gas fees. Developers should keep in mind that
     * this function has an unbounded cost, and using it as part of a state-changing function may render the function
     * uncallable if the set grows to a point where copying to memory consumes too much gas to fit in a block.
     */
    function values(Bytes32Set storage set) internal view returns (bytes32[] memory) {
        return _values(set._inner);
    }

    // AddressSet

    struct AddressSet {
        Set _inner;
    }

    /**
     * @dev Add a value to a set. O(1).
     *
     * Returns true if the value was added to the set, that is if it was not
     * already present.
     */
    function add(AddressSet storage set, address value) internal returns (bool) {
        return _add(set._inner, bytes32(uint256(uint160(value))));
    }

    /**
     * @dev Removes a value from a set. O(1).
     *
     * Returns true if the value was removed from the set, that is if it was
     * present.
     */
    function remove(AddressSet storage set, address value) internal returns (bool) {
        return _remove(set._inner, bytes32(uint256(uint160(value))));
    }

    /**
     * @dev Returns true if the value is in the set. O(1).
     */
    function contains(AddressSet storage set, address value) internal view returns (bool) {
        return _contains(set._inner, bytes32(uint256(uint160(value))));
    }

    /**
     * @dev Returns the number of values in the set. O(1).
     */
    function length(AddressSet storage set) internal view returns (uint256) {
        return _length(set._inner);
    }

    /**
     * @dev Returns the value stored at position `index` in the set. O(1).
     *
     * Note that there are no guarantees on the ordering of values inside the
     * array, and it may change when more values are added or removed.
     *
     * Requirements:
     *
     * - `index` must be strictly less than {length}.
     */
    function at(AddressSet storage set, uint256 index) internal view returns (address) {
        return address(uint160(uint256(_at(set._inner, index))));
    }

    /**
     * @dev Return the entire set in an array
     *
     * WARNING: This operation will copy the entire storage to memory, which can be quite expensive. This is designed
     * to mostly be used by view accessors that are queried without any gas fees. Developers should keep in mind that
     * this function has an unbounded cost, and using it as part of a state-changing function may render the function
     * uncallable if the set grows to a point where copying to memory consumes too much gas to fit in a block.
     */
    function values(AddressSet storage set) internal view returns (address[] memory) {
        bytes32[] memory store = _values(set._inner);
        address[] memory result;

        /// @solidity memory-safe-assembly
        assembly {
            result := store
        }

        return result;
    }

    // UintSet

    struct UintSet {
        Set _inner;
    }

    /**
     * @dev Add a value to a set. O(1).
     *
     * Returns true if the value was added to the set, that is if it was not
     * already present.
     */
    function add(UintSet storage set, uint256 value) internal returns (bool) {
        return _add(set._inner, bytes32(value));
    }

    /**
     * @dev Removes a value from a set. O(1).
     *
     * Returns true if the value was removed from the set, that is if it was
     * present.
     */
    function remove(UintSet storage set, uint256 value) internal returns (bool) {
        return _remove(set._inner, bytes32(value));
    }

    /**
     * @dev Returns true if the value is in the set. O(1).
     */
    function contains(UintSet storage set, uint256 value) internal view returns (bool) {
        return _contains(set._inner, bytes32(value));
    }

    /**
     * @dev Returns the number of values on the set. O(1).
     */
    function length(UintSet storage set) internal view returns (uint256) {
        return _length(set._inner);
    }

    /**
     * @dev Returns the value stored at position `index` in the set. O(1).
     *
     * Note that there are no guarantees on the ordering of values inside the
     * array, and it may change when more values are added or removed.
     *
     * Requirements:
     *
     * - `index` must be strictly less than {length}.
     */
    function at(UintSet storage set, uint256 index) internal view returns (uint256) {
        return uint256(_at(set._inner, index));
    }

    /**
     * @dev Return the entire set in an array
     *
     * WARNING: This operation will copy the entire storage to memory, which can be quite expensive. This is designed
     * to mostly be used by view accessors that are queried without any gas fees. Developers should keep in mind that
     * this function has an unbounded cost, and using it as part of a state-changing function may render the function
     * uncallable if the set grows to a point where copying to memory consumes too much gas to fit in a block.
     */
    function values(UintSet storage set) internal view returns (uint256[] memory) {
        bytes32[] memory store = _values(set._inner);
        uint256[] memory result;

        /// @solidity memory-safe-assembly
        assembly {
            result := store
        }

        return result;
    }
}

/**
 * @title PixCashier storage version 1
 */
abstract contract PixCashierStorageV1 is IPixCashierTypes {
    /// @dev The address of the underlying token.
    address internal _token;

    /// @dev The mapping of a pending cash-out balance for a given account.
    mapping(address => uint256) internal _cashOutBalances;

    /// @dev The mapping of a cash-out operation structure for a given off-chain transaction identifier.
    mapping(bytes32 => CashOut) internal _cashOuts;

    /// @dev The set of off-chain transaction identifiers that correspond the pending cash-out operations.
    EnumerableSetUpgradeable.Bytes32Set internal _pendingCashOutTxIds;

    /// @dev The processed cash-out operation counter that includes number of reversed and confirmed operations.
    uint256 internal _processedCashOutCounter;
}

/**
 * @title PixCashier storage version 2
 */
abstract contract PixCashierStorageV2 is IPixCashierTypes {
    /// @dev The mapping of a cash-in operation structure for a given off-chain transaction identifier.
    mapping(bytes32 => CashInOperation) internal _cashIns;
}

/**
 * @title PixCashier storage version 3
 */
abstract contract PixCashierStorageV3 is IPixCashierTypes {
    /// @dev The mapping of a cash-in batch operation structure for a given off-chain identifier.
    mapping(bytes32 => CashInBatchOperation) internal _cashInBatches;
}

/**
 * @title PixCashier storage
 * @dev Contains storage variables of the {PixCashier} contract.
 *
 * We are following Compound's approach of upgrading new contract implementations.
 * See https://github.com/compound-finance/compound-protocol.
 * When we need to add new storage variables, we create a new version of PixCashierStorage
 * e.g. PixCashierStorage<versionNumber>, so finally it would look like
 * "contract PixCashierStorage is PixCashierStorageV1, PixCashierStorageV2".
 */
abstract contract PixCashierStorage is PixCashierStorageV1, PixCashierStorageV2, PixCashierStorageV3 {

}

/**
 * @title IERC20Mintable interface
 * @dev The interface of a token that supports mint and burn operations.
 */
interface IERC20Mintable {
    /// @dev Emitted when the master minter is changed.
    event MasterMinterChanged(address indexed newMasterMinter);

    /// @dev Emitted when a minter account is configured.
    event MinterConfigured(address indexed minter, uint256 mintAllowance);

    /// @dev Emitted when a minter account is removed.
    event MinterRemoved(address indexed oldMinter);

    /// @dev Emitted when tokens are minted.
    event Mint(address indexed minter, address indexed to, uint256 amount);

    /// @dev Emitted when tokens are burned.
    event Burn(address indexed burner, uint256 amount);

    /**
     * @dev Returns the master minter address.
     */
    function masterMinter() external view returns (address);

    /**
     * @dev Checks if the account is configured as a minter.
     * @param account The address to check.
     * @return True if the account is a minter.
     */
    function isMinter(address account) external view returns (bool);

    /**
     * @dev Returns the mint allowance of a minter.
     * @param minter The minter to check.
     * @return The mint allowance of the minter.
     */
    function minterAllowance(address minter) external view returns (uint256);

    /**
     * @dev Updates the master minter address.
     *
     * Emits a {MasterMinterChanged} event.
     *
     * @param newMasterMinter The address of a new master minter.
     */
    function updateMasterMinter(address newMasterMinter) external;

    /**
     * @dev Configures a minter.
     *
     * Emits a {MinterConfigured} event.
     *
     * @param minter The address of the minter to configure.
     * @param mintAllowance The mint allowance.
     * @return True if the operation was successful.
     */
    function configureMinter(address minter, uint256 mintAllowance) external returns (bool);

    /**
     * @dev Removes a minter.
     *
     * Emits a {MinterRemoved} event.
     *
     * @param minter The address of the minter to remove.
     * @return True if the operation was successful.
     */
    function removeMinter(address minter) external returns (bool);

    /**
     * @dev Mints tokens.
     *
     * Emits a {Mint} event.
     *
     * @param account The address of a tokens recipient.
     * @param amount The amount of tokens to mint.
     * @return True if the operation was successful.
     */
    function mint(address account, uint256 amount) external returns (bool);

    /**
     * @dev Burns tokens.
     *
     * Emits a {Burn} event.
     *
     * @param amount The amount of tokens to burn.
     */
    function burn(uint256 amount) external;
}

/**
 * @dev Collection of functions related to the address type
 */
library AddressUpgradeable {
    /**
     * @dev Returns true if `account` is a contract.
     *
     * [IMPORTANT]
     * ====
     * It is unsafe to assume that an address for which this function returns
     * false is an externally-owned account (EOA) and not a contract.
     *
     * Among others, `isContract` will return false for the following
     * types of addresses:
     *
     *  - an externally-owned account
     *  - a contract in construction
     *  - an address where a contract will be created
     *  - an address where a contract lived, but was destroyed
     * ====
     *
     * [IMPORTANT]
     * ====
     * You shouldn't rely on `isContract` to protect against flash loan attacks!
     *
     * Preventing calls from contracts is highly discouraged. It breaks composability, breaks support for smart wallets
     * like Gnosis Safe, and does not provide security since it can be circumvented by calling from a contract
     * constructor.
     * ====
     */
    function isContract(address account) internal view returns (bool) {
        // This method relies on extcodesize/address.code.length, which returns 0
        // for contracts in construction, since the code is only stored at the end
        // of the constructor execution.

        return account.code.length > 0;
    }

    /**
     * @dev Replacement for Solidity's `transfer`: sends `amount` wei to
     * `recipient`, forwarding all available gas and reverting on errors.
     *
     * https://eips.ethereum.org/EIPS/eip-1884[EIP1884] increases the gas cost
     * of certain opcodes, possibly making contracts go over the 2300 gas limit
     * imposed by `transfer`, making them unable to receive funds via
     * `transfer`. {sendValue} removes this limitation.
     *
     * https://diligence.consensys.net/posts/2019/09/stop-using-soliditys-transfer-now/[Learn more].
     *
     * IMPORTANT: because control is transferred to `recipient`, care must be
     * taken to not create reentrancy vulnerabilities. Consider using
     * {ReentrancyGuard} or the
     * https://solidity.readthedocs.io/en/v0.5.11/security-considerations.html#use-the-checks-effects-interactions-pattern[checks-effects-interactions pattern].
     */
    function sendValue(address payable recipient, uint256 amount) internal {
        require(address(this).balance >= amount, "Address: insufficient balance");

        (bool success, ) = recipient.call{value: amount}("");
        require(success, "Address: unable to send value, recipient may have reverted");
    }

    /**
     * @dev Performs a Solidity function call using a low level `call`. A
     * plain `call` is an unsafe replacement for a function call: use this
     * function instead.
     *
     * If `target` reverts with a revert reason, it is bubbled up by this
     * function (like regular Solidity function calls).
     *
     * Returns the raw returned data. To convert to the expected return value,
     * use https://solidity.readthedocs.io/en/latest/units-and-global-variables.html?highlight=abi.decode#abi-encoding-and-decoding-functions[`abi.decode`].
     *
     * Requirements:
     *
     * - `target` must be a contract.
     * - calling `target` with `data` must not revert.
     *
     * _Available since v3.1._
     */
    function functionCall(address target, bytes memory data) internal returns (bytes memory) {
        return functionCall(target, data, "Address: low-level call failed");
    }

    /**
     * @dev Same as {xref-Address-functionCall-address-bytes-}[`functionCall`], but with
     * `errorMessage` as a fallback revert reason when `target` reverts.
     *
     * _Available since v3.1._
     */
    function functionCall(
        address target,
        bytes memory data,
        string memory errorMessage
    ) internal returns (bytes memory) {
        return functionCallWithValue(target, data, 0, errorMessage);
    }

    /**
     * @dev Same as {xref-Address-functionCall-address-bytes-}[`functionCall`],
     * but also transferring `value` wei to `target`.
     *
     * Requirements:
     *
     * - the calling contract must have an ETH balance of at least `value`.
     * - the called Solidity function must be `payable`.
     *
     * _Available since v3.1._
     */
    function functionCallWithValue(
        address target,
        bytes memory data,
        uint256 value
    ) internal returns (bytes memory) {
        return functionCallWithValue(target, data, value, "Address: low-level call with value failed");
    }

    /**
     * @dev Same as {xref-Address-functionCallWithValue-address-bytes-uint256-}[`functionCallWithValue`], but
     * with `errorMessage` as a fallback revert reason when `target` reverts.
     *
     * _Available since v3.1._
     */
    function functionCallWithValue(
        address target,
        bytes memory data,
        uint256 value,
        string memory errorMessage
    ) internal returns (bytes memory) {
        require(address(this).balance >= value, "Address: insufficient balance for call");
        require(isContract(target), "Address: call to non-contract");

        (bool success, bytes memory returndata) = target.call{value: value}(data);
        return verifyCallResult(success, returndata, errorMessage);
    }

    /**
     * @dev Same as {xref-Address-functionCall-address-bytes-}[`functionCall`],
     * but performing a static call.
     *
     * _Available since v3.3._
     */
    function functionStaticCall(address target, bytes memory data) internal view returns (bytes memory) {
        return functionStaticCall(target, data, "Address: low-level static call failed");
    }

    /**
     * @dev Same as {xref-Address-functionCall-address-bytes-string-}[`functionCall`],
     * but performing a static call.
     *
     * _Available since v3.3._
     */
    function functionStaticCall(
        address target,
        bytes memory data,
        string memory errorMessage
    ) internal view returns (bytes memory) {
        require(isContract(target), "Address: static call to non-contract");

        (bool success, bytes memory returndata) = target.staticcall(data);
        return verifyCallResult(success, returndata, errorMessage);
    }

    /**
     * @dev Tool to verifies that a low level call was successful, and revert if it wasn't, either by bubbling the
     * revert reason using the provided one.
     *
     * _Available since v4.3._
     */
    function verifyCallResult(
        bool success,
        bytes memory returndata,
        string memory errorMessage
    ) internal pure returns (bytes memory) {
        if (success) {
            return returndata;
        } else {
            // Look for revert reason and bubble it up if present
            if (returndata.length > 0) {
                // The easiest way to bubble the revert reason is using memory via assembly
                /// @solidity memory-safe-assembly
                assembly {
                    let returndata_size := mload(returndata)
                    revert(add(32, returndata), returndata_size)
                }
            } else {
                revert(errorMessage);
            }
        }
    }
}

/**
 * @dev This is a base contract to aid in writing upgradeable contracts, or any kind of contract that will be deployed
 * behind a proxy. Since proxied contracts do not make use of a constructor, it's common to move constructor logic to an
 * external initializer function, usually called `initialize`. It then becomes necessary to protect this initializer
 * function so it can only be called once. The {initializer} modifier provided by this contract will have this effect.
 *
 * The initialization functions use a version number. Once a version number is used, it is consumed and cannot be
 * reused. This mechanism prevents re-execution of each "step" but allows the creation of new initialization steps in
 * case an upgrade adds a module that needs to be initialized.
 *
 * For example:
 *
 * [.hljs-theme-light.nopadding]
 * ```
 * contract MyToken is ERC20Upgradeable {
 *     function initialize() initializer public {
 *         __ERC20_init("MyToken", "MTK");
 *     }
 * }
 * contract MyTokenV2 is MyToken, ERC20PermitUpgradeable {
 *     function initializeV2() reinitializer(2) public {
 *         __ERC20Permit_init("MyToken");
 *     }
 * }
 * ```
 *
 * TIP: To avoid leaving the proxy in an uninitialized state, the initializer function should be called as early as
 * possible by providing the encoded function call as the `_data` argument to {ERC1967Proxy-constructor}.
 *
 * CAUTION: When used with inheritance, manual care must be taken to not invoke a parent initializer twice, or to ensure
 * that all initializers are idempotent. This is not verified automatically as constructors are by Solidity.
 *
 * [CAUTION]
 * ====
 * Avoid leaving a contract uninitialized.
 *
 * An uninitialized contract can be taken over by an attacker. This applies to both a proxy and its implementation
 * contract, which may impact the proxy. To prevent the implementation contract from being used, you should invoke
 * the {_disableInitializers} function in the constructor to automatically lock it when it is deployed:
 *
 * [.hljs-theme-light.nopadding]
 * ```
 * /// @custom:oz-upgrades-unsafe-allow constructor
 * constructor() {
 *     _disableInitializers();
 * }
 * ```
 * ====
 */
abstract contract Initializable {
    /**
     * @dev Indicates that the contract has been initialized.
     * @custom:oz-retyped-from bool
     */
    uint8 private _initialized;

    /**
     * @dev Indicates that the contract is in the process of being initialized.
     */
    bool private _initializing;

    /**
     * @dev Triggered when the contract has been initialized or reinitialized.
     */
    event Initialized(uint8 version);

    /**
     * @dev A modifier that defines a protected initializer function that can be invoked at most once. In its scope,
     * `onlyInitializing` functions can be used to initialize parent contracts. Equivalent to `reinitializer(1)`.
     */
    modifier initializer() {
        bool isTopLevelCall = !_initializing;
        require(
            (isTopLevelCall && _initialized < 1) || (!AddressUpgradeable.isContract(address(this)) && _initialized == 1),
            "Initializable: contract is already initialized"
        );
        _initialized = 1;
        if (isTopLevelCall) {
            _initializing = true;
        }
        _;
        if (isTopLevelCall) {
            _initializing = false;
            emit Initialized(1);
        }
    }

    /**
     * @dev A modifier that defines a protected reinitializer function that can be invoked at most once, and only if the
     * contract hasn't been initialized to a greater version before. In its scope, `onlyInitializing` functions can be
     * used to initialize parent contracts.
     *
     * `initializer` is equivalent to `reinitializer(1)`, so a reinitializer may be used after the original
     * initialization step. This is essential to configure modules that are added through upgrades and that require
     * initialization.
     *
     * Note that versions can jump in increments greater than 1; this implies that if multiple reinitializers coexist in
     * a contract, executing them in the right order is up to the developer or operator.
     */
    modifier reinitializer(uint8 version) {
        require(!_initializing && _initialized < version, "Initializable: contract is already initialized");
        _initialized = version;
        _initializing = true;
        _;
        _initializing = false;
        emit Initialized(version);
    }

    /**
     * @dev Modifier to protect an initialization function so that it can only be invoked by functions with the
     * {initializer} and {reinitializer} modifiers, directly or indirectly.
     */
    modifier onlyInitializing() {
        require(_initializing, "Initializable: contract is not initializing");
        _;
    }

    /**
     * @dev Locks the contract, preventing any future reinitialization. This cannot be part of an initializer call.
     * Calling this in the constructor of a contract will prevent that contract from being initialized or reinitialized
     * to any version. It is recommended to use this to lock implementation contracts that are designed to be called
     * through proxies.
     */
    function _disableInitializers() internal virtual {
        require(!_initializing, "Initializable: contract is initializing");
        if (_initialized < type(uint8).max) {
            _initialized = type(uint8).max;
            emit Initialized(type(uint8).max);
        }
    }
}

/**
 * @dev Provides information about the current execution context, including the
 * sender of the transaction and its data. While these are generally available
 * via msg.sender and msg.data, they should not be accessed in such a direct
 * manner, since when dealing with meta-transactions the account sending and
 * paying for execution may not be the actual sender (as far as an application
 * is concerned).
 *
 * This contract is only required for intermediate, library-like contracts.
 */
abstract contract ContextUpgradeable is Initializable {
    function __Context_init() internal onlyInitializing {
    }

    function __Context_init_unchained() internal onlyInitializing {
    }
    function _msgSender() internal view virtual returns (address) {
        return msg.sender;
    }

    function _msgData() internal view virtual returns (bytes calldata) {
        return msg.data;
    }

    /**
     * @dev This empty reserved space is put in place to allow future versions to add new
     * variables without shifting down storage in the inheritance chain.
     * See https://docs.openzeppelin.com/contracts/4.x/upgradeable#storage_gaps
     */
    uint256[50] private __gap;
}

/**
 * @dev External interface of AccessControl declared to support ERC165 detection.
 */
interface IAccessControlUpgradeable {
    /**
     * @dev Emitted when `newAdminRole` is set as ``role``'s admin role, replacing `previousAdminRole`
     *
     * `DEFAULT_ADMIN_ROLE` is the starting admin for all roles, despite
     * {RoleAdminChanged} not being emitted signaling this.
     *
     * _Available since v3.1._
     */
    event RoleAdminChanged(bytes32 indexed role, bytes32 indexed previousAdminRole, bytes32 indexed newAdminRole);

    /**
     * @dev Emitted when `account` is granted `role`.
     *
     * `sender` is the account that originated the contract call, an admin role
     * bearer except when using {AccessControl-_setupRole}.
     */
    event RoleGranted(bytes32 indexed role, address indexed account, address indexed sender);

    /**
     * @dev Emitted when `account` is revoked `role`.
     *
     * `sender` is the account that originated the contract call:
     *   - if using `revokeRole`, it is the admin role bearer
     *   - if using `renounceRole`, it is the role bearer (i.e. `account`)
     */
    event RoleRevoked(bytes32 indexed role, address indexed account, address indexed sender);

    /**
     * @dev Returns `true` if `account` has been granted `role`.
     */
    function hasRole(bytes32 role, address account) external view returns (bool);

    /**
     * @dev Returns the admin role that controls `role`. See {grantRole} and
     * {revokeRole}.
     *
     * To change a role's admin, use {AccessControl-_setRoleAdmin}.
     */
    function getRoleAdmin(bytes32 role) external view returns (bytes32);

    /**
     * @dev Grants `role` to `account`.
     *
     * If `account` had not been already granted `role`, emits a {RoleGranted}
     * event.
     *
     * Requirements:
     *
     * - the caller must have ``role``'s admin role.
     */
    function grantRole(bytes32 role, address account) external;

    /**
     * @dev Revokes `role` from `account`.
     *
     * If `account` had been granted `role`, emits a {RoleRevoked} event.
     *
     * Requirements:
     *
     * - the caller must have ``role``'s admin role.
     */
    function revokeRole(bytes32 role, address account) external;

    /**
     * @dev Revokes `role` from the calling account.
     *
     * Roles are often managed via {grantRole} and {revokeRole}: this function's
     * purpose is to provide a mechanism for accounts to lose their privileges
     * if they are compromised (such as when a trusted device is misplaced).
     *
     * If the calling account had been granted `role`, emits a {RoleRevoked}
     * event.
     *
     * Requirements:
     *
     * - the caller must be `account`.
     */
    function renounceRole(bytes32 role, address account) external;
}

/**
 * @dev String operations.
 */
library StringsUpgradeable {
    bytes16 private constant _HEX_SYMBOLS = "0123456789abcdef";
    uint8 private constant _ADDRESS_LENGTH = 20;

    /**
     * @dev Converts a `uint256` to its ASCII `string` decimal representation.
     */
    function toString(uint256 value) internal pure returns (string memory) {
        // Inspired by OraclizeAPI's implementation - MIT licence
        // https://github.com/oraclize/ethereum-api/blob/b42146b063c7d6ee1358846c198246239e9360e8/oraclizeAPI_0.4.25.sol

        if (value == 0) {
            return "0";
        }
        uint256 temp = value;
        uint256 digits;
        while (temp != 0) {
            digits++;
            temp /= 10;
        }
        bytes memory buffer = new bytes(digits);
        while (value != 0) {
            digits -= 1;
            buffer[digits] = bytes1(uint8(48 + uint256(value % 10)));
            value /= 10;
        }
        return string(buffer);
    }

    /**
     * @dev Converts a `uint256` to its ASCII `string` hexadecimal representation.
     */
    function toHexString(uint256 value) internal pure returns (string memory) {
        if (value == 0) {
            return "0x00";
        }
        uint256 temp = value;
        uint256 length = 0;
        while (temp != 0) {
            length++;
            temp >>= 8;
        }
        return toHexString(value, length);
    }

    /**
     * @dev Converts a `uint256` to its ASCII `string` hexadecimal representation with fixed length.
     */
    function toHexString(uint256 value, uint256 length) internal pure returns (string memory) {
        bytes memory buffer = new bytes(2 * length + 2);
        buffer[0] = "0";
        buffer[1] = "x";
        for (uint256 i = 2 * length + 1; i > 1; --i) {
            buffer[i] = _HEX_SYMBOLS[value & 0xf];
            value >>= 4;
        }
        require(value == 0, "Strings: hex length insufficient");
        return string(buffer);
    }

    /**
     * @dev Converts an `address` with fixed length of 20 bytes to its not checksummed ASCII `string` hexadecimal representation.
     */
    function toHexString(address addr) internal pure returns (string memory) {
        return toHexString(uint256(uint160(addr)), _ADDRESS_LENGTH);
    }
}

/**
 * @dev Interface of the ERC165 standard, as defined in the
 * https://eips.ethereum.org/EIPS/eip-165[EIP].
 *
 * Implementers can declare support of contract interfaces, which can then be
 * queried by others ({ERC165Checker}).
 *
 * For an implementation, see {ERC165}.
 */
interface IERC165Upgradeable {
    /**
     * @dev Returns true if this contract implements the interface defined by
     * `interfaceId`. See the corresponding
     * https://eips.ethereum.org/EIPS/eip-165#how-interfaces-are-identified[EIP section]
     * to learn more about how these ids are created.
     *
     * This function call must use less than 30 000 gas.
     */
    function supportsInterface(bytes4 interfaceId) external view returns (bool);
}

/**
 * @dev Implementation of the {IERC165} interface.
 *
 * Contracts that want to implement ERC165 should inherit from this contract and override {supportsInterface} to check
 * for the additional interface id that will be supported. For example:
 *
 * ```solidity
 * function supportsInterface(bytes4 interfaceId) public view virtual override returns (bool) {
 *     return interfaceId == type(MyInterface).interfaceId || super.supportsInterface(interfaceId);
 * }
 * ```
 *
 * Alternatively, {ERC165Storage} provides an easier to use but more expensive implementation.
 */
abstract contract ERC165Upgradeable is Initializable, IERC165Upgradeable {
    function __ERC165_init() internal onlyInitializing {
    }

    function __ERC165_init_unchained() internal onlyInitializing {
    }
    /**
     * @dev See {IERC165-supportsInterface}.
     */
    function supportsInterface(bytes4 interfaceId) public view virtual override returns (bool) {
        return interfaceId == type(IERC165Upgradeable).interfaceId;
    }

    /**
     * @dev This empty reserved space is put in place to allow future versions to add new
     * variables without shifting down storage in the inheritance chain.
     * See https://docs.openzeppelin.com/contracts/4.x/upgradeable#storage_gaps
     */
    uint256[50] private __gap;
}

/**
 * @dev Contract module that allows children to implement role-based access
 * control mechanisms. This is a lightweight version that doesn't allow enumerating role
 * members except through off-chain means by accessing the contract event logs. Some
 * applications may benefit from on-chain enumerability, for those cases see
 * {AccessControlEnumerable}.
 *
 * Roles are referred to by their `bytes32` identifier. These should be exposed
 * in the external API and be unique. The best way to achieve this is by
 * using `public constant` hash digests:
 *
 * ```
 * bytes32 public constant MY_ROLE = keccak256("MY_ROLE");
 * ```
 *
 * Roles can be used to represent a set of permissions. To restrict access to a
 * function call, use {hasRole}:
 *
 * ```
 * function foo() public {
 *     require(hasRole(MY_ROLE, msg.sender));
 *     ...
 * }
 * ```
 *
 * Roles can be granted and revoked dynamically via the {grantRole} and
 * {revokeRole} functions. Each role has an associated admin role, and only
 * accounts that have a role's admin role can call {grantRole} and {revokeRole}.
 *
 * By default, the admin role for all roles is `DEFAULT_ADMIN_ROLE`, which means
 * that only accounts with this role will be able to grant or revoke other
 * roles. More complex role relationships can be created by using
 * {_setRoleAdmin}.
 *
 * WARNING: The `DEFAULT_ADMIN_ROLE` is also its own admin: it has permission to
 * grant and revoke this role. Extra precautions should be taken to secure
 * accounts that have been granted it.
 */
abstract contract AccessControlUpgradeable is Initializable, ContextUpgradeable, IAccessControlUpgradeable, ERC165Upgradeable {
    function __AccessControl_init() internal onlyInitializing {
    }

    function __AccessControl_init_unchained() internal onlyInitializing {
    }
    struct RoleData {
        mapping(address => bool) members;
        bytes32 adminRole;
    }

    mapping(bytes32 => RoleData) private _roles;

    bytes32 public constant DEFAULT_ADMIN_ROLE = 0x00;

    /**
     * @dev Modifier that checks that an account has a specific role. Reverts
     * with a standardized message including the required role.
     *
     * The format of the revert reason is given by the following regular expression:
     *
     *  /^AccessControl: account (0x[0-9a-f]{40}) is missing role (0x[0-9a-f]{64})$/
     *
     * _Available since v4.1._
     */
    modifier onlyRole(bytes32 role) {
        _checkRole(role);
        _;
    }

    /**
     * @dev See {IERC165-supportsInterface}.
     */
    function supportsInterface(bytes4 interfaceId) public view virtual override returns (bool) {
        return interfaceId == type(IAccessControlUpgradeable).interfaceId || super.supportsInterface(interfaceId);
    }

    /**
     * @dev Returns `true` if `account` has been granted `role`.
     */
    function hasRole(bytes32 role, address account) public view virtual override returns (bool) {
        return _roles[role].members[account];
    }

    /**
     * @dev Revert with a standard message if `_msgSender()` is missing `role`.
     * Overriding this function changes the behavior of the {onlyRole} modifier.
     *
     * Format of the revert message is described in {_checkRole}.
     *
     * _Available since v4.6._
     */
    function _checkRole(bytes32 role) internal view virtual {
        _checkRole(role, _msgSender());
    }

    /**
     * @dev Revert with a standard message if `account` is missing `role`.
     *
     * The format of the revert reason is given by the following regular expression:
     *
     *  /^AccessControl: account (0x[0-9a-f]{40}) is missing role (0x[0-9a-f]{64})$/
     */
    function _checkRole(bytes32 role, address account) internal view virtual {
        if (!hasRole(role, account)) {
            revert(
                string(
                    abi.encodePacked(
                        "AccessControl: account ",
                        StringsUpgradeable.toHexString(uint160(account), 20),
                        " is missing role ",
                        StringsUpgradeable.toHexString(uint256(role), 32)
                    )
                )
            );
        }
    }

    /**
     * @dev Returns the admin role that controls `role`. See {grantRole} and
     * {revokeRole}.
     *
     * To change a role's admin, use {_setRoleAdmin}.
     */
    function getRoleAdmin(bytes32 role) public view virtual override returns (bytes32) {
        return _roles[role].adminRole;
    }

    /**
     * @dev Grants `role` to `account`.
     *
     * If `account` had not been already granted `role`, emits a {RoleGranted}
     * event.
     *
     * Requirements:
     *
     * - the caller must have ``role``'s admin role.
     *
     * May emit a {RoleGranted} event.
     */
    function grantRole(bytes32 role, address account) public virtual override onlyRole(getRoleAdmin(role)) {
        _grantRole(role, account);
    }

    /**
     * @dev Revokes `role` from `account`.
     *
     * If `account` had been granted `role`, emits a {RoleRevoked} event.
     *
     * Requirements:
     *
     * - the caller must have ``role``'s admin role.
     *
     * May emit a {RoleRevoked} event.
     */
    function revokeRole(bytes32 role, address account) public virtual override onlyRole(getRoleAdmin(role)) {
        _revokeRole(role, account);
    }

    /**
     * @dev Revokes `role` from the calling account.
     *
     * Roles are often managed via {grantRole} and {revokeRole}: this function's
     * purpose is to provide a mechanism for accounts to lose their privileges
     * if they are compromised (such as when a trusted device is misplaced).
     *
     * If the calling account had been revoked `role`, emits a {RoleRevoked}
     * event.
     *
     * Requirements:
     *
     * - the caller must be `account`.
     *
     * May emit a {RoleRevoked} event.
     */
    function renounceRole(bytes32 role, address account) public virtual override {
        require(account == _msgSender(), "AccessControl: can only renounce roles for self");

        _revokeRole(role, account);
    }

    /**
     * @dev Grants `role` to `account`.
     *
     * If `account` had not been already granted `role`, emits a {RoleGranted}
     * event. Note that unlike {grantRole}, this function doesn't perform any
     * checks on the calling account.
     *
     * May emit a {RoleGranted} event.
     *
     * [WARNING]
     * ====
     * This function should only be called from the constructor when setting
     * up the initial roles for the system.
     *
     * Using this function in any other way is effectively circumventing the admin
     * system imposed by {AccessControl}.
     * ====
     *
     * NOTE: This function is deprecated in favor of {_grantRole}.
     */
    function _setupRole(bytes32 role, address account) internal virtual {
        _grantRole(role, account);
    }

    /**
     * @dev Sets `adminRole` as ``role``'s admin role.
     *
     * Emits a {RoleAdminChanged} event.
     */
    function _setRoleAdmin(bytes32 role, bytes32 adminRole) internal virtual {
        bytes32 previousAdminRole = getRoleAdmin(role);
        _roles[role].adminRole = adminRole;
        emit RoleAdminChanged(role, previousAdminRole, adminRole);
    }

    /**
     * @dev Grants `role` to `account`.
     *
     * Internal function without access restriction.
     *
     * May emit a {RoleGranted} event.
     */
    function _grantRole(bytes32 role, address account) internal virtual {
        if (!hasRole(role, account)) {
            _roles[role].members[account] = true;
            emit RoleGranted(role, account, _msgSender());
        }
    }

    /**
     * @dev Revokes `role` from `account`.
     *
     * Internal function without access restriction.
     *
     * May emit a {RoleRevoked} event.
     */
    function _revokeRole(bytes32 role, address account) internal virtual {
        if (hasRole(role, account)) {
            _roles[role].members[account] = false;
            emit RoleRevoked(role, account, _msgSender());
        }
    }

    /**
     * @dev This empty reserved space is put in place to allow future versions to add new
     * variables without shifting down storage in the inheritance chain.
     * See https://docs.openzeppelin.com/contracts/4.x/upgradeable#storage_gaps
     */
    uint256[49] private __gap;
}

/**
 * @dev Contract module which allows children to implement an emergency stop
 * mechanism that can be triggered by an authorized account.
 *
 * This module is used through inheritance. It will make available the
 * modifiers `whenNotPaused` and `whenPaused`, which can be applied to
 * the functions of your contract. Note that they will not be pausable by
 * simply including this module, only once the modifiers are put in place.
 */
abstract contract PausableUpgradeable is Initializable, ContextUpgradeable {
    /**
     * @dev Emitted when the pause is triggered by `account`.
     */
    event Paused(address account);

    /**
     * @dev Emitted when the pause is lifted by `account`.
     */
    event Unpaused(address account);

    bool private _paused;

    /**
     * @dev Initializes the contract in unpaused state.
     */
    function __Pausable_init() internal onlyInitializing {
        __Pausable_init_unchained();
    }

    function __Pausable_init_unchained() internal onlyInitializing {
        _paused = false;
    }

    /**
     * @dev Modifier to make a function callable only when the contract is not paused.
     *
     * Requirements:
     *
     * - The contract must not be paused.
     */
    modifier whenNotPaused() {
        _requireNotPaused();
        _;
    }

    /**
     * @dev Modifier to make a function callable only when the contract is paused.
     *
     * Requirements:
     *
     * - The contract must be paused.
     */
    modifier whenPaused() {
        _requirePaused();
        _;
    }

    /**
     * @dev Returns true if the contract is paused, and false otherwise.
     */
    function paused() public view virtual returns (bool) {
        return _paused;
    }

    /**
     * @dev Throws if the contract is paused.
     */
    function _requireNotPaused() internal view virtual {
        require(!paused(), "Pausable: paused");
    }

    /**
     * @dev Throws if the contract is not paused.
     */
    function _requirePaused() internal view virtual {
        require(paused(), "Pausable: not paused");
    }

    /**
     * @dev Triggers stopped state.
     *
     * Requirements:
     *
     * - The contract must not be paused.
     */
    function _pause() internal virtual whenNotPaused {
        _paused = true;
        emit Paused(_msgSender());
    }

    /**
     * @dev Returns to normal state.
     *
     * Requirements:
     *
     * - The contract must be paused.
     */
    function _unpause() internal virtual whenPaused {
        _paused = false;
        emit Unpaused(_msgSender());
    }

    /**
     * @dev This empty reserved space is put in place to allow future versions to add new
     * variables without shifting down storage in the inheritance chain.
     * See https://docs.openzeppelin.com/contracts/4.x/upgradeable#storage_gaps
     */
    uint256[49] private __gap;
}

/**
 * @title Pausable contract interface
 * @author CloudWalk Inc.
 * @dev Allows to trigger the paused or unpaused state of the contract.
 */
interface IPausable {
    // -------------------- Functions -----------------------------------

    /**
     * @dev Triggers the paused state of the contract.
     */
    function pause() external;

    /**
     * @dev Triggers the unpaused state of the contract.
     */
    function unpause() external;
}

/**
 * @title PausableExtUpgradeable base contract
 * @author CloudWalk Inc.
 * @dev Extends the OpenZeppelin's {PausableUpgradeable} contract by adding the {PAUSER_ROLE} role.
 *
 * This contract is used through inheritance. It introduces the {PAUSER_ROLE} role that is allowed to
 * trigger the paused or unpaused state of the contract that is inherited from this one.
 */
abstract contract PausableExtUpgradeable is AccessControlUpgradeable, PausableUpgradeable, IPausable {
    /// @dev The role of pauser that is allowed to trigger the paused or unpaused state of the contract.
    bytes32 public constant PAUSER_ROLE = keccak256("PAUSER_ROLE");

    // -------------------- Functions --------------------------------

    /**
     * @dev The internal initializer of the upgradable contract.
     *
     * See details https://docs.openzeppelin.com/upgrades-plugins/1.x/writing-upgradeable.
     */
    function __PausableExt_init(bytes32 pauserRoleAdmin) internal onlyInitializing {
        __Context_init_unchained();
        __ERC165_init_unchained();
        __AccessControl_init_unchained();
        __Pausable_init_unchained();

        __PausableExt_init_unchained(pauserRoleAdmin);
    }

    /**
     * @dev The unchained internal initializer of the upgradable contract.
     *
     * See {PausableExtUpgradeable-__PausableExt_init}.
     */
    function __PausableExt_init_unchained(bytes32 pauserRoleAdmin) internal onlyInitializing {
        _setRoleAdmin(PAUSER_ROLE, pauserRoleAdmin);
    }

    /**
     * @dev Triggers the paused state of the contract.
     *
     * Requirements:
     *
     * - The caller must have the {PAUSER_ROLE} role.
     */
    function pause() public onlyRole(PAUSER_ROLE) {
        _pause();
    }

    /**
     * @dev Triggers the unpaused state of the contract.
     *
     * Requirements:
     *
     * - The caller must have the {PAUSER_ROLE} role.
     */
    function unpause() public onlyRole(PAUSER_ROLE) {
        _unpause();
    }

    /**
     * @dev This empty reserved space is put in place to allow future versions to add new
     * variables without shifting down storage in the inheritance chain.
     */
    uint256[50] private __gap;
}

/**
 * @title Blacklistable contract interface
 * @author CloudWalk Inc.
 * @dev Allows to blacklist and unblacklist accounts.
 */
interface IBlacklistable {
    // -------------------- Events -----------------------------------

    /// @dev Emitted when an account is blacklisted.
    event Blacklisted(address indexed account);

    /// @dev Emitted when an account is unblacklisted.
    event UnBlacklisted(address indexed account);

    /// @dev Emitted when an account is self blacklisted.
    event SelfBlacklisted(address indexed account);

    // -------------------- Functions -----------------------------------

    /**
     * @dev Adds an account to the blacklist.
     *
     * Emits a {Blacklisted} event.
     *
     * @param account The address to add to the blacklist.
     */
    function blacklist(address account) external;

    /**
     * @dev Removes an account from the blacklist.
     *
     * Emits an {UnBlacklisted} event.
     *
     * @param account The address to remove from the blacklist.
     */
    function unBlacklist(address account) external;

    /**
     * @dev Adds the message sender to the blacklist.
     *
     * Emits a {SelfBlacklisted} event.
     */
    function selfBlacklist() external;

    /**
     * @dev Checks if an account is blacklisted.
     * @param account The address to check for presence in the blacklist.
     * @return True if the account is present in the blacklist.
     */
    function isBlacklisted(address account) external returns (bool);
}

/**
 * @title BlacklistableUpgradeable base contract
 * @author CloudWalk Inc.
 * @dev Allows to blacklist and unblacklist accounts using the {BLACKLISTER_ROLE} role.
 *
 * This contract is used through inheritance. It makes available the modifier `notBlacklisted`,
 * which can be applied to functions to restrict their usage to not blacklisted accounts only.
 */
abstract contract BlacklistableUpgradeable is AccessControlUpgradeable, IBlacklistable {
    /// @dev The role of the blacklister that is allowed to blacklist and unblacklist accounts.
    bytes32 public constant BLACKLISTER_ROLE = keccak256("BLACKLISTER_ROLE");

    /// @dev Mapping of presence in the blacklist for a given address.
    mapping(address => bool) private _blacklisted;

    // -------------------- Errors -----------------------------------

    /// @dev The account is blacklisted.
    error BlacklistedAccount(address account);

    // -------------------- Modifiers --------------------------------

    /**
     * @dev Throws if called by a blacklisted account.
     * @param account The address to check for presence in the blacklist.
     */
    modifier notBlacklisted(address account) {
        if (_blacklisted[account]) {
            revert BlacklistedAccount(account);
        }
        _;
    }

    // -------------------- Functions --------------------------------

    /**
     * @dev The internal initializer of the upgradable contract.
     *
     * See details https://docs.openzeppelin.com/upgrades-plugins/1.x/writing-upgradeable.
     */
    function __Blacklistable_init(bytes32 blacklisterRoleAdmin) internal onlyInitializing {
        __Context_init_unchained();
        __ERC165_init_unchained();
        __AccessControl_init_unchained();

        __Blacklistable_init_unchained(blacklisterRoleAdmin);
    }

    /**
     * @dev The unchained internal initializer of the upgradable contract.
     *
     * See {BlacklistableUpgradeable-__Blacklistable_init}.
     */
    function __Blacklistable_init_unchained(bytes32 blacklisterRoleAdmin) internal onlyInitializing {
        _setRoleAdmin(BLACKLISTER_ROLE, blacklisterRoleAdmin);
    }

    /**
     * @dev Adds an account to the blacklist.
     *
     * Requirements:
     *
     * - The caller must have the {BLACKLISTER_ROLE} role.
     *
     * Emits a {Blacklisted} event.
     *
     * @param account The address to blacklist.
     */
    function blacklist(address account) public onlyRole(BLACKLISTER_ROLE) {
        if (_blacklisted[account]) {
            return;
        }

        _blacklisted[account] = true;

        emit Blacklisted(account);
    }

    /**
     * @dev Removes an account from the blacklist.
     *
     * Requirements:
     *
     * - The caller must have the {BLACKLISTER_ROLE} role.
     *
     * Emits an {UnBlacklisted} event.
     *
     * @param account The address to remove from the blacklist.
     */
    function unBlacklist(address account) public onlyRole(BLACKLISTER_ROLE) {
        if (!_blacklisted[account]) {
            return;
        }

        _blacklisted[account] = false;

        emit UnBlacklisted(account);
    }

    /**
     * @dev Adds the message sender to the blacklist.
     *
     * Emits a {SelfBlacklisted} event.
     * Emits a {Blacklisted} event.
     */
    function selfBlacklist() public {
        address sender = _msgSender();

        if (_blacklisted[sender]) {
            return;
        }

        _blacklisted[sender] = true;

        emit SelfBlacklisted(sender);
        emit Blacklisted(sender);
    }

    /**
     * @dev Checks if an account is blacklisted.
     * @param account The address to check for presence in the blacklist.
     * @return True if the account is present in the blacklist.
     */
    function isBlacklisted(address account) public view returns (bool) {
        return _blacklisted[account];
    }

    /**
     * @dev This empty reserved space is put in place to allow future versions to add new
     * variables without shifting down storage in the inheritance chain.
     */
    uint256[49] private __gap;
}

/**
 * @dev Interface of the ERC20 standard as defined in the EIP.
 */
interface IERC20Upgradeable {
    /**
     * @dev Emitted when `value` tokens are moved from one account (`from`) to
     * another (`to`).
     *
     * Note that `value` may be zero.
     */
    event Transfer(address indexed from, address indexed to, uint256 value);

    /**
     * @dev Emitted when the allowance of a `spender` for an `owner` is set by
     * a call to {approve}. `value` is the new allowance.
     */
    event Approval(address indexed owner, address indexed spender, uint256 value);

    /**
     * @dev Returns the amount of tokens in existence.
     */
    function totalSupply() external view returns (uint256);

    /**
     * @dev Returns the amount of tokens owned by `account`.
     */
    function balanceOf(address account) external view returns (uint256);

    /**
     * @dev Moves `amount` tokens from the caller's account to `to`.
     *
     * Returns a boolean value indicating whether the operation succeeded.
     *
     * Emits a {Transfer} event.
     */
    function transfer(address to, uint256 amount) external returns (bool);

    /**
     * @dev Returns the remaining number of tokens that `spender` will be
     * allowed to spend on behalf of `owner` through {transferFrom}. This is
     * zero by default.
     *
     * This value changes when {approve} or {transferFrom} are called.
     */
    function allowance(address owner, address spender) external view returns (uint256);

    /**
     * @dev Sets `amount` as the allowance of `spender` over the caller's tokens.
     *
     * Returns a boolean value indicating whether the operation succeeded.
     *
     * IMPORTANT: Beware that changing an allowance with this method brings the risk
     * that someone may use both the old and the new allowance by unfortunate
     * transaction ordering. One possible solution to mitigate this race
     * condition is to first reduce the spender's allowance to 0 and set the
     * desired value afterwards:
     * https://github.com/ethereum/EIPs/issues/20#issuecomment-263524729
     *
     * Emits an {Approval} event.
     */
    function approve(address spender, uint256 amount) external returns (bool);

    /**
     * @dev Moves `amount` tokens from `from` to `to` using the
     * allowance mechanism. `amount` is then deducted from the caller's
     * allowance.
     *
     * Returns a boolean value indicating whether the operation succeeded.
     *
     * Emits a {Transfer} event.
     */
    function transferFrom(
        address from,
        address to,
        uint256 amount
    ) external returns (bool);
}

/**
 * @title Rescuable contract interface
 * @author CloudWalk Inc.
 * @dev Allows to rescue ERC20 tokens locked up in the contract.
 */
interface IRescuable {
    // -------------------- Functions -----------------------------------

    /**
     * @dev Withdraws ERC20 tokens locked up in the contract.
     * @param token The address of the ERC20 token contract.
     * @param to The address of the recipient of tokens.
     * @param amount The amount of tokens to withdraw.
     */
    function rescueERC20(address token, address to, uint256 amount) external;
}

/**
 * @dev Interface of the ERC20 Permit extension allowing approvals to be made via signatures, as defined in
 * https://eips.ethereum.org/EIPS/eip-2612[EIP-2612].
 *
 * Adds the {permit} method, which can be used to change an account's ERC20 allowance (see {IERC20-allowance}) by
 * presenting a message signed by the account. By not relying on {IERC20-approve}, the token holder account doesn't
 * need to send a transaction, and thus is not required to hold Ether at all.
 */
interface IERC20PermitUpgradeable {
    /**
     * @dev Sets `value` as the allowance of `spender` over ``owner``'s tokens,
     * given ``owner``'s signed approval.
     *
     * IMPORTANT: The same issues {IERC20-approve} has related to transaction
     * ordering also apply here.
     *
     * Emits an {Approval} event.
     *
     * Requirements:
     *
     * - `spender` cannot be the zero address.
     * - `deadline` must be a timestamp in the future.
     * - `v`, `r` and `s` must be a valid `secp256k1` signature from `owner`
     * over the EIP712-formatted function arguments.
     * - the signature must use ``owner``'s current nonce (see {nonces}).
     *
     * For more information on the signature format, see the
     * https://eips.ethereum.org/EIPS/eip-2612#specification[relevant EIP
     * section].
     */
    function permit(
        address owner,
        address spender,
        uint256 value,
        uint256 deadline,
        uint8 v,
        bytes32 r,
        bytes32 s
    ) external;

    /**
     * @dev Returns the current nonce for `owner`. This value must be
     * included whenever a signature is generated for {permit}.
     *
     * Every successful call to {permit} increases ``owner``'s nonce by one. This
     * prevents a signature from being used multiple times.
     */
    function nonces(address owner) external view returns (uint256);

    /**
     * @dev Returns the domain separator used in the encoding of the signature for {permit}, as defined by {EIP712}.
     */
    // solhint-disable-next-line func-name-mixedcase
    function DOMAIN_SEPARATOR() external view returns (bytes32);
}

/**
 * @title SafeERC20
 * @dev Wrappers around ERC20 operations that throw on failure (when the token
 * contract returns false). Tokens that return no value (and instead revert or
 * throw on failure) are also supported, non-reverting calls are assumed to be
 * successful.
 * To use this library you can add a `using SafeERC20 for IERC20;` statement to your contract,
 * which allows you to call the safe operations as `token.safeTransfer(...)`, etc.
 */
library SafeERC20Upgradeable {
    using AddressUpgradeable for address;

    function safeTransfer(
        IERC20Upgradeable token,
        address to,
        uint256 value
    ) internal {
        _callOptionalReturn(token, abi.encodeWithSelector(token.transfer.selector, to, value));
    }

    function safeTransferFrom(
        IERC20Upgradeable token,
        address from,
        address to,
        uint256 value
    ) internal {
        _callOptionalReturn(token, abi.encodeWithSelector(token.transferFrom.selector, from, to, value));
    }

    /**
     * @dev Deprecated. This function has issues similar to the ones found in
     * {IERC20-approve}, and its usage is discouraged.
     *
     * Whenever possible, use {safeIncreaseAllowance} and
     * {safeDecreaseAllowance} instead.
     */
    function safeApprove(
        IERC20Upgradeable token,
        address spender,
        uint256 value
    ) internal {
        // safeApprove should only be called when setting an initial allowance,
        // or when resetting it to zero. To increase and decrease it, use
        // 'safeIncreaseAllowance' and 'safeDecreaseAllowance'
        require(
            (value == 0) || (token.allowance(address(this), spender) == 0),
            "SafeERC20: approve from non-zero to non-zero allowance"
        );
        _callOptionalReturn(token, abi.encodeWithSelector(token.approve.selector, spender, value));
    }

    function safeIncreaseAllowance(
        IERC20Upgradeable token,
        address spender,
        uint256 value
    ) internal {
        uint256 newAllowance = token.allowance(address(this), spender) + value;
        _callOptionalReturn(token, abi.encodeWithSelector(token.approve.selector, spender, newAllowance));
    }

    function safeDecreaseAllowance(
        IERC20Upgradeable token,
        address spender,
        uint256 value
    ) internal {
        unchecked {
            uint256 oldAllowance = token.allowance(address(this), spender);
            require(oldAllowance >= value, "SafeERC20: decreased allowance below zero");
            uint256 newAllowance = oldAllowance - value;
            _callOptionalReturn(token, abi.encodeWithSelector(token.approve.selector, spender, newAllowance));
        }
    }

    function safePermit(
        IERC20PermitUpgradeable token,
        address owner,
        address spender,
        uint256 value,
        uint256 deadline,
        uint8 v,
        bytes32 r,
        bytes32 s
    ) internal {
        uint256 nonceBefore = token.nonces(owner);
        token.permit(owner, spender, value, deadline, v, r, s);
        uint256 nonceAfter = token.nonces(owner);
        require(nonceAfter == nonceBefore + 1, "SafeERC20: permit did not succeed");
    }

    /**
     * @dev Imitates a Solidity high-level call (i.e. a regular function call to a contract), relaxing the requirement
     * on the return value: the return value is optional (but if data is returned, it must not be false).
     * @param token The token targeted by the call.
     * @param data The call data (encoded using abi.encode or one of its variants).
     */
    function _callOptionalReturn(IERC20Upgradeable token, bytes memory data) private {
        // We need to perform a low level call here, to bypass Solidity's return data size checking mechanism, since
        // we're implementing it ourselves. We use {Address.functionCall} to perform this call, which verifies that
        // the target address contains contract code and also asserts for success in the low-level call.

        bytes memory returndata = address(token).functionCall(data, "SafeERC20: low-level call failed");
        if (returndata.length > 0) {
            // Return data is optional
            require(abi.decode(returndata, (bool)), "SafeERC20: ERC20 operation did not succeed");
        }
    }
}

/**
 * @title RescuableUpgradeable base contract
 * @author CloudWalk Inc.
 * @dev Allows to rescue ERC20 tokens locked up in the contract using the {RESCUER_ROLE} role.
 *
 * This contract is used through inheritance. It introduces the {RESCUER_ROLE} role that is allowed to
 * rescue tokens locked up in the contract that is inherited from this one.
 */
abstract contract RescuableUpgradeable is AccessControlUpgradeable, IRescuable {
    using SafeERC20Upgradeable for IERC20Upgradeable;

    /// @dev The role of rescuer that is allowed to rescue tokens locked up in the contract.
    bytes32 public constant RESCUER_ROLE = keccak256("RESCUER_ROLE");

    // -------------------- Functions --------------------------------

    /**
     * @dev The internal initializer of the upgradable contract.
     *
     * See details https://docs.openzeppelin.com/upgrades-plugins/1.x/writing-upgradeable.
     */
    function __Rescuable_init(bytes32 rescuerRoleAdmin) internal onlyInitializing {
        __Context_init_unchained();
        __ERC165_init_unchained();
        __AccessControl_init_unchained();

        __Rescuable_init_unchained(rescuerRoleAdmin);
    }

    /**
     * @dev The unchained internal initializer of the upgradable contract.
     *
     * See {RescuableUpgradeable-__Rescuable_init}.
     */
    function __Rescuable_init_unchained(bytes32 rescuerRoleAdmin) internal onlyInitializing {
        _setRoleAdmin(RESCUER_ROLE, rescuerRoleAdmin);
    }

    /**
     * @dev Withdraws ERC20 tokens locked up in the contract.
     *
     * Requirements:
     *
     * - The caller must have the {RESCUER_ROLE} role.
     *
     * @param token The address of the ERC20 token contract.
     * @param to The address of the recipient of tokens.
     * @param amount The amount of tokens to withdraw.
     */
    function rescueERC20(
        address token,
        address to,
        uint256 amount
    ) public onlyRole(RESCUER_ROLE) {
        IERC20Upgradeable(token).safeTransfer(to, amount);
    }

    /**
     * @dev This empty reserved space is put in place to allow future versions to add new
     * variables without shifting down storage in the inheritance chain.
     */
    uint256[50] private __gap;
}

/**
 * @title StoragePlaceholder200 base contract
 * @author CloudWalk Inc.
 * @dev Reserves 200 storage slots.
 * Such a storage placeholder contract allows future replacement of it with other contracts
 * without shifting down storage in the inheritance chain.
 *
 * E.g. the following code:
 * ```
 * abstract contract StoragePlaceholder200 {
 *     uint256[200] private __gap;
 * }
 *
 * contract A is B, StoragePlaceholder200, C {
 *     //Some implementation
 * }
 * ```
 * can be replaced with the following code without a storage shifting issue:
 * ```
 * abstract contract StoragePlaceholder150 {
 *     uint256[150] private __gap;
 * }
 *
 * abstract contract X {
 *     uint256[50] public values;
 *     // No more storage variables. Some set of functions should be here.
 * }
 *
 * contract A is B, X, StoragePlaceholder150, C {
 *     //Some implementation
 * }
 * ```
 */
abstract contract StoragePlaceholder200 {
    uint256[200] private __gap;
}

/**
 * @title PixCashier contract
 * @dev Wrapper contract for PIX cash-in and cash-out operations.
 *
 * Only accounts that have {CASHIER_ROLE} role can execute the cash-in operations and process the cash-out operations.
 * About roles see https://docs.openzeppelin.com/contracts/4.x/api/access#AccessControl.
 */
contract PixCashier is
    AccessControlUpgradeable,
    BlacklistableUpgradeable,
    PausableExtUpgradeable,
    RescuableUpgradeable,
    StoragePlaceholder200,
    PixCashierStorage,
    IPixCashier
{
    using SafeERC20Upgradeable for IERC20Upgradeable;
    using EnumerableSetUpgradeable for EnumerableSetUpgradeable.Bytes32Set;

    /// @dev The role of this contract owner.
    bytes32 public constant OWNER_ROLE = keccak256("OWNER_ROLE");

    /// @dev The role of cashier that is allowed to execute the cash-in operations.
    bytes32 public constant CASHIER_ROLE = keccak256("CASHIER_ROLE");

    // -------------------- Errors -----------------------------------

    /// @dev The zero token address has been passed as a function argument.
    error ZeroTokenAddress();

    /// @dev The zero account has been passed as a function argument.
    error ZeroAccount();

    /// @dev The zero token amount has been passed as a function argument.
    error ZeroAmount();

    /// @dev The zero off-chain transaction identifier has been passed as a function argument.
    error ZeroTxId();

    /// @dev The zero off-chain transaction batch identifier has been passed as a function argument.
    error ZeroBatchId();

    /// @dev An empty array of off-chain transaction identifiers has been passed as a function argument.
    error EmptyTransactionIdsArray();

    /// @dev The minting of tokens failed when processing an `cashIn` operation.
    error TokenMintingFailure();

    /// @dev The length of the one of the batch arrays is different to the others.
    error InvalidBatchArrays();

    /**
     * @dev The cash-in operation with the provided off-chain transaction is already executed.
     * @param txId The off-chain transaction identifiers of the operation.
     */
    error CashInAlreadyExecuted(bytes32 txId);

    /**
     * @dev The cash-in batch operation with the provided off-chain transaction is already executed.
     * @param batchId The off-chain transaction identifiers of the operation.
     */
    error CashInBatchAlreadyExecuted(bytes32 batchId);

    /**
     * @dev The cash-out operation with the provided off-chain transaction identifier has an inappropriate status.
     * @param txId The off-chain transaction identifiers of the operation.
     * @param status The current status of the operation.
     */
    error InappropriateCashOutStatus(bytes32 txId, CashOutStatus status);

    /**
     * @dev The cash-out operation with the provided txId cannot executed for the given account.
     * @param txId The off-chain transaction identifiers of the operation.
     * @param account The account that must be used for the operation.
     */
    error InappropriateCashOutAccount(bytes32 txId, address account);

    // -------------------- Functions --------------------------------

    /**
     * @dev The initialize function of the upgradable contract.
     * See details https://docs.openzeppelin.com/upgrades-plugins/1.x/writing-upgradeable
     *
     * Requirements:
     *
     * - The passed token address must not be zero.
     *
     * @param token_ The address of the token to set as the underlying one.
     */
    function initialize(address token_) external initializer {
        __PixCashier_init(token_);
    }

    function __PixCashier_init(address token_) internal onlyInitializing {
        __Context_init_unchained();
        __ERC165_init_unchained();
        __AccessControl_init_unchained();
        __Blacklistable_init_unchained(OWNER_ROLE);
        __Pausable_init_unchained();
        __PausableExt_init_unchained(OWNER_ROLE);
        __Rescuable_init_unchained(OWNER_ROLE);

        __PixCashier_init_unchained(token_);
    }

    function __PixCashier_init_unchained(address token_) internal onlyInitializing {
        if (token_ == address(0)) {
            revert ZeroTokenAddress();
        }

        _token = token_;

        _setRoleAdmin(OWNER_ROLE, OWNER_ROLE);
        _setRoleAdmin(CASHIER_ROLE, OWNER_ROLE);

        _setupRole(OWNER_ROLE, _msgSender());
    }

    /**
     * @dev See {IPixCashier-underlyingToken}.
     */
    function underlyingToken() external view returns (address) {
        return _token;
    }

    /**
     * @dev See {IPixCashier-cashOutBalanceOf}.
     */
    function cashOutBalanceOf(address account) external view returns (uint256) {
        return _cashOutBalances[account];
    }

    /**
     * @dev See {IPixCashier-pendingCashOutCounter}.
     */
    function pendingCashOutCounter() external view returns (uint256) {
        return _pendingCashOutTxIds.length();
    }

    /**
     * @dev See {IPixCashier-processedCashOutCounter}.
     */
    function processedCashOutCounter() external view returns (uint256) {
        return _processedCashOutCounter;
    }

    /**
     * See {IPixCashier-getPendingCashOutTxIds}.
     */
    function getPendingCashOutTxIds(uint256 index, uint256 limit) external view returns (bytes32[] memory txIds) {
        uint256 len = _pendingCashOutTxIds.length();
        if (len <= index || limit == 0) {
            txIds = new bytes32[](0);
        } else {
            len -= index;
            if (len > limit) {
                len = limit;
            }
            txIds = new bytes32[](len);
            for (uint256 i = 0; i < len; i++) {
                txIds[i] = _pendingCashOutTxIds.at(index);
                index++;
            }
        }
    }

    /**
     * @dev See {IPixCashier-getCashIn}.
     */
    function getCashIn(bytes32 txId) external view returns (CashInOperation memory) {
        return _cashIns[txId];
    }

    /**
     * @dev See {IPixCashier-getCashIns}.
     */
    function getCashIns(bytes32[] memory txIds) external view returns (CashInOperation[] memory cashIns) {
        uint256 len = txIds.length;
        cashIns = new CashInOperation[](len);
        for (uint256 i = 0; i < len; i++) {
            cashIns[i] = _cashIns[txIds[i]];
        }
    }

    /**
     * @dev See {IPixCashier-getCashInBatch}.
     */
    function getCashInBatch(bytes32 batchId) external view returns (CashInBatchOperation memory) {
        return _cashInBatches[batchId];
    }

    /**
     * @dev See {IPixCashier-getCashInBatches}.
     */
    function getCashInBatches(
        bytes32[] memory batchIds
    ) external view returns (CashInBatchOperation[] memory cashInBatches) {
        uint256 len = batchIds.length;
        cashInBatches = new CashInBatchOperation[](len);
        for (uint256 i = 0; i < len; i++) {
            cashInBatches[i] = _cashInBatches[batchIds[i]];
        }
    }

    /**
     * @dev See {IPixCashier-getCashOut}.
     */
    function getCashOut(bytes32 txIds) external view returns (CashOut memory) {
        return _cashOuts[txIds];
    }

    /**
     * @dev See {IPixCashier-getCashOuts}.
     */
    function getCashOuts(bytes32[] memory txIds) external view returns (CashOut[] memory cashOuts) {
        uint256 len = txIds.length;
        cashOuts = new CashOut[](len);
        for (uint256 i = 0; i < len; i++) {
            cashOuts[i] = _cashOuts[txIds[i]];
        }
    }

    /**
     * @dev See {IPixCashier-cashIn}.
     *
     * Requirements:
     *
     * - The contract must not be paused.
     * - The caller must have the {CASHIER_ROLE} role.
     * - The provided `account`, `amount`, and `txId` values must not be zero.
     * - The cash-in operation with the provided `txId` must not be already executed.
     */
    function cashIn(
        address account,
        uint256 amount,
        bytes32 txId
    ) external whenNotPaused onlyRole(CASHIER_ROLE) {
        _cashIn(account, amount, txId, CashInExecutionPolicy.Revert);
    }

    /**
     * @dev See {IPixCashier-cashInBatch}.
     *
     * Requirements
     *
     * - The length of each passed array must be equal.
     * - The contract must not be paused.
     * - The caller must have the {CASHIER_ROLE} role.
     * - The provided `account`, `amount`, and `txId` values must not be zero.
     * - The provided `accounts`, `amounts`, `txIds` arrays must not be empty and must have the same length.
     * - The provided `batchId` must not be zero
     * - The cash-in batch operation with the provided `batchId` must not be already executed.
     * - Each cash-in operation with the provided identifier from the `txIds` array must not be already executed.
     */
    function cashInBatch(
        address[] memory accounts,
        uint256[] memory amounts,
        bytes32[] memory txIds,
        bytes32 batchId
    ) external whenNotPaused onlyRole(CASHIER_ROLE) {
        if (accounts.length == 0 || accounts.length != amounts.length || accounts.length != txIds.length) {
            revert InvalidBatchArrays();
        }
        if (_cashInBatches[batchId].status == CashInBatchStatus.Executed) {
            revert CashInBatchAlreadyExecuted(batchId);
        }
        if (batchId == 0) {
            revert ZeroBatchId();
        }

        CashInExecutionResult[] memory executionResults = new CashInExecutionResult[](txIds.length);

        for (uint256 i = 0; i < accounts.length; i++) {
            executionResults[i] = _cashIn(accounts[i], amounts[i], txIds[i], CashInExecutionPolicy.Skip);
        }

        _cashInBatches[batchId].status = CashInBatchStatus.Executed;

        emit CashInBatch(batchId, txIds, executionResults);
    }

    /**
     * @dev See {IPixCashier-requestCashOutFrom}.
     *
     * Requirements:
     *
     * - The contract must not be paused.
     * - The caller must have the {CASHIER_ROLE} role.
     * - The `account` must not be blacklisted.
     * - The `account`, `amount`, and `txId` values must not be zero.
     * - The cash-out operation with the provided `txId` must not be already pending.
     */
    function requestCashOutFrom(
        address account,
        uint256 amount,
        bytes32 txId
    ) external whenNotPaused onlyRole(CASHIER_ROLE) {
        _requestCashOut(_msgSender(), account, amount, txId);
    }

    /**
     * @dev See {IPixCashier-requestCashOutFromBatch}.
     *
     * Requirements:
     *
     * - The contract must not be paused.
     * - The caller must have the {CASHIER_ROLE} role.
     * - Each `account` in the provided array must not be blacklisted.
     * - Each `account`, `amount`, and `txId` values in the provided arrays must not be zero.
     * - Each cash-out operation with the provided `txId` in the array must not be already pending.
     */
    function requestCashOutFromBatch(
        address[] memory accounts,
        uint256[] memory amounts,
        bytes32[] memory txIds
    ) external whenNotPaused onlyRole(CASHIER_ROLE) {
        if (accounts.length != amounts.length || accounts.length != txIds.length) {
            revert InvalidBatchArrays();
        }

        for (uint256 i = 0; i < accounts.length; i++) {
            _requestCashOut(_msgSender(), accounts[i], amounts[i], txIds[i]);
        }
    }

    /**
     * @dev See {IPixCashier-confirmCashOut}.
     *
     * Requirements:
     *
     * - The contract must not be paused.
     * - The caller must have the {CASHIER_ROLE} role.
     * - The provided `txId` value must not be zero.
     * - The cash-out operation corresponded the provided `txId` value must have the pending status.
     */
    function confirmCashOut(bytes32 txId) external whenNotPaused onlyRole(CASHIER_ROLE) {
        _processCashOut(txId, CashOutStatus.Confirmed);
    }

    /**
     * @dev See {IPixCashier-confirmCashOutBatch}.
     *
     * Requirements:
     *
     * - The contract must not be paused.
     * - The caller must have the {CASHIER_ROLE} role.
     * - The input `txIds` array must not be empty.
     * - All the values in the input `txIds` array must not be zero.
     * - All the cash-out operations corresponded the values in the input `txIds` array must have the pending status.
     */
    function confirmCashOutBatch(bytes32[] memory txIds) external whenNotPaused onlyRole(CASHIER_ROLE) {
        uint256 len = txIds.length;
        if (len == 0) {
            revert EmptyTransactionIdsArray();
        }

        for (uint256 i = 0; i < len; i++) {
            _processCashOut(txIds[i], CashOutStatus.Confirmed);
        }
    }

    /**
     * @dev See {IPixCashier-reverseCashOut}.
     *
     * Requirements:
     *
     * - The contract must not be paused.
     * - The caller must have the {CASHIER_ROLE} role.
     * - The provided `txId` value must not be zero.
     * - The cash-out operation corresponded the provided `txId` value must have the pending status.
     */
    function reverseCashOut(bytes32 txId) external whenNotPaused onlyRole(CASHIER_ROLE) {
        _processCashOut(txId, CashOutStatus.Reversed);
    }

    /**
     * @dev See {IPixCashier-reverseCashOutBatch}.
     *
     * Requirements:
     *
     * - The contract must not be paused.
     * - The caller must have the {CASHIER_ROLE} role.
     * - The input `txIds` array must not be empty.
     * - All the values in the input `txIds` array must not be zero.
     * - All the cash-out operations corresponded the values in the input `txIds` array must have the pending status.
     */
    function reverseCashOutBatch(bytes32[] memory txIds) external whenNotPaused onlyRole(CASHIER_ROLE) {
        uint256 len = txIds.length;
        if (len == 0) {
            revert EmptyTransactionIdsArray();
        }

        for (uint256 i = 0; i < len; i++) {
            _processCashOut(txIds[i], CashOutStatus.Reversed);
        }
    }

    /**
     * @dev See {PixCashier-cashIn}.
     */
    function _cashIn(
        address account,
        uint256 amount,
        bytes32 txId,
        CashInExecutionPolicy policy
    ) internal returns (CashInExecutionResult){
        if (account == address(0)) {
            revert ZeroAccount();
        }
        if (amount == 0) {
            revert ZeroAmount();
        }
        if (txId == 0) {
            revert ZeroTxId();
        }
        if (isBlacklisted(account)) {
            revert BlacklistedAccount(account);
        }

        if (_cashIns[txId].status != CashInStatus.Nonexistent) {
            if (policy == CashInExecutionPolicy.Skip) {
                return CashInExecutionResult.AlreadyExecuted;
            } else {
                revert CashInAlreadyExecuted(txId);
            }
        }

        _cashIns[txId] = CashInOperation({
            status: CashInStatus.Executed,
            account: account,
            amount: amount
        });

        emit CashIn(account, amount, txId);

        if (!IERC20Mintable(_token).mint(account, amount)) {
            revert TokenMintingFailure();
        }

        return CashInExecutionResult.Success;
    }

    /**
     * @dev See {PixCashier-requestCashOut}.
     */
    function _requestCashOut(
        address sender,
        address account,
        uint256 amount,
        bytes32 txId
    ) internal {
        if (account == address(0)) {
            revert ZeroAccount();
        }
        if (amount == 0) {
            revert ZeroAmount();
        }
        if (txId == 0) {
            revert ZeroTxId();
        }
        if (isBlacklisted(account)) {
            revert BlacklistedAccount(account);
        }

        CashOut storage operation = _cashOuts[txId];
        CashOutStatus status = operation.status;
        if (status == CashOutStatus.Pending || status == CashOutStatus.Confirmed) {
            revert InappropriateCashOutStatus(txId, status);
        } else if (status == CashOutStatus.Reversed && operation.account != account) {
            revert InappropriateCashOutAccount(txId, operation.account);
        }

        operation.account = account;
        operation.amount = amount;
        operation.status = CashOutStatus.Pending;

        uint256 newCashOutBalance = _cashOutBalances[account] + amount;
        _cashOutBalances[account] = newCashOutBalance;
        _pendingCashOutTxIds.add(txId);

        emit RequestCashOut(
            account,
            amount,
            newCashOutBalance,
            txId,
            sender
        );

        IERC20Upgradeable(_token).safeTransferFrom(
            account,
            address(this),
            amount
        );
    }

    /**
     * @dev See {PixCashier-confirmCashOut} and {PixCashier-reverseCashOut}.
     */
    function _processCashOut(bytes32 txId, CashOutStatus targetStatus) internal {
        if (txId == 0) {
            revert ZeroTxId();
        }

        CashOut storage operation = _cashOuts[txId];
        CashOutStatus status = operation.status;
        if (status != CashOutStatus.Pending) {
            revert InappropriateCashOutStatus(txId, status);
        }

        address account = operation.account;
        uint256 amount = operation.amount;
        uint256 newCashOutBalance = _cashOutBalances[account] - amount;

        _cashOutBalances[account] = newCashOutBalance;
        _pendingCashOutTxIds.remove(txId);
        _processedCashOutCounter += 1;

        operation.status = targetStatus;

        if (targetStatus == CashOutStatus.Confirmed) {
            emit ConfirmCashOut(
                account,
                amount,
                newCashOutBalance,
                txId
            );
            IERC20Mintable(_token).burn(amount);
        } else {
            emit ReverseCashOut(
                account,
                amount,
                newCashOutBalance,
                txId
            );
            IERC20Upgradeable(_token).safeTransfer(account, amount);
        }
    }
}
