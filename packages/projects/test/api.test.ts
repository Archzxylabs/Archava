import { describe, expect, it, vi } from 'vitest'
import {
  createProjectApi,
  ProjectService,
  ProjectError,
  type ProjectRepository,
  createProjectTools,
} from '../src/index.js'

const id = 'f3d87d79-06a8-43d3-93a0-d27387fa2665'
const mutation = {
  requestId: id,
  command: { operation: 'project.create', title: 'Website', scopeSummary: 'Five agreed pages' },
}
function fixture() {
  const repo = {
    access: vi.fn<ProjectRepository['access']>(),
    list: vi.fn<ProjectRepository['list']>(),
    detail: vi.fn<ProjectRepository['detail']>(),
    mutate: vi.fn<ProjectRepository['mutate']>(),
    commandStatus: vi.fn<ProjectRepository['commandStatus']>(),
  }
  const service = new ProjectService(repo)
  const api = createProjectApi(service, () => Promise.resolve({ kind: 'human', id: 'owner' }))
  const request = (body: unknown, headers: Record<string, string> = {}) =>
    api(
      new Request('https://app.test/api/projects/studio/commands', {
        method: 'POST',
        headers: { origin: 'https://app.test', ...headers },
        body: typeof body === 'string' ? body : JSON.stringify(body),
      }),
    )
  return { repo, service, api, request }
}
describe('project boundaries', () => {
  it('exposes only scoped reads to assistant tools and refuses tenant or role overrides', async () => {
    const f = fixture(),
      tools = createProjectTools(f.service, 'studio', { kind: 'human', id: 'owner' })
    f.repo.list.mockResolvedValue({ items: [], next: null })
    await tools.invoke('projects.list', {})
    expect(f.repo.list).toHaveBeenCalledWith(
      'studio',
      { kind: 'human', id: 'owner' },
      expect.objectContaining({ limit: 20 }),
    )
    await expect(tools.invoke('projects.list', { tenant: 'other' })).rejects.toThrow()
    await expect(tools.invoke('projects.detail', { id, role: 'owner' })).rejects.toThrow()
    await expect(tools.invoke('projects.create', mutation.command)).rejects.toThrow(ProjectError)
    expect(f.repo.mutate).not.toHaveBeenCalled()
  })
  it('binds writes to host identity and refuses roles, owner overrides and unknown fields', async () => {
    const f = fixture()
    vi.mocked(f.repo.mutate).mockResolvedValue({
      kind: 'project',
      id,
      projectId: id,
      revision: 1,
      projectRevision: 1,
    })
    expect((await f.request(mutation)).status).toBe(200)
    expect(f.repo.mutate).toHaveBeenCalledWith(
      'studio',
      { kind: 'human', id: 'owner' },
      expect.objectContaining({ requestId: id }),
    )
    for (const body of [
      { ...mutation, role: 'owner' },
      { ...mutation, principal: { kind: 'human', id: 'admin' } },
      { ...mutation, command: { ...mutation.command, ownerId: 'admin' } },
    ])
      expect((await f.request(body)).status).toBe(400)
    expect(f.repo.mutate).toHaveBeenCalledTimes(1)
  })
  it('denies machine writes and recovery before reaching persistence', () => {
    const f = fixture(),
      principal = { kind: 'service' as const, id: 'bot', onBehalfOf: 'owner' }
    expect(() => f.service.mutate('studio', principal, mutation)).toThrow(ProjectError)
    expect(() => f.service.commandStatus('studio', principal, id)).toThrow(ProjectError)
    expect(f.repo.mutate).not.toHaveBeenCalled()
  })
  it('rejects foreign origins, cookie requests without origin, oversized streams and malformed JSON', async () => {
    const f = fixture()
    expect((await f.request(mutation, { origin: 'https://evil.test' })).status).toBe(403)
    const noOrigin = new Request('https://app.test/api/projects/studio/commands', {
      method: 'POST',
      headers: { cookie: 'session=fixture' },
      body: JSON.stringify(mutation),
    })
    expect((await f.api(noOrigin)).status).toBe(403)
    expect((await f.request('x'.repeat(16_385))).status).toBe(413)
    expect((await f.request('{bad json')).status).toBe(400)
    expect(f.repo.mutate).not.toHaveBeenCalled()
  })
  it('requires identity for reads, rejects unknown filters, and never exposes database failures', async () => {
    const f = fixture()
    expect(
      (
        await createProjectApi(f.service, () => Promise.resolve(null))(
          new Request('https://app.test/api/projects/studio/access'),
        )
      ).status,
    ).toBe(401)
    expect(
      (await f.api(new Request('https://app.test/api/projects/studio/records?tenant=other')))
        .status,
    ).toBe(400)
    vi.mocked(f.repo.mutate).mockRejectedValue(new Error('secret-database-marker'))
    const response = await f.request(mutation)
    expect(response.status).toBe(503)
    expect(await response.text()).not.toContain('secret-database-marker')
  })
  it('requires cancellation reason and bounds child revisions and paging', async () => {
    const f = fixture()
    for (const command of [
      { operation: 'project.transition', projectId: id, projectRevision: 1, state: 'cancelled' },
      {
        operation: 'task.transition',
        projectId: id,
        projectRevision: 1,
        id,
        expectedRevision: 0,
        state: 'done',
      },
      {
        operation: 'milestone.complete',
        projectId: id,
        projectRevision: 1,
        id,
        expectedRevision: 1,
        evidence: '',
      },
    ])
      expect((await f.request({ requestId: id, command })).status).toBe(400)
    expect(
      (await f.api(new Request('https://app.test/api/projects/studio/records?limit=101'))).status,
    ).toBe(400)
    expect(f.repo.mutate).not.toHaveBeenCalled()
  })
})
