# Choosing Archava's visual character

This study explores the user's feedback that the calmer workspace still feels too common. It does not modify the Rust application, its stylesheet, backend, or avatar template.

The user initially selected **C: Percakapan**, then supplied a dark business-dashboard reference and requested both appearances. The current implementation combines its relationship layout with a charcoal-and-amber palette and sidebar; see [workspace appearance](WORKSPACE_APPEARANCE.md). The study below remains the original comparison with fictional records.

[Open the interactive comparison](ARCHAVA_VISUAL_DIRECTIONS.html). Switch A/B/C and light/dark using the controls at the top. The HTML contains its own fonts and license notices and does not need a server or network connection.

All directions use the same fictional contacts and task, so the comparison concerns composition, typography, density, and material. Search, contact selection, filtering, and preview dialogs work locally. Example conversations and tasks are design content, not evidence of existing production features.

| Direction | Character | What changes | Desktop |
| --- | --- | --- | --- |
| A: Editorial | Open, restrained, mineral green | Broad table, horizontal navigation, lighter visual structure | [Preview](visual-directions/editorial.png) |
| B: Presisi | Precise, sturdy, cool grey with muted copper | Compact rail, closer data grouping, geometric typography, anchored record pane | [Preview](visual-directions/presisi.png) |
| C: Percakapan | Personal, spacious, quiet ink blue | People index, larger names, more prominent conversation context | [Preview](visual-directions/percakapan.png) |

Suggested comparison: C gives Archava a clearer connection to conversations and relationships; B offers a firmer direction for dense operational screens. This is a design judgment, not user research or a claim that either aesthetic is unique in the market.

For feedback, the user only needs to identify what feels closer: the name scale, row density, navigation, record pane, or color. Mixing one coherent direction with a small number of details from another is possible after that preference is clear. No familiarity with ERP/CRM conventions is required.

## Boundaries and checks

- Product files remain unchanged by this study.
- No provider, backend, payment, or messaging calls are made.
- Checked all three directions in light and dark at 390, 768, and 1440px: 18 layout checks without page overflow or JavaScript page errors.
- Verified filtering, search, empty results, record selection, dialog opening, Escape dismissal, and theme switching.
- Desktop, mobile, and dark screenshots are saved beside this document.
- This is a visual study, not a production UI, an accessibility certification, or a migration plan.

The next implementation should keep the project's existing Leptos/Rust stack and real data/permission contracts. Visual exploration does not authorize replacing backend data with the fictional study records.
