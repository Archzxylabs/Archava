import type { Pool } from 'pg'

export type Row = Record<string, unknown>
export type StubResult = { rows: Row[] }
export type Query = { text: string; params: unknown[] }

export type Handler = (query: Query) => StubResult | undefined

export type StubClient = {
  queries: Query[]
  query(sql: string, params?: unknown[]): Promise<StubResult>
  release(): void
}

/**
 * The repository is handed a real `pg` pool, so the stub carries the same type.
 * It implements only the two members the repository calls (`query` and
 * `connect`); the remaining `Pool` surface is deliberately absent and asserted
 * once here rather than fake-implemented in every test.
 */
export type StubPool = Pool & { queries: Query[]; clientQueries: Query[] }

export function makeRow(values: Row): Row {
  return values
}

export function createStubPool(handler: Handler): StubPool {
  const queries: Query[] = []
  const clientQueries: Query[] = []

  const emit = (log: Query[], sql: string, params: unknown[] = []) => {
    const query: Query = { text: sql, params }
    log.push(query)
    const result = handler(query)
    return Promise.resolve(result ?? { rows: [] })
  }

  const stub = {
    queries,
    clientQueries,
    query(sql: string, params?: unknown[]) {
      return emit(queries, sql, params)
    },
    connect(): Promise<StubClient> {
      const client: StubClient = {
        queries: clientQueries,
        release: () => {},
        query(sql: string, params?: unknown[]) {
          return emit(clientQueries, sql, params)
        },
      }
      return Promise.resolve(client)
    },
  }

  return stub as unknown as StubPool
}
