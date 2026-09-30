// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {Test} from "forge-std/Test.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {Pausable} from "@openzeppelin/contracts/utils/Pausable.sol";
import {ArchavaMockUSDT} from "../src/ArchavaMockUSDT.sol";
import {ArchavaRentalV2} from "../src/ArchavaRentalV2.sol";

contract ArchavaRentalV2Test is Test {
    uint8 internal constant STARTER = 1;
    uint8 internal constant PRO = 2;
    uint256 internal constant STARTER_PRICE = 4_937_500;
    uint256 internal constant PRO_PRICE = 18_687_500;
    uint256 internal constant VALIDITY = 30 days;

    address internal owner = makeAddr("owner");
    address internal alice = makeAddr("alice");
    address internal treasury = makeAddr("treasury");

    ArchavaMockUSDT internal token;
    ArchavaRentalV2 internal rental;

    event AccessRented(
        address indexed wallet,
        uint8 indexed packageId,
        uint256 includedMinutes,
        uint256 expiresAt,
        uint256 amountPaid
    );

    function setUp() public {
        vm.warp(1_700_000_000);
        token = new ArchavaMockUSDT(owner);
        rental = new ArchavaRentalV2(owner, IERC20(address(token)));

        vm.prank(alice);
        token.faucet();
    }

    function test_MockUSDT_MetadataAndOneTimeFaucet() public {
        assertEq(token.name(), "Archava Mock USDT");
        assertEq(token.symbol(), "mUSDT");
        assertEq(token.decimals(), 6);
        assertEq(token.balanceOf(alice), 100_000_000);

        vm.prank(alice);
        vm.expectRevert(ArchavaMockUSDT.AlreadyClaimed.selector);
        token.faucet();
    }

    function test_PackagesMatchApprovedHackathonPricing() public view {
        ArchavaRentalV2.Package memory starter = rental.packageDetails(STARTER);
        assertEq(starter.price, STARTER_PRICE);
        assertEq(starter.validityDuration, VALIDITY);
        assertEq(starter.includedMinutes, 60);
        assertTrue(starter.active);

        ArchavaRentalV2.Package memory pro = rental.packageDetails(PRO);
        assertEq(pro.price, PRO_PRICE);
        assertEq(pro.validityDuration, VALIDITY);
        assertEq(pro.includedMinutes, 300);
        assertTrue(pro.active);
    }

    function test_RentRequiresAllowance() public {
        vm.prank(alice);
        vm.expectRevert();
        rental.rent(STARTER);
    }

    function test_RentStarterTransfersPaymentAndCreatesAccess() public {
        vm.startPrank(alice);
        token.approve(address(rental), STARTER_PRICE);
        vm.expectEmit(true, true, false, true);
        emit AccessRented(alice, STARTER, 60, block.timestamp + VALIDITY, STARTER_PRICE);
        rental.rent(STARTER);
        vm.stopPrank();

        assertEq(token.balanceOf(address(rental)), STARTER_PRICE);
        assertEq(rental.expiresAt(alice), block.timestamp + VALIDITY);
        assertEq(rental.activePackage(alice), STARTER);
        assertTrue(rental.hasActiveAccess(alice));
    }

    function test_RentExtendsActiveAccessAndCanUpgradePackage() public {
        vm.startPrank(alice);
        token.approve(address(rental), STARTER_PRICE + PRO_PRICE);
        rental.rent(STARTER);
        uint256 firstExpiry = rental.expiresAt(alice);
        vm.warp(block.timestamp + 1 days);
        rental.rent(PRO);
        vm.stopPrank();

        assertEq(rental.expiresAt(alice), firstExpiry + VALIDITY);
        assertEq(rental.activePackage(alice), PRO);
    }

    function test_UnsupportedPackageAndPausedRentRevert() public {
        vm.prank(alice);
        vm.expectRevert(abi.encodeWithSelector(ArchavaRentalV2.UnsupportedPackage.selector, 99));
        rental.rent(99);

        vm.prank(owner);
        rental.pause();
        vm.prank(alice);
        vm.expectRevert(Pausable.EnforcedPause.selector);
        rental.rent(STARTER);
    }

    function test_OwnerCanWithdrawPayments() public {
        vm.startPrank(alice);
        token.approve(address(rental), STARTER_PRICE);
        rental.rent(STARTER);
        vm.stopPrank();

        vm.prank(owner);
        rental.withdraw(treasury, STARTER_PRICE);
        assertEq(token.balanceOf(treasury), STARTER_PRICE);
        assertEq(token.balanceOf(address(rental)), 0);
    }

    function test_ZeroAddressesRevert() public {
        vm.expectRevert();
        new ArchavaMockUSDT(address(0));

        vm.expectRevert(ArchavaRentalV2.ZeroAddress.selector);
        new ArchavaRentalV2(owner, IERC20(address(0)));
    }
}
