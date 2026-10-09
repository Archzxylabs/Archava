/**
 * Postmark, behind {@link EmailGateway}.
 *
 * The template, the sender and the recipient are not this file's to choose.
 * They arrive inside the existing `EmailSendRequest`, from a tenant resolver
 * reading server configuration (§9 and §18 already checked them), and this
 * adapter sends exactly those and nothing else. A model-written body, a
 * browser-chosen sender and a mid-flight recipient are all outside its
 * vocabulary: it writes only the fields Postmark requires for a template send.
 *
 * What this file is *for* is the answer to one question, honestly. Postmark
 * says three distinguishable things about a send — a `MessageID` it issued, a
 * refusal it described, and silence — and only the first is an acceptance:
 *
 * 1. **`accepted`, only with the id Postmark issued.** A success returns
 *    `MessageID` alongside `ErrorCode: 0`. That id is the receipt, and it is
 *    the only thing that makes a replay answerable without asking the provider
 *    again. A 200 whose payload carries no usable id is not an acceptance, and
 *    an id this package re-derived or invented would be worse than none.
 * 2. **`rejected` is Postmark answering no** — an unconfirmed sender, an
 *    unknown template, a malformed field, a rate limit. It said something this
 *    adapter can verify in the official documentation, and what it said was no.
 * 3. **Everything else is `unknown`.** A timeout, a connection that never
 *    established, a body that is not JSON, a 500 or a 503. Postmark's API
 *    overview documents a 500 as "an issue with Postmark's servers processing
 *    your request" and a 503 as "planned service outages" — neither says
 *    anything about whether this message was accepted, so neither becomes a
 *    refusal here. Silence stays silence.
 *
 * What this adapter will not do is send twice. `EmailOutboxBoundary` owns the
 * at-most-once rule: it claims the key, calls this gateway once, and settles
 * the attempt from whatever comes back. A retry here would be a second message
 * into a real mailbox, so there is no retry, no fallback request, and no second
 * body variant on failure — the outcome goes back as `unknown` and the boundary
 * decides what a replay may look at.
 *
 * **On the idempotency key.** Postmark documents no idempotency-key field, so
 * there is nowhere honest to put one. The upstream raw key is still transformed
 * — keyed, opaque, single-purpose — before it reaches any provider field, and
 * it travels as a metadata *label*: searchable in Postmark's outbound search,
 * and not something Postmark promises to deduplicate on. The at-most-once
 * guarantee stays with the attempt store, where it already lives.
 *
 * **On the server token.** Injected, per process, by a server that read it from
 * a secret store. It is never part of an error message, a reason string or a
 * log this package writes — only the one header Postmark requires. No
 * `process.env` read happens here, so a bundle that somehow reached this module
 * would find nothing to leak, and constructing one without a token is refused
 * rather than silently attempted.
 */

import {
  isEmailAddress,
  isNonblank,
  isProviderMessageId,
  isTrustedTemplateShape,
  type EmailGateway,
  type EmailGatewayOutcome,
  type EmailSendRequest,
} from '@archava/act-email'
import { postmarkAttemptLabel } from './label.js'
import {
  defaultPostmarkTransport,
  type PostmarkHTTPRequest,
  type PostmarkTransport,
} from './transport.js'

/**
 * The one endpoint this adapter uses: "Send email with template".
 *
 * `POST /email/withTemplate`, confirmed against Postmark's templates API. The
 * docs write the path with and without a trailing slash and the host answers
 * either; the one sent is the one without it.
 */
export const POSTMARK_SEND_URL = 'https://api.postmarkapp.com/email/withTemplate'

/** The default message stream. Postmark defaults to `outbound` when omitted. */
const DEFAULT_MESSAGE_STREAM = 'outbound'

/** The metadata key the opaque attempt label travels under. Not a secret. */
export const POSTMARK_ATTEMPT_KEY = 'archava_attempt'

/**
 * The adapter's own bound, because Postmark documents none.
 *
 * Long enough that a slow-but-successful send is not discarded as a timeout —
 * which would report `unknown` for a message that really went out — and short
 * enough that a hung socket does not pin a turn open.
 */
export const POSTMARK_SEND_TIMEOUT_MS = 15_000

/** Reasons this adapter produced on its own. Machine codes, never provider text. */
const REASON = {
  /** The transport did not answer within the adapter's bound. */
  timeout: 'mail_provider_did_not_answer',
  /** The transport threw, or could not be reached. */
  unreachable: 'mail_provider_unreachable',
  /** The reply body was not JSON. */
  notJson: 'mail_provider_reply_unreadable',
  /** The reply parsed, but carried no `MessageID` on a success. */
  noMessageId: 'mail_provider_receipt_absent',
  /** The reply parsed, but is not a send response this adapter can name. */
  unrecognised: 'mail_provider_reply_unrecognised',
  /** The request is not one this adapter will put on the wire. */
  unusableRequest: 'mail_request_not_sendable',
} as const

/**
 * Refusals, keyed by the `ErrorCode` Postmark puts in the body.
 *
 * Every code here, its HTTP status and its wording, is taken from the API error
 * codes list in Postmark's API overview. Postmark answers a send *refusal with
 * a non-zero `ErrorCode` in the same schema as a success, which is why the code
 * is the discriminator and the HTTP status is only corroborating evidence.
 * A code that is not listed is not interpreted — "the provider declined" is a
 * claim, and this adapter only makes claims it has checked. A documented
 * non-refusal that arrives as a code (100, "offline for maintenance"; 101, "an
 * error that shouldn't have occurred") therefore falls through to `unknown`,
 * which is where an outage belongs.
 *
 * Note that 413 appears on both sides of the boundary meaning different
 * things: as an *HTTP status* it is a payload over the size limit, and as an
 * *ErrorCode* it is an account not yet approved to send. The two maps are kept
 * separate for exactly that reason.
 */
const REFUSAL_BY_CODE: Readonly<Record<number, string>> = {
  10: 'mail_provider_rejected_credential', // "Request does not contain a valid Server or Account token, or the wrong token type was used for the endpoint."
  300: 'mail_provider_rejected_request', // send validation: zero recipients, an invalid address
  402: 'mail_provider_rejected_request', // "Invalid JSON."
  403: 'mail_provider_rejected_request', // "Invalid request field(s)."
  406: 'mail_provider_rejected_recipient', // "Inactive recipient."
  410: 'mail_provider_rejected_request', // more than the 500 messages a batch request may carry
  411: 'mail_provider_rejected_request', // "Attachment file type not allowed."
  412: 'mail_account_not_approved', // pending approval: recipients must share the From address's domain
  413: 'mail_account_not_approved', // "This account is not approved to send email."
  422: 'mail_provider_rejected_configuration', // "Invalid Server or Account."
  1101: 'mail_provider_template_unknown', // "neither TemplateId nor TemplateAlias", or a template/alias/layout not found
  1105: 'mail_provider_rejected_configuration', // a server's active-template limit would be exceeded
  1235: 'mail_provider_stream_unknown', // "The stream provided does not exist on this server."
  1236: 'mail_provider_stream_unsupported', // "Sending is not supported for this stream type."
  1480: 'mail_provider_rejected_configuration', // "not authorized to send emails from your current IP address"
}

/**
 * Refusals, keyed by HTTP status, for a reply whose body carries no usable
 * `ErrorCode`. Every status here is one Postmark's API overview defines for its
 * endpoints.
 *
 * 429 is here because a rate-limited message is one Postmark explicitly did not
 * accept, and saying so is true rather than evasive.
 */
const REFUSAL_BY_STATUS: Readonly<Record<number, string>> = {
  401: 'mail_provider_rejected_credential', // "Missing or incorrect API token in header."
  413: 'mail_provider_rejected_request', // over the 10 MB Email API size limit
  415: 'mail_provider_rejected_request', // "missing the expected request headers"
  422: 'mail_provider_rejected_request', // "malformed JSON or invalid fields"
  429: 'mail_provider_rate_limited',
}

/**
 * Statuses this adapter will not interpret, and why, for the record.
 *
 * Postmark documents all three: 404 is "Entity doesn't exist", 500 is "an issue
 * with Postmark's servers processing your request", and 503 covers "planned
 * service outages". None is a statement about whether this particular message
 * was taken, so none of them is allowed to become a refusal here.
 */
const SILENT_STATUSES = new Set([404, 500, 503])

export interface PostmarkEmailGatewayOptions {
  /**
   * A Postmark server token, from that server's API Tokens tab. Server-level
   * privileges, because the send endpoint requires them. Injected: this module
   * never reads an environment variable itself.
   */
  readonly serverToken: string
  /** The network boundary. Defaults to {@link defaultPostmarkTransport}. */
  readonly transport?: PostmarkTransport
  /**
   * A server-only secret of at least 32 bytes, stable for as long as any
   * attempt can replay, used to derive the opaque attempt label. The key the
   * outbox already uses is a reasonable choice; a distinct one is also correct.
   */
  readonly attemptLabelKey: Uint8Array
  /** The message stream to send through. Defaults to `outbound`. */
  readonly messageStream?: string
  /** How long to allow the provider to answer. Defaults to {@link POSTMARK_SEND_TIMEOUT_MS}. */
  readonly timeoutMs?: number
}

/**
 * The Postmark gateway: one send per call, and one call per attempt.
 *
 * Stateless except for the credential and the label key. Two callers may share
 * one instance and interleave freely, because nothing about an attempt is held
 * on it — which attempt this is lives in the request, where the boundary put
 * it.
 */
export class PostmarkEmailGateway implements EmailGateway {
  /** Which gateway this is, for the audit trail. Not a secret. */
  readonly gatewayId = 'postmark:withTemplate@1'

  readonly #serverToken: string
  readonly #transport: PostmarkTransport
  readonly #attemptLabelKey: Uint8Array
  readonly #messageStream: string
  readonly #timeoutMs: number

  constructor(options: PostmarkEmailGatewayOptions) {
    if (!isNonblank(options.serverToken)) {
      // Refused rather than attempted: a request with no credential is a 401
      // Postmark would answer, and a 401 is a *refusal*, when the truth is that
      // the deployment never supplied a server token at all.
      throw new TypeError('A Postmark server token is required.')
    }
    if (
      !(options.attemptLabelKey instanceof Uint8Array) ||
      options.attemptLabelKey.byteLength < 32
    ) {
      throw new TypeError('A Postmark attempt label key of at least 32 bytes is required.')
    }
    if (
      options.timeoutMs !== undefined &&
      (!Number.isFinite(options.timeoutMs) || options.timeoutMs <= 0)
    ) {
      throw new TypeError('A Postmark send timeout must be a positive number of milliseconds.')
    }
    this.#serverToken = options.serverToken
    this.#transport = options.transport ?? defaultPostmarkTransport()
    this.#attemptLabelKey = Uint8Array.from(options.attemptLabelKey)
    this.#messageStream = options.messageStream ?? DEFAULT_MESSAGE_STREAM
    this.#timeoutMs = options.timeoutMs ?? POSTMARK_SEND_TIMEOUT_MS
  }

  async send(request: EmailSendRequest): Promise<EmailGatewayOutcome> {
    // The defensive half of the checks the executor and the resolver already
    // made, for a direct caller. An unapproved template or a sender belonging to
    // another tenant never reaches the provider.
    if (!sendable(request)) return { outcome: 'rejected', reason: REASON.unusableRequest }

    let reply: PostmarkReply
    try {
      reply = await this.postOnce(
        buildSendPayload(request, this.#messageStream, this.attemptLabel(request)),
      )
    } catch (error) {
      return unknown(error instanceof PostmarkCallFailure ? error.reason : REASON.unreachable)
    }
    return interpret(reply)
  }

  /**
   * The one and only network call, and it is raced.
   *
   * Started and not awaited: a transport that ignores its abort signal must be
   * *passed* rather than waited on, or a hung socket holds the attempt open
   * until Postmark eventually gives up on it. A transport that hangs therefore
   * still lands in the timeout branch, and a transport that throws still lands
   * in the unreachable one — the two are told apart by the failure that
   * reached this caller, not by which promise settled first.
   */
  private async postOnce(payload: string): Promise<PostmarkReply> {
    const controller = new AbortController()
    let timer: ReturnType<typeof setTimeout> | undefined
    const bounded = new Promise<never>((_resolve, reject) => {
      timer = setTimeout(() => {
        // The abort is what lets a real `fetch` release its socket; the race is
        // what bounds a transport that never answers at all.
        controller.abort()
        reject(new PostmarkCallFailure(REASON.timeout))
      }, this.#timeoutMs)
    })
    const request: PostmarkHTTPRequest = {
      url: POSTMARK_SEND_URL,
      headers: {
        accept: 'application/json',
        'content-type': 'application/json',
        'x-postmark-server-token': this.#serverToken,
      },
      body: payload,
      signal: controller.signal,
    }
    try {
      const sent = this.#transport.post(request)
      // A transport that loses the race and rejects later must not surface as
      // an unhandled rejection; the call has already been reported.
      void sent.catch(() => undefined)
      const reply = await Promise.race([sent, bounded])
      return { status: reply.status, body: parseJsonBody(reply.body) }
    } catch (error) {
      throw error instanceof PostmarkCallFailure
        ? error
        : new PostmarkCallFailure(REASON.unreachable)
    } finally {
      if (timer !== undefined) clearTimeout(timer)
    }
  }

  /**
   * The opaque label derived from the upstream raw idempotency key.
   *
   * A tenant-separated keyed digest, so the key that identified the attempt
   * to the rest of the system is not recoverable from it and never appears in
   * a provider field, a log line, or on an operator's Postmark dashboard.
   * It is a label, not a promise: Postmark documents no idempotency-key
   * support, so at-most-once is the attempt store's guarantee and stays there.
   */
  private attemptLabel(request: EmailSendRequest): string {
    return postmarkAttemptLabel(this.#attemptLabelKey, request.tenantId, request.idempotencyKey)
  }
}

/** A call that did not return a usable reply, carrying one machine reason. */
class PostmarkCallFailure extends Error {
  readonly reason: string

  constructor(reason: string) {
    super(reason)
    this.name = 'PostmarkCallFailure'
    this.reason = reason
  }
}

/** A reply, as it came back from the transport: the status and the parsed body. */
interface PostmarkReply {
  readonly status: number
  readonly body: unknown
}

function unknown(reason: string): { readonly outcome: 'unknown'; readonly reason: string } {
  return { outcome: 'unknown', reason }
}

/** The defensive half of the executor's own checks, for direct callers. */
function sendable(request: EmailSendRequest): boolean {
  const templateId = request.template?.providerTemplateId
  return (
    isNonblank(request.tenantId) &&
    isNonblank(request.idempotencyKey) &&
    isEmailAddress(request.recipient) &&
    isTrustedTemplateShape(request.template) &&
    request.template.tenantId === request.tenantId &&
    // A digit-only ID becomes a JSON number. Reject values that Number would
    // round into another tenant-approved template ID.
    (typeof templateId !== 'string' ||
      !/^[0-9]+$/.test(templateId) ||
      (Number.isSafeInteger(Number(templateId)) && Number(templateId) > 0))
  )
}

/**
 * The request body Postmark's send-with-template endpoint takes.
 *
 * Only what a template send requires, and nothing else. There is deliberately
 * no `HtmlBody`, no `TextBody` and no `Subject`: those come from the template
 * Postmark renders, and a body written here would be text this package has no
 * authority over. `TemplateModel` is required by the endpoint and is empty
 * because every value a template renders comes from the tenant's own template,
 * not from a model's output.
 *
 * `TemplateId` is an integer in the API and `TemplateAlias` a string, so the
 * provider-side template id is read as a number where it is one and sent as
 * the alias where it is not. The documented alternatives are sent alone: one
 * field per request.
 */
function buildSendPayload(
  request: EmailSendRequest,
  messageStream: string,
  attemptLabel: string,
): string {
  const template = request.template
  const body: Record<string, unknown> = {
    // One address, the one the request carried. Never a comma-joined list,
    // which would let a mistyped address become an undisclosed recipient.
    To: request.recipient,
    From: template.senderAddress,
    TemplateModel: {},
    MessageStream: messageStream,
    Metadata: { [POSTMARK_ATTEMPT_KEY]: attemptLabel },
  }
  if (/^[0-9]+$/.test(template.providerTemplateId)) {
    body['TemplateId'] = Number(template.providerTemplateId)
  } else {
    body['TemplateAlias'] = template.providerTemplateId
  }
  return JSON.stringify(body)
}

/**
 * Read a reply that crossed a boundary. Anything this package cannot name is
 * not an answer.
 *
 * Postmark sends the same schema for a success and for a documented refusal,
 * so the `ErrorCode` is the discriminator and the HTTP status corroborates it.
 * That ordering is not cosmetic: a 200 does not mean accepted — Postmark's own
 * batch documentation warns that a batch returns 200-level even when
 * individual messages fail validation — and a 401 does not mean the message
 * was refused, it means the request was.
 */
function interpret(reply: PostmarkReply): EmailGatewayOutcome {
  // A success-shaped body attached to a failed HTTP response is not a receipt.
  // In particular, Postmark documents 500/503 as server/outage failures.
  if (SILENT_STATUSES.has(reply.status)) return unknown(REASON.unrecognised)
  const send = sendResponse(reply.body)
  if (send !== null && send.errorCode !== 0) {
    const refused = REFUSAL_BY_CODE[send.errorCode]
    return refused === undefined
      ? unknown(REASON.unrecognised)
      : { outcome: 'rejected', reason: refused }
  }
  const refused = REFUSAL_BY_STATUS[reply.status]
  if (refused !== undefined) return { outcome: 'rejected', reason: refused }
  if (send === null) {
    // A status this adapter does not interpret, or a body that is not JSON.
    // Neither is a refusal, and neither is a receipt.
    return SILENT_STATUSES.has(reply.status) || reply.status !== 200
      ? unknown(REASON.unrecognised)
      : unknown(REASON.notJson)
  }
  // A success, and only with the id Postmark issued.
  if (reply.status !== 200) return unknown(REASON.unrecognised)
  if (!isProviderMessageId(send.messageId)) return unknown(REASON.noMessageId)
  return { outcome: 'accepted', providerMessageId: send.messageId }
}

/**
 * The reply body, parsed, or `undefined` when it is not JSON.
 *
 * A body that does not parse is neither a refusal nor a receipt, so it is
 * handed to {@link interpret} as no body at all — which is the only thing it
 * can honestly be told. Nothing about the raw text travels onward: a provider
 * error page is not a reason string, and this package does not quote one.
 */
function parseJsonBody(body: string): unknown {
  try {
    return JSON.parse(body) as unknown
  } catch {
    return undefined
  }
}

/** A body carrying the fields the send endpoint returns, or null when it is not one. */
interface PostmarkSendResponse {
  readonly errorCode: number
  readonly messageId: unknown
}

function sendResponse(body: unknown): PostmarkSendResponse | null {
  if (typeof body !== 'object' || body === null || Array.isArray(body)) return null
  const candidate = body as Record<string, unknown>
  const errorCode = candidate['ErrorCode']
  if (typeof errorCode !== 'number' || !Number.isInteger(errorCode)) return null
  return { errorCode, messageId: candidate['MessageID'] }
}
