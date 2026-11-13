# Enterprise engine source map

> **Status: implementation reference — reviewed 29 October 2026.** This page maps source files; it does not certify financial, tax, medical, hardware, or external-provider compliance. The [previous specification](../archive/history/subsystem-drafts-2026-10-29/architecture/ENTERPRISE_ENGINES.md) is preserved as a dated draft.

## Domain and protocol components

| Area | Source | Read this boundary carefully |
| --- | --- | --- |
| Journal rules and reports | [general_ledger.rs](../../crates/domain/src/erp/general_ledger.rs) | Library validation/calculation is distinct from an authorized persisted posting workflow |
| Tax calculations and serialization | [tax_indonesia.rs](../../crates/domain/src/erp/tax_indonesia.rs) | Constants/formulas in source are not verification of current rules or authority acceptance |
| Inventory costing | [inventory_costing.rs](../../crates/domain/src/erp/inventory_costing.rs) | Costing calculations do not by themselves prove stock reservation/receipt concurrency |
| ESC/POS bytes | [escpos.rs](../../crates/domain/src/hardware/escpos.rs) | Byte generation is distinct from transport, printer compatibility, and successful device output |
| Telematics parsing | [telematics module](../../crates/domain/src/telematics/mod.rs) | Parser support does not establish deployment against every named device/protocol variant |
| WhatsApp payload models | [whatsapp.rs](../../crates/domain/src/messaging/whatsapp.rs) | Payload construction/parsing is distinct from authenticated live delivery and reconciliation |
| Mission/task components | [Cockpit reference](COCKPIT_TASK_ARCHITECTURE.md) | In-memory engine and host SQL paths have different persistence guarantees |
| Memory components | [Orbz reference](ORBZ_MEMORY_ARCHITECTURE.md) | Formatting tiers and token estimates are not measured percentage savings |

## Wiring and product scope

The [domain module registry](../../crates/domain/src/erp/mod.rs), [HTTP registry](../../crates/host/src/erp/mod.rs), [main router](../../crates/host/src/router.rs), and [persistent bootstrap](../../crates/slice/src/bin/archava_server.rs) establish different parts of the implementation. Check all relevant layers before publishing a feature as an end-to-end product capability.

The current Operations UI uses real authorized projections and mutations for catalog/sales, purchasing, invoices and support, plus read-tool missions. This does not establish operator screens for the other ledgers or live accounting/hardware/messaging integrations. Module specifications are indexed under [modules](../modules/README.md), with current Rust source links and historical acceptance context.

## Evidence and ownership

Add business calculations to the domain layer, transactional persistence to adapters, and session/grant checks to the host. Record tests and external conformance results with their exact scope and date. Revalidate time-sensitive tax/provider requirements through primary sources before making current compliance claims; this documentation audit did not perform that external verification.
