// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";
import {Ownable2Step} from "@openzeppelin/contracts/access/Ownable2Step.sol";
import {Pausable} from "@openzeppelin/contracts/utils/Pausable.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";

/**
 * @title ArchavaRental
 * @notice Time-limited access-rental entitlement contract for Archava Platform.
 * @dev BNB Smart Chain (Testnet/Mainnet) is strictly the payment and entitlement settlement layer.
 *      Gemini, Spatius, LiveKit, provider credentials, usage metering, and session orchestration
 *      remain strictly offchain.
 *
 * All durations are specified in SECONDS.
 * All prices are specified in WEI (tBNB / BNB).
 */
contract ArchavaRental is Ownable2Step, Pausable, ReentrancyGuard {
    // =========================================================================
    // Custom Errors
    // =========================================================================

    /// @notice Thrown when an input duration is 0.
    error ZeroDuration();

    /// @notice Thrown when an input duration is not an active supported package.
    /// @param duration The unsupported duration requested (in seconds).
    error UnsupportedDuration(uint256 duration);

    /// @notice Thrown when exact payment is not provided.
    /// @param expected The exact price required in wei.
    /// @param actual The actual msg.value received in wei.
    error IncorrectPayment(uint256 expected, uint256 actual);

    /// @notice Thrown when a direct BNB transfer is attempted outside of rent().
    error DirectDepositNotAllowed();

    /// @notice Thrown when native BNB transfer to recipient fails.
    error TransferFailed();

    /// @notice Thrown when a zero address is provided for recipient or owner.
    error ZeroAddress();

    /// @notice Thrown when zero amount is specified for withdrawal.
    error ZeroAmount();

    /// @notice Thrown when a rental package is configured with no price.
    error ZeroPrice();

    /// @notice Thrown when input array lengths do not match.
    error ArrayLengthMismatch();

    // =========================================================================
    // Events
    // =========================================================================

    /// @notice Emitted when access time is purchased or extended for a wallet.
    /// @param wallet The address of the renting wallet.
    /// @param duration The duration added to the wallet's access in seconds.
    /// @param expiresAt The new absolute Unix timestamp (in seconds) when access expires.
    /// @param amountPaid The exact payment in wei received for this rental.
    event AccessRented(
        address indexed wallet,
        uint256 duration,
        uint256 expiresAt,
        uint256 amountPaid
    );

    /// @notice Emitted when a rental package pricing is added or updated.
    /// @param duration The duration in seconds.
    /// @param price The exact price in wei.
    event PackageConfigured(uint256 indexed duration, uint256 price);

    /// @notice Emitted when a rental package is deactivated/removed.
    /// @param duration The duration in seconds.
    event PackageRemoved(uint256 indexed duration);

    /// @notice Emitted when contract funds are securely withdrawn by the owner.
    /// @param recipient The address receiving the native BNB.
    /// @param amount The amount in wei withdrawn.
    event FundsWithdrawn(address indexed recipient, uint256 amount);

    // =========================================================================
    // State Variables
    // =========================================================================

    /// @dev Mapping of wallet address => absolute expiration Unix timestamp (in seconds).
    mapping(address => uint256) private _expiresAt;

    /// @dev Mapping of duration in seconds => exact price in wei.
    mapping(uint256 => uint256) public packagePrices;

    /// @dev Mapping of duration in seconds => whether the package is actively supported.
    mapping(uint256 => bool) public isPackageSupported;

    /// @dev List of all currently configured package durations.
    uint256[] private _supportedDurations;

    // =========================================================================
    // Constructor
    // =========================================================================

    /**
     * @notice Initializes the rental contract with initial owner and demo packages.
     * @param initialOwner Address of the contract owner.
     * @param initialDurations Array of initial supported package durations in seconds.
     * @param initialPrices Array of exact package prices in wei.
     */
    constructor(
        address initialOwner,
        uint256[] memory initialDurations,
        uint256[] memory initialPrices
    ) Ownable(initialOwner) {
        if (initialOwner == address(0)) revert ZeroAddress();
        if (initialDurations.length != initialPrices.length) revert ArrayLengthMismatch();

        for (uint256 i = 0; i < initialDurations.length; i++) {
            _setPackage(initialDurations[i], initialPrices[i]);
        }
    }

    // =========================================================================
    // External View Functions
    // =========================================================================

    /**
     * @notice Quotes the exact rental price in wei for a given package duration.
     * @param duration Package duration in seconds.
     * @return price Exact price in wei.
     */
    function quoteRent(uint256 duration) external view returns (uint256 price) {
        if (duration == 0) revert ZeroDuration();
        if (!isPackageSupported[duration]) revert UnsupportedDuration(duration);
        return packagePrices[duration];
    }

    /**
     * @notice Returns the access expiration Unix timestamp for a wallet.
     * @param wallet The address to inspect.
     * @return timestamp Unix timestamp in seconds when access expires (0 if never rented).
     */
    function expiresAt(address wallet) external view returns (uint256 timestamp) {
        return _expiresAt[wallet];
    }

    /**
     * @notice Returns whether a wallet currently has active (unexpired) rental access.
     * @param wallet The address to inspect.
     * @return active True if current block.timestamp is strictly before expiresAt(wallet).
     */
    function hasActiveAccess(address wallet) external view returns (bool active) {
        return _expiresAt[wallet] > block.timestamp;
    }

    /**
     * @notice Returns list of all currently supported package durations.
     * @return Array of durations in seconds.
     */
    function getSupportedDurations() external view returns (uint256[] memory) {
        return _supportedDurations;
    }

    // =========================================================================
    // External State-Changing Functions
    // =========================================================================

    /**
     * @notice Purchases or extends time-limited rental access for msg.sender.
     * @dev Follows checks-effects-interactions. Protected against reentrancy and pause.
     *      If active: extends from current expiry.
     *      If expired: extends from block.timestamp.
     *      Requires exact payment matching package price.
     * @param duration Supported package duration in seconds.
     */
    function rent(uint256 duration) external payable nonReentrant whenNotPaused {
        if (duration == 0) revert ZeroDuration();
        if (!isPackageSupported[duration]) revert UnsupportedDuration(duration);

        uint256 expectedPrice = packagePrices[duration];
        if (msg.value != expectedPrice) {
            revert IncorrectPayment(expectedPrice, msg.value);
        }

        uint256 currentExpiry = _expiresAt[msg.sender];
        uint256 newExpiry;

        if (currentExpiry > block.timestamp) {
            // Access is active: extend from current expiry
            newExpiry = currentExpiry + duration;
        } else {
            // Access is expired or never rented: start from current block.timestamp
            newExpiry = block.timestamp + duration;
        }

        // State update (Effect)
        _expiresAt[msg.sender] = newExpiry;

        // Event emission
        emit AccessRented(msg.sender, duration, newExpiry, msg.value);
    }

    // =========================================================================
    // Administrative / Owner Functions
    // =========================================================================

    /**
     * @notice Sets or updates the price for a package duration.
     * @param duration Package duration in seconds (must be > 0).
     * @param price Exact price in wei.
     */
    function setPackage(uint256 duration, uint256 price) external onlyOwner {
        _setPackage(duration, price);
    }

    /**
     * @notice Removes / deactivates a package duration.
     * @param duration Package duration in seconds to remove.
     */
    function removePackage(uint256 duration) external onlyOwner {
        if (!isPackageSupported[duration]) revert UnsupportedDuration(duration);

        isPackageSupported[duration] = false;
        delete packagePrices[duration];

        // Remove from _supportedDurations array
        uint256 len = _supportedDurations.length;
        for (uint256 i = 0; i < len; i++) {
            if (_supportedDurations[i] == duration) {
                _supportedDurations[i] = _supportedDurations[len - 1];
                _supportedDurations.pop();
                break;
            }
        }

        emit PackageRemoved(duration);
    }

    /**
     * @notice Pauses new rental purchases in case of emergency.
     */
    function pause() external onlyOwner {
        _pause();
    }

    /**
     * @notice Resumes rental purchases.
     */
    function unpause() external onlyOwner {
        _unpause();
    }

    /**
     * @notice Withdraws a specific amount of native BNB to a recipient.
     * @param recipient Target address to receive BNB.
     * @param amount Amount in wei to withdraw.
     */
    function withdraw(
        address payable recipient,
        uint256 amount
    ) external nonReentrant onlyOwner {
        if (recipient == address(0)) revert ZeroAddress();
        if (amount == 0) revert ZeroAmount();
        if (amount > address(this).balance) {
            revert IncorrectPayment(amount, address(this).balance);
        }

        emit FundsWithdrawn(recipient, amount);

        (bool success, ) = recipient.call{value: amount}("");
        if (!success) revert TransferFailed();
    }

    /**
     * @notice Withdraws the full contract balance of native BNB to recipient.
     * @param recipient Target address to receive BNB.
     */
    function withdrawAll(address payable recipient) external nonReentrant onlyOwner {
        if (recipient == address(0)) revert ZeroAddress();
        uint256 balance = address(this).balance;
        if (balance == 0) revert ZeroAmount();

        emit FundsWithdrawn(recipient, balance);

        (bool success, ) = recipient.call{value: balance}("");
        if (!success) revert TransferFailed();
    }

    // =========================================================================
    // Internal Helper Functions
    // =========================================================================

    function _setPackage(uint256 duration, uint256 price) internal {
        if (duration == 0) revert ZeroDuration();
        if (price == 0) revert ZeroPrice();

        if (!isPackageSupported[duration]) {
            isPackageSupported[duration] = true;
            _supportedDurations.push(duration);
        }

        packagePrices[duration] = price;
        emit PackageConfigured(duration, price);
    }

    // =========================================================================
    // Rejection of Accidental Direct Transfers
    // =========================================================================

    receive() external payable {
        revert DirectDepositNotAllowed();
    }

    fallback() external payable {
        revert DirectDepositNotAllowed();
    }
}
