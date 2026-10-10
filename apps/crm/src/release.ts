import { join } from 'node:path'
import { verifyInstalledRelease, type ResolvedRelease } from '@archava/config/release'

// Embedded by the host build, outside tenant/private config; replacing a sidecar cannot change trust.
declare const ARCHAVA_RELEASE_PROFILE: ResolvedRelease
export async function verifyRuntimeRelease(
  directory = import.meta.dirname,
): Promise<ResolvedRelease> {
  if (typeof ARCHAVA_RELEASE_PROFILE === 'undefined')
    throw new Error('Missing supported release profile')
  return verifyInstalledRelease(
    ARCHAVA_RELEASE_PROFILE,
    ARCHAVA_RELEASE_PROFILE.catalog,
    join(directory, 'node_modules'),
  )
}
export { verifyReleaseDatabase, ReleaseProfileError } from '@archava/config/release'
