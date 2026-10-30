# Frontier Agentic Crates: MCP, Evals, Campaigns, & RPA

> **Technical Architecture Specification**  
> Covers the 4 advanced autonomous capabilities integrated into Archava's pure-Rust kernel: Native Model Context Protocol (`archava-mcp`), Synthetic Evals Harness (`archava-evals`), Proactive Operations (`archava-campaigns`), and Computer Use / RPA (`archava-rpa`).

---

## 1. Native Model Context Protocol (`crates/mcp`)

Standardized under the Linux Foundation Agentic AI Framework (AAIF), the **Model Context Protocol (MCP)** provides open JSON-RPC 2.0 connectivity between Archava's RLS-protected database and frontier agents (Claude Desktop, Cursor, OpenAI, custom assistants).

### Architectural Highlights
* **Zero External Node/Python Runtime:** Pure Rust JSON-RPC 2.0 parsing and dispatching.
* **Bi-directional Capability:**
  * **`ArchavaMcpServer`:** Exposes typed ERP operations (`archava_query_stock`, `archava_post_gl_journal`, `archava_dispatch_flight_mission`, `archava_query_memory`) and virtual resources (`archava://tenants/{slug}/ledgers/gl`).
  * **`McpClientGateway`:** Enables Ava to connect to external MCP servers (Shopify, Google Drive, Jira, Slack) dynamically.
* **RLS & Authority Gating:** MCP queries are pinned to the tenant caller's `TenantSlug` and enforce L1-L5 authority levels.

```text
External Agents (Claude / Cursor / IDE)
                 │
                 ▼ JSON-RPC 2.0 (stdio / HTTP SSE)
┌───────────────────────────────────────────────┐
│              Archava MCP Server               │
│  - Tool Registry: Tools / Resources / Prompts │
│  - Authority Gate: L1-L5 Gating               │
│  - Tenant Scoping: RLS Context Enforcement    │
└───────────────────────┬───────────────────────┘
                        │
                        ▼
           Archava Ledger / ERP Engines
```

---

## 2. Automated Agentic Evals & Synthetic Red-Teaming (`crates/evals`)

Enterprise deployments cannot rely on vibes. `archava-evals` executes continuous adversarial simulations against Ava and the ERP kernel to measure safety, financial consistency, and resilience to jailbreaks.

### Adversarial Attack Vectors Tested
1. **Prompt Injection / Jailbreak Inversion:** System prompt extraction attempts, boundary parsing exploits, and instruction overrides.
2. **Financial Ledger Imbalance:** Attempts to create unbalanced journal entries ($\sum \text{Debit} \neq \sum \text{Credit}$).
3. **Privilege Escalation:** Low-authority users (L1/L2) attempting to trigger L5 transactions without owner signatures.
4. **Bi-Temporal Memory Drift:** Forging historical timestamps to corrupt Orbz fact graphs.

### Output Metrics
* `eval_score`: Overall compliance percentage ($0.0 - 1.0$).
* `financial_invariants_adhered`: Strictly enforced ($1.00$ / 100%).
* `data_leakage_detected`: Zero tolerance ($0.00$ / 0%).

---

## 3. Proactive Operations & Autonomous Campaigns (`crates/campaigns`)

Traditional ERPs are passive databases waiting for user input. Archava Proactive Campaigns continuously listen for business trigger signals and formulate automated execution plans.

### Trigger Sources
* **`OverdueInvoice`:** Identifies outstanding accounts receivable.
  * Formulates localized WhatsApp collection message.
  * Dynamically renders personalized instant QRIS payment link.
  * Gating rule: Receivables $> \text{Rp } 10.000.000$ automatically route into an **Ava Cockpit Holding Pattern** requiring human approval.
* **`LowStockAlert`:** Evaluates current inventory against reorder points ($Q \le \text{reorder\_point}$).
  * Drafts supplier Purchase Order (PO) with suggested reorder quantity.
  * Held in Cockpit until verified by warehouse manager.
* **`DormantCustomer`:** Identifies customers with no purchases in $> 60$ days and drafts tailored win-back voucher campaigns.

---

## 4. Computer Use & Headless Vision RPA Bridge (`crates/rpa`)

Legacy institutions (Indonesian DJP Coretax, corporate banking like KlikBCA, BPJS) lack standardized open APIs. `archava-rpa` gives Ava vision-guided robotic process automation to interact with legacy portals securely.

### Security Guarantees
* **Strict Domain Allowlist:** Only sanctioned domains (`pajak.go.id`, `klikbca.com`, `bpjs-kesehatan.go.id`) are executable. Untrusted URLs are rejected before dispatch.
* **Automated Credential Masking:** Passwords, PINs, and session tokens are masked with `[REDACTED_CREDENTIAL]` across all execution logs and audit trails.

### Pre-configured Industrial Recipes
1. **DJP Coretax e-Faktur Sync (`plan_djp_efaktur_download`):** Navigates to DJP tax portal, navigates to monthly VAT reports, extracts e-Faktur table, and downloads validated XMLs.
2. **Bank Statement Scrape (`plan_bank_statement_scrape`):** Navigates to KlikBCA / Corporate Internet Banking, opens account mutations, extracts transaction tables, and downloads CSV/MT940 for automatic double-entry GL reconciliation.

---

## 5. Verification Commands

```bash
# Test MCP JSON-RPC Server & Gateway
cargo test -p archava-mcp --lib

# Run Synthetic Adversarial Red-Teaming Suite
cargo test -p archava-evals --lib

# Test Proactive Operational Campaigns
cargo test -p archava-campaigns --lib

# Test Vision RPA Sandbox & Allowlist
cargo test -p archava-rpa --lib
```
