# Archava workspace appearance

The current ERP/CRM direction is **Charcoal & Amber**, implemented on 28 October 2026 from the user's `Archava Dark Mode Business Dashboard.png` reference. It replaces direction C's ink-blue palette and horizontal navigation, while retaining its person index and real contact-context pane. The avatar template is outside this change.

Soft light remains the default. Dark mode uses neutral charcoal surfaces and warm amber accents. Both appearances use the same components, spacing, route hierarchy, and local fonts. The latest layout pass uses compact page headers, a wide primary work area, and a narrower context rail. Home no longer has a decorative hero. Contacts uses a denser person index and a wider record pane; Operations and Settings each have a layout suited to their existing controls.

## Preview

Screenshots show the actual Rust application with disposable fixture records.

| Screen | Light | Dark |
| --- | --- | --- |
| Home | [Desktop](workspace-layout-2026-10-28/home-light.png) | [Desktop](workspace-layout-2026-10-28/home-dark.png) |
| Home, mobile | [390px](workspace-layout-2026-10-28/home-light-mobile.png) | [390px](workspace-layout-2026-10-28/home-dark-mobile.png) |
| Contacts | [Desktop](workspace-layout-2026-10-28/contacts-light.png) | [Desktop](workspace-layout-2026-10-28/contacts-dark.png) |
| Contacts, mobile | [390px](workspace-layout-2026-10-28/contacts-light-mobile.png) | [390px](workspace-layout-2026-10-28/contacts-dark-mobile.png) |
| Operations | [Desktop](workspace-layout-2026-10-28/erp-light.png) | [Desktop](workspace-layout-2026-10-28/erp-dark.png) |
| Settings | [Desktop](workspace-layout-2026-10-28/owner-light.png) | [Desktop](workspace-layout-2026-10-28/owner-dark.png) |

Earlier visual studies and screenshots remain historical comparisons. They do not describe the current palette or shell.

## Before / after review

| Before | After | Why |
| --- | --- | --- |
| Ink blue and cool slate throughout | Warm off-white light mode; neutral charcoal dark mode; restrained amber accents | Applies the user's reference consistently across ERP/CRM |
| Horizontal navigation | Persistent desktop sidebar grouped into Workspace, CRM, ERP, and Tools | Gives the growing workspace a stable hierarchy |
| Contacts is the only overview entry | Home at `/app/home`, also linked from the wordmark | Provides an overview and direct routes into existing work |
| A conceptual reference with revenue charts and approvals | Actual accessible contact counts, a record directory, detail coverage, and working shortcuts | Avoids presenting unsupported financial totals or a fictional task inbox |
| Large hero followed by similarly weighted panels | Compact header, thin metric strip, wide relationship directory, and a narrow context rail | Makes Home priorities clear and brings working content into the first screen |
| Tall contact rows and equally wide columns | Compact index beside a record pane roughly twice its width on desktop | Leaves more room for the selected person, activity, and next steps |
| Search and creation controls occupy the contact list | One toolbar with keyboard-submitted search and a native add-contact disclosure | Makes both actions available without expanding the index |
| Follow-up actions sit below the activity | Actions beside the identity, above the supporting details | Makes the next step available before reading a long record |
| Industry navigation consumes a second sidebar | Locally scrolling horizontal industry selector above a full-width detail area | Gives Operations more room for its current sample figures |
| Settings is one long configuration card | Section navigation alongside modules, review, and revision history | Supports focused configuration and finding the review result |
| Cool-blue people index | Amber selection and a warm, readable context pane | Retains the selected relationship-focused layout |
| Nested main landmark in Operations | One document main with an ordinary content container | Keeps the page's landmark structure coherent |

## Tokens and typography

| Token | Light | Dark |
| --- | --- | --- |
| Canvas | `#f4f2ee` | `#181818` |
| Surface | `#fcfbf8` | `#202020` |
| Sidebar | `#eeebe4` | `#141414` |
| Subtle surface | `#efede7` | `#282827` |
| Text | `#302e2a` | `#eceae5` |
| Secondary text | `#706b61` | `#b5b4ac` |
| Accent | `#8b4b21` | `#e8ae78` |
| Input boundary | `#8a8274` | `#807d75` |

DM Sans is the 15px body face. Contact names in the compact index are 16px; the selected record uses a larger name between 27px and 36px on desktop. The new overview uses 12px supporting text and 14px directory names, with smaller metadata where appropriate. JetBrains Mono remains reserved for identifiers and diagnostics. Fonts and line icons are local; this change adds no UI dependency or external font service.

Calculated contrast on the main surface is 13.09:1 / 13.55:1 for main text, 5.12:1 / 7.83:1 for secondary text, and 6.47:1 / 8.57:1 for primary-action text, light / dark respectively. Input boundaries are 3.67:1 / 3.96:1 against the surface. These are token checks, not certification of all UI states or a medical claim about eye strain. [Calculated results](workspace-layout-2026-10-28/contrast.json).

## Component ownership and behavior

- `crates/slice/src/workspace.rs` owns the shell, grouped navigation, shared line icons, wordmark, and theme control. Navigation uses route-based `aria-current`. The sidebar only links to implemented screens.
- `crates/slice/src/home.rs` owns the read-only overview. Its composition is a compact heading, a thin metric strip, a wide relationship directory, and a narrower coverage/context rail, with distinct assistant and demo-operation entry points below the directory. It makes one session-scoped `GET /contacts?tenant=…&limit=20` request per load; it does not fan out into individual record requests or fetch the entire directory.
- Home counts describe that first accessible page. Organizations are distinct nonempty organization strings in that page, not a count of organization entities. A next-page cursor displays the explicit first-20 scope. At most five directory links appear on Home; Contacts retains pagination.
- Loading, signed-out, empty, refused, malformed, and unavailable states are distinct. A refused overview does not show zero counts. Refresh clears the previous view while loading. Generation and session checks ignore late responses and safely handle navigation during a request.
- `crates/slice/src/contacts.rs` owns the toolbar, keyboard-submitted search, native create disclosure, and scrollable person index; `contact_context.rs` reads the selected record through the existing `GET /crm/records` endpoint. It shows up to three actual activities and three open follow-ups, with explicit empty/refused states. The follow-up and full-record actions sit near the identity. At wide sizes the activity and next-step sections share the record pane; they stack as space narrows.
- Full-record and follow-up links preserve their route and revision contracts. The selected record pane ignores stale responses. Selecting a person brings the context pane into view and focuses it when the layout is stacked at 760px or narrower; the initial selection does not move the page.
- `crates/slice/src/owner.rs` presents Settings with in-page section navigation. A returned preview scrolls and focuses the review section; the existing preview/confirm state machine remains unchanged.
- `crates/slice/src/erp.rs` retains its explicitly labelled demo. The industry selector scrolls locally above the full-width detail view, preserving industry selection and the demo action.
- `static/workspace.css` owns shared tokens, components, and responsive behavior. Reuse these tokens when adding modules rather than introducing separate page palettes or appended override layers.
- `crates/slice/src/server.rs` embeds the stylesheet and serves the local fonts. Native servers must be rebuilt to pick up stylesheet changes; rebuild the hydration package when view markup changes.
- `archava.appearance` persists `light` or `dark` in local storage. A new browser starts light; reloads restore the selection during hydration. Storage failure still permits an in-memory theme toggle.
- At 900px or narrower, the sidebar becomes a native menu. Link selection and Escape close it. Inputs retain visible boundaries; focus indicators and reduced-motion handling remain in the shared stylesheet.

The Operations screen remains an explicitly labelled demonstration. This visual implementation does not create financial reporting, global task/approval queries, or production ERP command wiring. Existing CRM permissions, tenant isolation, concurrency, idempotency, and receipt recovery stay with their backend contracts.

## Verification

- Native server and fixture example built; hydration WASM built and its browser package generated.
- Slice Clippy passed with warnings denied; all five existing slice library tests passed; repository-wide formatting passed.
- Browser checked eight routes in both themes at 320, 360, 390, 768, 1024, and 1440px: **96 layouts**, without document overflow or JavaScript page errors.
- Verified real overview counts against the fixture API, record navigation, bounded pagination labels, empty/refused/malformed responses, signed-out behavior, and navigation during an overview request.
- Verified theme persistence, mobile menu dismissal including Escape, and focus movement to the selected contact.
- Checked the asymmetric Home column ratio, compact heading/metrics, wide Contacts context, tablet split view, and placement of the follow-up action above activity.
- Verified Enter submits the contact search and the add-contact form stays inside the mobile viewport.
- Switched to the last industry and ran its demo. Verified Settings section links and automatic focus on a real preview result.
- Created actual activity and a follow-up through the UI, received its receipt, and read the follow-up back in the context pane. Creating and opening a synthetic contact also passed.
- Controlled delayed-response, denied-read, empty-search, and unmount scenarios passed without stale contact content or page errors.
- Confirmed the native preview served the exact stylesheet bytes in the current source. The generated browser package matches the package used for verification.

[Browser evidence](workspace-layout-2026-10-28/verification.json), [build/check results](workspace-layout-2026-10-28/checks.json), and [verified UI source hashes](workspace-layout-2026-10-28/verified-ui-manifest.json).

Task-owned browsers, preview servers, and fixture databases are stopped after verification. No deployment, external provider calls, or production data changes are involved.

---

## Historical Visual Studies (Directions A/B/C)

Before the current Charcoal & Amber theme was established, an interactive comparison evaluated three candidate visual directions:

- [Interactive comparison study](ARCHAVA_VISUAL_DIRECTIONS.html) (standalone offline study with fictional records).

| Direction | Character | What changes | Desktop |
| --- | --- | --- | --- |
| A: Editorial | Open, restrained, mineral green | Broad table, horizontal navigation, lighter visual structure | [Preview](visual-directions/editorial.png) |
| B: Presisi | Precise, sturdy, cool grey with muted copper | Compact rail, closer data grouping, geometric typography, anchored record pane | [Preview](visual-directions/presisi.png) |
| C: Percakapan | Personal, spacious, quiet ink blue | People index, larger names, more prominent conversation context | [Preview](visual-directions/percakapan.png) |

Direction C provided the relationship-centric layout basis, which was subsequently synthesized with dark enterprise dashboard requirements to create the production Charcoal & Amber workspace.
