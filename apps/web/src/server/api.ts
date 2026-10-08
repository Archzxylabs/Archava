/**
 * The one route that has to run server-side, and the reason it is one.
 *
 * The studio answers questions with money in them. Pricing is deterministic out
 * of `@archava/pricing` reading a pricebook off disk, and the pricebook is the
 * authority — a browser cannot hold it without shipping it, and shipping it
 * would let anyone read the whole book and argue with the quote. So intake goes
 * up as JSON and the numbers come back the same way, which is also why this is
 * the only route in the app: everything else a page does runs in the visitor's
 * own browser, against a graph the visitor's own page produced.
 *
 * What this route is not: stateful. A POST in, a config set out, nothing kept.
 * Two identical posts get two identical answers, which is the property that
 * makes a quote reproducible from a saved intake.
 */
import type { IncomingMessage, ServerResponse } from 'node:http'
import { dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import {
  loadPricebook,
  loadTemplates,
  TemplateRegistry,
  ClientConfigResolver,
  type Branding,
  type ClientConfig,
  type DeploymentPlan,
} from '@archava/config'
import {
  buildConfigSet,
  emitClientConfig,
  parseIntake,
  ClientConfigEmitError,
  IntakeError,
  type ConfigSet,
  type ConfiguratorIntake,
} from '@archava/configurator'
import { PricingEngine } from '@archava/pricing'

const appDir = dirname(fileURLToPath(import.meta.url))

/** A quote request is a form submission, not an upload. 1 MiB is generous. */
const MAX_BODY_BYTES = 1 << 20

/** What the browser sends, before anything has checked it. */
interface QuoteRequestBody {
  readonly intake?: unknown
  readonly branding?: Branding
  /** `YYYY-MM-DD`. Absent leaves it out, because a clock is the caller's to own. */
  readonly updatedAt?: string
  readonly allowedOrigins?: readonly string[]
}

/** What comes back, minus the envelope. */
interface QuoteResult {
  readonly set: ConfigSet
  readonly config: ClientConfig | null
  readonly deploymentPlan: DeploymentPlan | null
}

class BodyTooLargeError extends Error {
  constructor(readonly limit: number) {
    super(`Request body exceeds the ${limit} byte limit.`)
    this.name = 'BodyTooLargeError'
  }
}

/**
 * The engine, built once and read many times.
 *
 * `loadPricebook` reads a file synchronously, and it walks upward from here
 * (`apps/web/src/server`) until it finds `config/pricing.v1.json` — so the route
 * works from any working directory and no path is duplicated in this file. It is
 * memoised rather than built per request because the pricebook does not change
 * while the server runs, and a route that re-read and re-parsed it per POST
 * would make a quote cost a disk read.
 */
let engine: PricingEngine | null = null

function pricebookEngine(): PricingEngine {
  if (engine === null) {
    engine = new PricingEngine(loadPricebook(appDir))
  }
  return engine
}

/**
 * Answer `POST /api/quote` with the priced config set for one intake.
 *
 * Validation is at the boundary, and it fails closed in the only direction that
 * matters: an intake the configurator cannot parse is a `400` with the issues,
 * and a pricebook that cannot be read is a `500` with no numbers in it. A quote
 * missing a component would be worse than no quote, because it would look like
 * an answer.
 */
export async function answerQuote(
  request: IncomingMessage,
  response: ServerResponse,
): Promise<void> {
  if (request.method !== 'POST') {
    respond(response, 405, {
      error: { message: 'POST only. A quote is a submission, not a fetch.' },
    })
    return
  }

  let raw: string
  try {
    raw = await readBody(request)
  } catch (error) {
    if (error instanceof BodyTooLargeError) {
      respond(response, 413, { error: { message: error.message } })
      return
    }
    respond(response, 400, { error: { message: messageOf(error) } })
    return
  }

  let body: unknown
  try {
    body = JSON.parse(raw) as unknown
  } catch {
    respond(response, 400, { error: { message: 'The request body is not valid JSON.' } })
    return
  }

  const payload = asBody(body)
  if (payload === null) {
    respond(response, 400, { error: { message: 'The request body must be a JSON object.' } })
    return
  }

  let intake: ConfiguratorIntake
  try {
    intake = parseIntake(payload.intake)
  } catch (error) {
    if (error instanceof IntakeError) {
      respond(response, 400, {
        error: { message: 'The intake is not valid.', issues: error.issues.map(String) },
      })
      return
    }
    respond(response, 500, { error: { message: messageOf(error) } })
    return
  }

  let set: ConfigSet
  try {
    set = buildConfigSet(intake, pricebookEngine())
  } catch (error) {
    respond(response, 500, { error: { message: messageOf(error) } })
    return
  }

  // A config is emitted only when the caller supplied what emission cannot
  // derive: branding, and a date. Asking for a quote is not asking for a config,
  // and a route that emitted one anyway would invent a name and a date the
  // client never gave.
  let config: ClientConfig | null = null
  let deploymentPlan: DeploymentPlan | null = null
  if (payload.branding !== undefined && payload.updatedAt !== undefined) {
    try {
      config = emitConfig(set, payload)
      deploymentPlan = new ClientConfigResolver(new TemplateRegistry(loadTemplates(appDir))).plan(
        config,
      )
    } catch (error) {
      respond(response, 400, {
        error: {
          message: messageOf(error),
          ...(error instanceof ClientConfigEmitError ? { issues: error.issues.map(String) } : {}),
        },
      })
      return
    }
  }

  respond(response, 200, { set, config, deploymentPlan } satisfies QuoteResult)
}

/** Emit the selected variant's scope as a tenant config. */
function emitConfig(set: ConfigSet, payload: QuoteRequestBody): ClientConfig {
  const selected = set.variants.find((entry) => entry.variant === set.selected) ?? set.variants[0]
  if (selected === undefined) throw new Error('the configurator produced no variant to emit')
  if (payload.branding === undefined || payload.updatedAt === undefined) {
    throw new Error('emission needs both branding and an updatedAt date')
  }
  return emitClientConfig(selected.scope, {
    branding: payload.branding,
    updatedAt: payload.updatedAt,
    ...(payload.allowedOrigins === undefined
      ? {}
      : { allowedOrigins: [...payload.allowedOrigins] }),
  })
}

/** Read a request body, refusing anything larger than the limit outright. */
function readBody(request: IncomingMessage): Promise<string> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = []
    let size = 0

    request.on('data', (chunk: Buffer) => {
      size += chunk.byteLength
      if (size > MAX_BODY_BYTES) {
        reject(new BodyTooLargeError(MAX_BODY_BYTES))
        request.destroy()
        return
      }
      chunks.push(chunk)
    })
    request.on('end', () => {
      resolve(Buffer.concat(chunks).toString('utf8'))
    })
    request.on('error', reject)
  })
}

/** A body that is not an object is not a request, whatever it parses as. */
function asBody(raw: unknown): QuoteRequestBody | null {
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) return null
  // Every field is optional, so an object already is a body; what was worth
  // checking was only that it is one object and not a list of them.
  return raw
}

function respond(response: ServerResponse, status: number, payload: unknown): void {
  response.statusCode = status
  response.setHeader('content-type', 'application/json; charset=utf-8')
  response.end(JSON.stringify(payload))
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}
