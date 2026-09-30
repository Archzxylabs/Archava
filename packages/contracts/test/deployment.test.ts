import { describe, expect, it } from 'vitest'
import {
  assertSuccessfulReceipt,
  buildDeploymentArtifact,
  resolveDeploymentEnvironment,
  DEMO_PACKAGES,
} from '../scripts/deploy.js'
import { buildVerificationArgs } from '../scripts/verify.js'

const deployer = '0x1111111111111111111111111111111111111111' as const
const contractAddress = '0x2222222222222222222222222222222222222222' as const
const deployTxHash = `0x${'33'.repeat(32)}` as const

describe('deployment readiness', () => {
  it('requires an explicit BSC Testnet RPC URL', () => {
    expect(() =>
      resolveDeploymentEnvironment({ BSC_DEPLOYER_PRIVATE_KEY: `0x${'44'.repeat(32)}` }),
    ).toThrow(/BSC_TESTNET_RPC_URL/)
  })

  it('requires an explicit BSC deployer private key', () => {
    expect(() =>
      resolveDeploymentEnvironment({
        BSC_TESTNET_RPC_URL: 'https://data-seed-prebsc-1-s1.binance.org:8545/',
      }),
    ).toThrow(/BSC_DEPLOYER_PRIVATE_KEY/)
  })

  it('rejects a reverted deployment receipt', () => {
    expect(() => assertSuccessfulReceipt({ status: 'reverted', contractAddress: null })).toThrow(
      /reverted/i,
    )
  })

  it('rejects a successful receipt missing a contract address', () => {
    expect(() => assertSuccessfulReceipt({ status: 'success', contractAddress: null })).toThrow(
      /contract address/i,
    )
  })

  it('stores exact constructor arguments in the deployment artifact', () => {
    const artifact = buildDeploymentArtifact({
      contractAddress,
      deployTxHash,
      deployBlock: 123n,
      deployer,
      abi: [],
      deployedAt: '2026-09-30T00:00:00.000Z',
    })

    expect(artifact.constructorArguments).toEqual({
      initialOwner: deployer,
      initialDurations: DEMO_PACKAGES.map((item) => item.duration.toString()),
      initialPrices: DEMO_PACKAGES.map((item) => item.priceWei.toString()),
    })
  })

  it('builds BscScan verification arguments with encoded constructor arguments', () => {
    const args = buildVerificationArgs({
      chainId: 97,
      contractAddress,
      constructorArguments: {
        initialOwner: deployer,
        initialDurations: ['3600'],
        initialPrices: ['500000000000000'],
      },
    })

    expect(args).toContain('--chain-id')
    expect(args).toContain('97')
    expect(args).toContain('--verifier')
    expect(args).toContain('etherscan')
    expect(args).toContain('--constructor-args')
    expect(args.at(args.indexOf('--constructor-args') + 1)).toMatch(/^0x[0-9a-f]+$/)
  })

  it('rejects verification artifacts for a different chain', () => {
    expect(() =>
      buildVerificationArgs({
        chainId: 56,
        contractAddress,
        constructorArguments: {
          initialOwner: deployer,
          initialDurations: ['3600'],
          initialPrices: ['1'],
        },
      }),
    ).toThrow(/chain ID 97/i)
  })
})
