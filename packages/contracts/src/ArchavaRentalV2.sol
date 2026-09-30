// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";
import {Ownable2Step} from "@openzeppelin/contracts/access/Ownable2Step.sol";
import {Pausable} from "@openzeppelin/contracts/utils/Pausable.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";

/// @title Archava Rental V2
/// @notice Settles testnet mUSDT payments and records time-limited Archava access.
/// @dev Conversation-minute metering remains authoritative offchain. Package IDs avoid coupling
///      included minutes to the onchain expiry duration.
contract ArchavaRentalV2 is Ownable2Step, Pausable, ReentrancyGuard {
    using SafeERC20 for IERC20;

    uint8 public constant STARTER_PACKAGE = 1;
    uint8 public constant PRO_PACKAGE = 2;
    uint256 public constant ACCESS_VALIDITY = 30 days;

    struct Package {
        uint256 price;
        uint256 validityDuration;
        uint256 includedMinutes;
        bool active;
    }

    IERC20 public immutable paymentToken;

    mapping(uint8 => Package) private _packages;
    mapping(address => uint256) private _expiresAt;
    mapping(address => uint8) public activePackage;

    error ZeroAddress();
    error ZeroAmount();
    error InvalidPackageConfiguration();
    error UnsupportedPackage(uint8 packageId);

    event PackageConfigured(
        uint8 indexed packageId,
        uint256 price,
        uint256 validityDuration,
        uint256 includedMinutes,
        bool active
    );
    event AccessRented(
        address indexed wallet,
        uint8 indexed packageId,
        uint256 includedMinutes,
        uint256 expiresAt,
        uint256 amountPaid
    );
    event FundsWithdrawn(address indexed recipient, uint256 amount);

    constructor(address initialOwner, IERC20 paymentToken_) Ownable(initialOwner) {
        if (address(paymentToken_) == address(0)) revert ZeroAddress();
        paymentToken = paymentToken_;

        _setPackage(STARTER_PACKAGE, 4_937_500, ACCESS_VALIDITY, 60, true);
        _setPackage(PRO_PACKAGE, 18_687_500, ACCESS_VALIDITY, 300, true);
    }

    function quoteRent(uint8 packageId) external view returns (uint256) {
        return _activePackage(packageId).price;
    }

    function packageDetails(uint8 packageId) external view returns (Package memory) {
        return _packages[packageId];
    }

    function expiresAt(address wallet) external view returns (uint256) {
        return _expiresAt[wallet];
    }

    function hasActiveAccess(address wallet) external view returns (bool) {
        return _expiresAt[wallet] > block.timestamp;
    }

    function rent(uint8 packageId) external nonReentrant whenNotPaused {
        Package memory selected = _activePackage(packageId);
        paymentToken.safeTransferFrom(msg.sender, address(this), selected.price);

        uint256 currentExpiry = _expiresAt[msg.sender];
        uint256 newExpiry =
            (currentExpiry > block.timestamp ? currentExpiry : block.timestamp) +
            selected.validityDuration;

        _expiresAt[msg.sender] = newExpiry;
        activePackage[msg.sender] = packageId;

        emit AccessRented(
            msg.sender,
            packageId,
            selected.includedMinutes,
            newExpiry,
            selected.price
        );
    }

    function setPackage(
        uint8 packageId,
        uint256 price,
        uint256 validityDuration,
        uint256 includedMinutes,
        bool active
    ) external onlyOwner {
        _setPackage(packageId, price, validityDuration, includedMinutes, active);
    }

    function pause() external onlyOwner {
        _pause();
    }

    function unpause() external onlyOwner {
        _unpause();
    }

    function withdraw(address recipient, uint256 amount) external onlyOwner nonReentrant {
        if (recipient == address(0)) revert ZeroAddress();
        if (amount == 0) revert ZeroAmount();
        paymentToken.safeTransfer(recipient, amount);
        emit FundsWithdrawn(recipient, amount);
    }

    function _activePackage(uint8 packageId) private view returns (Package memory selected) {
        selected = _packages[packageId];
        if (!selected.active) revert UnsupportedPackage(packageId);
    }

    function _setPackage(
        uint8 packageId,
        uint256 price,
        uint256 validityDuration,
        uint256 includedMinutes,
        bool active
    ) private {
        if (packageId == 0 || price == 0 || validityDuration == 0 || includedMinutes == 0) {
            revert InvalidPackageConfiguration();
        }
        _packages[packageId] = Package(price, validityDuration, includedMinutes, active);
        emit PackageConfigured(packageId, price, validityDuration, includedMinutes, active);
    }
}
