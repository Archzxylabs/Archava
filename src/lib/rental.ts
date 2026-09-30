import { BrowserProvider, Contract } from "ethers";
import { ApiError } from "./apiError.ts";

export interface AppConfig {
  chainId: number;
  contractAddress: string;
  paymentTokenAddress?: string;
  paymentTokenSymbol?: string;
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
  tokenAmount: string;
  symbol: string;
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

export const RENTAL_ABI = [
  "function quoteRent(uint8 packageId) view returns (uint256)",
  "function rent(uint8 packageId)",
];

export const PAYMENT_TOKEN_ABI = [
  "function approve(address spender, uint256 amount) returns (bool)",
  "function allowance(address owner, address spender) view returns (uint256)",
  "function balanceOf(address account) view returns (uint256)",
  "function faucet()",
  "function hasClaimed(address wallet) view returns (bool)",
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

function packageIdForMinutes(minutes: number): number {
  if (minutes === 60) return 1;
  if (minutes === 300) return 2;
  throw new Error("Unsupported minute pack");
}

export async function claimDemoUsdt(config: AppConfig, expectedWallet: string): Promise<string> {
  if (!config.paymentTokenAddress) throw new Error("Mock USDT contract not configured");
  const provider = injectedProvider();
  await ensureChain(config.chainId);
  const signer = await provider.getSigner();
  if ((await signer.getAddress()).toLowerCase() !== expectedWallet.toLowerCase()) {
    throw new Error("Connected wallet changed. Connect again before claiming tokens.");
  }
  const token = new Contract(config.paymentTokenAddress, PAYMENT_TOKEN_ABI, signer);
  const tx = await token.faucet();
  const receipt = await tx.wait();
  if (receipt?.status !== 1) throw new Error("Mock USDT faucet transaction failed");
  return tx.hash as string;
}

export async function buyMinutePack(config: AppConfig, minutes: number, expectedWallet: string): Promise<string> {
  if (!config.contractAddress || !config.paymentTokenAddress) throw new Error("Minute-pack contracts not configured");
  const packageId = packageIdForMinutes(minutes);
  if (!Array.isArray(config.packMinutes) || !config.packMinutes.includes(minutes)) {
    throw new Error("This site needs the minute-pack API. Refresh after the API is updated.");
  }
  const provider = injectedProvider();
  await ensureChain(config.chainId);
  const signer = await provider.getSigner();
  if ((await signer.getAddress()).toLowerCase() !== expectedWallet.toLowerCase()) {
    throw new Error("Connected wallet changed. Connect again before buying minutes.");
  }
  const token = new Contract(config.paymentTokenAddress, PAYMENT_TOKEN_ABI, signer);
  const rental = new Contract(config.contractAddress, RENTAL_ABI, signer);
  const value: bigint = await rental.quoteRent(packageId);
  const tokenBalance: bigint = await token.balanceOf(expectedWallet);
  if (tokenBalance < value) {
    const claimed: boolean = await token.hasClaimed(expectedWallet);
    throw new Error(claimed
      ? "Not enough demo mUSDT. This wallet has already used its one-time test faucet."
      : "Not enough demo mUSDT. Claim test tokens before buying a minute pack.");
  }
  const allowance: bigint = await token.allowance(expectedWallet, config.contractAddress);
  if (allowance < value) {
    const approval = await token.approve(config.contractAddress, value);
    const approvalReceipt = await approval.wait();
    if (approvalReceipt?.status !== 1) throw new Error("Mock USDT approval failed");
  }
  const tx = await rental.rent(packageId);
  const receipt = await tx.wait();
  if (receipt?.status !== 1) throw new Error("Minute-pack purchase failed");
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
