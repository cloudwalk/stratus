// SPDX-License-Identifier: MIT

pragma solidity 0.8.16;

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
     *
     * Furthermore, `isContract` will also return true if the target contract within
     * the same transaction is already scheduled for destruction by `SELFDESTRUCT`,
     * which only has an effect at the end of a transaction.
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
     * https://consensys.net/diligence/blog/2019/09/stop-using-soliditys-transfer-now/[Learn more].
     *
     * IMPORTANT: because control is transferred to `recipient`, care must be
     * taken to not create reentrancy vulnerabilities. Consider using
     * {ReentrancyGuard} or the
     * https://solidity.readthedocs.io/en/v0.8.0/security-considerations.html#use-the-checks-effects-interactions-pattern[checks-effects-interactions pattern].
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
        return functionCallWithValue(target, data, 0, "Address: low-level call failed");
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
    function functionCallWithValue(address target, bytes memory data, uint256 value) internal returns (bytes memory) {
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
        (bool success, bytes memory returndata) = target.call{value: value}(data);
        return verifyCallResultFromTarget(target, success, returndata, errorMessage);
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
        (bool success, bytes memory returndata) = target.staticcall(data);
        return verifyCallResultFromTarget(target, success, returndata, errorMessage);
    }

    /**
     * @dev Same as {xref-Address-functionCall-address-bytes-}[`functionCall`],
     * but performing a delegate call.
     *
     * _Available since v3.4._
     */
    function functionDelegateCall(address target, bytes memory data) internal returns (bytes memory) {
        return functionDelegateCall(target, data, "Address: low-level delegate call failed");
    }

    /**
     * @dev Same as {xref-Address-functionCall-address-bytes-string-}[`functionCall`],
     * but performing a delegate call.
     *
     * _Available since v3.4._
     */
    function functionDelegateCall(
        address target,
        bytes memory data,
        string memory errorMessage
    ) internal returns (bytes memory) {
        (bool success, bytes memory returndata) = target.delegatecall(data);
        return verifyCallResultFromTarget(target, success, returndata, errorMessage);
    }

    /**
     * @dev Tool to verify that a low level call to smart-contract was successful, and revert (either by bubbling
     * the revert reason or using the provided one) in case of unsuccessful call or if target was not a contract.
     *
     * _Available since v4.8._
     */
    function verifyCallResultFromTarget(
        address target,
        bool success,
        bytes memory returndata,
        string memory errorMessage
    ) internal view returns (bytes memory) {
        if (success) {
            if (returndata.length == 0) {
                // only check isContract if the call was successful and the return data is empty
                // otherwise we already know that it was a contract
                require(isContract(target), "Address: call to non-contract");
            }
            return returndata;
        } else {
            _revert(returndata, errorMessage);
        }
    }

    /**
     * @dev Tool to verify that a low level call was successful, and revert if it wasn't, either by bubbling the
     * revert reason or using the provided one.
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
            _revert(returndata, errorMessage);
        }
    }

    function _revert(bytes memory returndata, string memory errorMessage) private pure {
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
 * ```solidity
 * contract MyToken is ERC20Upgradeable {
 *     function initialize() initializer public {
 *         __ERC20_init("MyToken", "MTK");
 *     }
 * }
 *
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
     * `onlyInitializing` functions can be used to initialize parent contracts.
     *
     * Similar to `reinitializer(1)`, except that functions marked with `initializer` can be nested in the context of a
     * constructor.
     *
     * Emits an {Initialized} event.
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
     * A reinitializer may be used after the original initialization step. This is essential to configure modules that
     * are added through upgrades and that require initialization.
     *
     * When `version` is 1, this modifier is similar to `initializer`, except that functions marked with `reinitializer`
     * cannot be nested. If one is invoked in the context of another, execution will revert.
     *
     * Note that versions can jump in increments greater than 1; this implies that if multiple reinitializers coexist in
     * a contract, executing them in the right order is up to the developer or operator.
     *
     * WARNING: setting the version to 255 will prevent any future reinitialization.
     *
     * Emits an {Initialized} event.
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
     *
     * Emits an {Initialized} event the first time it is successfully executed.
     */
    function _disableInitializers() internal virtual {
        require(!_initializing, "Initializable: contract is initializing");
        if (_initialized != type(uint8).max) {
            _initialized = type(uint8).max;
            emit Initialized(type(uint8).max);
        }
    }

    /**
     * @dev Returns the highest version that has been initialized. See {reinitializer}.
     */
    function _getInitializedVersion() internal view returns (uint8) {
        return _initialized;
    }

    /**
     * @dev Returns `true` if the contract is currently initializing. See {onlyInitializing}.
     */
    function _isInitializing() internal view returns (bool) {
        return _initializing;
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
 * @dev Contract module which provides a basic access control mechanism, where
 * there is an account (an owner) that can be granted exclusive access to
 * specific functions.
 *
 * By default, the owner account will be the one that deploys the contract. This
 * can later be changed with {transferOwnership}.
 *
 * This module is used through inheritance. It will make available the modifier
 * `onlyOwner`, which can be applied to your functions to restrict their use to
 * the owner.
 */
abstract contract OwnableUpgradeable is Initializable, ContextUpgradeable {
    address private _owner;

    event OwnershipTransferred(address indexed previousOwner, address indexed newOwner);

    /**
     * @dev Initializes the contract setting the deployer as the initial owner.
     */
    function __Ownable_init() internal onlyInitializing {
        __Ownable_init_unchained();
    }

    function __Ownable_init_unchained() internal onlyInitializing {
        _transferOwnership(_msgSender());
    }

    /**
     * @dev Throws if called by any account other than the owner.
     */
    modifier onlyOwner() {
        _checkOwner();
        _;
    }

    /**
     * @dev Returns the address of the current owner.
     */
    function owner() public view virtual returns (address) {
        return _owner;
    }

    /**
     * @dev Throws if the sender is not the owner.
     */
    function _checkOwner() internal view virtual {
        require(owner() == _msgSender(), "Ownable: caller is not the owner");
    }

    /**
     * @dev Leaves the contract without owner. It will not be possible to call
     * `onlyOwner` functions. Can only be called by the current owner.
     *
     * NOTE: Renouncing ownership will leave the contract without an owner,
     * thereby disabling any functionality that is only available to the owner.
     */
    function renounceOwnership() public virtual onlyOwner {
        _transferOwnership(address(0));
    }

    /**
     * @dev Transfers ownership of the contract to a new account (`newOwner`).
     * Can only be called by the current owner.
     */
    function transferOwnership(address newOwner) public virtual onlyOwner {
        require(newOwner != address(0), "Ownable: new owner is the zero address");
        _transferOwnership(newOwner);
    }

    /**
     * @dev Transfers ownership of the contract to a new account (`newOwner`).
     * Internal function without access restriction.
     */
    function _transferOwnership(address newOwner) internal virtual {
        address oldOwner = _owner;
        _owner = newOwner;
        emit OwnershipTransferred(oldOwner, newOwner);
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
    function transferFrom(address from, address to, uint256 amount) external returns (bool);
}

/**
 * @dev Interface for the optional metadata functions from the ERC20 standard.
 *
 * _Available since v4.1._
 */
interface IERC20MetadataUpgradeable is IERC20Upgradeable {
    /**
     * @dev Returns the name of the token.
     */
    function name() external view returns (string memory);

    /**
     * @dev Returns the symbol of the token.
     */
    function symbol() external view returns (string memory);

    /**
     * @dev Returns the decimals places of the token.
     */
    function decimals() external view returns (uint8);
}

/**
 * @dev Implementation of the {IERC20} interface.
 *
 * This implementation is agnostic to the way tokens are created. This means
 * that a supply mechanism has to be added in a derived contract using {_mint}.
 * For a generic mechanism see {ERC20PresetMinterPauser}.
 *
 * TIP: For a detailed writeup see our guide
 * https://forum.openzeppelin.com/t/how-to-implement-erc20-supply-mechanisms/226[How
 * to implement supply mechanisms].
 *
 * The default value of {decimals} is 18. To change this, you should override
 * this function so it returns a different value.
 *
 * We have followed general OpenZeppelin Contracts guidelines: functions revert
 * instead returning `false` on failure. This behavior is nonetheless
 * conventional and does not conflict with the expectations of ERC20
 * applications.
 *
 * Additionally, an {Approval} event is emitted on calls to {transferFrom}.
 * This allows applications to reconstruct the allowance for all accounts just
 * by listening to said events. Other implementations of the EIP may not emit
 * these events, as it isn't required by the specification.
 *
 * Finally, the non-standard {decreaseAllowance} and {increaseAllowance}
 * functions have been added to mitigate the well-known issues around setting
 * allowances. See {IERC20-approve}.
 */
contract ERC20Upgradeable is Initializable, ContextUpgradeable, IERC20Upgradeable, IERC20MetadataUpgradeable {
    mapping(address => uint256) private _balances;

    mapping(address => mapping(address => uint256)) private _allowances;

    uint256 private _totalSupply;

    string private _name;
    string private _symbol;

    /**
     * @dev Sets the values for {name} and {symbol}.
     *
     * All two of these values are immutable: they can only be set once during
     * construction.
     */
    function __ERC20_init(string memory name_, string memory symbol_) internal onlyInitializing {
        __ERC20_init_unchained(name_, symbol_);
    }

    function __ERC20_init_unchained(string memory name_, string memory symbol_) internal onlyInitializing {
        _name = name_;
        _symbol = symbol_;
    }

    /**
     * @dev Returns the name of the token.
     */
    function name() public view virtual override returns (string memory) {
        return _name;
    }

    /**
     * @dev Returns the symbol of the token, usually a shorter version of the
     * name.
     */
    function symbol() public view virtual override returns (string memory) {
        return _symbol;
    }

    /**
     * @dev Returns the number of decimals used to get its user representation.
     * For example, if `decimals` equals `2`, a balance of `505` tokens should
     * be displayed to a user as `5.05` (`505 / 10 ** 2`).
     *
     * Tokens usually opt for a value of 18, imitating the relationship between
     * Ether and Wei. This is the default value returned by this function, unless
     * it's overridden.
     *
     * NOTE: This information is only used for _display_ purposes: it in
     * no way affects any of the arithmetic of the contract, including
     * {IERC20-balanceOf} and {IERC20-transfer}.
     */
    function decimals() public view virtual override returns (uint8) {
        return 18;
    }

    /**
     * @dev See {IERC20-totalSupply}.
     */
    function totalSupply() public view virtual override returns (uint256) {
        return _totalSupply;
    }

    /**
     * @dev See {IERC20-balanceOf}.
     */
    function balanceOf(address account) public view virtual override returns (uint256) {
        return _balances[account];
    }

    /**
     * @dev See {IERC20-transfer}.
     *
     * Requirements:
     *
     * - `to` cannot be the zero address.
     * - the caller must have a balance of at least `amount`.
     */
    function transfer(address to, uint256 amount) public virtual override returns (bool) {
        address owner = _msgSender();
        _transfer(owner, to, amount);
        return true;
    }

    /**
     * @dev See {IERC20-allowance}.
     */
    function allowance(address owner, address spender) public view virtual override returns (uint256) {
        return _allowances[owner][spender];
    }

    /**
     * @dev See {IERC20-approve}.
     *
     * NOTE: If `amount` is the maximum `uint256`, the allowance is not updated on
     * `transferFrom`. This is semantically equivalent to an infinite approval.
     *
     * Requirements:
     *
     * - `spender` cannot be the zero address.
     */
    function approve(address spender, uint256 amount) public virtual override returns (bool) {
        address owner = _msgSender();
        _approve(owner, spender, amount);
        return true;
    }

    /**
     * @dev See {IERC20-transferFrom}.
     *
     * Emits an {Approval} event indicating the updated allowance. This is not
     * required by the EIP. See the note at the beginning of {ERC20}.
     *
     * NOTE: Does not update the allowance if the current allowance
     * is the maximum `uint256`.
     *
     * Requirements:
     *
     * - `from` and `to` cannot be the zero address.
     * - `from` must have a balance of at least `amount`.
     * - the caller must have allowance for ``from``'s tokens of at least
     * `amount`.
     */
    function transferFrom(address from, address to, uint256 amount) public virtual override returns (bool) {
        address spender = _msgSender();
        _spendAllowance(from, spender, amount);
        _transfer(from, to, amount);
        return true;
    }

    /**
     * @dev Atomically increases the allowance granted to `spender` by the caller.
     *
     * This is an alternative to {approve} that can be used as a mitigation for
     * problems described in {IERC20-approve}.
     *
     * Emits an {Approval} event indicating the updated allowance.
     *
     * Requirements:
     *
     * - `spender` cannot be the zero address.
     */
    function increaseAllowance(address spender, uint256 addedValue) public virtual returns (bool) {
        address owner = _msgSender();
        _approve(owner, spender, allowance(owner, spender) + addedValue);
        return true;
    }

    /**
     * @dev Atomically decreases the allowance granted to `spender` by the caller.
     *
     * This is an alternative to {approve} that can be used as a mitigation for
     * problems described in {IERC20-approve}.
     *
     * Emits an {Approval} event indicating the updated allowance.
     *
     * Requirements:
     *
     * - `spender` cannot be the zero address.
     * - `spender` must have allowance for the caller of at least
     * `subtractedValue`.
     */
    function decreaseAllowance(address spender, uint256 subtractedValue) public virtual returns (bool) {
        address owner = _msgSender();
        uint256 currentAllowance = allowance(owner, spender);
        require(currentAllowance >= subtractedValue, "ERC20: decreased allowance below zero");
        unchecked {
            _approve(owner, spender, currentAllowance - subtractedValue);
        }

        return true;
    }

    /**
     * @dev Moves `amount` of tokens from `from` to `to`.
     *
     * This internal function is equivalent to {transfer}, and can be used to
     * e.g. implement automatic token fees, slashing mechanisms, etc.
     *
     * Emits a {Transfer} event.
     *
     * Requirements:
     *
     * - `from` cannot be the zero address.
     * - `to` cannot be the zero address.
     * - `from` must have a balance of at least `amount`.
     */
    function _transfer(address from, address to, uint256 amount) internal virtual {
        require(from != address(0), "ERC20: transfer from the zero address");
        require(to != address(0), "ERC20: transfer to the zero address");

        _beforeTokenTransfer(from, to, amount);

        uint256 fromBalance = _balances[from];
        require(fromBalance >= amount, "ERC20: transfer amount exceeds balance");
        unchecked {
            _balances[from] = fromBalance - amount;
            // Overflow not possible: the sum of all balances is capped by totalSupply, and the sum is preserved by
            // decrementing then incrementing.
            _balances[to] += amount;
        }

        emit Transfer(from, to, amount);

        _afterTokenTransfer(from, to, amount);
    }

    /** @dev Creates `amount` tokens and assigns them to `account`, increasing
     * the total supply.
     *
     * Emits a {Transfer} event with `from` set to the zero address.
     *
     * Requirements:
     *
     * - `account` cannot be the zero address.
     */
    function _mint(address account, uint256 amount) internal virtual {
        require(account != address(0), "ERC20: mint to the zero address");

        _beforeTokenTransfer(address(0), account, amount);

        _totalSupply += amount;
        unchecked {
            // Overflow not possible: balance + amount is at most totalSupply + amount, which is checked above.
            _balances[account] += amount;
        }
        emit Transfer(address(0), account, amount);

        _afterTokenTransfer(address(0), account, amount);
    }

    /**
     * @dev Destroys `amount` tokens from `account`, reducing the
     * total supply.
     *
     * Emits a {Transfer} event with `to` set to the zero address.
     *
     * Requirements:
     *
     * - `account` cannot be the zero address.
     * - `account` must have at least `amount` tokens.
     */
    function _burn(address account, uint256 amount) internal virtual {
        require(account != address(0), "ERC20: burn from the zero address");

        _beforeTokenTransfer(account, address(0), amount);

        uint256 accountBalance = _balances[account];
        require(accountBalance >= amount, "ERC20: burn amount exceeds balance");
        unchecked {
            _balances[account] = accountBalance - amount;
            // Overflow not possible: amount <= accountBalance <= totalSupply.
            _totalSupply -= amount;
        }

        emit Transfer(account, address(0), amount);

        _afterTokenTransfer(account, address(0), amount);
    }

    /**
     * @dev Sets `amount` as the allowance of `spender` over the `owner` s tokens.
     *
     * This internal function is equivalent to `approve`, and can be used to
     * e.g. set automatic allowances for certain subsystems, etc.
     *
     * Emits an {Approval} event.
     *
     * Requirements:
     *
     * - `owner` cannot be the zero address.
     * - `spender` cannot be the zero address.
     */
    function _approve(address owner, address spender, uint256 amount) internal virtual {
        require(owner != address(0), "ERC20: approve from the zero address");
        require(spender != address(0), "ERC20: approve to the zero address");

        _allowances[owner][spender] = amount;
        emit Approval(owner, spender, amount);
    }

    /**
     * @dev Updates `owner` s allowance for `spender` based on spent `amount`.
     *
     * Does not update the allowance amount in case of infinite allowance.
     * Revert if not enough allowance is available.
     *
     * Might emit an {Approval} event.
     */
    function _spendAllowance(address owner, address spender, uint256 amount) internal virtual {
        uint256 currentAllowance = allowance(owner, spender);
        if (currentAllowance != type(uint256).max) {
            require(currentAllowance >= amount, "ERC20: insufficient allowance");
            unchecked {
                _approve(owner, spender, currentAllowance - amount);
            }
        }
    }

    /**
     * @dev Hook that is called before any transfer of tokens. This includes
     * minting and burning.
     *
     * Calling conditions:
     *
     * - when `from` and `to` are both non-zero, `amount` of ``from``'s tokens
     * will be transferred to `to`.
     * - when `from` is zero, `amount` tokens will be minted for `to`.
     * - when `to` is zero, `amount` of ``from``'s tokens will be burned.
     * - `from` and `to` are never both zero.
     *
     * To learn more about hooks, head to xref:ROOT:extending-contracts.adoc#using-hooks[Using Hooks].
     */
    function _beforeTokenTransfer(address from, address to, uint256 amount) internal virtual {}

    /**
     * @dev Hook that is called after any transfer of tokens. This includes
     * minting and burning.
     *
     * Calling conditions:
     *
     * - when `from` and `to` are both non-zero, `amount` of ``from``'s tokens
     * has been transferred to `to`.
     * - when `from` is zero, `amount` tokens have been minted for `to`.
     * - when `to` is zero, `amount` of ``from``'s tokens have been burned.
     * - `from` and `to` are never both zero.
     *
     * To learn more about hooks, head to xref:ROOT:extending-contracts.adoc#using-hooks[Using Hooks].
     */
    function _afterTokenTransfer(address from, address to, uint256 amount) internal virtual {}

    /**
     * @dev This empty reserved space is put in place to allow future versions to add new
     * variables without shifting down storage in the inheritance chain.
     * See https://docs.openzeppelin.com/contracts/4.x/upgradeable#storage_gaps
     */
    uint256[45] private __gap;
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

    /**
     * @dev Transfer `value` amount of `token` from the calling contract to `to`. If `token` returns no value,
     * non-reverting calls are assumed to be successful.
     */
    function safeTransfer(IERC20Upgradeable token, address to, uint256 value) internal {
        _callOptionalReturn(token, abi.encodeWithSelector(token.transfer.selector, to, value));
    }

    /**
     * @dev Transfer `value` amount of `token` from `from` to `to`, spending the approval given by `from` to the
     * calling contract. If `token` returns no value, non-reverting calls are assumed to be successful.
     */
    function safeTransferFrom(IERC20Upgradeable token, address from, address to, uint256 value) internal {
        _callOptionalReturn(token, abi.encodeWithSelector(token.transferFrom.selector, from, to, value));
    }

    /**
     * @dev Deprecated. This function has issues similar to the ones found in
     * {IERC20-approve}, and its usage is discouraged.
     *
     * Whenever possible, use {safeIncreaseAllowance} and
     * {safeDecreaseAllowance} instead.
     */
    function safeApprove(IERC20Upgradeable token, address spender, uint256 value) internal {
        // safeApprove should only be called when setting an initial allowance,
        // or when resetting it to zero. To increase and decrease it, use
        // 'safeIncreaseAllowance' and 'safeDecreaseAllowance'
        require(
            (value == 0) || (token.allowance(address(this), spender) == 0),
            "SafeERC20: approve from non-zero to non-zero allowance"
        );
        _callOptionalReturn(token, abi.encodeWithSelector(token.approve.selector, spender, value));
    }

    /**
     * @dev Increase the calling contract's allowance toward `spender` by `value`. If `token` returns no value,
     * non-reverting calls are assumed to be successful.
     */
    function safeIncreaseAllowance(IERC20Upgradeable token, address spender, uint256 value) internal {
        uint256 oldAllowance = token.allowance(address(this), spender);
        _callOptionalReturn(token, abi.encodeWithSelector(token.approve.selector, spender, oldAllowance + value));
    }

    /**
     * @dev Decrease the calling contract's allowance toward `spender` by `value`. If `token` returns no value,
     * non-reverting calls are assumed to be successful.
     */
    function safeDecreaseAllowance(IERC20Upgradeable token, address spender, uint256 value) internal {
        unchecked {
            uint256 oldAllowance = token.allowance(address(this), spender);
            require(oldAllowance >= value, "SafeERC20: decreased allowance below zero");
            _callOptionalReturn(token, abi.encodeWithSelector(token.approve.selector, spender, oldAllowance - value));
        }
    }

    /**
     * @dev Set the calling contract's allowance toward `spender` to `value`. If `token` returns no value,
     * non-reverting calls are assumed to be successful. Meant to be used with tokens that require the approval
     * to be set to zero before setting it to a non-zero value, such as USDT.
     */
    function forceApprove(IERC20Upgradeable token, address spender, uint256 value) internal {
        bytes memory approvalCall = abi.encodeWithSelector(token.approve.selector, spender, value);

        if (!_callOptionalReturnBool(token, approvalCall)) {
            _callOptionalReturn(token, abi.encodeWithSelector(token.approve.selector, spender, 0));
            _callOptionalReturn(token, approvalCall);
        }
    }

    /**
     * @dev Use a ERC-2612 signature to set the `owner` approval toward `spender` on `token`.
     * Revert on invalid signature.
     */
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
        // we're implementing it ourselves. We use {Address-functionCall} to perform this call, which verifies that
        // the target address contains contract code and also asserts for success in the low-level call.

        bytes memory returndata = address(token).functionCall(data, "SafeERC20: low-level call failed");
        require(returndata.length == 0 || abi.decode(returndata, (bool)), "SafeERC20: ERC20 operation did not succeed");
    }

    /**
     * @dev Imitates a Solidity high-level call (i.e. a regular function call to a contract), relaxing the requirement
     * on the return value: the return value is optional (but if data is returned, it must not be false).
     * @param token The token targeted by the call.
     * @param data The call data (encoded using abi.encode or one of its variants).
     *
     * This is a variant of {_callOptionalReturn} that silents catches all reverts and returns a bool instead.
     */
    function _callOptionalReturnBool(IERC20Upgradeable token, bytes memory data) private returns (bool) {
        // We need to perform a low level call here, to bypass Solidity's return data size checking mechanism, since
        // we're implementing it ourselves. We cannot use {Address-functionCall} here since this should return false
        // and not revert is the subcall reverts.

        (bool success, bytes memory returndata) = address(token).call(data);
        return
            success && (returndata.length == 0 || abi.decode(returndata, (bool))) && AddressUpgradeable.isContract(address(token));
    }
}

/**
 * @title BlacklistableUpgradeable base contract
 * @author CloudWalk Inc.
 * @notice Allows to blacklist and unblacklist accounts using the `blacklister` account
 * @dev This contract is used through inheritance. It makes available the modifier `notBlacklisted`,
 * which can be applied to functions to restrict their usage to not blacklisted accounts only.
 */
abstract contract BlacklistableUpgradeable is OwnableUpgradeable {
    /// @notice The structure that represents balacklistable contract storage
    struct BlacklistableStorageSlot {
        /// @notice The mapping of presence in the blacklist for a given address
        mapping(address => bool) blacklisters;
        /// @notice The enabled/disabled status of the blacklist
        bool enabled;
    }

    /// @notice The memory slot used to store the blacklistable contract storage
    bytes32 private constant _BLACKLISTABLE_STORAGE_SLOT =
        0xff11fdfa16fed3260ed0e7147f7cc6da11a60208b5b9406d12a635614ffd9141;

    /// @notice The address of the blacklister that is allowed to add and remove accounts from the blacklist
    address private _mainBlacklister;

    /// @notice Mapping of presence in the blacklist for a given address
    mapping(address => bool) private _blacklisted;

    // -------------------- Events -----------------------------------

    /**
     * @notice Emitted when an account is blacklisted
     *
     * @param account The address of the blacklisted account
     */
    event Blacklisted(address indexed account);

    /**
     * @notice Emitted when an account is unblacklisted
     *
     * @param account The address of the unblacklisted account
     */
    event UnBlacklisted(address indexed account);

    /**
     * @notice Emitted when an account is self blacklisted
     *
     * @param account The address of the self blacklisted account
     */
    event SelfBlacklisted(address indexed account);

    /**
     * @notice Emitted when the main blacklister was changed
     *
     * @param newMainBlacklister The address of the new main blacklister
     */
    event MainBlackListerChanged(address indexed newMainBlacklister);

    /**
     * @notice Emitted when the blacklister configuration is updated
     *
     * @param blacklister The address of the blacklister
     * @param status The new status of the blacklister
     */
    event BlacklisterConfigured(address indexed blacklister, bool status);

    /**
     * @notice Emitted when the blacklist is enabled or disabled
     *
     * @param status The new enabled/disabled status of the blacklist
     */
    event BlacklistEnabled(bool indexed status);

    // -------------------- Errors -----------------------------------

    /**
     * @notice The transaction sender is not a blacklister
     *
     * @param account The address of the transaction sender
     */
    error UnauthorizedBlacklister(address account);

    /**
     * @notice The transaction sender is not a main blacklister
     *
     * @param account The address of the transaction sender
     */
    error UnauthorizedMainBlacklister(address account);

    /**
     * @notice The account is blacklisted
     *
     * @param account The address of the blacklisted account
     */
    error BlacklistedAccount(address account);

    /**
     * @notice The address to blacklist is zero address
     */
    error ZeroAddressToBlacklist();

    /**
     * @notice The account is already configured
     */
    error AlreadyConfigured();

    // -------------------- Modifiers --------------------------------

    /**
     * @notice Throws if called by any account other than the blacklister or main blacklister
     */
    modifier onlyBlacklister() {
        address sender = _msgSender();
        if (!isBlacklister(sender) && sender != _mainBlacklister) {
            revert UnauthorizedBlacklister(_msgSender());
        }
        _;
    }

    /**
     * @notice Throws if called by any account other than the main blacklister
     */
    modifier onlyMainBlacklister() {
        if (_msgSender() != _mainBlacklister) {
            revert UnauthorizedMainBlacklister(_msgSender());
        }
        _;
    }

    /**
     * @notice Throws if the account is blacklisted
     *
     * @param account The address to check for presence in the blacklist
     */
    modifier notBlacklisted(address account) {
        if (_blacklisted[account] && isBlacklistEnabled()) {
            revert BlacklistedAccount(account);
        }
        _;
    }

    /**
     * @notice Throws if the account is blacklisted, but allows the blacklister to bypass the check
     *
     * @param account The address to check for presence in the blacklist
     */
    modifier notBlacklistedOrBypassIfBlacklister(address account) {
        if (_blacklisted[account] && isBlacklistEnabled() && !isBlacklister(_msgSender())) {
            revert BlacklistedAccount(account);
        }
        _;
    }

    // -------------------- Functions --------------------------------

    /**
     * @notice The internal initializer of the upgradable contract
     *
     * See details https://docs.openzeppelin.com/upgrades-plugins/1.x/writing-upgradeable
     */
    function __Blacklistable_init() internal onlyInitializing {
        __Context_init_unchained();
        __Ownable_init_unchained();
        __Blacklistable_init_unchained();
    }

    /**
     * @notice The unchained internal initializer of the upgradable contract
     *
     * See {BlacklistableUpgradeable-__Blacklistable_init}
     */
    function __Blacklistable_init_unchained() internal onlyInitializing {}

    /**
     * @notice Adds an account to the blacklist
     *
     * Requirements:
     *
     * - Can only be called by the blacklister account
     *
     * Emits a {Blacklisted} event
     *
     * @param account The address to blacklist
     */
    function blacklist(address account) external onlyBlacklister {
        if (account == address(0)) {
            revert ZeroAddressToBlacklist();
        }
        if (_blacklisted[account]) {
            return;
        }

        _blacklisted[account] = true;

        emit Blacklisted(account);
    }

    /**
     * @notice Removes an account from the blacklist
     *
     * Requirements:
     *
     * - Can only be called by the blacklister account
     *
     * Emits an {UnBlacklisted} event
     *
     * @param account The address to remove from the blacklist
     */
    function unBlacklist(address account) external onlyBlacklister {
        if (!_blacklisted[account]) {
            return;
        }

        _blacklisted[account] = false;

        emit UnBlacklisted(account);
    }

    /**
     * @notice Adds the transaction sender to the blacklist
     *
     * Emits a {SelfBlacklisted} event
     * Emits a {Blacklisted} event
     */
    function selfBlacklist() external {
        address sender = _msgSender();

        if (_blacklisted[sender]) {
            return;
        }

        _blacklisted[sender] = true;

        emit SelfBlacklisted(sender);
        emit Blacklisted(sender);
    }

    /**
     * @notice Enables or disables the blacklist
     *
     * Requirements:
     *
     * - Can only be called by the owner
     *
     * Emits a {BlacklistEnabled} event
     *
     * @param status The new enabled/disabled status of the blacklist
     */
    function enableBlacklist(bool status) external onlyOwner {
        BlacklistableStorageSlot storage storageSlot = _getBlacklistableSlot(_BLACKLISTABLE_STORAGE_SLOT);
        if (storageSlot.enabled == status) {
            revert AlreadyConfigured();
        }

        storageSlot.enabled = status;
        emit BlacklistEnabled(status);
    }

    /**
     * @notice Updates the main blacklister address
     *
     * Requirements:
     *
     * - Can only be called by the owner
     *
     * Emits a {MainBlackListerChanged} event
     *
     * @param newMainBlacklister The address of the new main blacklister
     */
    function setMainBlacklister(address newMainBlacklister) external onlyOwner {
        if (_mainBlacklister == newMainBlacklister) {
            revert AlreadyConfigured();
        }

        _mainBlacklister = newMainBlacklister;
        emit MainBlackListerChanged(newMainBlacklister);
    }

    /**
     * @notice Updates the blacklister address
     *
     * Requirements:
     *
     * - Can only be called by the main blacklister
     *
     * Emits a {BlacklisterConfigured} event
     *
     * @param account The address of the blacklister to be configured
     * @param status The new status of the blacklister
     */
    function configureBlacklister(address account, bool status) external onlyMainBlacklister {
        BlacklistableStorageSlot storage storageSlot = _getBlacklistableSlot(_BLACKLISTABLE_STORAGE_SLOT);
        if (storageSlot.blacklisters[account] == status) {
            revert AlreadyConfigured();
        }

        storageSlot.blacklisters[account] = status;
        emit BlacklisterConfigured(account, status);
    }

    /**
     * @notice Returns the address of the blacklister
     */
    function mainBlacklister() public view virtual returns (address) {
        return _mainBlacklister;
    }

    /**
     * @notice Checks if an account is present in the blacklist
     *
     * @param account The address to check for presence in the blacklist
     * @return True if the account is present in the blacklist, false otherwise
     */
    function isBlacklisted(address account) public view returns (bool) {
        return _blacklisted[account];
    }

    /**
     * @notice Checks if the account is a blacklister
     *
     * @param account The address to check for blacklister configuration
     * @return True if the account is a configured blacklister, False otherwise
     */
    function isBlacklister(address account) public view returns (bool) {
        return _getBlacklistableSlot(_BLACKLISTABLE_STORAGE_SLOT).blacklisters[account];
    }

    /**
     * @notice Checks if the blacklist is enabled
     *
     * @return True if the blacklist is enabled, False otherwise
     */
    function isBlacklistEnabled() public view returns (bool) {
        return _getBlacklistableSlot(_BLACKLISTABLE_STORAGE_SLOT).enabled;
    }

    /**
     * @dev Returns an `MappingSlot` with member `value` located at `slot`
     */
    function _getBlacklistableSlot(bytes32 slot) internal pure returns (BlacklistableStorageSlot storage r) {
        /// @solidity memory-safe-assembly
        assembly {
            r.slot := slot
        }
    }
}

/**
 * @title PausableExtUpgradeable base contract
 * @author CloudWalk Inc.
 * @notice Extends the OpenZeppelin's {PausableUpgradeable} contract by adding the `pauser` account
 * @dev This contract is used through inheritance. It introduces the `pauser` role that is allowed to
 * trigger the paused or unpaused state of the contract that is inherited from this one.
 */
abstract contract PausableExtUpgradeable is OwnableUpgradeable, PausableUpgradeable {
    /// @notice The address of the pauser that is allowed to trigger the paused or unpaused state of the contract
    address private _pauser;

    // -------------------- Events -----------------------------------

    /**
     * @notice Emitted when the pauser is changed
     *
     * @param pauser The address of the new pauser
     */
    event PauserChanged(address indexed pauser);

    // -------------------- Errors -----------------------------------

    /**
     * @notice The transaction sender is not a pauser
     *
     * @param account The address of the transaction sender
     */
    error UnauthorizedPauser(address account);

    // -------------------- Modifiers --------------------------------

    /**
     * @notice Throws if called by any account other than the pauser
     */
    modifier onlyPauser() {
        if (_msgSender() != _pauser) {
            revert UnauthorizedPauser(_msgSender());
        }
        _;
    }

    // -------------------- Functions --------------------------------

    /**
     * @notice The internal initializer of the upgradable contract
     *
     * See details https://docs.openzeppelin.com/upgrades-plugins/1.x/writing-upgradeable
     */
    function __PausableExt_init() internal onlyInitializing {
        __Context_init_unchained();
        __Ownable_init_unchained();
        __Pausable_init_unchained();
        __PausableExt_init_unchained();
    }

    /**
     * @notice The unchained internal initializer of the upgradable contract
     *
     * See {PausableExtUpgradeable-__PausableExt_init}
     */
    function __PausableExt_init_unchained() internal onlyInitializing {}

    /**
     * @notice Triggers the paused state of the contract
     *
     * Requirements:
     *
     * - Can only be called by the contract pauser
     */
    function pause() external onlyPauser {
        _pause();
    }

    /**
     * @notice Triggers the unpaused state of the contract
     *
     * Requirements:
     *
     * - Can only be called by the contract pauser
     */
    function unpause() external onlyPauser {
        _unpause();
    }

    /**
     * @notice Updates the pauser address
     *
     * Requirements:
     *
     * - Can only be called by the contract owner
     *
     * Emits a {PauserChanged} event
     *
     * @param newPauser The address of a new pauser
     */
    function setPauser(address newPauser) external onlyOwner {
        if (_pauser == newPauser) {
            return;
        }

        _pauser = newPauser;

        emit PauserChanged(newPauser);
    }

    /**
     * @notice Returns the pauser address
     */
    function pauser() public view virtual returns (address) {
        return _pauser;
    }
}

/**
 * @title RescuableUpgradeable base contract
 * @author CloudWalk Inc.
 * @notice Allows to rescue ERC20 tokens locked up in the contract using the `rescuer` account
 * @dev This contract is used through inheritance. It introduces the `rescuer` role that is allowed
 * to rescue tokens locked up in the contract that is inherited from this one.
 */
abstract contract RescuableUpgradeable is OwnableUpgradeable {
    using SafeERC20Upgradeable for IERC20Upgradeable;

    /// @notice The address of the rescuer that is allowed to rescue tokens locked up in the contract
    address private _rescuer;

    // -------------------- Events -----------------------------------

    /**
     * @notice Emitted when the rescuer is changed
     *
     * @param newRescuer The address of the new rescuer
     */
    event RescuerChanged(address indexed newRescuer);

    // -------------------- Errors -----------------------------------

    /**
     * @notice The transaction sender is not a rescuer
     *
     * @param account The address of the transaction sender
     */
    error UnauthorizedRescuer(address account);

    // -------------------- Modifiers --------------------------------

    /**
     * @notice Throws if called by any account other than the rescuer
     */
    modifier onlyRescuer() {
        if (_msgSender() != _rescuer) {
            revert UnauthorizedRescuer(_msgSender());
        }
        _;
    }

    // -------------------- Functions --------------------------------

    /**
     * @notice The internal initializer of the upgradable contract
     *
     * See details https://docs.openzeppelin.com/upgrades-plugins/1.x/writing-upgradeable
     */
    function __Rescuable_init() internal onlyInitializing {
        __Context_init_unchained();
        __Ownable_init_unchained();
        __Rescuable_init_unchained();
    }

    /**
     * @notice The unchained internal initializer of the upgradable contract
     *
     * See {RescuableUpgradeable-__Rescuable_init}
     */
    function __Rescuable_init_unchained() internal onlyInitializing {}

    /**
     * @notice Withdraws ERC20 tokens locked up in the contract
     *
     * Requirements:
     *
     * - Can only be called by the rescuer account
     *
     * @param token The address of the ERC20 token contract
     * @param to The address of the recipient of tokens
     * @param amount The amount of tokens to withdraw
     */
    function rescueERC20(address token, address to, uint256 amount) external onlyRescuer {
        IERC20Upgradeable(token).safeTransfer(to, amount);
    }

    /**
     * @notice Updates the rescuer address
     *
     * Requirements:
     *
     * - Can only be called by the contract owner
     *
     * Emits a {RescuerChanged} event
     *
     * @param newRescuer The address of a new rescuer
     */
    function setRescuer(address newRescuer) external onlyOwner {
        if (_rescuer == newRescuer) {
            return;
        }

        _rescuer = newRescuer;

        emit RescuerChanged(newRescuer);
    }

    /**
     * @notice Returns the rescuer address
     */
    function rescuer() public view virtual returns (address) {
        return _rescuer;
    }
}

/**
 * @title ERC20Base contract
 * @author CloudWalk Inc.
 * @notice This contract is base implementation of the BRLC token with inherited Rescuable,
 * Pausable, and Blacklistable functionality.
 */
abstract contract ERC20Base is
    OwnableUpgradeable,
    RescuableUpgradeable,
    PausableExtUpgradeable,
    BlacklistableUpgradeable,
    ERC20Upgradeable
{
    /**
     * @notice The internal initializer of the upgradable contract
     *
     * See details https://docs.openzeppelin.com/upgrades-plugins/1.x/writing-upgradeable
     *
     * @param name_ The name of the token
     * @param symbol_ The symbol of the token
     */
    function __ERC20Base_init(string memory name_, string memory symbol_) internal onlyInitializing {
        __Context_init_unchained();
        __Ownable_init_unchained();
        __Rescuable_init_unchained();
        __Pausable_init_unchained();
        __PausableExt_init_unchained();
        __Blacklistable_init_unchained();
        __ERC20_init_unchained(name_, symbol_);
        __ERC20Base_init_unchained();
    }

    /**
     * @notice The internal unchained initializer of the upgradable contract
     *
     * See {ERC20Base-__ERC20Base_init}
     */
    function __ERC20Base_init_unchained() internal onlyInitializing {}

    /**
     * @inheritdoc ERC20Upgradeable
     */
    function decimals() public pure virtual override returns (uint8) {
        return 6;
    }

    /**
     * @inheritdoc ERC20Upgradeable
     *
     * @dev The contract must not be paused
     * @dev The `owner` address must not be blacklisted
     * @dev The `spender` address must not be blacklisted
     */
    function _approve(
        address owner,
        address spender,
        uint256 amount
    ) internal virtual override whenNotPaused notBlacklisted(owner) notBlacklisted(spender) {
        super._approve(owner, spender, amount);
    }

    /**
     * @inheritdoc ERC20Upgradeable
     *
     * @dev The contract must not be paused
     * @dev The `owner` address must not be blacklisted
     * @dev The `spender` address must not be blacklisted
     */
    function _spendAllowance(
        address owner,
        address spender,
        uint256 amount
    ) internal virtual override whenNotPaused notBlacklisted(owner) notBlacklisted(spender) {
        super._spendAllowance(owner, spender, amount);
    }

    /**
     * @inheritdoc ERC20Upgradeable
     *
     * @dev The contract must not be paused
     * @dev The `from` address must not be blacklisted
     * @dev The `to` address must not be blacklisted
     */
    function _beforeTokenTransfer(
        address from,
        address to,
        uint256 amount
    ) internal virtual override whenNotPaused notBlacklisted(from) notBlacklistedOrBypassIfBlacklister(to) {
        super._beforeTokenTransfer(from, to, amount);
    }

    /**
     * @inheritdoc ERC20Upgradeable
     */
    function _afterTokenTransfer(address from, address to, uint256 amount) internal virtual override {
        super._afterTokenTransfer(from, to, amount);
    }
}

/**
 * @title IERC20Freezable interface
 * @author CloudWalk Inc.
 * @notice The interface of a token that supports freezing operations
 */
interface IERC20Freezable {
    /**
     * @notice Emitted when token freezing has been approved for an account
     *
     * @param account The account for which token freezing has been approved
     */
    event FreezeApproval(address indexed account);

    /**
     * @notice Emitted when frozen tokens have been transferred from an account
     *
     * @param account The account from which frozen tokens have been transferred
     * @param amount The amount of frozen tokens transferred
     */
    event FreezeTransfer(address indexed account, uint256 amount);

    /**
     * @notice Emitted when token freezing has been performed for a specific account
     *
     * @param account The account for which token freezing has been performed
     * @param newFrozenBalance The updated frozen balance of the account
     * @param oldFrozenBalance The previous frozen balance of the account
     */
    event Freeze(address indexed account, uint256 newFrozenBalance, uint256 oldFrozenBalance);

    /**
     * @notice Approves token freezing for the caller
     *
     * Emits a {FreezeApproval} event
     */
    function approveFreezing() external;

    /**
     * @notice Freezes tokens of the specified account
     *
     * Emits a {Freeze} event
     *
     * @param account The account whose tokens will be frozen
     * @param amount The amount of tokens to freeze
     */
    function freeze(address account, uint256 amount) external;

    /**
     * @notice Transfers frozen tokens on behalf of an account
     *
     * Emits a {FreezeTransfer} event
     *
     * @param from The account tokens will be transferred from
     * @param to The account tokens will be transferred to
     * @param amount The amount of tokens to transfer
     */
    function transferFrozen(address from, address to, uint256 amount) external;

    /**
     * @notice Checks if token freezing is approved for an account
     *
     * @param account The account to check the approval for
     * @return True if token freezing is approved for the account
     */
    function freezeApproval(address account) external view returns (bool);

    /**
     * @notice Retrieves the frozen balance of an account
     *
     * @param account The account to check the balance of
     * @return The amount of tokens that are frozen for the account
     */
    function balanceOfFrozen(address account) external view returns (uint256);
}

/**
 * @title ERC20Freezable contract
 * @author CloudWalk Inc.
 * @notice The ERC20 token implementation that supports the freezing operations
 */
abstract contract ERC20Freezable is ERC20Base, IERC20Freezable {
    /// @notice The mapping of the freeze approvals
    mapping(address => bool) private _freezeApprovals;

    /// @notice The mapping of the frozen balances
    mapping(address => uint256) private _frozenBalances;

    // -------------------- Errors -----------------------------------

    /// @notice The token freezing operation is not approved by the account
    error FreezingNotApproved();

    /// @notice The token freezing is already approved by the account
    error FreezingAlreadyApproved();

    /// @notice The frozen balance is exceeded during the operation
    error LackOfFrozenBalance();

    /// @notice The transfer amount exceeded the frozen amount
    error TransferExceededFrozenAmount();

    // -------------------- Functions --------------------------------

    /**
     * @notice The internal initializer of the upgradable contract
     */
    function __ERC20Freezable_init(string memory name_, string memory symbol_) internal onlyInitializing {
        __Context_init_unchained();
        __Ownable_init_unchained();
        __Pausable_init_unchained();
        __PausableExt_init_unchained();
        __Blacklistable_init_unchained();
        __ERC20_init_unchained(name_, symbol_);
        __ERC20Base_init_unchained();
        __ERC20Freezable_init_unchained();
    }

    /**
     * @notice The internal unchained initializer of the upgradable contract
     */
    function __ERC20Freezable_init_unchained() internal onlyInitializing {}

    /**
     * @inheritdoc IERC20Freezable
     *
     * @notice The contract must not be paused
     */
    function approveFreezing() external whenNotPaused {
        if (_freezeApprovals[_msgSender()]) {
            revert FreezingAlreadyApproved();
        }

        _freezeApprovals[_msgSender()] = true;

        emit FreezeApproval(_msgSender());
    }

    /**
     * @inheritdoc IERC20Freezable
     *
     * @dev The contract must not be paused
     * @dev Can only be called by the blacklister account
     * @dev The token freezing must be approved by the `account`
     */
    function freeze(address account, uint256 amount) external whenNotPaused onlyBlacklister {
        if (!_freezeApprovals[account]) {
            revert FreezingNotApproved();
        }

        emit Freeze(account, amount, _frozenBalances[account]);

        _frozenBalances[account] = amount;
    }

    /**
     * @inheritdoc IERC20Freezable
     *
     * @dev The contract must not be paused
     * @dev Can only be called by the blacklister account
     * @dev The frozen balance must be greater than the `amount`
     */
    function transferFrozen(address from, address to, uint256 amount) public virtual whenNotPaused onlyBlacklister {
        uint256 balance = _frozenBalances[from];

        if (amount > balance) {
            revert LackOfFrozenBalance();
        }

        unchecked {
            _frozenBalances[from] -= amount;
        }

        emit FreezeTransfer(from, amount);
        emit Freeze(from, _frozenBalances[from], balance);

        _transfer(from, to, amount);
    }

    /**
     * @inheritdoc IERC20Freezable
     */
    function freezeApproval(address account) external view returns (bool) {
        return _freezeApprovals[account];
    }

    /**
     * @inheritdoc IERC20Freezable
     */
    function balanceOfFrozen(address account) public view returns (uint256) {
        return _frozenBalances[account];
    }

    /**
     * @dev [DEPRECATED] Keep this function for backward compatibility
     */
    function frozenBalance(address account) public view returns (uint256) {
        return balanceOfFrozen(account);
    }

    /**
     * @inheritdoc ERC20Base
     */
    function _afterTokenTransfer(address from, address to, uint256 amount) internal virtual override {
        super._afterTokenTransfer(from, to, amount);
        uint256 frozen = _frozenBalances[from];
        if (frozen != 0) {
            if (balanceOf(from) < frozen) {
                revert TransferExceededFrozenAmount();
            }
        }
    }

    /**
     * @dev This empty reserved space is put in place to allow future versions
     * to add new variables without shifting down storage in the inheritance chain
     */
    uint256[48] private __gap;
}

/**
 * @title IERC20Hook interface
 * @author CloudWalk Inc.
 * @notice The interface of a contract that supports hookable token operations
 */
interface IERC20Hook {
    /**
     * @notice Hook function that is called by a token contract before token transfer
     *
     * @param from The address that tokens will be transferred from
     * @param to The address that tokens will be transferred to
     * @param amount The amount of tokens to be transferred
     */
    function beforeTokenTransfer(address from, address to, uint256 amount) external;

    /**
     * @notice Hook function that is called by a token contract after token transfer
     *
     * @param from The address that tokens have been transferred from
     * @param to The address that tokens have been transferred to
     * @param amount The amount of tokens transferred
     */
    function afterTokenTransfer(address from, address to, uint256 amount) external;
}

/**
 * @title IERC20Hookable interface
 * @author CloudWalk Inc.
 * @notice The interface of a token that supports hooking operations
 */
interface IERC20Hookable {
    /// @notice An enum describing the error handling policy
    enum ErrorHandlingPolicy {
        Revert,
        Event
    }

    /// @notice A structure describing a hook
    struct Hook {
        /// @notice The address of the hook contract
        address account;
        /// @notice The error handling policy of the hook
        ErrorHandlingPolicy policy;
    }

    /**
     * @notice Updates the `beforeTokenTransfer` hooks attached to the token
     * @param hooks The hooks to be attached
     */
    function setBeforeTokenTransferHooks(Hook[] calldata hooks) external;

    /**
     * @notice Updates the `afterTokenTransfer` hooks attached to the token
     * @param hooks The hooks to be attached
     */
    function setAfterTokenTransferHooks(Hook[] calldata hooks) external;

    /**
     * @notice Returns the array of `beforeTokenTransfer` hooks attached to the token
     */
    function getBeforeTokenTransferHooks() external view returns (Hook[] memory);

    /**
     * @notice Returns the array of `afterTokenTransfer` hooks attached to the token
     */
    function getAfterTokenTransferHooks() external view returns (Hook[] memory);
}

/**
 * @title ERC20Hookable contract
 * @author CloudWalk Inc.
 * @notice The ERC20 token implementation that supports hooking operations
 */
abstract contract ERC20Hookable is ERC20Base, IERC20Hookable {
    /// @notice The array of the attached hook contracts that are triggered before the token transfer
    Hook[] private _beforeTokenTransferHooks;

    /// @notice The array of the attached hook contracts that are triggered after the token transfer
    Hook[] private _afterTokenTransferHooks;

    /**
     * @notice Emitted when the `beforeTokenTransfer` hooks are updated
     *
     * @param hooks The array of the updated hooks
     */
    event BeforeTokenTransferHooksSet(Hook[] hooks);

    /**
     * @notice Emitted when the `afterTokenTransfer` hooks are updated
     *
     * @param hooks The array of the updated hooks
     */
    event AfterTokenTransferHooksSet(Hook[] hooks);

    /**
     * @notice Emitted when a call of the `beforeTokenTransfer` hook failed
     *
     * @param hook The address of the hook contract that was called
     * @param reason The reason message of the hook failure
     * @param code The error code of the hook failure
     * @param data The low level error data
     */
    event BeforeTokenTransferHookFailure(address indexed hook, string reason, uint256 code, bytes data);

    /**
     * @notice Emitted when a call of the `afterTokenTransfer` hook failed
     *
     * @param hook The address of the hook contract that was called
     * @param reason The reason message of the hook failure
     * @param code The error code of the hook failure
     * @param data The low level error data
     */
    event AfterTokenTransferHookFailure(address indexed hook, string reason, uint256 code, bytes data);

    // -------------------- Functions --------------------------------

    /**
     * @notice The internal initializer of the upgradable contract
     */
    function __ERC20Hookable_init(string memory name_, string memory symbol_) internal onlyInitializing {
        __Context_init_unchained();
        __Ownable_init_unchained();
        __Pausable_init_unchained();
        __PausableExt_init_unchained();
        __Blacklistable_init_unchained();
        __ERC20_init_unchained(name_, symbol_);
        __ERC20Base_init_unchained();
        __ERC20Hookable_init_unchained();
    }

    /**
     * @notice The internal unchained initializer of the upgradable contract
     */
    function __ERC20Hookable_init_unchained() internal onlyInitializing {}

    /**
     * @inheritdoc IERC20Hookable
     */
    function setBeforeTokenTransferHooks(Hook[] calldata hooks) external onlyOwner {
        delete _beforeTokenTransferHooks;
        for (uint i = 0; i < hooks.length; ++i) {
            _beforeTokenTransferHooks.push(hooks[i]);
        }
        emit BeforeTokenTransferHooksSet(hooks);
    }

    /**
     * @inheritdoc IERC20Hookable
     */
    function setAfterTokenTransferHooks(Hook[] calldata hooks) external onlyOwner {
        delete _afterTokenTransferHooks;
        for (uint i = 0; i < hooks.length; ++i) {
            _afterTokenTransferHooks.push(hooks[i]);
        }
        emit AfterTokenTransferHooksSet(hooks);
    }

    /**
     * @inheritdoc IERC20Hookable
     */
    function getBeforeTokenTransferHooks() external view returns (Hook[] memory) {
        return _beforeTokenTransferHooks;
    }

    /**
     * @inheritdoc IERC20Hookable
     */
    function getAfterTokenTransferHooks() external view returns (Hook[] memory) {
        return _afterTokenTransferHooks;
    }

    /**
     * @dev Overrides the `_beforeTokenTransfer` function by calling attached hooks after the base logic
     */
    function _beforeTokenTransfer(address from, address to, uint256 amount) internal virtual override(ERC20Base) {
        super._beforeTokenTransfer(from, to, amount);
        for (uint256 i = 0; i < _beforeTokenTransferHooks.length; ++i) {
            if (_beforeTokenTransferHooks[i].policy == ErrorHandlingPolicy.Revert) {
                IERC20Hook(_beforeTokenTransferHooks[i].account).beforeTokenTransfer(from, to, amount);
            } else {
                // ErrorHandlingPolicy.Event
                try IERC20Hook(_beforeTokenTransferHooks[i].account).beforeTokenTransfer(from, to, amount) {
                    // Do nothing
                } catch Error(string memory reason) {
                    emit BeforeTokenTransferHookFailure(_beforeTokenTransferHooks[i].account, reason, 0, "");
                } catch Panic(uint errorCode) {
                    emit BeforeTokenTransferHookFailure(_beforeTokenTransferHooks[i].account, "", errorCode, "");
                } catch (bytes memory lowLevelData) {
                    emit BeforeTokenTransferHookFailure(_beforeTokenTransferHooks[i].account, "", 0, lowLevelData);
                }
            }
        }
    }

    /**
     * @dev Overrides the `_afterTokenTransfer` function by calling attached hooks after the base logic
     */
    function _afterTokenTransfer(address from, address to, uint256 amount) internal virtual override {
        super._afterTokenTransfer(from, to, amount);
        for (uint256 i = 0; i < _afterTokenTransferHooks.length; ++i) {
            if (_afterTokenTransferHooks[i].policy == ErrorHandlingPolicy.Revert) {
                IERC20Hook(_afterTokenTransferHooks[i].account).afterTokenTransfer(from, to, amount);
            } else {
                // ErrorHandlingPolicy.Event
                try IERC20Hook(_afterTokenTransferHooks[i].account).afterTokenTransfer(from, to, amount) {
                    // Do nothing
                } catch Error(string memory reason) {
                    emit AfterTokenTransferHookFailure(_afterTokenTransferHooks[i].account, reason, 0, "");
                } catch Panic(uint errorCode) {
                    emit AfterTokenTransferHookFailure(_afterTokenTransferHooks[i].account, "", errorCode, "");
                } catch (bytes memory lowLevelData) {
                    emit AfterTokenTransferHookFailure(_afterTokenTransferHooks[i].account, "", 0, lowLevelData);
                }
            }
        }
    }
}

/**
 * @title IERC20Mintable interface
 * @author CloudWalk Inc.
 * @notice The interface of a token that supports mint and burn operations
 */
interface IERC20Mintable {
    /**
     * @notice Emitted when the master minter is changed
     *
     * @param newMasterMinter The address of a new master minter
     */
    event MasterMinterChanged(address indexed newMasterMinter);

    /**
     * @notice Emitted when a minter account is configured
     *
     * @param minter The address of the minter to configure
     * @param mintAllowance The mint allowance
     */
    event MinterConfigured(address indexed minter, uint256 mintAllowance);

    /**
     * @notice Emitted when a minter account is removed
     *
     * @param oldMinter The address of the minter to remove
     */
    event MinterRemoved(address indexed oldMinter);

    /**
     * @notice Emitted when tokens are minted
     *
     * @param minter The address of the minter
     * @param to The address of the tokens recipient
     * @param amount The amount of tokens being minted
     */
    event Mint(address indexed minter, address indexed to, uint256 amount);

    /**
     * @notice Emitted when tokens are burned
     *
     * @param burner The address of the tokens burner
     * @param amount The amount of tokens being burned
     */
    event Burn(address indexed burner, uint256 amount);

    /**
     * @notice Returns the master minter address
     */
    function masterMinter() external view returns (address);

    /**
     * @notice Checks if the account is configured as a minter
     *
     * @param account The address to check
     * @return True if the account is a minter
     */
    function isMinter(address account) external view returns (bool);

    /**
     * @notice Returns the mint allowance of a minter
     *
     * @param minter The minter to check
     * @return The mint allowance of the minter
     */
    function minterAllowance(address minter) external view returns (uint256);

    /**
     * @notice Updates the master minter address
     *
     * Emits a {MasterMinterChanged} event
     *
     * @param newMasterMinter The address of a new master minter
     */
    function updateMasterMinter(address newMasterMinter) external;

    /**
     * @notice Configures a minter
     *
     * Emits a {MinterConfigured} event
     *
     * @param minter The address of the minter to configure
     * @param mintAllowance The mint allowance
     * @return True if the operation was successful
     */
    function configureMinter(address minter, uint256 mintAllowance) external returns (bool);

    /**
     * @notice Removes a minter
     *
     * Emits a {MinterRemoved} event
     *
     * @param minter The address of the minter to remove
     * @return True if the operation was successful
     */
    function removeMinter(address minter) external returns (bool);

    /**
     * @notice Mints tokens
     *
     * Emits a {Mint} event
     *
     * @param account The address of a tokens recipient
     * @param amount The amount of tokens to mint
     * @return True if the operation was successful
     */
    function mint(address account, uint256 amount) external returns (bool);

    /**
     * @notice Burns tokens
     *
     * Emits a {Burn} event
     *
     * @param amount The amount of tokens to burn
     */
    function burn(uint256 amount) external;
}

/**
 * @title ERC20Mintable contract
 * @author CloudWalk Inc.
 * @notice The ERC20 token implementation that supports the mint and burn operations
 */
abstract contract ERC20Mintable is ERC20Base, IERC20Mintable {
    /// @notice The address of the master minter
    address private _masterMinter;

    /// @notice The mapping of the configured minters
    mapping(address => bool) private _minters;

    /// @notice The mapping of the configured mint allowances
    mapping(address => uint256) private _mintersAllowance;

    // -------------------- Errors -----------------------------------

    /**
     * @notice The transaction sender is not a master minter
     *
     * @param account The address of the transaction sender
     */
    error UnauthorizedMasterMinter(address account);

    /**
     * @notice The transaction sender is not a minter
     *
     * @param account The address of the transaction sender
     */
    error UnauthorizedMinter(address account);

    /// @notice The mint allowance is exceeded during the mint operation
    error ExceededMintAllowance();

    /// @notice The zero amount of tokens is passed during the mint operation
    error ZeroMintAmount();

    /// @notice The zero amount of tokens is passed during the burn operation
    error ZeroBurnAmount();

    // -------------------- Modifiers --------------------------------

    /**
     * @notice Throws if called by any account other than the master minter
     */
    modifier onlyMasterMinter() {
        if (_msgSender() != _masterMinter) {
            revert UnauthorizedMasterMinter(_msgSender());
        }
        _;
    }

    /**
     * @notice Throws if called by any account other than the minter
     */
    modifier onlyMinter() {
        if (!_minters[_msgSender()]) {
            revert UnauthorizedMinter(_msgSender());
        }
        _;
    }

    // -------------------- Functions --------------------------------

    /**
     * @notice The internal initializer of the upgradable contract
     */
    function __ERC20Mintable_init(string memory name_, string memory symbol_) internal onlyInitializing {
        __Context_init_unchained();
        __Ownable_init_unchained();
        __Pausable_init_unchained();
        __PausableExt_init_unchained();
        __Blacklistable_init_unchained();
        __ERC20_init_unchained(name_, symbol_);
        __ERC20Base_init_unchained();
        __ERC20Mintable_init_unchained();
    }

    /**
     * @notice The internal unchained initializer of the upgradable contract
     */
    function __ERC20Mintable_init_unchained() internal onlyInitializing {}

    /**
     * @inheritdoc IERC20Mintable
     *
     * @dev Can only be called by the contract owner
     */
    function updateMasterMinter(address newMasterMinter) external onlyOwner {
        if (_masterMinter == newMasterMinter) {
            return;
        }

        _masterMinter = newMasterMinter;

        emit MasterMinterChanged(_masterMinter);
    }

    /**
     * @inheritdoc IERC20Mintable
     *
     * @dev The contract must not be paused
     * @dev Can only be called by the master minter
     */
    function configureMinter(
        address minter,
        uint256 mintAllowance
    ) external override whenNotPaused onlyMasterMinter returns (bool) {
        _minters[minter] = true;
        _mintersAllowance[minter] = mintAllowance;

        emit MinterConfigured(minter, mintAllowance);

        return true;
    }

    /**
     * @inheritdoc IERC20Mintable
     *
     * @dev Can only be called by the master minter
     */
    function removeMinter(address minter) external onlyMasterMinter returns (bool) {
        if (!_minters[minter]) {
            return true;
        }

        _minters[minter] = false;
        _mintersAllowance[minter] = 0;

        emit MinterRemoved(minter);

        return true;
    }

    /**
     * @inheritdoc IERC20Mintable
     *
     * @dev The contract must not be paused
     * @dev Can only be called by a minter account
     * @dev The message sender must not be blacklisted
     * @dev The `account` address must not be blacklisted
     * @dev The `amount` value must be greater than zero and not
     * greater than the mint allowance of the minter
     */
    function mint(
        address account,
        uint256 amount
    ) external onlyMinter notBlacklisted(_msgSender()) returns (bool) {
        if (amount == 0) {
            revert ZeroMintAmount();
        }

        uint256 mintAllowance = _mintersAllowance[_msgSender()];
        if (amount > mintAllowance) {
            revert ExceededMintAllowance();
        }

        _mintersAllowance[_msgSender()] = mintAllowance - amount;
        emit Mint(_msgSender(), account, amount);

        _mint(account, amount);

        return true;
    }

    /**
     * @inheritdoc IERC20Mintable
     *
     * @dev The contract must not be paused
     * @dev Can only be called by a minter account
     * @dev The message sender must not be blacklisted
     * @dev The `amount` value must be greater than zero
     */
    function burn(uint256 amount) external onlyMinter notBlacklisted(_msgSender()) {
        if (amount == 0) {
            revert ZeroBurnAmount();
        }

        _burn(_msgSender(), amount);

        emit Burn(_msgSender(), amount);
    }

    /**
     * @inheritdoc IERC20Mintable
     */
    function masterMinter() external view returns (address) {
        return _masterMinter;
    }

    /**
     * @inheritdoc IERC20Mintable
     */
    function isMinter(address account) external view returns (bool) {
        return _minters[account];
    }

    /**
     * @inheritdoc IERC20Mintable
     */
    function minterAllowance(address minter) external view returns (uint256) {
        return _mintersAllowance[minter];
    }
}

/**
 * @title IERC20Restrictable interface
 * @author CloudWalk Inc.
 * @notice The interface of a token that supports restriction operations
 */
interface IERC20Restrictable {
    /**
     * @notice Emitted when the restriction purposes are assigned to an account
     *
     * @param account The account the restriction purposes are assigned to
     * @param newPurposes The array of the new restriction purposes
     * @param oldPurposes The array of the old restriction purposes
     */
    event AssignPurposes(address indexed account, bytes32[] newPurposes, bytes32[] oldPurposes);

    /**
     * @notice Emitted when the restriction is updated for an account
     *
     * @param account The account the restriction is updated for
     * @param purpose The restriction purpose
     * @param newBalance The new restricted balance
     * @param oldBalance The old restricted balance
     */
    event UpdateRestriction(address indexed account, bytes32 indexed purpose, uint256 newBalance, uint256 oldBalance);

    /**
     * @notice Assigns the restriction purposes to an account
     *
     * @param account The account to assign purposes to
     * @param purposes The purposes to assign
     */
    function assignPurposes(address account, bytes32[] memory purposes) external;

    /**
     * @notice Returns the restriction purposes assigned to an account
     *
     * @param account The account to fetch the purposes for
     */
    function assignedPurposes(address account) external view returns (bytes32[] memory);

    /**
     * @notice Updates the restriction balance for an account
     *
     * @param account The account to update restriction for
     * @param purpose The restriction purpose
     * @param balance The new restricted balance
     */
    function updateRestriction(address account, bytes32 purpose, uint256 balance) external;

    /**
     * @notice Returns the restricted balance for the account and the restriction purpose
     *
     * @param account The account to get the balance of
     * @param purpose The restriction purpose to check (if zero, returns the total restricted balance)
     */
    function balanceOfRestricted(address account, bytes32 purpose) external view returns (uint256);
}

/**
 * @title ERC20Restrictable contract
 * @author CloudWalk Inc.
 * @notice The ERC20 token implementation that supports restriction operations
 */
abstract contract ERC20Restrictable is ERC20Base, IERC20Restrictable {
    /// @notice The mapping of the assigned purposes: account => purposes
    mapping(address => bytes32[]) private _purposeAssignments;

    /// @notice The mapping of the total restricted balances: account => total balance
    mapping(address => uint256) private _totalRestrictedBalances;

    /// @notice The mapping of the restricted purpose balances: account => purpose => balance
    mapping(address => mapping(bytes32 => uint256)) private _restrictedPurposeBalances;

    // -------------------- Errors -----------------------------------

    /// @notice Thrown when the zero restriction purpose is passed to the function
    error ZeroPurpose();

    /// @notice Thrown when the transfer amount exceeds the restricted balance
    error TransferExceededRestrictedAmount();

    // -------------------- Functions --------------------------------

    /**
     * @notice The internal initializer of the upgradable contract
     */
    function __ERC20Restrictable_init(string memory name_, string memory symbol_) internal onlyInitializing {
        __Context_init_unchained();
        __Ownable_init_unchained();
        __Pausable_init_unchained();
        __PausableExt_init_unchained();
        __Blacklistable_init_unchained();
        __ERC20_init_unchained(name_, symbol_);
        __ERC20Base_init_unchained();
        __ERC20Restrictable_init_unchained();
    }

    /**
     * @notice The internal unchained initializer of the upgradable contract
     */
    function __ERC20Restrictable_init_unchained() internal onlyInitializing {}

    /**
     * @inheritdoc IERC20Restrictable
     */
    function assignPurposes(address account, bytes32[] memory purposes) external onlyOwner {
        for (uint256 i = 0; i < purposes.length; i++) {
            if (purposes[i] == bytes32(0)) {
                revert ZeroPurpose();
            }
        }

        emit AssignPurposes(account, purposes, _purposeAssignments[account]);

        _purposeAssignments[account] = purposes;
    }

    /**
     * @inheritdoc IERC20Restrictable
     */
    function updateRestriction(address account, bytes32 purpose, uint256 balance) external onlyBlacklister {
        if (purpose == bytes32(0)) {
            revert ZeroPurpose();
        }

        uint256 oldBalance = _restrictedPurposeBalances[account][purpose];
        _restrictedPurposeBalances[account][purpose] = balance;

        if (oldBalance > balance) {
            _totalRestrictedBalances[account] -= oldBalance - balance;
        } else {
            _totalRestrictedBalances[account] += balance - oldBalance;
        }

        emit UpdateRestriction(account, purpose, balance, oldBalance);
    }

    /**
     * @inheritdoc IERC20Restrictable
     */
    function assignedPurposes(address account) external view returns (bytes32[] memory) {
        return _purposeAssignments[account];
    }

    /**
     * @inheritdoc IERC20Restrictable
     */
    function balanceOfRestricted(address account, bytes32 purpose) external view returns (uint256) {
        if (purpose == bytes32(0)) {
            return _totalRestrictedBalances[account];
        } else {
            return _restrictedPurposeBalances[account][purpose];
        }
    }

    /**
     * @inheritdoc ERC20Base
     */
    function _afterTokenTransfer(address from, address to, uint256 amount) internal virtual override {
        // Execute basic transfer logic
        super._afterTokenTransfer(from, to, amount);

        // Execute restricted transfer logic
        uint256 restrictedBalance = _totalRestrictedBalances[from];
        if (restrictedBalance != 0) {
            uint256 purposeAmount = amount;
            bytes32[] memory purposes = _purposeAssignments[to];

            for (uint256 i = 0; i < purposes.length; i++) {
                bytes32 purpose = purposes[i];
                uint256 purposeBalance = _restrictedPurposeBalances[from][purpose];

                if (purposeBalance != 0) {
                    if (purposeBalance > purposeAmount) {
                        restrictedBalance -= purposeAmount;
                        purposeBalance -= purposeAmount;
                        purposeAmount = 0;
                    } else {
                        restrictedBalance -= purposeBalance;
                        purposeAmount -= purposeBalance;
                        purposeBalance = 0;
                    }
                    _restrictedPurposeBalances[from][purpose] = purposeBalance;
                }

                if (purposeAmount == 0) {
                    break;
                }
            }

            if (_balanceOf_ERC20Restrictable(from) < restrictedBalance) {
                revert TransferExceededRestrictedAmount();
            }

            _totalRestrictedBalances[from] = restrictedBalance;
        }
    }

    /**
     * @notice Returns the transferable amount of tokens owned by account
     *
     * @param account The account to get the balance of
     */
    function _balanceOf_ERC20Restrictable(address account) internal view virtual returns (uint256);

    /**
     * @dev This empty reserved space is put in place to allow future versions
     * to add new variables without shifting down storage in the inheritance chain
     */
    uint256[47] private __gap;
}

/**
 * @title BRLCToken contract
 * @author CloudWalk Inc.
 * @notice The BRLC token implementation that supports minting, burning and freezing operations
 */
contract BRLCToken is ERC20Base, ERC20Mintable, ERC20Freezable, ERC20Restrictable, ERC20Hookable {
    /**
     * @notice Constructor that prohibits the initialization of the implementation of the upgradable contract
     *
     * See details
     * https://docs.openzeppelin.com/upgrades-plugins/1.x/writing-upgradeable#initializing_the_implementation_contract
     *
     * @custom:oz-upgrades-unsafe-allow constructor
     */
    constructor() {
        _disableInitializers();
    }

    /**
     * @notice The initializer of the upgradable contract
     *
     * See details https://docs.openzeppelin.com/upgrades-plugins/1.x/writing-upgradeable
     *
     * @param name_ The name of the token
     * @param symbol_ The symbol of the token
     */
    function initialize(string memory name_, string memory symbol_) external virtual initializer {
        __BRLCToken_init(name_, symbol_);
    }

    /**
     * @notice The internal initializer of the upgradable contract
     *
     * See {BRLCToken-initialize}
     */
    function __BRLCToken_init(string memory name_, string memory symbol_) internal onlyInitializing {
        __Context_init_unchained();
        __Ownable_init_unchained();
        __Pausable_init_unchained();
        __PausableExt_init_unchained();
        __Blacklistable_init_unchained();
        __ERC20_init_unchained(name_, symbol_);
        __ERC20Base_init_unchained();
        __ERC20Mintable_init_unchained();
        __ERC20Freezable_init_unchained();
        __BRLCToken_init_unchained();
    }

    /**
     * @notice The internal unchained initializer of the upgradable contract
     *
     * See {BRLCToken-initialize}
     */
    function __BRLCToken_init_unchained() internal onlyInitializing {}

    /**
     * @notice Returns true if token is BRLCoin implementation
     */
    function isBRLCoin() external pure returns (bool) {
        return true;
    }

    /**
     * @dev See {ERC20Base-_beforeTokenTransfer}
     * @dev See {ERC20Hookable-_beforeTokenTransfer}
     */
    function _beforeTokenTransfer(
        address from,
        address to,
        uint256 amount
    ) internal virtual override(ERC20Base, ERC20Hookable) {
        super._beforeTokenTransfer(from, to, amount);
    }

    /**
     * @dev See {ERC20Base-_afterTokenTransfer}
     * @dev See {ERC20Freezable-_afterTokenTransfer}
     * @dev See {ERC20Restrictable-_afterTokenTransfer}
     * @dev See {ERC20Hookable-_afterTokenTransfer}
     */
    function _afterTokenTransfer(
        address from,
        address to,
        uint256 amount
    ) internal virtual override(ERC20Base, ERC20Freezable, ERC20Restrictable, ERC20Hookable) {
        super._afterTokenTransfer(from, to, amount);
    }

    /**
     * @inheritdoc ERC20Restrictable
     */
    function _balanceOf_ERC20Restrictable(address account) internal view virtual override returns (uint256) {
        return balanceOf(account) - balanceOfFrozen(account);
    }
}
