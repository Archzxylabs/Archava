import {
  createPublicClient,
  createWalletClient,
  formatEther,
  http,
  parseEther,
  type Abi,
  type Address,
  type Hex,
} from 'viem'
import { privateKeyToAccount } from 'viem/accounts'
import { bscTestnet } from 'viem/chains'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { assertSuccessfulReceipt, resolveDeploymentEnvironment } from './deploy.js'

const scriptDir = dirname(fileURLToPath(import.meta.url))
const rootDir = join(scriptDir, '..')

export const V2_PACKAGES = [
  {
    packageId: 1,
    name: 'Starter',
    displayPriceIdr: 79_000,
    priceMusdt: '4.9375',
    priceUnits: '4937500',
    includedMinutes: 60,
    validityDays: 30,
  },
  {
    packageId: 2,
    name: 'Pro',
    displayPriceIdr: 299_000,
    priceMusdt: '18.6875',
    priceUnits: '18687500',
    includedMinutes: 300,
    validityDays: 30,
  },
] as const

interface ForgeArtifact {
  readonly abi: readonly unknown[]
  readonly bytecode: { readonly object: Hex }
}

export interface V2DeploymentArtifact {
  readonly chainId: 97
  readonly network: 'BNB Smart Chain Testnet'
  readonly deployedAt: string
  readonly deployer: Address
  readonly mockUsdt: {
    readonly address: Address
    readonly deployTxHash: Hex
    readonly deployBlock: string
    readonly constructorArguments: readonly [Address]
    readonly abi: readonly unknown[]
  }
  readonly rentalV2: {
    readonly address: Address
    readonly deployTxHash: Hex
    readonly deployBlock: string
    readonly constructorArguments: readonly [Address, Address]
    readonly abi: readonly unknown[]
  }
  readonly packages: typeof V2_PACKAGES
}

export function buildV2DeploymentArtifact(input: {
  readonly owner: Address
  readonly tokenAddress: Address
  readonly tokenTxHash: Hex
  readonly tokenBlock: bigint
  readonly tokenAbi: readonly unknown[]
  readonly rentalAddress: Address
  readonly rentalTxHash: Hex
  readonly rentalBlock: bigint
  readonly rentalAbi: readonly unknown[]
  readonly deployedAt: string
}): V2DeploymentArtifact {
  return {
    chainId: 97,
    network: 'BNB Smart Chain Testnet',
    deployedAt: input.deployedAt,
    deployer: input.owner,
    mockUsdt: {
      address: input.tokenAddress,
      deployTxHash: input.tokenTxHash,
      deployBlock: input.tokenBlock.toString(),
      constructorArguments: [input.owner],
      abi: input.tokenAbi,
    },
    rentalV2: {
      address: input.rentalAddress,
      deployTxHash: input.rentalTxHash,
      deployBlock: input.rentalBlock.toString(),
      constructorArguments: [input.owner, input.tokenAddress],
      abi: input.rentalAbi,
    },
    packages: V2_PACKAGES,
  }
}

function loadArtifact(contractName: string): ForgeArtifact {
  const path = join(rootDir, 'out', `${contractName}.sol`, `${contractName}.json`)
  try {
    return JSON.parse(readFileSync(path, 'utf8')) as ForgeArtifact
  } catch (error) {
    throw new Error(`Unable to load ${contractName} artifact at ${path}; run forge build first.`, {
      cause: error,
    })
  }
}

export async function deployRentalV2(): Promise<V2DeploymentArtifact> {
  const { rpcUrl, privateKey } = resolveDeploymentEnvironment(process.env)
  const account = privateKeyToAccount(privateKey)
  const publicClient = createPublicClient({ chain: bscTestnet, transport: http(rpcUrl) })
  const walletClient = createWalletClient({ account, chain: bscTestnet, transport: http(rpcUrl) })

  const connectedChainId = await publicClient.getChainId()
  if (connectedChainId !== 97) {
    throw new Error(`Expected BSC Testnet chain ID 97, received ${connectedChainId}.`)
  }

  const balance = await publicClient.getBalance({ address: account.address })
  const minimumBalance = parseEther('0.004')
  process.stdout.write(`Deployer address: ${account.address}\n`)
  process.stdout.write(`Deployer balance: ${formatEther(balance)} tBNB\n`)
  if (balance < minimumBalance) {
    throw new Error(`At least ${formatEther(minimumBalance)} tBNB is required for two deployments.`)
  }

  const tokenArtifact = loadArtifact('ArchavaMockUSDT')
  const rentalArtifact = loadArtifact('ArchavaRentalV2')

  process.stdout.write('Deploying ArchavaMockUSDT...\n')
  const tokenTxHash = await walletClient.deployContract({
    abi: tokenArtifact.abi as Abi,
    bytecode: tokenArtifact.bytecode.object,
    args: [account.address],
  })
  const tokenReceipt = await publicClient.waitForTransactionReceipt({ hash: tokenTxHash })
  assertSuccessfulReceipt(tokenReceipt)
  const tokenAddress = tokenReceipt.contractAddress
  process.stdout.write(`ArchavaMockUSDT: ${tokenAddress}\n`)
  process.stdout.write(`Token tx: https://testnet.bscscan.com/tx/${tokenTxHash}\n`)

  process.stdout.write('Deploying ArchavaRentalV2...\n')
  const rentalTxHash = await walletClient.deployContract({
    abi: rentalArtifact.abi as Abi,
    bytecode: rentalArtifact.bytecode.object,
    args: [account.address, tokenAddress],
  })
  const rentalReceipt = await publicClient.waitForTransactionReceipt({ hash: rentalTxHash })
  assertSuccessfulReceipt(rentalReceipt)
  const rentalAddress = rentalReceipt.contractAddress

  const artifact = buildV2DeploymentArtifact({
    owner: account.address,
    tokenAddress,
    tokenTxHash,
    tokenBlock: tokenReceipt.blockNumber,
    tokenAbi: tokenArtifact.abi,
    rentalAddress,
    rentalTxHash,
    rentalBlock: rentalReceipt.blockNumber,
    rentalAbi: rentalArtifact.abi,
    deployedAt: new Date().toISOString(),
  })

  const deploymentsDir = join(rootDir, 'deployments')
  mkdirSync(deploymentsDir, { recursive: true })
  const outputPath = join(deploymentsDir, 'bscTestnetV2.json')
  writeFileSync(outputPath, JSON.stringify(artifact, null, 2), 'utf8')

  process.stdout.write(`ArchavaRentalV2: ${rentalAddress}\n`)
  process.stdout.write(`Rental tx: https://testnet.bscscan.com/tx/${rentalTxHash}\n`)
  process.stdout.write(`Saved deployment artifact to ${outputPath}\n`)
  return artifact
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  deployRentalV2().catch((error: unknown) => {
    process.stderr.write(
      `V2 deployment failed: ${error instanceof Error ? error.message : String(error)}\n`,
    )
    process.exit(1)
  })
}
