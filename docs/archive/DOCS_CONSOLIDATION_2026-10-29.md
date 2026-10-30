# Documentation consolidation — 29 October 2026

Archava documentation now has **six top-level folders and two root files**. Overlapping current pages were consolidated, earlier CRM/provider contracts moved into the archive, and developer entry points follow the canonical locations.

```text
docs/
├── README.md
├── DEVELOPER_GUIDE.md
├── architecture/
├── guides/
├── modules/
├── product/
├── design/
└── archive/
```

## Canonical pages

| Previous documents | Current home |
| --- | --- |
| Root architecture overview + architecture index | [Architecture](../architecture/README.md) |
| Host guide + embed guide + integration index | [Host and browser integration](../guides/INTEGRATION.md) |
| Plans/research indexes and their collections | [Product requirements and research](../product/README.md) |
| Jev provider research + SHADOW evaluation readiness | [Jev research](../product/JEV_RESEARCH.md) |
| Root build status | [Frozen verification checkpoint](audits/BUILD_STATUS_2026-10-29.md); the current docs index links the latest consolidation checks |
| Audit, history, migration and earlier contracts | [Archive index](README.md) |

The 16 module contracts remain separate because each owns distinct business rules. Six subsystem references remain beside the combined architecture overview. Product requirements and research retain explicit status/date boundaries; consolidation does not establish implemented scope or revalidate vendor claims.

The four groups above absorb **nine source pages/indexes into four canonical pages**, removing five overlapping page locations. The input had 519 files; the final tree has **517 files**, including the new archive index, this record, and a single relocation/check manifest. File count includes screenshots, raw logs and structured evidence, which are preserved rather than merged into prose.

## Preservation and navigation

**408 original frozen files remain byte-identical**, including audit proof, historical narratives, migration records, the ten legacy contract bodies and the previous build-status checkpoint. Collection indexes are maintained separately and have updated destinations. No frozen report was relabelled as a new pass, and no old script was executed.

[relocations-2026-10-29.json](relocations-2026-10-29.json) maps every one of the 519 input paths to its current file, hash and disposition. `defaultFragment` identifies the section for a merged guide. Resolve a relative link in a frozen body against its `originalPath` before looking up its destination: archive bodies intentionally retain their original path vocabulary. Use the maintained collection indexes to browse current destinations directly.

The previous [492-file audit](audits/ARCHAVA_DOCS_AUDIT_2026-10-29.md), including its historical hashes and paths, is preserved at its original checkpoint. This relocation manifest supplies the next mapping step; its contents were not rewritten to pretend the later layout existed then.

README, contributing, deploy/configuration guides, Cargo comments and Rust documentation references were updated. Rust source edits are comments only; parsed Cargo manifest values are unchanged. Two already-missing ERP acceptance-matrix citations were removed from comments while retaining their available known-differences reference.

## Verification

| Check actually run | Result |
| --- | --- |
| Maintained local navigation | 60 Markdown files including repository entry guides; 646 file/heading links, zero problems |
| Node 22.22.3 helper regressions | 15 tests passed |
| Repository boundaries | 22 crates, zero problems |
| Rust formatting | `cargo fmt --all --check` passed |
| Original preservation | 408 frozen files, all 129 original JSON and all 49 PNG retain their SHA-256 |
| Relocation coverage | All 519 original paths resolve to a current file with matching recorded final hash |
| Product logic / Cargo values | Non-comment Rust lines identical; parsed TOML values identical |
| Final layout / JSON | Six folders, two root files, 517 total files; 130 JSON files parse |
| Stale paths / whitespace | No old fully-qualified docs paths in maintained files; scoped `git diff --check` passed |

There are **43 working Markdown pages outside the archive**. Archived navigation/report overviews and external repository entry guides are also checked. Exact command output and the preservation dispositions are stored in the relocation manifest above. CI already runs the docs checker and helper suite; remote CI was not run here.

This follow-up does not rerun Rust application tests, database workflows, browser acceptance, external integrations or deployment. Earlier application-check results remain scoped to their dated checkpoint. No task-owned background services were started.
