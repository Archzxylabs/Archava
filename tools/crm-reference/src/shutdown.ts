/** Install before setup; repeated interrupts must not kill database cleanup. */
export function referenceShutdown() {
  let requested = false
  let release = () => {}
  const stopped = new Promise<void>((done) => {
    release = done
  })
  const request = () => {
    requested = true
    release()
  }
  process.on('SIGINT', request)
  process.on('SIGTERM', request)
  return {
    get requested() {
      return requested
    },
    wait: () => stopped,
  }
}
