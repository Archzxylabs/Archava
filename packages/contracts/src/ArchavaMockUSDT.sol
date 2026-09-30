// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";
import {Ownable2Step} from "@openzeppelin/contracts/access/Ownable2Step.sol";

/// @title Archava Mock USDT
/// @notice Testnet-only payment token for the Archava hackathon demo. This is not official USDT.
contract ArchavaMockUSDT is ERC20, Ownable2Step {
    uint256 public constant FAUCET_AMOUNT = 100 * 10 ** 6;

    mapping(address => bool) public hasClaimed;

    error AlreadyClaimed();

    event FaucetClaimed(address indexed wallet, uint256 amount);

    constructor(address initialOwner) ERC20("Archava Mock USDT", "mUSDT") Ownable(initialOwner) {}

    function decimals() public pure override returns (uint8) {
        return 6;
    }

    /// @notice Mints 100 mUSDT once per wallet for BSC Testnet demo use.
    function faucet() external {
        if (hasClaimed[msg.sender]) revert AlreadyClaimed();
        hasClaimed[msg.sender] = true;
        _mint(msg.sender, FAUCET_AMOUNT);
        emit FaucetClaimed(msg.sender, FAUCET_AMOUNT);
    }
}
