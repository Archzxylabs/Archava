import { createHmac, randomUUID } from 'node:crypto'
import { describe, it, expect } from 'vitest'
import type { EmailDeliveryStatusLookup } from '@archava/act-email'
import {
  PostmarkStatusPort,
  POSTMARK_ATTEMPT_KEY,
  PostmarkEmailGateway,
  defaultPostmarkReadTransport,
  defaultPostmarkTransport,
  type PostmarkReadRequest,
  type PostmarkHTTPResponse,
} from '../src/index.js'

const key = new Uint8Array(32).fill(17)
const token = 'synthetic-not-a-real-server-token'
const lookup: EmailDeliveryStatusLookup = {
  tenantId: 'tenant-a',
  idempotencyKey: 'raw-private-upstream-key',
  fingerprint: 'a'.repeat(64),
}
const label = (tenant = 'tenant-a') =>
  createHmac('sha256', key)
    .update(JSON.stringify(['archava-postmark-attempt-v2', tenant, lookup.idempotencyKey]))
    .digest('base64url')
const message = () => ({
  MessageID: randomUUID(),
  MessageStream: 'outbound',
  Status: 'Sent',
  Sandboxed: false,
  Metadata: { [POSTMARK_ATTEMPT_KEY]: label() },
  To: [{ Email: 'private-recipient@example.test' }],
  Subject: 'Private subject',
})
const reply = (value: unknown, status = 200): PostmarkHTTPResponse => ({
  status,
  body: JSON.stringify(value),
})
function fixture(response = reply({ TotalCount: 1, Messages: [message()] })) {
  const calls: PostmarkReadRequest[] = []
  const port = new PostmarkStatusPort({
    recovery: {
      tenantId: lookup.tenantId,
      serverToken: token,
      attemptLabelKey: key,
      transport: {
        get: (request) => {
          calls.push(request)
          return Promise.resolve(response)
        },
      },
    },
  })
  return { port, calls }
}
describe('Postmark positive acceptance recovery', () => {
  it('uses one exact tenant-separated metadata read and exposes only acceptance plus provider id', async () => {
    const record = message(),
      f = fixture(reply({ TotalCount: 1, Messages: [record] }))
    expect(await f.port.status(lookup)).toEqual({
      outcome: 'accepted',
      providerMessageId: record.MessageID,
    })
    expect(f.calls).toHaveLength(1)
    const url = new URL(required(f.calls[0]).url)
    expect(url.origin + url.pathname).toBe('https://api.postmarkapp.com/messages/outbound')
    expect(Object.fromEntries(url.searchParams)).toEqual({
      count: '2',
      offset: '0',
      messagestream: 'outbound',
      metadata_archava_attempt: label(),
    })
    expect(required(f.calls[0]).headers).toEqual({
      accept: 'application/json',
      'x-postmark-server-token': token,
    })
    expect(f.calls[0]).not.toHaveProperty('body')
    expect(required(f.calls[0]).url).not.toContain(lookup.idempotencyKey)
    expect(required(f.calls[0]).url).not.toContain(token)
    expect(JSON.stringify(f.port)).not.toContain(token)
    expect(JSON.stringify(f.port)).not.toContain(lookup.idempotencyKey)
  })
  it('shares exactly the send gateway label without altering approved-template send semantics', async () => {
    let wire: Record<string, unknown> = {}
    const gateway = new PostmarkEmailGateway({
      serverToken: token,
      attemptLabelKey: key,
      transport: {
        post: async (request) => {
          await Promise.resolve()
          wire = JSON.parse(request.body) as Record<string, unknown>
          return reply({
            ErrorCode: 0,
            MessageID: randomUUID(),
            SubmittedAt: '2026-10-09T10:00:00Z',
            Message: 'OK',
          })
        },
      },
    })
    expect(JSON.stringify(gateway)).toBe(JSON.stringify({ gatewayId: 'postmark:withTemplate@1' }))
    await gateway.send({
      tenantId: 'tenant-a',
      idempotencyKey: lookup.idempotencyKey,
      recipient: 'synthetic@example.test',
      template: {
        tenantId: 'tenant-a',
        templateId: 'confirmation',
        providerTemplateId: 'template-123',
        senderAddress: 'sender@example.test',
        approved: true,
      },
    })
    const f = fixture()
    await f.port.status(lookup)
    expect((wire.Metadata as Record<string, unknown>)[POSTMARK_ATTEMPT_KEY]).toBe(
      new URL(required(f.calls[0]).url).searchParams.get('metadata_archava_attempt'),
    )
  })
  it('refuses another tenant and invalid lookup before the transport', async () => {
    const f = fixture()
    for (const attempted of [
      { ...lookup, tenantId: 'tenant-b' },
      { ...lookup, fingerprint: 'invalid' },
      { ...lookup, idempotencyKey: '' },
    ])
      expect((await f.port.status(attempted)).outcome).toBe('unknown')
    expect(f.calls).toHaveLength(0)
    const other = new PostmarkStatusPort({
      recovery: {
        tenantId: 'tenant-b',
        serverToken: token,
        attemptLabelKey: key,
        transport: {
          get: async (request) => {
            await Promise.resolve()
            expect(new URL(request.url).searchParams.get('metadata_archava_attempt')).toBe(
              label('tenant-b'),
            )
            return reply({ TotalCount: 0, Messages: [] })
          },
        },
      },
    })
    expect((await other.status({ ...lookup, tenantId: 'tenant-b' })).outcome).toBe('unknown')
    expect(label('tenant-b')).not.toBe(label())
  })
  it('misses, duplicates, partial pages and large totals remain unknown with no pagination/retry/send', async () => {
    for (const payload of [
      { TotalCount: 0, Messages: [] },
      { TotalCount: 2, Messages: [message(), message()] },
      { TotalCount: 2, Messages: [message()] },
      { TotalCount: 1, Messages: [] },
      { TotalCount: 10001, Messages: [message()] },
      { TotalCount: '1', Messages: [message()] },
    ]) {
      const f = fixture(reply(payload))
      expect(await f.port.status(lookup)).toEqual({
        outcome: 'unknown',
        reason: 'status_not_available',
      })
      expect(f.calls).toHaveLength(1)
    }
  })
  it('rejects uncorrelated, wrong-stream/sandbox and malformed provider records', async () => {
    for (const patch of [
      { MessageID: 'bad id with spaces' },
      { Metadata: { [POSTMARK_ATTEMPT_KEY]: label('tenant-b') } },
      { Metadata: null },
      { MessageStream: 'other-stream' },
      { Sandboxed: true },
      { Sandboxed: undefined },
      { Status: 'Delivered' },
      { Status: 'Failed' },
    ]) {
      const f = fixture(reply({ TotalCount: 1, Messages: [{ ...message(), ...patch }] }))
      expect((await f.port.status(lookup)).outcome).toBe('unknown')
    }
  })
  it('queued/processed are acceptance, never a delivery claim', async () => {
    for (const Status of ['Queued', 'Processed']) {
      const record = { ...message(), Status },
        f = fixture(reply({ TotalCount: 1, Messages: [record] }))
      expect(await f.port.status(lookup)).toEqual({
        outcome: 'accepted',
        providerMessageId: record.MessageID,
      })
    }
  })
  it('bounds replies and flattens every HTTP/network/parser failure to unknown', async () => {
    for (const response of [
      { status: 200, body: 'not JSON' },
      { status: 200, body: ' '.repeat(65537) },
      ...[401, 404, 429, 500, 503].map((status) => reply({ private: 'provider body' }, status)),
    ]) {
      const f = fixture(response)
      expect(await f.port.status(lookup)).toEqual({
        outcome: 'unknown',
        reason: 'status_not_available',
      })
    }
    const throwing = new PostmarkStatusPort({
      recovery: {
        tenantId: 'tenant-a',
        serverToken: token,
        attemptLabelKey: key,
        transport: { get: () => Promise.reject(new Error(token + ' ' + lookup.idempotencyKey)) },
      },
    })
    expect(await throwing.status(lookup)).toEqual({
      outcome: 'unknown',
      reason: 'status_not_available',
    })
  })
  it('times out even a transport that ignores abort, and never initiates a retry', async () => {
    let calls = 0
    const port = new PostmarkStatusPort({
      recovery: {
        tenantId: 'tenant-a',
        serverToken: token,
        attemptLabelKey: key,
        timeoutMs: 5,
        transport: {
          get: () => {
            calls++
            return new Promise(() => {})
          },
        },
      },
    })
    expect(await port.status(lookup)).toEqual({
      outcome: 'unknown',
      reason: 'status_not_available',
    })
    expect(calls).toBe(1)
  })
  it('copies its secret key and rejects incomplete/injectable configuration', async () => {
    const mutable = Uint8Array.from(key)
    const port = new PostmarkStatusPort({
      recovery: {
        tenantId: 'tenant-a',
        serverToken: token,
        attemptLabelKey: mutable,
        transport: {
          get: async (request) => {
            await Promise.resolve()
            expect(new URL(request.url).searchParams.get('metadata_archava_attempt')).toBe(label())
            return reply({ TotalCount: 0, Messages: [] })
          },
        },
      },
    })
    mutable.fill(0)
    await port.status(lookup)
    for (const patch of [
      { serverToken: 'bad\r\nheader' },
      { attemptLabelKey: new Uint8Array(8) },
      { messageStream: '../other' },
      { timeoutMs: 0 },
    ])
      expect(
        () =>
          new PostmarkStatusPort({
            recovery: { tenantId: 'tenant-a', serverToken: token, attemptLabelKey: key, ...patch },
          }),
      ).toThrow()
  })
})
describe('read-only transport bounds', () => {
  it('also bounds send responses and prevents redirecting its credential', async () => {
    let init: RequestInit | undefined
    const transport = defaultPostmarkTransport((_url, options) => {
      init = options
      return Promise.resolve(new Response('{}', { status: 200 }))
    })
    expect(
      await transport.post({
        url: 'https://api.postmarkapp.com/email/withTemplate',
        headers: {},
        body: '{}',
      }),
    ).toEqual({ status: 200, body: '{}' })
    expect(init).toMatchObject({ method: 'POST', redirect: 'error', credentials: 'omit' })
    const oversized = defaultPostmarkTransport(() =>
      Promise.resolve(new Response('x'.repeat(65537))),
    )
    await expect(
      oversized.post({
        url: 'https://api.postmarkapp.com/email/withTemplate',
        headers: {},
        body: '{}',
      }),
    ).rejects.toThrow('the request to the mail provider did not complete')
  })
  it('uses GET with no body and forbids redirect/token forwarding', async () => {
    let init: RequestInit | undefined
    const transport = defaultPostmarkReadTransport((_url, options) => {
      init = options
      return Promise.resolve(new Response('{}', { status: 200 }))
    })
    await transport.get({
      url: 'https://api.postmarkapp.com/messages/outbound',
      headers: { 'x-postmark-server-token': token },
    })
    expect(init).toMatchObject({ method: 'GET', redirect: 'error', credentials: 'omit' })
    expect(init).not.toHaveProperty('body')
  })
  it('cancels oversized streams and does not echo their content', async () => {
    const transport = defaultPostmarkReadTransport(() =>
      Promise.resolve(new Response('x'.repeat(65537))),
    )
    await expect(
      transport.get({ url: 'https://api.postmarkapp.com/messages/outbound', headers: {} }),
    ).rejects.toThrow('the request to the mail provider did not complete')
  })
})

function required<T>(value: T | undefined): T {
  if (value === undefined) throw new Error('Missing fixture result')
  return value
}
