// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {Test} from "forge-std/Test.sol";
import {ArchavaRental} from "../src/ArchavaRental.sol";
import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";
import {Pausable} from "@openzeppelin/contracts/utils/Pausable.sol";

contract MaliciousRecipient {
    // Reverts on receiving native BNB
    receive() external payable {
        revert("Rejecting BNB");
    }
}

contract ArchavaRentalTest is Test {
    ArchavaRental public rental;

    address public owner = makeAddr("owner");
    address public alice = makeAddr("alice");
    address public bob = makeAddr("bob");

    // Standard demo packages:
    // Package 1: 1 hour (3600 seconds) -> 0.0005 tBNB
    // Package 2: 1 day (86400 seconds) -> 0.002 tBNB
    // Package 3: 7 days (604800 seconds) -> 0.01 tBNB
    uint256 public constant DURATION_1_HOUR = 3600;
    uint256 public constant PRICE_1_HOUR = 0.0005 ether;

    uint256 public constant DURATION_1_DAY = 86400;
    uint256 public constant PRICE_1_DAY = 0.002 ether;

    uint256 public constant DURATION_7_DAYS = 604800;
    uint256 public constant PRICE_7_DAYS = 0.01 ether;

    event AccessRented(
        address indexed wallet,
        uint256 duration,
        uint256 expiresAt,
        uint256 amountPaid
    );
    event PackageConfigured(uint256 indexed duration, uint256 price);
    event PackageRemoved(uint256 indexed duration);
    event FundsWithdrawn(address indexed recipient, uint256 amount);

    function setUp() public {
        vm.warp(1_700_000_000); // Set baseline realistic timestamp

        uint256[] memory durations = new uint256[](3);
        durations[0] = DURATION_1_HOUR;
        durations[1] = DURATION_1_DAY;
        durations[2] = DURATION_7_DAYS;

        uint256[] memory prices = new uint256[](3);
        prices[0] = PRICE_1_HOUR;
        prices[1] = PRICE_1_DAY;
        prices[2] = PRICE_7_DAYS;

        rental = new ArchavaRental(owner, durations, prices);

        // Fund test users
        vm.deal(alice, 10 ether);
        vm.deal(bob, 10 ether);
    }

    // =========================================================================
    // Constructor Tests
    // =========================================================================

    function test_Constructor_ZeroOwner_Reverts() public {
        uint256[] memory durations = new uint256[](1);
        uint256[] memory prices = new uint256[](1);

        vm.expectRevert(
            abi.encodeWithSelector(
                Ownable.OwnableInvalidOwner.selector,
                address(0)
            )
        );
        new ArchavaRental(address(0), durations, prices);
    }

    function test_Constructor_MismatchedArrays_Reverts() public {
        uint256[] memory durations = new uint256[](2);
        uint256[] memory prices = new uint256[](1);

        vm.expectRevert(ArchavaRental.ArrayLengthMismatch.selector);
        new ArchavaRental(owner, durations, prices);
    }

    function test_Constructor_ZeroPrice_Reverts() public {
        uint256[] memory durations = new uint256[](1);
        durations[0] = DURATION_1_HOUR;
        uint256[] memory prices = new uint256[](1);
        prices[0] = 0;

        vm.expectRevert(ArchavaRental.ZeroPrice.selector);
        new ArchavaRental(owner, durations, prices);
    }

    // =========================================================================
    // Quote Tests
    // =========================================================================

    function test_QuoteRent_SupportedPackages() public view {
        assertEq(rental.quoteRent(DURATION_1_HOUR), PRICE_1_HOUR);
        assertEq(rental.quoteRent(DURATION_1_DAY), PRICE_1_DAY);
        assertEq(rental.quoteRent(DURATION_7_DAYS), PRICE_7_DAYS);
    }

    function test_QuoteRent_ZeroDuration_Reverts() public {
        vm.expectRevert(ArchavaRental.ZeroDuration.selector);
        rental.quoteRent(0);
    }

    function test_QuoteRent_UnsupportedDuration_Reverts() public {
        vm.expectRevert(abi.encodeWithSelector(ArchavaRental.UnsupportedDuration.selector, 1234));
        rental.quoteRent(1234);
    }

    // =========================================================================
    // Rent Payment Validation Tests
    // =========================================================================

    function test_Rent_ZeroDuration_Reverts() public {
        vm.prank(alice);
        vm.expectRevert(ArchavaRental.ZeroDuration.selector);
        rental.rent{value: PRICE_1_HOUR}(0);
    }

    function test_Rent_UnsupportedDuration_Reverts() public {
        vm.prank(alice);
        vm.expectRevert(abi.encodeWithSelector(ArchavaRental.UnsupportedDuration.selector, 999));
        rental.rent{value: PRICE_1_HOUR}(999);
    }

    function test_Rent_InsufficientPayment_Reverts() public {
        vm.prank(alice);
        vm.expectRevert(
            abi.encodeWithSelector(
                ArchavaRental.IncorrectPayment.selector,
                PRICE_1_HOUR,
                PRICE_1_HOUR - 1
            )
        );
        rental.rent{value: PRICE_1_HOUR - 1}(DURATION_1_HOUR);
    }

    function test_Rent_ExcessPayment_Reverts() public {
        vm.prank(alice);
        vm.expectRevert(
            abi.encodeWithSelector(
                ArchavaRental.IncorrectPayment.selector,
                PRICE_1_HOUR,
                PRICE_1_HOUR + 1
            )
        );
        rental.rent{value: PRICE_1_HOUR + 1}(DURATION_1_HOUR);
    }

    // =========================================================================
    // Rent Execution & Expiration Tests
    // =========================================================================

    function test_Rent_FirstPurchase_SetsExpiryFromCurrentTimestamp() public {
        uint256 startTime = block.timestamp;
        assertEq(rental.expiresAt(alice), 0);
        assertFalse(rental.hasActiveAccess(alice));

        vm.prank(alice);
        vm.expectEmit(true, false, false, true);
        emit AccessRented(alice, DURATION_1_HOUR, startTime + DURATION_1_HOUR, PRICE_1_HOUR);

        rental.rent{value: PRICE_1_HOUR}(DURATION_1_HOUR);

        uint256 expectedExpiry = startTime + DURATION_1_HOUR;
        assertEq(rental.expiresAt(alice), expectedExpiry);
        assertTrue(rental.hasActiveAccess(alice));
        assertEq(address(rental).balance, PRICE_1_HOUR);
    }

    function test_Rent_ExtensionWhileActive_ExtendsFromCurrentExpiry() public {
        uint256 t0 = block.timestamp;

        // First purchase: 1 hour
        vm.prank(alice);
        rental.rent{value: PRICE_1_HOUR}(DURATION_1_HOUR);
        uint256 firstExpiry = t0 + DURATION_1_HOUR;
        assertEq(rental.expiresAt(alice), firstExpiry);

        // Advance time by 20 minutes (access still active)
        vm.warp(t0 + 1200);
        assertTrue(rental.hasActiveAccess(alice));

        // Second purchase: 1 day
        vm.prank(alice);
        vm.expectEmit(true, false, false, true);
        emit AccessRented(
            alice,
            DURATION_1_DAY,
            firstExpiry + DURATION_1_DAY,
            PRICE_1_DAY
        );
        rental.rent{value: PRICE_1_DAY}(DURATION_1_DAY);

        assertEq(rental.expiresAt(alice), firstExpiry + DURATION_1_DAY);
        assertTrue(rental.hasActiveAccess(alice));
    }

    function test_Rent_PurchaseAfterExpiry_ExtendsFromBlockTimestamp() public {
        uint256 t0 = block.timestamp;

        // First purchase: 1 hour
        vm.prank(alice);
        rental.rent{value: PRICE_1_HOUR}(DURATION_1_HOUR);
        uint256 firstExpiry = t0 + DURATION_1_HOUR;
        assertEq(rental.expiresAt(alice), firstExpiry);

        // Advance time to 2 hours later (access has expired)
        uint256 t1 = t0 + 7200;
        vm.warp(t1);
        assertFalse(rental.hasActiveAccess(alice));

        // Renewal purchase: 7 days
        vm.prank(alice);
        vm.expectEmit(true, false, false, true);
        emit AccessRented(
            alice,
            DURATION_7_DAYS,
            t1 + DURATION_7_DAYS,
            PRICE_7_DAYS
        );
        rental.rent{value: PRICE_7_DAYS}(DURATION_7_DAYS);

        assertEq(rental.expiresAt(alice), t1 + DURATION_7_DAYS);
        assertTrue(rental.hasActiveAccess(alice));
    }

    function test_Rent_BoundaryTimestamp_AtExactExpiry() public {
        uint256 t0 = block.timestamp;

        vm.prank(alice);
        rental.rent{value: PRICE_1_HOUR}(DURATION_1_HOUR);
        uint256 firstExpiry = t0 + DURATION_1_HOUR;

        // Warp exactly to the expiry timestamp
        vm.warp(firstExpiry);
        // At exact expiry, block.timestamp == expiresAt, so hasActiveAccess must be false (strictly greater)
        assertFalse(rental.hasActiveAccess(alice));

        // When renting at exact expiry (currentExpiry <= block.timestamp), starts from block.timestamp
        vm.prank(alice);
        rental.rent{value: PRICE_1_HOUR}(DURATION_1_HOUR);
        assertEq(rental.expiresAt(alice), firstExpiry + DURATION_1_HOUR);
    }

    // =========================================================================
    // Pause / Unpause Tests
    // =========================================================================

    function test_Rent_WhenPaused_Reverts() public {
        vm.prank(owner);
        rental.pause();

        vm.prank(alice);
        vm.expectRevert(Pausable.EnforcedPause.selector);
        rental.rent{value: PRICE_1_HOUR}(DURATION_1_HOUR);

        // Unpause allows renting again
        vm.prank(owner);
        rental.unpause();

        vm.prank(alice);
        rental.rent{value: PRICE_1_HOUR}(DURATION_1_HOUR);
        assertTrue(rental.hasActiveAccess(alice));
    }

    // =========================================================================
    // Direct Transfer Rejection Tests
    // =========================================================================

    function test_DirectTransfer_Receive_Reverts() public {
        vm.prank(alice);
        vm.expectRevert(ArchavaRental.DirectDepositNotAllowed.selector);
        (bool success, ) = address(rental).call{value: 1 ether}("");
        assertTrue(success); // foundry call status
    }

    function test_DirectTransfer_Fallback_Reverts() public {
        vm.prank(alice);
        vm.expectRevert(ArchavaRental.DirectDepositNotAllowed.selector);
        (bool success, ) = address(rental).call{value: 1 ether}(hex"12345678");
        assertTrue(success);
    }

    // =========================================================================
    // Administrative Tests
    // =========================================================================

    function test_Admin_UnauthorizedCalls_Revert() public {
        vm.startPrank(alice);

        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, alice));
        rental.setPackage(100, 1 ether);

        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, alice));
        rental.removePackage(DURATION_1_HOUR);

        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, alice));
        rental.pause();

        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, alice));
        rental.unpause();

        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, alice));
        rental.withdraw(payable(alice), 1 ether);

        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, alice));
        rental.withdrawAll(payable(alice));

        vm.stopPrank();
    }

    function test_Admin_SetAndRemovePackage() public {
        uint256 newDuration = 7200; // 2 hours
        uint256 newPrice = 0.001 ether;

        vm.startPrank(owner);

        // Set new package
        vm.expectEmit(true, false, false, true);
        emit PackageConfigured(newDuration, newPrice);
        rental.setPackage(newDuration, newPrice);

        assertTrue(rental.isPackageSupported(newDuration));
        assertEq(rental.quoteRent(newDuration), newPrice);

        // Remove package
        vm.expectEmit(true, false, false, true);
        emit PackageRemoved(newDuration);
        rental.removePackage(newDuration);

        assertFalse(rental.isPackageSupported(newDuration));
        vm.expectRevert(abi.encodeWithSelector(ArchavaRental.UnsupportedDuration.selector, newDuration));
        rental.quoteRent(newDuration);

        // Zero duration reverts
        vm.expectRevert(ArchavaRental.ZeroDuration.selector);
        rental.setPackage(0, 1 ether);

        // Zero price reverts
        vm.expectRevert(ArchavaRental.ZeroPrice.selector);
        rental.setPackage(1800, 0);

        vm.stopPrank();
    }

    function test_Admin_Withdraw_Success() public {
        // Alice buys access
        vm.prank(alice);
        rental.rent{value: PRICE_1_HOUR}(DURATION_1_HOUR);

        uint256 contractBalance = address(rental).balance;
        assertEq(contractBalance, PRICE_1_HOUR);

        address recipient = makeAddr("treasury");
        uint256 withdrawAmount = 0.0002 ether;

        vm.prank(owner);
        vm.expectEmit(true, false, false, true);
        emit FundsWithdrawn(recipient, withdrawAmount);
        rental.withdraw(payable(recipient), withdrawAmount);

        assertEq(recipient.balance, withdrawAmount);
        assertEq(address(rental).balance, contractBalance - withdrawAmount);
    }

    function test_Admin_WithdrawAll_Success() public {
        vm.prank(alice);
        rental.rent{value: PRICE_1_DAY}(DURATION_1_DAY);

        address recipient = makeAddr("treasury");
        uint256 totalBalance = address(rental).balance;

        vm.prank(owner);
        vm.expectEmit(true, false, false, true);
        emit FundsWithdrawn(recipient, totalBalance);
        rental.withdrawAll(payable(recipient));

        assertEq(recipient.balance, totalBalance);
        assertEq(address(rental).balance, 0);
    }

    function test_Admin_Withdraw_ValidationReverts() public {
        vm.startPrank(owner);

        // Zero address
        vm.expectRevert(ArchavaRental.ZeroAddress.selector);
        rental.withdraw(payable(address(0)), 1 ether);

        // Zero amount
        vm.expectRevert(ArchavaRental.ZeroAmount.selector);
        rental.withdraw(payable(owner), 0);

        // Excessive amount
        vm.expectRevert(
            abi.encodeWithSelector(
                ArchavaRental.IncorrectPayment.selector,
                1 ether,
                0
            )
        );
        rental.withdraw(payable(owner), 1 ether);

        // WithdrawAll on 0 balance
        vm.expectRevert(ArchavaRental.ZeroAmount.selector);
        rental.withdrawAll(payable(owner));

        vm.stopPrank();
    }

    function test_Admin_Withdraw_TransferFailed_Reverts() public {
        // Fund contract
        vm.prank(alice);
        rental.rent{value: PRICE_1_HOUR}(DURATION_1_HOUR);

        MaliciousRecipient mal = new MaliciousRecipient();

        vm.prank(owner);
        vm.expectRevert(ArchavaRental.TransferFailed.selector);
        rental.withdraw(payable(address(mal)), PRICE_1_HOUR);
    }
}
