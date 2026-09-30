# Archava testnet contracts

This package contains the Solidity sources, Foundry tests, deployment scripts,
verification scripts, and public BNB Smart Chain Testnet deployment artifacts
for ArchavaRentalV2 and ArchavaMockUSDT.

Install the package dependencies with `npm install`, then run
`git submodule update --init --recursive` to fetch `forge-std`. From this
directory, `npm test` runs Solidity tests, `npm run test:deployment` runs
deployment-script tests, and `npm run build` compiles the contracts. Foundry
is also required. Run deployment scripts only when intentionally deploying;
they require `BSC_TESTNET_RPC_URL` and `BSC_DEPLOYER_PRIVATE_KEY`. Never commit
`.env` files or expose deployer keys.
