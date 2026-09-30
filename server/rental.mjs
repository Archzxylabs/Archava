import { Contract, JsonRpcProvider, formatEther, getAddress, isAddress } from "ethers";

// Contract handoff: purchases are cumulative. Usage is measured offchain in
// seconds, so a browser timer never determines a wallet's balance.
export const PACK_ABI = [
  "function purchasedMinutes(address buyer) view returns (uint256)",
  "function quotePack(uint64 minutes) view returns (uint256)",
  "function buyPack(uint64 minutes) payable",
];

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

function configuredContract({ rpcUrl, contractAddress, chainId }) {
  if (!contractAddress) return null;
  if (!isAddress(contractAddress)) throw new Error("PACK_CONTRACT is invalid");
  if (!rpcUrl) throw new Error("RPC_URL is required when PACK_CONTRACT is set");
  const provider = new JsonRpcProvider(rpcUrl, Number(chainId));
  return { provider, contract: new Contract(contractAddress, PACK_ABI, provider) };
}

async function verifyChain(provider, chainId) {
  if ((await provider.getNetwork()).chainId !== BigInt(chainId)) throw new Error("RPC chain mismatch");
}

export function createCreditReader(config) {
  const connection = configuredContract(config);
  if (!connection) return null;
  const { provider, contract } = connection;
  return async (wallet) => {
    await verifyChain(provider, config.chainId);
    return purchasedState(await contract.purchasedMinutes(normalizeWallet(wallet)));
  };
}

export function createPackQuoter(config) {
  const connection = configuredContract(config);
  if (!connection) return null;
  const { provider, contract } = connection;
  return async (minutes) => {
    if (!Number.isSafeInteger(minutes) || minutes <= 0) throw new Error("Invalid pack minutes");
    await verifyChain(provider, config.chainId);
    const wei = await contract.quotePack(minutes);
    return { minutes, wei: wei.toString(), bnb: formatEther(wei) };
  };
}
