import { BrowserProvider, Contract } from "ethers";
import { ApiError } from "./apiError.ts";

export interface AppConfig {
  chainId: number;
  contractAddress: string;
  livekitReady: boolean;
  avatarProvider: "tavus" | "spatius";
  spatiusAppId?: string;
  spatiusAvatarId?: string;
  previewEnabled: boolean;
  previewSeconds: number;
  packMinutes: number[];
}

export interface CreditStatus {
  active: boolean;
  purchasedMinutes: number;
  usedSeconds: number;
  reservedSeconds: number;
  remainingSeconds: number;
}

export interface AvatarSession {
  serverUrl: string;
  token: string;
  endsAt: number;
  preview: boolean;
  ticket: string;
  avatarProvider: "tavus" | "spatius";
  spatiusAppId?: string;
  spatiusAvatarId?: string;
}

export interface PackQuote {
  wei: string;
  bnb: string;
  minutes: number;
}

declare global {
  interface Window {
    ethereum?: {
      request(args: { method: string; params?: unknown[] | object }): Promise<unknown>;
      on?(event: string, listener: (...args: unknown[]) => void): void;
      removeListener?(event: string, listener: (...args: unknown[]) => void): void;
    };
  }
}

export const PACK_ABI = [
  "function purchasedMinutes(address buyer) view returns (uint256)",
  "function quotePack(uint64 minutes) view returns (uint256)",
  "function buyPack(uint64 minutes) payable",
];

async function api<T>(path: string, body?: unknown): Promise<T> {
  const response = await fetch(path, body === undefined ? undefined : {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const data = (await response.json().catch(() => ({}))) as { error?: string };
  if (!response.ok) throw new ApiError(response.status, data.error || "Request failed");
  return data as T;
}

export async function startAnonymousPreview(): Promise<AvatarSession> {
  return api<AvatarSession>("/api/preview", {});
}

export async function getConfig(): Promise<AppConfig> {
  return api<AppConfig>("/api/config");
}

export async function getCreditStatus(wallet: string): Promise<CreditStatus> {
  return api<CreditStatus>("/api/access?wallet=" + encodeURIComponent(wallet));
}

export async function getPackQuote(minutes: number): Promise<PackQuote> {
  return api<PackQuote>("/api/quote?minutes=" + encodeURIComponent(minutes));
}

export function injectedProvider(): BrowserProvider {
  if (!window.ethereum) throw new Error("Install a wallet such as MetaMask or Rabby");
  return new BrowserProvider(window.ethereum);
}

export async function connectWallet(): Promise<string> {
  const provider = injectedProvider();
  await provider.send("eth_requestAccounts", []);
  return (await provider.getSigner()).getAddress();
}

export async function ensureChain(chainId: number): Promise<void> {
  const ethereum = window.ethereum;
  if (!ethereum) throw new Error("Wallet not found");
  const current = await ethereum.request({ method: "eth_chainId" });
  const expected = "0x" + chainId.toString(16);
  if (current === expected) return;
  try {
    await ethereum.request({
      method: "wallet_switchEthereumChain",
      params: [{ chainId: expected }],
    });
  } catch (error) {
    const code = (error as { code?: number }).code;
    if (code !== 4902 || chainId !== 97) throw error;
    await ethereum.request({
      method: "wallet_addEthereumChain",
      params: [{
        chainId: expected,
        chainName: "BNB Smart Chain Testnet",
        nativeCurrency: { name: "tBNB", symbol: "tBNB", decimals: 18 },
        rpcUrls: ["https://data-seed-prebsc-1-s1.bnbchain.org:8545"],
        blockExplorerUrls: ["https://testnet.bscscan.com"],
      }],
    });
  }
}

export async function buyMinutePack(config: AppConfig, minutes: number, expectedWallet: string): Promise<string> {
  if (!config.contractAddress) throw new Error("Minute-pack contract not configured");
  if (!Array.isArray(config.packMinutes) || !config.packMinutes.includes(minutes)) {
    throw new Error("This site needs the minute-pack API. Refresh after the API is updated.");
  }
  const provider = injectedProvider();
  await ensureChain(config.chainId);
  const signer = await provider.getSigner();
  if ((await signer.getAddress()).toLowerCase() !== expectedWallet.toLowerCase()) {
    throw new Error("Connected wallet changed. Connect again before buying minutes.");
  }
  const contract = new Contract(config.contractAddress, PACK_ABI, signer);
  const value: bigint = await contract.quotePack(minutes);
  const tx = await contract.buyPack(minutes, { value });
  await tx.wait();
  return tx.hash as string;
}

export async function startAvatarSession(wallet: string): Promise<AvatarSession> {
  const provider = injectedProvider();
  const signer = await provider.getSigner();
  if ((await signer.getAddress()).toLowerCase() !== wallet.toLowerCase()) {
    throw new Error("Connected wallet changed. Connect again.");
  }
  const challenge = await api<{ nonce: string; message: string }>("/api/challenge", { wallet });
  const signature = await signer.signMessage(challenge.message);
  return api<AvatarSession>("/api/session", { wallet, nonce: challenge.nonce, signature });
}

export async function endAvatarSession(ticket: string): Promise<void> {
  await api<{ ok: true }>("/api/session/end", { ticket });
}
