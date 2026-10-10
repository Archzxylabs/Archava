/* global process */
/** Internal compiled artifacts; no publication and no source-workspace imports. */
import { readdir, readFile, mkdir, rm, cp, writeFile } from 'node:fs/promises'
import { resolve, dirname, relative, join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { createHash } from 'node:crypto'
import { spawnSync } from 'node:child_process'
import ts from 'typescript'
import { build } from 'esbuild'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const output = join(root, 'dist/library')
const migrationPackages = new Set([
  'scheduling-pg',
  'projects-pg',
  'crm-pg',
  'identity-pg',
  'provider-runtime',
  'act-host-pg',
  'act-confirmation-pg',
  'act-storage',
  'crm-act',
])

async function sourceFiles(directory) {
  const files = []
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const file = join(directory, entry.name)
    if (entry.isDirectory()) files.push(...(await sourceFiles(file)))
    else if (entry.name.endsWith('.ts')) files.push(file)
  }
  return files
}

export async function packLibrary() {
  const catalog = JSON.parse(await readFile(join(root, 'config/library-release.v1.json'), 'utf8'))
  const manifests = new Map()
  async function include(name) {
    if (manifests.has(name)) return
    const directory = join(root, 'packages', name.slice('@archava/'.length))
    const manifest = JSON.parse(await readFile(join(directory, 'package.json'), 'utf8'))
    if (manifest.name !== name) throw new Error(`Invalid package identity: ${name}`)
    manifests.set(name, { manifest, directory })
    for (const dependency of Object.keys(manifest.dependencies ?? {})) {
      if (dependency.startsWith('@archava/')) await include(dependency)
    }
  }
  for (const name of [
    '@archava/scheduling-pg',
    '@archava/projects-pg',
    '@archava/kits',
    '@archava/crm-act',
    '@archava/presence',
    '@archava/sdk',
    '@archava/assistant',
    '@archava/chat',
    '@archava/crm',
    '@archava/crm-pg',
    '@archava/identity-pg',
    '@archava/crm-assistant',
    '@archava/provider-runtime',
    '@archava/act-host-pg',
    '@archava/act-confirmation-pg',
    '@archava/act',
    '@archava/act-postmark',
  ])
    await include(name)
  if (
    catalog.packages.length !== manifests.size ||
    catalog.packages.some((pkg) => manifests.get(pkg.name)?.manifest.version !== pkg.version)
  )
    throw new Error(
      'Unsupported release package/version set; update the trusted local profile explicitly',
    )
  const host = JSON.parse(await readFile(join(root, 'apps/crm/package.json'), 'utf8'))
  if (host.name !== catalog.host.name || host.version !== catalog.host.version)
    throw new Error('Unsupported release host version')
  await rm(output, { recursive: true, force: true })
  await mkdir(join(output, 'tarballs'), { recursive: true })

  const base = ts.readConfigFile(join(root, 'tsconfig.base.json'), ts.sys.readFile)
  if (base.error) throw new Error(ts.flattenDiagnosticMessageText(base.error.messageText, '\n'))
  const options = ts.convertCompilerOptionsFromJson(
    {
      ...base.config.compilerOptions,
      noEmit: false,
      declaration: true,
      emitDeclarationOnly: true,
      allowImportingTsExtensions: false,
      rootDir: join(root, 'packages'),
      outDir: join(output, 'types'),
      lib: ['ES2022', 'DOM', 'DOM.Iterable'],
    },
    root,
  )
  const roots = []
  for (const { directory } of manifests.values())
    roots.push(...(await sourceFiles(join(directory, 'src'))))
  const program = ts.createProgram(roots, options.options)
  const diagnostics = [...options.errors, ...ts.getPreEmitDiagnostics(program)]
  if (diagnostics.length) {
    throw new Error(
      ts.formatDiagnosticsWithColorAndContext(diagnostics, {
        getCurrentDirectory: () => root,
        getCanonicalFileName: (file) => file,
        getNewLine: () => '\n',
      }),
    )
  }
  if (program.emit().emitSkipped) throw new Error('Declaration build skipped')

  const artifacts = []
  // Compile the shared validator before importing it; never require a TS loader in consumers.
  let releaseValidator
  for (const [name, { manifest, directory }] of manifests) {
    const shortName = name.slice('@archava/'.length)
    const stage = join(output, 'packages', shortName)
    await mkdir(join(stage, 'dist'), { recursive: true })
    await cp(join(output, 'types', shortName, 'src'), join(stage, 'dist'), { recursive: true })
    // Preserve modules so root/subpath exports share class and singleton identity.
    await build({
      entryPoints: await sourceFiles(join(directory, 'src')),
      outbase: join(directory, 'src'),
      outdir: join(stage, 'dist'),
      bundle: false,
      format: 'esm',
      platform: 'neutral',
      target: 'es2022',
      logLevel: 'silent',
    })
    if (migrationPackages.has(shortName))
      await cp(join(directory, 'migrations'), join(stage, 'migrations'), { recursive: true })
    const exports = {}
    for (const [subpath, source] of Object.entries(manifest.exports)) {
      if (typeof source !== 'string' || !source.startsWith('./src/'))
        throw new Error(`Unsupported export ${name}/${subpath}`)
      const js = source.replace('./src/', './dist/').replace(/\.ts$/, '.js')
      const types = js.replace(/\.js$/, '.d.ts')
      exports[subpath] = { types, import: js, default: js }
    }
    const dependencies = Object.fromEntries(
      Object.entries(manifest.dependencies ?? {}).map(([id, version]) => [
        id,
        version.startsWith('workspace:') ? manifests.get(id).manifest.version : version,
      ]),
    )
    await writeFile(
      join(stage, 'package.json'),
      JSON.stringify(
        {
          name,
          version: manifest.version,
          private: true,
          type: 'module',
          description: manifest.description,
          main: './dist/index.js',
          types: './dist/index.d.ts',
          exports,
          files: migrationPackages.has(shortName) ? ['dist', 'migrations'] : ['dist'],
          dependencies,
        },
        null,
        2,
      ) + '\n',
    )
    if (shortName === 'config')
      releaseValidator = await import(
        pathToFileURL(join(stage, 'dist/release.js')).href + '?build=' + Date.now()
      )
    const packed = spawnSync(
      process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm',
      ['pack', '--pack-destination', join(output, 'tarballs')],
      { cwd: stage, encoding: 'utf8' },
    )
    if (packed.status !== 0) throw new Error(packed.stderr || packed.stdout)
    const tarball = join(output, 'tarballs', `archava-${shortName}-${manifest.version}.tgz`)
    const sha256 = createHash('sha256')
      .update(await readFile(tarball))
      .digest('hex')
    artifacts.push({ name, version: manifest.version, tarball: relative(output, tarball), sha256 })
  }
  if (!releaseValidator) throw new Error('Missing release validator')
  const trusted = releaseValidator.releaseCatalogSchema.parse(catalog)
  const resolved = { catalog: trusted, artifacts: [] }
  for (const artifact of artifacts) {
    const pkg = trusted.packages.find((item) => item.name === artifact.name)
    const directory = join(output, 'packages', artifact.name.slice('@archava/'.length))
    await releaseValidator.verifyReleasePackage(directory, pkg)
    resolved.artifacts.push({
      ...artifact,
      implementationSha256: await releaseValidator.releaseImplementationHash(directory),
    })
  }
  releaseValidator.validateResolvedRelease(resolved, trusted)
  await writeFile(join(output, 'release-profile.json'), JSON.stringify(resolved, null, 2) + '\n')
  await writeFile(
    join(output, 'artifacts.json'),
    JSON.stringify({ schema_version: '1.0.0', artifacts }, null, 2) + '\n',
  )
  process.stdout.write(
    `Packed ${artifacts.length} compiled internal packages in dist/library/tarballs.\n`,
  )
  return { output, artifacts, profile: resolved }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await packLibrary()
}
