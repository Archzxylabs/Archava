import { idSchema } from '@archava/crm/contracts'

/** Only an opaque request id belongs here. Draft content stays transient. */
export interface FollowupRecoveryStore {
  load(): string | null
  remember(requestId: string): void
  forget(requestId: string): void
}

/** Host opts in with tab-scoped storage and a verified account/workspace namespace. */
export function createTabFollowupRecovery(
  storage: Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>,
  scope: string,
): FollowupRecoveryStore {
  if (!scope || scope.length > 1000) throw new Error('Recovery scope is required')
  const key = 'archava.crm.followup-recovery.v1:' + scope
  const load = () => {
    const value = storage.getItem(key)
    return value === null ? null : idSchema.parse(value)
  }
  return {
    load,
    remember(requestId) {
      const id = idSchema.parse(requestId)
      const pending = load()
      if (pending !== null && pending !== id) throw new Error('An earlier Save is unresolved')
      storage.setItem(key, id)
      if (load() !== id) throw new Error('Recovery marker could not be verified')
    },
    forget(requestId) {
      const id = idSchema.parse(requestId)
      if (load() === id) storage.removeItem(key)
      if (load() === id) throw new Error('Recovery marker could not be cleared')
    },
  }
}
