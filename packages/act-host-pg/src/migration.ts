import { readFile } from 'node:fs/promises'
import { createHash } from 'node:crypto'
import type { SqlClient } from '@archava/act-storage'

export async function actHostMigration() {
  const template = await readFile(
    new URL('../migrations/001_host_intents.sql', import.meta.url),
    'utf8',
  )
  const checksum = createHash('sha256').update(template).digest('hex')
  return {
    id: '001_host_intents',
    checksum,
    sql: template.replaceAll('__ARCHAVA_HOST_CHECKSUM__', checksum),
  }
}
/** One DO statement owns its own transaction and serialization lock. Operator connection only. */
export async function migrateActHost(client: SqlClient) {
  const migration = await actHostMigration()
  await client.run({ sql: migration.sql, params: [] })
}
export async function grantActHostRuntime(client: SqlClient, role: string) {
  if (!/^[a-z][a-z0-9_]{0,62}$/.test(role)) throw new Error('Invalid runtime role')
  // Role is an operator-owned SQL identifier, validated before interpolation.
  await client.run({
    sql: `GRANT USAGE ON SCHEMA archava_act_host TO "${role}";
GRANT SELECT ON archava_act_host.schema_migrations TO "${role}";
GRANT SELECT,INSERT,UPDATE ON archava_act_host.heads,archava_act_host.intents TO "${role}";
GRANT SELECT,INSERT ON archava_act_host.challenges TO "${role}";
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA archava_act_host TO "${role}";`,
    params: [],
  })
}
