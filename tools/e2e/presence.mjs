/** Real-browser presence acceptance against the fixture's owned in-memory transport. */
import assert from 'node:assert/strict'

async function mutate(page, path, body) {
  const csrf = await page.evaluate("localStorage.getItem('archava_csrf') || ''")
  assert.ok(csrf, 'CSRF token present')
  return page.evaluate(`fetch(${JSON.stringify(path)},{method:'POST',headers:{'content-type':'application/json','x-archava-csrf':${JSON.stringify(csrf)}},body:${JSON.stringify(JSON.stringify(body))}}).then(async r=>({status:r.status,body:await r.json().catch(()=>null)}))`)
}

const turn = (sequence, text) => ({
  frame_id: `frame-${sequence}`,
  sequence_number: sequence,
  speaker: 'alice',
  text,
  is_final: true,
  latency_ms: 50,
  confidence_score: 0.95,
})

export async function verifyPresence({ page, pass, suffix }) {
  const sessionId = `presence-${suffix}`
  const started = await mutate(page, '/presence/sessions', { tenant_id: 'acme-retail', session_id: sessionId, mode: 'voice' })
  assert.equal(started.status, 201, JSON.stringify(started.body))
  assert.equal(started.body.session.session_id, sessionId)
  assert.equal(started.body.session.tenant_id, 'acme-retail')
  assert.equal(started.body.session.participant_id, 'alice')
  assert.equal(started.body.session.current_mode, 'voice')
  assert.equal(started.body.token.split('.').length, 3)

  const read = await page.evaluate(`fetch('/presence/sessions/${sessionId}').then(async r=>({status:r.status,body:await r.json()}))`)
  assert.equal(read.status, 200)
  assert.equal(read.body.session_id, sessionId)

  const first = await mutate(page, `/presence/sessions/${sessionId}/turn`, turn(1, 'hello presence'))
  assert.equal(first.status, 200, JSON.stringify(first.body))
  assert.equal(first.body.sequence_number, 2)
  assert.ok(String(first.body.text).includes('hello presence'))

  const duplicate = await mutate(page, `/presence/sessions/${sessionId}/turn`, turn(1, 'out of order'))
  assert.equal(duplicate.status, 409, JSON.stringify(duplicate.body))
  assert.equal(duplicate.body.error, 'sequence_mismatch')

  const degraded = await mutate(page, `/presence/sessions/${sessionId}/degrade`, { reason: 'synthetic media drop' })
  assert.equal(degraded.status, 200, JSON.stringify(degraded.body))
  assert.equal(degraded.body.current_mode, 'chat')

  const collision = await mutate(page, '/presence/sessions', { tenant_id: 'acme-retail', session_id: sessionId, mode: 'voice' })
  assert.equal(collision.status, 409, JSON.stringify(collision.body))

  const invalid = await mutate(page, '/presence/sessions', { tenant_id: 'acme-retail', session_id: 'bad session!', mode: 'voice' })
  assert.equal(invalid.status, 400, JSON.stringify(invalid.body))
  assert.equal(invalid.body.error, 'invalid_session_id')

  pass('PRESENCE-loopback-lifecycle', { sessionId, tokenParts: 3, firstReplySequence: first.body.sequence_number, degradedTo: degraded.body.current_mode })
  return { sessionId }
}

export async function verifyPresenceIsolation({ page, pass, sessionId }) {
  const deniedRead = await page.evaluate(`fetch('/presence/sessions/${sessionId}').then(r=>r.status)`)
  assert.equal(deniedRead, 403)

  const deniedTurn = await mutate(page, `/presence/sessions/${sessionId}/turn`, turn(3, 'foreign write'))
  assert.equal(deniedTurn.status, 403)
  assert.equal(deniedTurn.body.error, 'forbidden_session_access')

  const foreignTenant = await mutate(page, '/presence/sessions', { tenant_id: 'globex-corp', session_id: `foreign-${sessionId}`, mode: 'voice' })
  assert.equal(foreignTenant.status, 403, JSON.stringify(foreignTenant.body))
  assert.equal(foreignTenant.body.error, 'forbidden_tenant_access')

  pass('PRESENCE-cross-user-and-tenant-refusal', { sessionId, deniedRead, deniedTurn: deniedTurn.status, foreignTenant: foreignTenant.status })
}
