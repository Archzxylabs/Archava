/* global process */
import { execFileSync } from 'node:child_process'
import { readFile, writeFile, mkdir, cp, rm } from 'node:fs/promises'
import { resolve, join } from 'node:path'
import { build } from 'esbuild'

const root = resolve(import.meta.dirname, '../../..')
const output = join(root, 'dist/crm-runtime')
execFileSync('pnpm', ['library:pack'], { cwd: root, stdio: 'pipe' })
await mkdir(output, { recursive: true })
const { artifacts } = JSON.parse(await readFile(join(root, 'dist/library/artifacts.json'), 'utf8'))
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
      version: '0.1.0',
      private: true,
      type: 'module',
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
const server = await build({
  entryPoints: [join(root, 'apps/crm/src/server.ts'), join(root, 'apps/crm/src/operator.ts')],
  outdir: output,
  outExtension: { '.js': '.mjs' },
  bundle: true,
  packages: 'external',
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
    '@archava/crm/dashboard': join(output, 'node_modules/@archava/crm/dist/dashboard.js'),
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
  },
  metafile: true,
  logLevel: 'silent',
})
if (
  Object.keys(browser.metafile.inputs).some(
    (path) =>
      path.includes('identity-pg') ||
      path.includes('crm-pg') ||
      path.includes('node:') ||
      /crm-assistant\/dist\/(conversation|planner|index)\.js/.test(path) ||
      /@archava\/assistant\//.test(path),
  )
)
  throw new Error('Server implementation entered browser bundle')
await writeFile(join(output, 'browser-meta.json'), JSON.stringify(browser.metafile, null, 2) + '\n')
process.stdout.write(
  'Built CRM runtime with installed compiled library modules and a shared browser dashboard.\n',
)
