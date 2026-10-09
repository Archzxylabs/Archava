import { afterEach, describe, it, expect, vi } from 'vitest'
import { startBackupLoop } from '../src/backups.js'

afterEach(() => {
  vi.useRealTimers()
})
describe('local backup worker lifecycle', () => {
  it('prevents overlapping polls and aborts/waits for in-flight work before shutdown', async () => {
    vi.useFakeTimers()
    let concurrent = 0,
      maximum = 0
    const run = vi.fn(
      (signal: AbortSignal) =>
        new Promise<void>((resolve) => {
          concurrent++
          maximum = Math.max(maximum, concurrent)
          signal.addEventListener(
            'abort',
            () => {
              concurrent--
              resolve()
            },
            { once: true },
          )
        }),
    )
    const loop = startBackupLoop(run, vi.fn(), 10)
    await vi.advanceTimersByTimeAsync(50)
    expect(run).toHaveBeenCalledTimes(1)
    expect(maximum).toBe(1)
    await loop.stop()
    await vi.advanceTimersByTimeAsync(100)
    expect(concurrent).toBe(0)
    expect(run).toHaveBeenCalledTimes(1)
  })
  it('does not begin work when stopped before the initial tick starts', async () => {
    vi.useFakeTimers()
    const run = vi.fn(() => Promise.resolve())
    const loop = startBackupLoop(run, vi.fn(), 10)
    await loop.stop()
    await vi.advanceTimersByTimeAsync(100)
    expect(run).not.toHaveBeenCalled()
  })
  it('isolates backup and diagnostic failures while later ticks can recover', async () => {
    vi.useFakeTimers()
    const run = vi
        .fn()
        .mockRejectedValueOnce(new Error('private failure'))
        .mockResolvedValue(undefined),
      onFailure = vi.fn(() => {
        throw new Error('diagnostic unavailable')
      })
    const loop = startBackupLoop(run, onFailure, 10)
    await loop.ready
    await vi.advanceTimersByTimeAsync(10)
    expect(run).toHaveBeenCalledTimes(2)
    expect(onFailure).toHaveBeenCalledTimes(1)
    await loop.stop()
  })
  it('reports no failure after an operator stops an active backup', async () => {
    vi.useFakeTimers()
    const onFailure = vi.fn(),
      run = (signal: AbortSignal) =>
        new Promise<void>((_, reject) =>
          signal.addEventListener('abort', () => reject(new Error('cancelled')), { once: true }),
        )
    const loop = startBackupLoop(run, onFailure, 10)
    await vi.advanceTimersByTimeAsync(1)
    await loop.stop()
    expect(onFailure).not.toHaveBeenCalled()
  })
})
