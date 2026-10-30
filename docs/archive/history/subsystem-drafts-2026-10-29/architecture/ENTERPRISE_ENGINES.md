# Archava Enterprise Engines Specification

> **Scope:** Detailed engineering reference for Archava's mathematical and hardware domain engines.  
> **Location:** `crates/domain/src/`

---

## 1. General Ledger & Double-Entry Accounting Engine

**Source:** [`crates/domain/src/erp/general_ledger.rs`](file:///home/haikaru/Archverse/Lab/Archava/crates/domain/src/erp/general_ledger.rs)

### Standard Chart of Accounts (COA)
| Account Code | Account Name | Class | Normal Balance |
| :--- | :--- | :--- | :--- |
| `10100` | Kas Utama (Cash on Hand) | Asset | Debit |
| `10200` | Bank BCA Operasional | Asset | Debit |
| `11100` | Piutang Usaha (Accounts Receivable) | Asset | Debit |
| `12100` | Persediaan Barang (Inventory Asset) | Asset | Debit |
| `13100` | PPN Masukan (Input VAT) | Asset | Debit |
| `20100` | Hutang Usaha (Accounts Payable) | Liability | Credit |
| `21100` | Hutang PPN Keluaran (Output VAT Payable) | Liability | Credit |
| `21200` | Hutang PPh 21 Karyawan | Liability | Credit |
| `21300` | Hutang Iuran BPJS | Liability | Credit |
| `30100` | Modal Disetor (Paid-in Capital) | Equity | Credit |
| `30200` | Laba Ditahan (Retained Earnings) | Equity | Credit |
| `40100` | Pendapatan Penjualan (Sales Revenue) | Revenue | Credit |
| `50100` | Harga Pokok Penjualan (COGS) | Cost of Goods Sold | Debit |
| `60100` | Beban Gaji & Upah (Salaries Expense) | Expense | Debit |
| `60200` | Beban Sewa & Utilitas (Rent & Utilities) | Expense | Debit |

### Invariant & Validation Rules
1. **Balance Guard:** Every journal entry must satisfy:
   $$\sum \text{Debit Lines} == \sum \text{Credit Lines}$$
2. **Posting Transition:** A journal begins in `Draft` state and transitions monotonically to `Posted`. Posted entries are immutable. Voiding an entry generates an offsetting reversal journal rather than mutating existing rows.
3. **Financial Statement Generation:**
   * **Trial Balance:** Aggregates line items by account code to confirm global debits equal credits.
   * **Balance Sheet:** Verifies that $\text{Assets} = \text{Liabilities} + \text{Equity}$.
   * **Profit & Loss:** Computes $\text{Net Income} = \text{Revenue} - \text{COGS} - \text{Expenses}$.

---

## 2. Indonesian Regulatory Tax & Coretax Engine

**Source:** [`crates/domain/src/erp/tax_indonesia.rs`](file:///home/haikaru/Archverse/Lab/Archava/crates/domain/src/erp/tax_indonesia.rs)

### Coretax e-Faktur Generator
* Generates official XML/JSON e-Faktur schemas conforming to DJP requirements.
* Requires 16-digit NPWP (incorporating NIK format for individuals).
* Computes standard PPN 11% (or dynamic 12% via `VatRate::Vat12Percent`).

### PPh 21 TER (Tarif Efektif Rata-Rata per PP 58/2023 & PMK 168/2023)
* **Categories:**
  * **Category A:** TK/0 (PTKP Rp 54.000.000), TK/1, K/0 (PTKP Rp 58.500.000).
  * **Category B:** TK/2, K/1 (PTKP Rp 63.000.000), TK/3, K/2 (PTKP Rp 67.500.000).
  * **Category C:** K/3 (PTKP Rp 72.000.000).
* **Sample Rate Brackets (Category A):**
  * Gross $\le \text{Rp } 5.400.000$: $0.0\%$
  * Gross $\text{Rp } 5.400.000 - 5.650.000$: $0.25\%$
  * Gross $\text{Rp } 5.650.000 - 5.950.000$: $0.5\%$
  * Gross $\text{Rp } 9.650.000 - 10.050.000$: $2.0\%$
  * Gross $> \text{Rp } 1.400.000.000$: $34.0\%$

### Pasal 17 Progressive True-Up
Year-end December true-up calculates cumulative annual tax against PKP:
1. First Rp 60.000.000: $5\%$
2. Next Rp 190.000.000 (up to Rp 250M): $15\%$
3. Next Rp 250.000.000 (up to Rp 500M): $25\%$
4. Next Rp 4.500.000.000 (up to Rp 5B): $30\%$
5. Over Rp 5.000.000.000: $35\%$

### BPJS Statutory Caps & Splits
* **BPJS Kesehatan:** $4.0\%$ Employer / $1.0\%$ Employee (Maximum wage basis: Rp 12.000.000).
* **BPJS JHT:** $3.7\%$ Employer / $2.0\%$ Employee (No cap).
* **BPJS JP:** $2.0\%$ Employer / $1.0\%$ Employee (Maximum wage basis: Rp 10.042.300).
* **BPJS JKK:** $0.24\% - 1.74\%$ Employer (Industry risk tier basis).
* **BPJS JKM:** $0.30\%$ Employer.

---

## 3. Inventory Valuation & Landed Costing Engine

**Source:** [`crates/domain/src/erp/inventory_costing.rs`](file:///home/haikaru/Archverse/Lab/Archava/crates/domain/src/erp/inventory_costing.rs)

### FIFO Queue Model
Each receipt layer stores `(qty, unit_cost_cents, received_at)`.
* Depletion removes quantity starting from layer index 0.
* If a sale quantity spans across multiple layers, each layer's specific cost is accumulated to compute the exact Cost of Goods Sold (COGS).

### Landed Cost Allocation Methods
When inbound purchase shipments incur landed fees (shipping, import tariff, insurance), costs are allocated across items:
1. **Value Ratio:** Proporsional terhadap nilai total barang (`price * qty`).
2. **Quantity Ratio:** Proporsional terhadap total unit fisik.
3. **Weight Ratio:** Proporsional terhadap berat kotor barang dalam gram/kg.

---

## 4. Hardware ESC/POS Protocol & Receipt Engine

**Source:** [`crates/domain/src/hardware/escpos.rs`](file:///home/haikaru/Archverse/Lab/Archava/crates/domain/src/hardware/escpos.rs) & [`packages/sdk/src/hardware/escpos.ts`](file:///home/haikaru/Archverse/Lab/Archava/packages/sdk/src/hardware/escpos.ts)

### Command Sequence
```text
1. Initialize Printer       -> ESC @      (0x1B, 0x40)
2. Header Align Center      -> ESC a 1    (0x1B, 0x61, 0x01)
3. Header Title Double Hgt  -> GS ! 1     (0x1D, 0x21, 0x01)
4. Order Line Items         -> ASCII text formatted to 32/48 char width
5. Totals & Grand Total     -> Bold (ESC E 1)
6. QRIS Barcode (Model 2)   -> GS ( k sequence
7. Pulse Cash Drawer        -> ESC p 0 25 250 (0x1B, 0x70, 0x00, 0x19, 0xFA)
8. Partial Paper Cut        -> GS V 66 0  (0x1D, 0x56, 0x42, 0x00)
```

---

## 5. IoT Telematics Protocol Engine

**Source:** [`crates/domain/src/telematics/mod.rs`](file:///home/haikaru/Archverse/Lab/Archava/crates/domain/src/telematics/mod.rs)

### Teltonika AVL Codec 8 Big-Endian Frame
```text
[4B Preamble (00000000)]
[4B Data Field Length]
[1B Codec ID (0x08)]
[1B Number of Records]
  [8B Timestamp (epoch ms)]
  [1B Priority]
  [4B Longitude (signed i32 / 10,000,000.0)]
  [4B Latitude  (signed i32 / 10,000,000.0)]
  [2B Altitude (meters)]
  [2B Angle (degrees)]
  [1B Satellites count]
  [2B Speed (km/h)]
  [IO Elements]
    [1B Event IO ID]
    [1B Total IO Count]
    [N 1-byte IO: ID 239 -> Ignition ON/OFF]
    [N 2-byte IO: ID 9   -> Fuel Sensor mV]
    [N 4-byte IO: ID 16  -> Total Odometer in meters]
[1B Number of Records]
[4B CRC-16]
```

---

## 6. Omnichannel WhatsApp Business Cloud API

**Source:** [`crates/domain/src/messaging/whatsapp.rs`](file:///home/haikaru/Archverse/Lab/Archava/crates/domain/src/messaging/whatsapp.rs)

* **Protocol:** Meta Graph API v21.0.
* **Payload Serialization:** JSON template messages containing customer phone (E.164), template slug, and positional parameters (`invoice_no`, `amount_idr`, `due_date`, `payment_link`).
* **Inbound Webhook Verification:** Parses delivery status updates (`sent`, `delivered`, `read`, `failed`) and correlates status back to customer invoices.

---

## 7. Ava Cockpit Durable Execution Engine

**Source:** [`crates/cockpit`](file:///home/haikaru/Archverse/Lab/Archava/crates/cockpit)  
**Blueprint:** [`docs/COCKPIT_TASK_ARCHITECTURE.md`](file:///home/haikaru/Archverse/Lab/Archava/docs/COCKPIT_TASK_ARCHITECTURE.md)

* **Aviation Lifecycle State Machine:** `PreFlight` ➡️ `InFlight` ➡️ `HoldingPattern` ➡️ `Touchdown` ➡️ `Aborted`.
* **Subagent Context Isolation:** Green thread workers return 50:1 compressed summaries (`WorkerReport`), keeping the LLM prompt context clean.
* **Temporal.io-Grade Checkpoints:** PostgreSQL table `archava_cockpit.checkpoints` persists intermediate step outputs so process restarts resume without re-running expensive side-effects.
* **Holding Pattern Gate:** Asynchronous holding pattern awaiting external signal key confirmation for high-risk operations (e.g. wire transfers, tax filings).

---

## 8. Ava Orbz Multi-Layer Memory Engine

**Source:** [`crates/orbz`](file:///home/haikaru/Archverse/Lab/Archava/crates/orbz)  
**Blueprint:** [`docs/ORBZ_MEMORY_ARCHITECTURE.md`](file:///home/haikaru/Archverse/Lab/Archava/docs/ORBZ_MEMORY_ARCHITECTURE.md)

* **Virtual URI Metaphor:** `orbz://{tenant}/{scope}/{category}/{resource}`.
* **Progressive Disclosure:** 3-tier progressive context rendering saving up to 63% prompt tokens (`L0Abstract` ~50 tokens, `L1Overview` ~500 tokens, `L2RawPayload` on demand).
* **Graphiti Bi-Temporal Invalidation:** Tracking `valid_from` and `valid_to` to prevent hallucinations from superseded business facts or pricing.
* **Row-Level Security (Tiers L1-L5):** Mathematical RBAC isolation ensuring sovereign owner memory (margins, cash reserves, tax strategy) is never leaked.
