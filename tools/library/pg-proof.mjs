// pg-pool's end() may resolve before its clients finish closing. A restart proof
// must also await the public remove events before stopping its isolated cluster.
export async function endProofPool(pool) {
  let remaining = pool.totalCount
  if (remaining === 0) return pool.end()
  const removed = new Promise((resolve) => {
    const onRemove = () => {
      remaining -= 1
      if (remaining === 0) {
        pool.off('remove', onRemove)
        resolve()
      }
    }
    pool.on('remove', onRemove)
  })
  await pool.end()
  await removed
}
