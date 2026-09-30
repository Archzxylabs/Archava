import { describe, expect, it } from 'vitest'
import { buildV2DeploymentArtifact, V2_PACKAGES } from '../scripts/deploy-v2.js'

describe('V2 deployment artifact', () => {
  it('records both contracts and exact RentalV2 constructor arguments', () => {
    const owner = '0x1111111111111111111111111111111111111111' as const
    const token = '0x2222222222222222222222222222222222222222' as const
    const rental = '0x3333333333333333333333333333333333333333' as const
    const tokenTx = `0x${'44'.repeat(32)}` as const
    const rentalTx = `0x${'55'.repeat(32)}` as const

    const artifact = buildV2DeploymentArtifact({
      owner,
      tokenAddress: token,
      tokenTxHash: tokenTx,
      tokenBlock: 10n,
      tokenAbi: [],
      rentalAddress: rental,
      rentalTxHash: rentalTx,
      rentalBlock: 11n,
      rentalAbi: [],
      deployedAt: '2026-09-30T00:00:00.000Z',
    })

    expect(artifact.chainId).toBe(97)
    expect(artifact.mockUsdt.address).toBe(token)
    expect(artifact.mockUsdt.constructorArguments).toEqual([owner])
    expect(artifact.rentalV2.address).toBe(rental)
    expect(artifact.rentalV2.constructorArguments).toEqual([owner, token])
    expect(artifact.packages).toEqual(V2_PACKAGES)
  })

  it('uses six-decimal mUSDT amounts for the approved IDR packages', () => {
    expect(V2_PACKAGES).toEqual([
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
    ])
  })
})
