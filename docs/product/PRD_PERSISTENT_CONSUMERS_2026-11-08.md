# Configured persistent consumer seam — 8 November 2026

[The persistent-consumer helper](../../tools/rust/persistent-consumers.mjs)
builds two independent Rust clients from identical packaged Core/Domain archives,
then sends their actual envelopes to separate native servers and disposable
databases. This extends the [pure contract evidence](PRD_COLD_CONSUMERS_2026-11-08.md);
it is a scoped application-composition proof, not complete K04/K05 delivery.

Run `target/prd-completion-20261107/persistent-consumers-02/result.json` exited 0
and reports verified true. Its build and persistent result hashes are bound in
that result. The current serving-source snapshot was
`e638ed2ec933e55b29b66328d1b73f15db6269f7f02a5718379fdfa52f28c028`.
The actual native distribution was
`dist/native-candidate-20261108-prd-purchase-posting-02`, source
`7b9ea46f8dbe087c5fc6591971f24ba7de05b0d74f4077f9f64ac48c9e8a5516`.
Their complete portable Core/Domain, root dependency/lock and toolchain inputs
were compared before packaging and match; the newer UI edits are not accepted
by this older native binary's API evidence.

Both clients use identical archive bytes, separate build roots and targets,
minimal locked manifests and Cargo offline. Their dependency trees exclude Host,
Adapters, Slice, Axum, SQLx, Leptos, Reqwest and Tokio. Four client tests pass;
the transport port uses standard input/output and the owned driver handles HTTP.
No private environment or source-workspace imports enter either client.

Eleven persistent scenarios pass:

- Each native installation previews and explicitly confirms different Owner
  pipeline captions, persists the revision and replays the original apply receipt.
- Each cold client creates a real contact, replays its original receipt, creates
  an opportunity and moves the canonical stage to `qualified`. Both the native
  record and client view retain the reviewed labels and configuration revision.
- A forged client observation (`proposal` while the database is `qualified`) and
  a stale revision each receive 409 without changing the stored stage/revision.
- The agency client creates a real Projects record. The wholesale client creates
  a Commerce order linked to the actual CRM contact/revision. Native setup of
  the wholesale product is explicit fixture setup.
- Current CRM membership revocation refuses an old command replay with 403.
  Restoring that membership and restarting the actual native binary preserves
  the original receipt, contact digest, canonical stage and Owner captions.
- Cross-tenant access receives 403, the other database's contact receives 404,
  and a session signed by the other installation receives 401. Each database
  contains exactly one contact and no implicit financial book/journal.

Native artifact hashes and `archava-ops verify-release` pass before execution.
Both native servers and PostgreSQL stop; cleanup exits 0 and both build/runtime
scratch directories are removed. Exact client sources, manifests, locks,
archives, binaries, reviewed configs, server logs and results remain in the
new evidence directory. The earlier exploratory apply-replay assertion compared
the response's changing `replayed` flag; it was corrected to check both flags
and the identical immutable receipt, with the failure retained separately.

This is **not** a completed agency delivery or wholesale business application.
Full business workflows, manifest/upgrade combinations, industry screens,
financial lifecycle, actual selected profiles and customer acceptance remain.
Owner UI/chat tests are separate browser evidence. No live provider, shared
database, public deployment or real customer action was used.
