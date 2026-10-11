/* global process */
import { execFileSync } from 'node:child_process'
import { readFile, writeFile, mkdir, cp, rm } from 'node:fs/promises'
import { resolve, join } from 'node:path'
import { build } from 'esbuild'
import { createRequire } from 'node:module'
import { pathToFileURL } from 'node:url'

const root = resolve(import.meta.dirname, '../../..')
const output = join(root, 'dist/crm-runtime')
try {
  const pid = Number(
    await readFile(
      join(resolve(process.env.ARCHAVA_CRM_HOME ?? join(root, '.local/crm')), 'runtime.lock'),
      'utf8',
    ),
  )
  if (!Number.isSafeInteger(pid) || pid <= 0)
    throw new Error('Invalid runtime lock; inspect before rebuilding')
  let live = true
  try {
    process.kill(pid, 0)
  } catch (error) {
    if (error.code !== 'ESRCH') throw error
    live = false
  }
  if (live) throw new Error('Stop the CRM host before replacing its installed runtime')
} catch (error) {
  if (error.code !== 'ENOENT') throw error
}
execFileSync('pnpm', ['library:pack'], { cwd: root, stdio: 'pipe' })
await mkdir(output, { recursive: true })
const { artifacts } = JSON.parse(await readFile(join(root, 'dist/library/artifacts.json'), 'utf8'))
const profile = JSON.parse(await readFile(join(root, 'dist/library/release-profile.json'), 'utf8'))
// Development artifacts can change before a release version is assigned.
// Remove the generated install so its local-file lock cannot retain older code.
await rm(join(output, 'node_modules'), { recursive: true, force: true })
await rm(join(output, 'package-lock.json'), { force: true })
await writeFile(
  join(output, 'library-artifacts.json'),
  JSON.stringify({ artifacts }, null, 2) + '\n',
)
await writeFile(
  join(output, 'package.json'),
  JSON.stringify(
    {
      name: 'archava-crm-runtime',
      version: profile.catalog.host.version,
      private: true,
      type: 'module',
      overrides: Object.fromEntries(
        (profile.catalog.externals ?? []).map((item) => [item.name, item.version]),
      ),
      dependencies: Object.fromEntries(
        artifacts.map((item) => [item.name, 'file:' + join(root, 'dist/library', item.tarball)]),
      ),
    },
    null,
    2,
  ) + '\n',
)
execFileSync('npm', ['install', '--ignore-scripts', '--no-audit', '--no-fund'], {
  cwd: output,
  stdio: 'pipe',
})
const installed = createRequire(join(output, 'package.json'))
const release = await import(pathToFileURL(installed.resolve('@archava/config/release')).href)
await release.verifyInstalledRelease(profile, profile.catalog, join(output, 'node_modules'))
await writeFile(join(output, 'release-profile.json'), JSON.stringify(profile, null, 2) + '\n')
const server = await build({
  entryPoints: [join(root, 'apps/crm/src/server.ts'), join(root, 'apps/crm/src/operator.ts')],
  outdir: output,
  outExtension: { '.js': '.mjs' },
  bundle: true,
  packages: 'external',
  define: { ARCHAVA_RELEASE_PROFILE: JSON.stringify(profile) },
  // Workspace TypeScript paths must not replace the installed public artifacts.
  tsconfigRaw: { compilerOptions: {} },
  platform: 'node',
  format: 'esm',
  target: 'es2022',
  logLevel: 'silent',
  metafile: true,
})
if (Object.keys(server.metafile.inputs).some((path) => path.includes('packages/')))
  throw new Error('CRM host bundled workspace library source')
await writeFile(join(output, 'server-meta.json'), JSON.stringify(server.metafile, null, 2) + '\n')
await cp(join(root, 'apps/crm/public'), join(output, 'public'), { recursive: true })
const browser = await build({
  entryPoints: [join(root, 'apps/crm/src/browser.ts')],
  outfile: join(output, 'public/app.js'),
  bundle: true,
  platform: 'browser',
  format: 'esm',
  target: 'es2022',
  alias: {
    '@archava/projects/dashboard': join(output, 'node_modules/@archava/projects/dist/dashboard.js'),
    '@archava/scheduling/dashboard': join(
      output,
      'node_modules/@archava/scheduling/dist/dashboard.js',
    ),
    '@archava/support/dashboard': join(output, 'node_modules/@archava/support/dist/dashboard.js'),
    '@archava/foodservice/dashboard': join(
      output,
      'node_modules/@archava/foodservice/dist/dashboard.js',
    ),
    '@archava/commerce/dashboard': join(output, 'node_modules/@archava/commerce/dist/dashboard.js'),
    '@archava/accommodation/dashboard': join(
      output,
      'node_modules/@archava/accommodation/dist/dashboard.js',
    ),
    '@archava/config/assembly': join(output, 'node_modules/@archava/config/dist/assembly.js'),
    '@archava/crm/contracts': join(output, 'node_modules/@archava/crm/dist/contracts.js'),
    '@archava/crm/overview-contracts': join(
      output,
      'node_modules/@archava/crm/dist/overview-contracts.js',
    ),
    '@archava/crm/overview-dashboard': join(
      output,
      'node_modules/@archava/crm/dist/overview-dashboard.js',
    ),
    '@archava/crm/dashboard': join(output, 'node_modules/@archava/crm/dist/dashboard.js'),
    '@archava/crm/worklist-dashboard': join(
      output,
      'node_modules/@archava/crm/dist/worklist-dashboard.js',
    ),
    '@archava/crm/worklist-contracts': join(
      output,
      'node_modules/@archava/crm/dist/worklist-contracts.js',
    ),
    '@archava/crm/pipeline-dashboard': join(
      output,
      'node_modules/@archava/crm/dist/pipeline-dashboard.js',
    ),
    '@archava/crm/pipeline-contracts': join(
      output,
      'node_modules/@archava/crm/dist/pipeline-contracts.js',
    ),
    '@archava/crm-assistant/panel': join(
      output,
      'node_modules/@archava/crm-assistant/dist/panel.js',
    ),
    '@archava/crm-assistant/pilot': join(
      output,
      'node_modules/@archava/crm-assistant/dist/pilot.js',
    ),
    '@archava/crm-assistant/recovery': join(
      output,
      'node_modules/@archava/crm-assistant/dist/recovery.js',
    ),
    '@archava/crm-act/contracts': join(output, 'node_modules/@archava/crm-act/dist/contracts.js'),
  },
  metafile: true,
  logLevel: 'silent',
})
if (
  Object.keys(browser.metafile.inputs).some(
    (path) =>
      path.includes('identity-pg') ||
      path.includes('crm-pg') ||
      path.includes('projects-pg') ||
      path.includes('scheduling-pg') ||
      path.includes('accommodation-pg') ||
      path.includes('commerce-pg') ||
      path.includes('foodservice-pg') ||
      path.includes('support-pg') ||
      path.includes('provider-runtime') ||
      path.includes('node:') ||
      /crm-assistant\/dist\/(conversation|planner|gemini-planner|server|index)\.js/.test(path) ||
      /@archava\/assistant\//.test(path),
  )
)
  throw new Error('Server implementation entered browser bundle')
await writeFile(join(output, 'browser-meta.json'), JSON.stringify(browser.metafile, null, 2) + '\n')
process.stdout.write(
  'Built CRM runtime with installed compiled library modules and a shared browser dashboard.\n',
)
