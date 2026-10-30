# Archava frontend

The website and workspace implement the direction in [design.md](../design.md). Both run inside the existing `@archava/web` app, with TypeScript, esbuild, and native browser UI. No new runtime dependency or framework migration is required.

## Open locally

From the repository root:

```sh
pnpm dev
```

- Website: <http://127.0.0.1:4173/archava>
- CRM workspace: <http://127.0.0.1:4173/workspace>
- Pipeline: <http://127.0.0.1:4173/workspace?view=pipeline>
- Operations concept: <http://127.0.0.1:4173/workspace?module=erp>

`PORT=4174 pnpm dev` uses port 4174 when 4173 is occupied. The dev server compiles scripts on startup; restart it after TypeScript edits. HTML and CSS are read from source on each request.

To preview a production bundle locally:

```sh
pnpm build:web
PORT=4174 pnpm start
```

The existing reference assistant at `/`, configurator at `/studio`, and `/api/quote` continue to use their original implementation.

## What you can try

The website has a chrome SVG hero, pointer tilt, orbit animation, reveal, a scroll story with three chapters, keyboard-accessible Chat/Voice/Human tabs, a scripted chat, a visual voice simulation, an existing Archava portrait, FAQs, and a local enquiry brief form. It supports system reduced motion and a manual motion preference.

The workspace includes:

- Overview with totals derived from the sample records, pipeline distribution, open tasks, and activity.
- Pipeline board/list, text search, stage/owner filters, record details, editable contact information, and stage changes.
- Contacts, follow-ups, completion controls, sample conversations, and unsent reply drafts.
- Sample knowledge documents that can be added and edited locally.
- AI follow-up proposals with a review step, cancellation, and approval. One proposal creates at most one sample task per record, including after reload.
- Orders, inventory, and invoice screens for the ERP concept, with record details and local order status changes.
- Four industry presets with separate local data, light/dark themes, command search (`Ctrl/Cmd + K`), mobile navigation, and explicit reset of the selected industry's sample data.

## Data and integration boundary

Names, amounts, orders, stock, invoice statuses, conversations, and assistant proposals are fictional. Monetary values are demo records, not the Archava pricebook or client quotes.

Browser storage keeps the demo per industry. Stored data is validated before restoration; an unsupported or damaged dataset falls back to the appropriate seed. If browser storage is unavailable, the interface reports that changes cannot persist.

Website briefs and reply drafts are saved locally and are not sent. Knowledge documents are not indexed for live retrieval. Voice uses a visual simulation without microphone access; Human shows a portrait without starting a live avatar session.

Frappe CRM, ERPNext, model providers, realtime voice/avatar, authentication, and server permissions are not connected to these new frontend pages. Connection cards show their actual `Not connected` status. These screens do not issue invoices, post a ledger, send messages, change warehouse stock, or make payments.

## Files

| Responsibility                                          | Source                                                          |
| ------------------------------------------------------- | --------------------------------------------------------------- |
| Website markup                                          | [archava.html](../apps/web/pages/archava.html)                  |
| Website interactions                                    | [marketing.ts](../apps/web/src/frontend/marketing.ts)           |
| Workspace shell                                         | [workspace.html](../apps/web/pages/workspace.html)              |
| Workspace views and interactions                        | [workspace.ts](../apps/web/src/frontend/workspace.ts)           |
| Demo schema, presets, storage validation, and proposals | [data.ts](../apps/web/src/frontend/data.ts)                     |
| Shared DOM/formatting helpers                           | [ui.ts](../apps/web/src/frontend/ui.ts)                         |
| Website and shared visual system                        | [archava.css](../apps/web/public/archava.css)                   |
| Workspace themes and layout                             | [workspace.css](../apps/web/public/workspace.css)               |
| Page, bundle, and guarded static asset routes           | [server.ts](../apps/web/src/server/server.ts)                   |
| Persistence/proposal regression tests                   | [frontend-data.test.ts](../apps/web/test/frontend-data.test.ts) |

The chrome sculpture is the original SVG from the design study. The portrait comes from the existing `Archava Template/public/assets/archava_hero_mobile.webp`. Space Grotesk, DM Sans, and JetBrains Mono are self-hosted variable Latin fonts; their SIL Open Font License files are included beside the font files in `apps/web/public/assets/fonts/`. Font sources are [Google Fonts](https://github.com/google/fonts), with downloads served by Google Fonts. The frontend doesn't request external fonts at runtime.

## Validation

```sh
pnpm typecheck:web
pnpm lint
pnpm test
pnpm structure
pnpm build:web
```

The new regression tests cover restoring valid edits, rejecting malformed records/duplicate IDs/orphan tasks, industry fallback, proposal idempotency after reload, unavailable records, and open pipeline totals.

With the app running, `pnpm e2e:frontend` checks form submission/cancellation, filtering, reload persistence, stage updates, approval idempotency, safe document rendering, and mobile navigation through the actual served DOM. It uses the existing CDP harness; set `E2E_BROWSER` to your Chromium/Chrome/Brave executable and `BASE` to the running app URL when needed. Its default isolated profile is `/tmp/archava-frontend-e2e/profile`, on debug port 9337.

Production Core Web Vitals, assistive technology coverage, authenticated multi-user behavior, and integrations against live CRM/ERP systems require a later implementation and validation phase.

During implementation, lint, web typecheck, build, workspace structure, and all 821 unit tests passed. The original reference-page browser regression passed 31/31 checks; the committed frontend browser regression passed 16/16 checks. A separate Chromium inspection of the production bundle covered website interactions, forms, local persistence, approval, task completion, draft/document storage, industry isolation, themes, command search, history, operations records, mobile navigation, and unavailable storage. All 11 pages fit 360, 390, 768, 1024, 1440, and 1920px without horizontal page overflow; there were no JavaScript page errors, failed resources, or external network requests.
