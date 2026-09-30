import { existsSync, readFileSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawnSync } from 'node:child_process'
import { encodeAbiParameters, isAddress, type Address } from 'viem'

const scriptDir = dirname(fileURLToPath(import.meta.url))
const rootDir = join(scriptDir, '..')
const deploymentPath = join(rootDir, 'deployments', 'bscTestnet.json')

interface DeploymentJson {
  readonly chainId: number
  readonly contractAddress: string
  readonly constructorArguments: {
    readonly initialOwner: string
    readonly initialDurations: readonly string[]
    readonly initialPrices: readonly string[]
  }
}

export function buildVerificationArgs(deployment: DeploymentJson): string[] {
  if (deployment.chainId !== 97) {
    throw new Error(`Verification requires BSC Testnet chain ID 97; got ${deployment.chainId}.`)
  }
  if (!isAddress(deployment.contractAddress)) {
    throw new Error(`Invalid deployed contract address: ${deployment.contractAddress}`)
  }
  if (!isAddress(deployment.constructorArguments.initialOwner)) {
    throw new Error(`Invalid constructor owner: ${deployment.constructorArguments.initialOwner}`)
  }

  const contractAddress: Address = deployment.contractAddress
  const initialOwner: Address = deployment.constructorArguments.initialOwner

  const constructorArgs = encodeAbiParameters(
    [
      { type: 'address', name: 'initialOwner' },
      { type: 'uint256[]', name: 'initialDurations' },
      { type: 'uint256[]', name: 'initialPrices' },
    ],
    [
      initialOwner,
      deployment.constructorArguments.initialDurations.map(BigInt),
      deployment.constructorArguments.initialPrices.map(BigInt),
    ],
  )

  return [
    'verify-contract',
    contractAddress,
    'src/ArchavaRental.sol:ArchavaRental',
    '--chain-id',
    '97',
    '--verifier',
    'etherscan',
    '--constructor-args',
    constructorArgs,
    '--etherscan-api-key',
    process.env['BSCSCAN_API_KEY'] ?? '',
    '--watch',
  ]
}

export function verifyContract(): Promise<void> {
  const apiKey = process.env['BSCSCAN_API_KEY']
  if (!apiKey || apiKey.trim() === '') {
    process.stdout.write(
      'BSCSCAN_API_KEY not provided. Skipping contract verification on BscScan.\n',
    )
    process.stdout.write(
      'To verify manually, run with BSCSCAN_API_KEY set or submit source code on https://testnet.bscscan.com.\n',
    )
    return Promise.resolve()
  }

  if (!existsSync(deploymentPath)) {
    return Promise.reject(
      new Error(
        `Deployment artifact not found at ${deploymentPath}. Deploy first before verifying.`,
      ),
    )
  }

  const deployment = JSON.parse(readFileSync(deploymentPath, 'utf8')) as DeploymentJson
  const contractAddress = deployment.contractAddress

  process.stdout.write(`Verifying contract ${contractAddress} on BSC Testnet (BscScan)...\n`)

  // forge verify-contract command
  const args = buildVerificationArgs(deployment)

  const result = spawnSync('forge', args, { cwd: rootDir, stdio: 'inherit' })
  if (result.status !== 0) {
    process.stderr.write(
      '\nNote: Verification did not complete successfully, but deployment is intact.\n',
    )
    process.stderr.write('You can verify the flattened or multi-part source on BscScan at:\n')
    process.stderr.write(`https://testnet.bscscan.com/verifyContract?a=${contractAddress}\n`)
    return Promise.reject(
      new Error(`BscScan verification failed for ${contractAddress}; deployment remains intact.`),
    )
  } else {
    process.stdout.write(
      `\nContract successfully verified on BscScan: https://testnet.bscscan.com/address/${contractAddress}#code\n`,
    )
  }

  return Promise.resolve()
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  verifyContract().catch((err: unknown) => {
    process.stderr.write(`${err instanceof Error ? err.message : String(err)}\n`)
    process.exit(1)
  })
}
