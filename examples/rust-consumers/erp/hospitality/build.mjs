import { build } from 'esbuild'
import { writeFile } from 'node:fs/promises'
const result = await build({
  entryPoints: ['src/browser.ts'],
  outfile: 'dist/browser.js',
  bundle: true,
  platform: 'browser',
  format: 'esm',
  target: 'es2022',
  metafile: true,
})
await writeFile('dist/metafile.json', JSON.stringify(result.metafile, null, 2))
