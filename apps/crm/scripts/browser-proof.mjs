/* global process, fetch, setTimeout */
import assert from 'node:assert/strict'
import { spawn, execFileSync } from 'node:child_process'
import { createServer } from 'node:net'
import { writeFile, mkdir, readFile } from 'node:fs/promises'
import { join } from 'node:path'

export async function verifyAuthenticatedBrowser({
  root,
  home,
  origin,
  credentials,
  minimal = false,
}) {
  assert.ok(home.startsWith('/tmp/archava-crm-runtime-'))
  const reservation = createServer()
  await new Promise((done, fail) => {
    reservation.once('error', fail)
    reservation.listen(0, '127.0.0.1', done)
  })
  const port = reservation.address().port
  await new Promise((done) => reservation.close(done))
  const target = join(home, 'browser-target.json')
  await writeFile(target, JSON.stringify({ origin, credentials }) + '\n', { mode: 0o600 })
  const browser = spawn(
    process.env.E2E_BROWSER ?? '/usr/bin/brave',
    [
      '--headless=new',
      '--no-sandbox',
      '--disable-gpu',
      '--disable-background-networking',
      '--no-first-run',
      '--remote-debugging-port=' + port,
      '--user-data-dir=' + join(home, 'browser-profile'),
      'about:blank',
    ],
    { stdio: 'ignore' },
  )
  const exited = new Promise((done) => {
    browser.once('exit', done)
    browser.once('error', done)
  })
  try {
    let ready = false
    for (let i = 0; i < 60; i++) {
      await new Promise((done) => setTimeout(done, 250))
      try {
        const response = await fetch('http://127.0.0.1:' + port + '/json')
        if (response.ok) {
          ready = true
          break
        }
      } catch {
        /* browser still starting */
      }
    }
    assert.ok(ready, 'isolated Chromium debug endpoint unavailable')
    const stdout = execFileSync(
      process.execPath,
      [join(root, minimal ? 'tools/e2e/crm-minimal.mjs' : 'tools/e2e/crm-runtime.mjs')],
      {
        cwd: root,
        env: {
          ...process.env,
          CDP_PORT: String(port),
          E2E_PROFILE: join(home, 'browser-profile'),
          ARCHAVA_CRM_BROWSER_TARGET: target,
        },
        stdio: 'pipe',
        encoding: 'utf8',
        maxBuffer: 4 * 1024 * 1024,
      },
    )
    const checks = stdout
      .split('\n')
      .filter((line) => line.startsWith('PASS '))
      .map((line) => line.slice(5))
    assert.ok(checks.length > 0)
    const profile = JSON.parse(
      await readFile(join(root, 'dist/crm-runtime/release-profile.json'), 'utf8'),
    )
    const output = join(root, 'dist/browser-proof')
    await mkdir(output, { recursive: true })
    await writeFile(
      join(output, minimal ? 'minimal-results.json' : 'results.json'),
      JSON.stringify(
        {
          schemaVersion: '1.0.0',
          profile: profile.catalog.version,
          composition: minimal ? 'minimal' : 'full',
          checks,
          result: 'passed',
          browser: 'isolated headless Chromium',
          fixture: 'synthetic authenticated persistent owner/staff CRM',
          providerCalls: 0,
        },
        null,
        2,
      ) + '\n',
    )
    process.stdout.write(
      `${checks.length}/${checks.length} isolated authenticated Chromium checks passed.\n`,
    )
  } finally {
    browser.kill('SIGTERM')
    await exited
  }
}
