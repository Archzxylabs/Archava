import { Contract, JsonRpcProvider, formatUnits, getAddress, isAddress } from "ethers";

// V2 packages are indexed onchain: 1 = 60 minutes, 2 = 300 minutes.
// Purchases are summed from AccessRented events because V2 deliberately stores
// access expiry rather than a mutable lifetime-minute counter.
export const RENTAL_ABI = [
  "function paymentToken() view returns (address)",
  "function quoteRent(uint8 packageId) view returns (uint256)",
  "function expiresAt(address wallet) view returns (uint256)",
  "function hasActiveAccess(address wallet) view returns (bool)",
  "event AccessRented(address indexed wallet, uint8 indexed packageId, uint256 includedMinutes, uint256 expiresAt, uint256 amountPaid)",
];

export const TOKEN_ABI = [
  "function decimals() view returns (uint8)",
  "function symbol() view returns (string)",
];

const PACKAGES = new Map([[60, 1], [300, 2]]);
const START_BLOCK = 134052025;
const LOG_BLOCK_SPAN = 5000;
const TOKEN_DECIMALS = 6;

export function normalizeWallet(value) {
  if (typeof value !== "string" || !isAddress(value)) {
    throw new Error("Invalid wallet address");
  }
  return getAddress(value);
}

export function purchasedState(minutes) {
  const purchasedMinutes = Number(minutes);
  if (!Number.isSafeInteger(purchasedMinutes) || purchasedMinutes < 0 ||
      !Number.isSafeInteger(purchasedMinutes * 60)) {
    throw new Error("Invalid purchased minutes from contract");
  }
  return { purchasedMinutes };
}

function configuredContracts({ rpcUrl, contractAddress, tokenAddress, chainId, deploymentBlock = START_BLOCK }) {
  if (!contractAddress) return null;
  if (!isAddress(contractAddress)) throw new Error("RENTAL_CONTRACT (formerly PACK_CONTRACT) is invalid");
  if (!isAddress(tokenAddress || "")) throw new Error("PAYMENT_TOKEN_CONTRACT is missing or invalid");
  if (!rpcUrl) throw new Error("RPC_URL is required when RENTAL_CONTRACT is set");
  if (Number(chainId) !== 97) throw new Error("Archava Rental V2 is configured for BNB Testnet chain ID 97");
  if (!Number.isSafeInteger(Number(deploymentBlock)) || Number(deploymentBlock) <= 0) {
    throw new Error("RENTAL_DEPLOYMENT_BLOCK must be a positive block number");
  }
  const provider = new JsonRpcProvider(rpcUrl, Number(chainId));
  const rental = new Contract(contractAddress, RENTAL_ABI, provider);
  const token = new Contract(tokenAddress, TOKEN_ABI, provider);
  return { provider, rental, token, contractAddress: getAddress(contractAddress), tokenAddress: getAddress(tokenAddress), chainId: Number(chainId), deploymentBlock: Number(deploymentBlock) };
}

async function verifyConnection(connection) {
  if ((await connection.provider.getNetwork()).chainId !== BigInt(connection.chainId)) {
    throw new Error("RPC chain mismatch");
  }
  if (getAddress(await connection.rental.paymentToken()) !== connection.tokenAddress) {
    throw new Error("Payment token does not match RENTAL_CONTRACT");
  }
}

export function createCreditReader(config) {
  const connection = configuredContracts(config);
  if (!connection) return null;
  const { provider, rental, deploymentBlock } = connection;
  const eventFilter = rental.filters.AccessRented;
  return async (walletInput) => {
    await verifyConnection(connection);
    const wallet = normalizeWallet(walletInput);
    const latestBlock = await provider.getBlockNumber();
    const topics = eventFilter(wallet).topics;
    let purchasedMinutes = 0n;
    for (let fromBlock = deploymentBlock; fromBlock <= latestBlock; fromBlock += LOG_BLOCK_SPAN) {
      const toBlock = Math.min(fromBlock + LOG_BLOCK_SPAN - 1, latestBlock);
      const logs = await provider.getLogs({ address: connection.contractAddress, topics, fromBlock, toBlock });
      for (const log of logs) {
        const event = rental.interface.parseLog(log);
        if (event) purchasedMinutes += event.args.includedMinutes;
      }
    }
    const active = await rental.hasActiveAccess(wallet);
    return purchasedState(active ? purchasedMinutes : 0n);
  };
}

export function createPackQuoter(config) {
  const connection = configuredContracts(config);
  if (!connection) return null;
  const { rental, token } = connection;
  return async (minutes) => {
    const packageId = PACKAGES.get(minutes);
    if (!packageId) throw new Error("Unsupported minute pack");
    await verifyConnection(connection);
    const [wei, decimals, symbol] = await Promise.all([
      rental.quoteRent(packageId),
      token.decimals(),
      token.symbol(),
    ]);
    if (Number(decimals) !== TOKEN_DECIMALS || symbol !== "mUSDT") {
      throw new Error("Configured payment token is not Archava Mock USDT");
    }
    return { minutes, wei: wei.toString(), tokenAmount: formatUnits(wei, TOKEN_DECIMALS), symbol };
  };
}
