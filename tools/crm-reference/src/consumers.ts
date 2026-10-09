import { execFileSync } from 'node:child_process'
import { mkdtemp, cp, readFile, writeFile, rm, mkdir, readdir } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { resolve, join } from 'node:path'
import { createRequire } from 'node:module'
import { pathToFileURL } from 'node:url'
import { createHash } from 'node:crypto'
import type * as CrmModule from '@archava/crm'
import type * as PostgresModule from '@archava/crm-pg'
import type * as ConversationModule from '@archava/crm-assistant'
import type * as PipelineModule from '@archava/crm/pipeline'
import type * as GeminiModule from '@archava/crm-assistant/server'
import type * as PilotModule from '@archava/crm-assistant/pilot'
import type * as ProviderModule from '@archava/provider-runtime'
import type * as ProviderPgModule from '@archava/provider-runtime/postgres'

const root = resolve(import.meta.dirname, '../../..')
async function hash(directory: string): Promise<string> {
  const digest = createHash('sha256')
  const visit = async (directory: string, prefix: string): Promise<void> => {
    for (const entry of (await readdir(directory, { withFileTypes: true })).sort((a, b) =>
      a.name.localeCompare(b.name),
    )) {
      const file = join(directory, entry.name)
      const name = prefix + entry.name
      if (entry.isDirectory()) await visit(file, name + '/')
      else {
        digest.update(name)
        digest.update('\0')
        digest.update(await readFile(file))
        digest.update('\0')
      }
    }
  }
  await visit(directory, '')
  return digest.digest('hex')
}
export async function prepareConsumers() {
  execFileSync('pnpm', ['library:pack'], { cwd: root, stdio: 'pipe' })
  const artifacts = JSON.parse(
    await readFile(join(root, 'dist/library/artifacts.json'), 'utf8'),
  ) as { artifacts: { name: string; version: string; tarball: string; sha256: string }[] }
  const scratch = await mkdtemp(join(tmpdir(), 'archava-crm-consumers-'))
  const proofs: {
    consumer: string
    modules: {
      name: string
      version: string
      implementationSha256: string
      tarballSha256: string
    }[]
  }[] = []
  const conversations: (typeof ConversationModule)[] = []
  const pipelines: (typeof PipelineModule)[] = []
  const geminiModules: (typeof GeminiModule)[] = []
  const pilotModules: (typeof PilotModule)[] = []
  const providerModules: (typeof ProviderModule)[] = []
  const providerPgModules: (typeof ProviderPgModule)[] = []
  const crmModules: (typeof CrmModule)[] = []
  const postgresModules: (typeof PostgresModule)[] = []
  try {
    for (const name of ['hospitality', 'professional-services']) {
      const directory = join(scratch, name)
      await cp(join(root, 'examples/crm-consumers', name), directory, { recursive: true })
      const manifest = JSON.parse(await readFile(join(directory, 'package.json'), 'utf8')) as {
        dependencies: Record<string, string>
      }
      manifest.dependencies = Object.fromEntries(
        artifacts.artifacts.map((item) => [
          item.name,
          'file:' + join(root, 'dist/library', item.tarball),
        ]),
      )
      await writeFile(join(directory, 'package.json'), JSON.stringify(manifest, null, 2))
      for (const args of [
        ['install', '--ignore-scripts', '--no-audit', '--no-fund'],
        ['run', 'typecheck'],
        ['run', 'build'],
        ['run', 'verify'],
      ])
        execFileSync('npm', args, { cwd: directory, stdio: 'pipe' })
      const modules = []
      for (const name of [
        '@archava/crm',
        '@archava/crm-pg',
        '@archava/crm-assistant',
        '@archava/provider-runtime',
      ]) {
        const artifact = artifacts.artifacts.find((item) => item.name === name)
        if (!artifact) throw new Error('Missing CRM artifact')
        modules.push({
          name,
          version: artifact.version,
          implementationSha256: await hash(join(directory, 'node_modules', name)),
          tarballSha256: artifact.sha256,
        })
      }
      proofs.push({ consumer: name, modules })
      const consumerRequire = createRequire(join(directory, 'package.json'))
      const crmEntry = consumerRequire.resolve('@archava/crm')
      const pgEntry = consumerRequire.resolve('@archava/crm-pg')
      if (![crmEntry, pgEntry].every((entry) => entry.includes('/node_modules/')))
        throw new Error('CRM recovery resolved to workspace source')
      crmModules.push((await import(pathToFileURL(crmEntry).href)) as typeof CrmModule)
      postgresModules.push((await import(pathToFileURL(pgEntry).href)) as typeof PostgresModule)
      const conversationEntry = consumerRequire.resolve('@archava/crm-assistant')
      const pipelineEntry = consumerRequire.resolve('@archava/crm/pipeline')
      const geminiEntry = consumerRequire.resolve('@archava/crm-assistant/server')
      const pilotEntry = consumerRequire.resolve('@archava/crm-assistant/pilot')
      const providerEntry = consumerRequire.resolve('@archava/provider-runtime')
      const providerPgEntry = consumerRequire.resolve('@archava/provider-runtime/postgres')
      if (![providerEntry, providerPgEntry].every((entry) => entry.includes('/node_modules/')))
        throw new Error('Provider runtime resolved to workspace source')
      providerModules.push(
        (await import(pathToFileURL(providerEntry).href)) as typeof ProviderModule,
      )
      providerPgModules.push(
        (await import(pathToFileURL(providerPgEntry).href)) as typeof ProviderPgModule,
      )
      if (![geminiEntry, pilotEntry].every((entry) => entry.includes('/node_modules/')))
        throw new Error('Planner resolved to workspace source')
      geminiModules.push((await import(pathToFileURL(geminiEntry).href)) as typeof GeminiModule)
      pilotModules.push((await import(pathToFileURL(pilotEntry).href)) as typeof PilotModule)
      if (!pipelineEntry.includes('/node_modules/'))
        throw new Error('Pipeline resolved to workspace source')
      pipelines.push((await import(pathToFileURL(pipelineEntry).href)) as typeof PipelineModule)
      if (!conversationEntry.includes('/node_modules/'))
        throw new Error('Conversation resolved to workspace source')
      conversations.push(
        (await import(pathToFileURL(conversationEntry).href)) as typeof ConversationModule,
      )
      const target = join(root, 'dist/crm-demo', name)
      await mkdir(target, { recursive: true })
      await cp(join(directory, 'dist'), join(target, 'dist'), { recursive: true })
      await cp(join(directory, 'index.html'), join(target, 'index.html'))
    }
    if (JSON.stringify(proofs[0]?.modules) !== JSON.stringify(proofs[1]?.modules))
      throw new Error('CRM implementations differ across consumers')
    const require = createRequire(join(scratch, 'hospitality/package.json'))
    // Load the installed compiled public exports, not workspace TypeScript.
    const crmEntry = require.resolve('@archava/crm')
    const pgEntry = require.resolve('@archava/crm-pg')
    if (!crmEntry.includes('/node_modules/') || !pgEntry.includes('/node_modules/'))
      throw new Error('Backend resolved to workspace source')
    const crm = (await import(pathToFileURL(crmEntry).href)) as typeof CrmModule
    const postgres = (await import(pathToFileURL(pgEntry).href)) as typeof PostgresModule
    return {
      crm,
      postgres,
      conversations,
      pipelines,
      geminiModules,
      pilotModules,
      providerModules,
      providerPgModules,
      crmModules,
      postgresModules,
      proofs,
      publicDirectory: join(root, 'dist/crm-demo'),
      async close() {
        await rm(scratch, { recursive: true, force: true })
      },
    }
  } catch (error) {
    await rm(scratch, { recursive: true, force: true })
    throw error
  }
}
