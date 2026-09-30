import {
  createPublicClient,
  createWalletClient,
  http,
  parseEther,
  formatEther,
  type Hex,
  type Address,
  type Abi,
} from 'viem'
import { privateKeyToAccount } from 'viem/accounts'
import { bscTestnet } from 'viem/chains'
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const scriptDir = dirname(fileURLToPath(import.meta.url))
const rootDir = join(scriptDir, '..')

// Contract Artifact Path
const artifactPath = join(rootDir, 'out', 'ArchavaRental.sol', 'ArchavaRental.json')

export interface DeploymentPackageConfig {
  readonly duration: bigint
  readonly durationLabel: string
  readonly priceWei: bigint
  readonly priceBnb: string
}

export const DEMO_PACKAGES: readonly DeploymentPackageConfig[] = [
  {
    duration: 3600n, // 1 hour
    durationLabel: '1 Hour (Demo)',
    priceWei: parseEther('0.0005'),
    priceBnb: '0.0005',
  },
  {
    duration: 86400n, // 1 day
    durationLabel: '1 Day (Standard)',
    priceWei: parseEther('0.002'),
    priceBnb: '0.002',
  },
  {
    duration: 604800n, // 7 days
    durationLabel: '7 Days (Extended)',
    priceWei: parseEther('0.01'),
    priceBnb: '0.01',
  },
]

export interface DeploymentArtifact {
  readonly chainId: number
  readonly network: string
  readonly contractAddress: Address
  readonly deployTxHash: Hex
  readonly deployBlock: string
  readonly deployedAt: string
  readonly deployer: Address
  readonly constructorArguments: {
    readonly initialOwner: Address
    readonly initialDurations: readonly string[]
    readonly initialPrices: readonly string[]
  }
  readonly packages: readonly {
    readonly duration: number
    readonly durationLabel: string
    readonly priceWei: string
    readonly priceBnb: string
  }[]
  readonly abi: readonly unknown[]
}

export interface DeploymentEnvironment {
  readonly rpcUrl: string
  readonly privateKey: Hex
}

export function resolveDeploymentEnvironment(
  environment: NodeJS.ProcessEnv,
): DeploymentEnvironment {
  const rpcUrl = environment['BSC_TESTNET_RPC_URL']?.trim()
  if (!rpcUrl) {
    throw new Error('Missing required environment variable: BSC_TESTNET_RPC_URL.')
  }

  const rawKey = environment['BSC_DEPLOYER_PRIVATE_KEY']?.trim()
  if (!rawKey) {
    throw new Error('Missing required environment variable: BSC_DEPLOYER_PRIVATE_KEY.')
  }

  return {
    rpcUrl,
    privateKey: (rawKey.startsWith('0x') ? rawKey : `0x${rawKey}`) as Hex,
  }
}

export function assertSuccessfulReceipt(receipt: {
  readonly status: 'success' | 'reverted'
  readonly contractAddress?: Address | null
}): asserts receipt is { readonly status: 'success'; readonly contractAddress: Address } {
  if (receipt.status !== 'success') {
    throw new Error('Deployment transaction reverted; no deployment artifact was written.')
  }
  if (!receipt.contractAddress) {
    throw new Error('Successful deployment receipt did not contain a contract address.')
  }
}

export function buildDeploymentArtifact(input: {
  readonly contractAddress: Address
  readonly deployTxHash: Hex
  readonly deployBlock: bigint
  readonly deployer: Address
  readonly abi: readonly unknown[]
  readonly deployedAt: string
}): DeploymentArtifact {
  return {
    chainId: 97,
    network: 'BNB Smart Chain Testnet',
    contractAddress: input.contractAddress,
    deployTxHash: input.deployTxHash,
    deployBlock: input.deployBlock.toString(),
    deployedAt: input.deployedAt,
    deployer: input.deployer,
    constructorArguments: {
      initialOwner: input.deployer,
      initialDurations: DEMO_PACKAGES.map((item) => item.duration.toString()),
      initialPrices: DEMO_PACKAGES.map((item) => item.priceWei.toString()),
    },
    packages: DEMO_PACKAGES.map((item) => ({
      duration: Number(item.duration),
      durationLabel: item.durationLabel,
      priceWei: item.priceWei.toString(),
      priceBnb: item.priceBnb,
    })),
    abi: input.abi,
  }
}

interface ForgeContractArtifact {
  readonly abi: readonly unknown[]
  readonly bytecode: {
    readonly object: Hex
  }
}

export async function deployRental(): Promise<DeploymentArtifact> {
  const { rpcUrl, privateKey } = resolveDeploymentEnvironment(process.env)
  const account = privateKeyToAccount(privateKey)

  const publicClient = createPublicClient({
    chain: bscTestnet,
    transport: http(rpcUrl),
  })

  // 1. Validate connected network chainId is exactly 97
  const connectedChainId = await publicClient.getChainId()
  if (connectedChainId !== 97) {
    throw new Error(
      `Safety check failed: Expected chainId 97 (BSC Testnet), but connected to chainId ${connectedChainId}.`,
    )
  }

  // 2. Validate deployer balance
  const balance = await publicClient.getBalance({ address: account.address })
  const minRequiredBalance = parseEther('0.002') // estimated gas safety cushion

  process.stdout.write(`Deployer address: ${account.address}\n`)
  process.stdout.write(`Connected chain ID: ${connectedChainId} (BSC Testnet)\n`)
  process.stdout.write(`Deployer balance: ${formatEther(balance)} tBNB\n`)

  if (balance < minRequiredBalance) {
    throw new Error(
      `Insufficient funds for deployment. Deployer has ${formatEther(balance)} tBNB, ` +
        `but needs at least ${formatEther(minRequiredBalance)} tBNB for gas.\n` +
        'Acquire tBNB from the official faucet: https://testnet.bnbchain.org/faucet-smart',
    )
  }

  // 3. Load compiled contract artifact
  let artifactRaw: string
  try {
    artifactRaw = readFileSync(artifactPath, 'utf8')
  } catch (err) {
    throw new Error(
      `Failed to read artifact at ${artifactPath}. Run 'forge build' first. ${String(err)}`,
    )
  }

  const artifact = JSON.parse(artifactRaw) as ForgeContractArtifact
  const abi = artifact.abi as Abi
  const bytecode = artifact.bytecode.object

  const walletClient = createWalletClient({
    account,
    chain: bscTestnet,
    transport: http(rpcUrl),
  })

  const durations = DEMO_PACKAGES.map((p) => p.duration)
  const prices = DEMO_PACKAGES.map((p) => p.priceWei)

  process.stdout.write('Sending deployment transaction to BSC Testnet...\n')

  const deployTxHash = await walletClient.deployContract({
    abi,
    bytecode,
    args: [account.address, durations, prices],
  })

  process.stdout.write(`Deploy transaction submitted: ${deployTxHash}\n`)
  process.stdout.write(`Waiting for confirmation on BSC Testnet...\n`)

  const receipt = await publicClient.waitForTransactionReceipt({
    hash: deployTxHash,
  })

  assertSuccessfulReceipt(receipt)

  const contractAddress = receipt.contractAddress
  process.stdout.write(`\n========================================\n`)
  process.stdout.write(`ArchavaRental successfully deployed!\n`)
  process.stdout.write(`Contract Address: ${contractAddress}\n`)
  process.stdout.write(`Block Number:     ${receipt.blockNumber}\n`)
  process.stdout.write(`Explorer Link:    https://testnet.bscscan.com/address/${contractAddress}\n`)
  process.stdout.write(`Tx Link:          https://testnet.bscscan.com/tx/${deployTxHash}\n`)
  process.stdout.write(`========================================\n\n`)

  const deploymentArtifact = buildDeploymentArtifact({
    contractAddress,
    deployTxHash,
    deployBlock: receipt.blockNumber,
    deployedAt: new Date().toISOString(),
    deployer: account.address,
    abi: artifact.abi,
  })

  // Save to deployments directory
  const deploymentsDir = join(rootDir, 'deployments')
  mkdirSync(deploymentsDir, { recursive: true })
  const outputPath = join(deploymentsDir, 'bscTestnet.json')
  writeFileSync(outputPath, JSON.stringify(deploymentArtifact, null, 2), 'utf8')
  process.stdout.write(`Saved deployment artifact to ${outputPath}\n`)

  return deploymentArtifact
}

// Only execute when run directly
if (process.argv[1] === fileURLToPath(import.meta.url)) {
  deployRental().catch((err: unknown) => {
    process.stderr.write(
      `\nDeployment failed:\n${err instanceof Error ? err.message : String(err)}\n`,
    )
    process.exit(1)
  })
}
