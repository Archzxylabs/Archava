import type { Pool, PoolClient } from 'pg'
import { z } from 'zod'
import { schedulingIdSchema, schedulingTimezoneSchema } from '@archava/scheduling'
import { schedulingMigrationLedger } from './migrate.js'

const resourceInput = z
  .object({
    tenant: schedulingIdSchema,
    id: z.uuid(),
    title: z.string().trim().min(1).max(160),
    timezone: schedulingTimezoneSchema,
  })
  .strict()
const ref = z.object({ tenant: schedulingIdSchema, resourceId: z.uuid() }).strict()
const allocation = z
  .object({
    tenant: schedulingIdSchema,
    resourceId: z.uuid(),
    id: z.uuid(),
    startsAt: z.iso.datetime(),
    endsAt: z.iso.datetime(),
    expiresAt: z.iso.datetime(),
  })
  .strict()
async function tenantContext(client: PoolClient, tenant: string) {
  if (
    (
      await client.query<{ tenant: string }>(
        "SELECT current_setting('archava.tenant_id',true) AS tenant",
      )
    ).rows[0]?.tenant !== tenant
  )
    throw new Error('Missing occupancy transaction tenant')
}
/** Startup support check, not an authorization token. Callers must use their own current domain grants. */
export async function assertOccupancyKernel(pool: Pool) {
  const role = await pool.query<{ unsafe: boolean }>(
    "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class WHERE relnamespace='archava_scheduling'::regnamespace AND relowner=pg_roles.oid)) AS unsafe FROM pg_roles WHERE rolname=current_user",
  )
  if (role.rows[0]?.unsafe !== false) throw new Error('Occupancy requires non-owner runtime')
  const expected = await schedulingMigrationLedger()
  const actual = (
    await pool.query<{ id: string; checksum: string }>(
      'SELECT id,checksum FROM archava_scheduling.schema_migrations ORDER BY id',
    )
  ).rows
  if (
    actual.length !== expected.length ||
    actual.some((x, i) => x.id !== expected[i]?.id || x.checksum !== expected[i]?.checksum)
  )
    throw new Error('Unsupported occupancy kernel ledger')
  const policies = await pool.query<{ count: number }>(
    "SELECT count(*)::integer AS count FROM pg_class WHERE relnamespace='archava_scheduling'::regnamespace AND relname=ANY($1::text[]) AND relrowsecurity AND relforcerowsecurity",
    [['memberships', 'resources', 'reservations', 'commands', 'audit', 'occupancy']],
  )
  if (policies.rows[0]?.count !== 6) throw new Error('Occupancy tenant policies must be forced')
}
export async function grantOccupancyRuntime(pool: Pool, role: string) {
  if (!/^[a-z][a-z0-9_]{0,62}$/.test(role)) throw new Error('Invalid occupancy runtime role')
  await pool.query(`GRANT USAGE ON SCHEMA archava_scheduling TO "${role}"`)
  await pool.query(
    `GRANT SELECT ON archava_scheduling.schema_migrations,archava_scheduling.resources,archava_scheduling.occupancy TO "${role}"`,
  )
  await pool.query(`GRANT UPDATE ON archava_scheduling.resources TO "${role}"`)
  await pool.query(`GRANT INSERT,UPDATE ON archava_scheduling.occupancy TO "${role}"`)
  await pool.query(
    `GRANT EXECUTE ON FUNCTION archava_scheduling.create_accommodation_resource(text,uuid,text,text) TO "${role}"`,
  )
}
/** Managed rooms remain inaccessible to timed scheduling APIs; only the accommodation adapter owns them. */
export async function createAccommodationResource(
  client: PoolClient,
  raw: z.infer<typeof resourceInput>,
) {
  const x = resourceInput.parse(raw)
  await tenantContext(client, x.tenant)
  await client.query('SELECT archava_scheduling.create_accommodation_resource($1,$2,$3,$4)', [
    x.tenant,
    x.id,
    x.title,
    x.timezone,
  ])
}
export async function lockAccommodationResource(
  client: PoolClient,
  tenant: string,
  resourceId: string,
) {
  ref.parse({ tenant, resourceId })
  await tenantContext(client, tenant)
  const row = (
    await client.query<{ id: string; active: boolean }>(
      "SELECT id,active FROM archava_scheduling.resources WHERE tenant_id=$1 AND id=$2 AND managed_by='accommodation' FOR UPDATE",
      [tenant, resourceId],
    )
  ).rows[0]
  if (!row) throw new Error('Missing accommodation resource')
  return row
}
export async function accommodationOccupancyAvailable(
  client: PoolClient,
  tenant: string,
  resourceId: string,
  startsAt: string,
  endsAt: string,
  observed: string,
) {
  ref.parse({ tenant, resourceId })
  z.iso.datetime().parse(startsAt)
  z.iso.datetime().parse(endsAt)
  z.iso.datetime().parse(observed)
  await tenantContext(client, tenant)
  return (
    (
      await client.query<{ available: boolean }>(
        "SELECT NOT EXISTS(SELECT 1 FROM archava_scheduling.occupancy WHERE tenant_id=$1 AND resource_id=$2 AND (state='confirmed' OR (state='held' AND expires_at>$5)) AND starts_at<$4 AND ends_at>$3) AS available",
        [tenant, resourceId, startsAt, endsAt, observed],
      )
    ).rows[0]?.available === true
  )
}
export async function holdAccommodationOccupancy(
  client: PoolClient,
  raw: z.infer<typeof allocation>,
) {
  const x = allocation.parse(raw)
  await tenantContext(client, x.tenant)
  await client.query(
    "INSERT INTO archava_scheduling.occupancy(tenant_id,id,resource_id,owner_module,state,starts_at,ends_at,expires_at) VALUES($1,$2,$3,'accommodation','held',$4,$5,$6)",
    [x.tenant, x.id, x.resourceId, x.startsAt, x.endsAt, x.expiresAt],
  )
}
export async function transitionAccommodationOccupancy(
  client: PoolClient,
  tenant: string,
  resourceId: string,
  id: string,
  state: 'confirmed' | 'cancelled',
) {
  ref.parse({ tenant, resourceId })
  z.uuid().parse(id)
  z.enum(['confirmed', 'cancelled']).parse(state)
  await tenantContext(client, tenant)
  const result = await client.query(
    "UPDATE archava_scheduling.occupancy SET state=$4,updated_at=clock_timestamp() WHERE tenant_id=$1 AND resource_id=$2 AND id=$3 AND owner_module='accommodation'",
    [tenant, resourceId, id, state],
  )
  if (result.rowCount !== 1) throw new Error('Missing accommodation occupancy')
}
export async function setAccommodationResourceActive(
  client: PoolClient,
  tenant: string,
  resourceId: string,
  active: boolean,
) {
  ref.parse({ tenant, resourceId })
  z.boolean().parse(active)
  await tenantContext(client, tenant)
  const result = await client.query(
    "UPDATE archava_scheduling.resources SET active=$3,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2 AND managed_by='accommodation'",
    [tenant, resourceId, active],
  )
  if (result.rowCount !== 1) throw new Error('Missing accommodation resource')
}
