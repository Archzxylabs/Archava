import { execFileSync } from 'node:child_process'
import { mkdtempSync, mkdirSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { Pool } from 'pg'
import { randomBytes } from 'node:crypto'
import type * as PostgresModule from '@archava/crm-pg'
import type { CrmPermission } from '@archava/crm'

/** Always a private disposable cluster. Never reads a user's database URL. */
export async function createCluster(
  postgres: typeof PostgresModule,
  permissions: readonly CrmPermission[],
) {
  const directory = mkdtempSync(join(tmpdir(), 'archava-crm-'))
  const data = join(directory, 'data')
  const socket = join(directory, 'socket')
  mkdirSync(socket)
  const bin = execFileSync('pg_config', ['--bindir'], { encoding: 'utf8' }).trim()
  const port = 20000 + Math.floor(Math.random() * 25000)
  const run = (name: string, args: string[]) =>
    execFileSync(join(bin, name), args, { encoding: 'utf8', stdio: 'pipe' })
  let running = false
  const stop = () => {
    if (running) {
      run('pg_ctl', ['-D', data, '-m', 'fast', '-w', 'stop'])
      running = false
    }
  }
  const start = () => {
    run('pg_ctl', [
      '-D',
      data,
      '-l',
      join(directory, 'postgres.log'),
      '-w',
      'start',
      '-o',
      `-k ${socket} -p ${port} -c listen_addresses=''`,
    ])
    running = true
  }
  const target = { host: socket, port, database: 'postgres', user: 'postgres' }
  let owner = new Pool(target)
  try {
    run('initdb', ['-D', data, '-U', 'postgres', '--auth=trust', '--no-sync'])
    start()
    await postgres.migrateCrm(owner)
    await owner.query('CREATE ROLE crm_app LOGIN NOSUPERUSER NOBYPASSRLS')
    await postgres.grantCrmRuntime(owner, 'crm_app')
    for (const [tenant, principal, kind, grants, scope] of [
      ['hotel-demo', 'hotel-owner', 'human', [...permissions], 'all'],
      [
        'hotel-demo',
        'hotel-viewer',
        'human',
        ['contacts.read', 'activities.read', 'followups.read'],
        'all',
      ],
      ['hotel-demo', 'hotel-editor', 'human', [...permissions], 'assigned'],
      ['hotel-demo', 'hotel-bot', 'service', ['contacts.read'], 'all'],
      ['studio-demo', 'studio-owner', 'human', [...permissions], 'all'],
    ] as const) {
      await owner.query(
        'INSERT INTO archava_crm.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES($1,$2,$3,$4,$5)',
        [tenant, principal, kind, grants, scope],
      )
    }
    return {
      get owner() {
        return owner
      },
      target,
      application: { ...target, user: 'crm_app' },
      fingerprintKey: randomBytes(32).toString('hex'),
      async restart() {
        await owner.end()
        stop()
        start()
        owner = new Pool(target)
      },
      async close() {
        if (!owner.ended) await owner.end()
        stop()
        rmSync(directory, { recursive: true, force: true })
      },
    }
  } catch (error) {
    await owner.end()
    stop()
    rmSync(directory, { recursive: true, force: true })
    throw error
  }
}
