import { build } from 'esbuild'
await build({
  entryPoints: ['src/main.ts'],
  outfile: 'dist/main.js',
  bundle: true,
  packages: 'external',
  platform: 'neutral',
  format: 'esm',
  target: 'es2022',
})
await build({
  entryPoints: ['src/browser.ts'],
  outfile: 'dist/browser.js',
  bundle: true,
  platform: 'browser',
  format: 'esm',
  target: 'es2022',
  metafile: true,
}).then(async (result) => {
  const { writeFile } = await import('node:fs/promises')
  await writeFile('dist/browser-metafile.json', JSON.stringify(result.metafile, null, 2))
})
