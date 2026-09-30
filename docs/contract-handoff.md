# BNB Testnet contract integration

Archava uses the deployed ArchavaRentalV2 contract for time-limited wallet access and ArchavaMockUSDT for testnet checkout. Conversation usage stays offchain in the API's shared usage ledger.

## Deployments

| Contract | Chain | Address |
| --- | --- | --- |
| ArchavaRentalV2 | BNB Smart Chain Testnet, chain ID 97 | `0xefaacd259136d40cd26a8672697b591c1f84200c` |
| ArchavaMockUSDT (mUSDT) | BNB Smart Chain Testnet, chain ID 97 | `0xd63574ac426169cda5495e1e196bf64d124267d0` |

Both contracts are verified on [BscScan Testnet](https://testnet.bscscan.com). mUSDT is a demo token with six decimals and is not official USDT. Its faucet mints 100 mUSDT once per wallet.

## Purchase rules

| Package ID | Minutes | Price |
| ---: | ---: | ---: |
| 1 | 60 | 4.9375 mUSDT |
| 2 | 300 | 18.6875 mUSDT |

The wallet approves the exact quoted token amount and calls `rent(packageId)`. `AccessRented` records the wallet, package, included minutes, expiry, and amount paid. Active rentals extend from the existing expiry; a new rental after expiry begins a fresh 30-day access period. Packages include minutes independently of that access duration.

The API sums `includedMinutes` from the wallet's purchase events, checks `hasActiveAccess(wallet)`, and subtracts completed usage and active reservations from the local usage ledger. A minute is 60 seconds of allocated LiveKit room time. Repeated purchases add minutes. The ledger is shared by website calls and all API keys; one wallet can have one active paid room at a time.

## Runtime configuration

Set these in the private API environment:

```dotenv
CHAIN_ID=97
RPC_URL=https://your-private-quicknode-bnb-testnet-endpoint
RENTAL_CONTRACT=0xefaacd259136d40cd26a8672697b591c1f84200c
PAYMENT_TOKEN_CONTRACT=0xd63574ac426169cda5495e1e196bf64d124267d0
RENTAL_DEPLOYMENT_BLOCK=134052025
```

`RPC_URL` must remain server-side. `RENTAL_CONTRACT`, `PAYMENT_TOKEN_CONTRACT`, and the deployment block are public values. The API reads purchase events in 5,000-block pages from the deployment block so RPC providers with log-range limits can serve balances.

Keep one API process on persistent storage for the hackathon demo. The JSON usage ledger is not suitable for multiple instances or ephemeral serverless storage.
