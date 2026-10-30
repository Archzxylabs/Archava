# Archava Specification Validation Report

Validated files: `PRD.md`, `agent.md`, `config/pricing.v1.json`, `config/templates.v1.json`.

**Result: 85/85 checks passed.**

- PASS — pricing schema version
- PASS — template schema version
- PASS — effective date aligned
- PASS — PRD references pricing source
- PASS — agent references pricing source
- PASS — agent exact region rule present
- PASS — agent zero discretionary discount
- PASS — generic_business: environment exists
- PASS — generic_business: presence exists
- PASS — generic_business: capability exists
- PASS — generic_business: has pages
- PASS — generic_business: has modules
- PASS — hospitality: environment exists
- PASS — hospitality: presence exists
- PASS — hospitality: capability exists
- PASS — hospitality: has pages
- PASS — hospitality: has modules
- PASS — restaurant: environment exists
- PASS — restaurant: presence exists
- PASS — restaurant: capability exists
- PASS — restaurant: has pages
- PASS — restaurant: has modules
- PASS — clinic: environment exists
- PASS — clinic: presence exists
- PASS — clinic: capability exists
- PASS — clinic: has pages
- PASS — clinic: has modules
- PASS — real_estate: environment exists
- PASS — real_estate: presence exists
- PASS — real_estate: capability exists
- PASS — real_estate: has pages
- PASS — real_estate: has modules
- PASS — ecommerce: environment exists
- PASS — ecommerce: presence exists
- PASS — ecommerce: capability exists
- PASS — ecommerce: has pages
- PASS — ecommerce: has modules
- PASS — professional_services: environment exists
- PASS — professional_services: presence exists
- PASS — professional_services: capability exists
- PASS — professional_services: has pages
- PASS — professional_services: has modules
- PASS — education: environment exists
- PASS — education: presence exists
- PASS — education: capability exists
- PASS — education: has pages
- PASS — education: has modules
- PASS — GLOBAL chat setup positive
- PASS — GLOBAL chat monthly positive
- PASS — GLOBAL chat included usage positive
- PASS — GLOBAL voice setup positive
- PASS — GLOBAL voice monthly positive
- PASS — GLOBAL voice included usage positive
- PASS — GLOBAL human setup positive
- PASS — GLOBAL human monthly positive
- PASS — GLOBAL human included usage positive
- PASS — GLOBAL act standard allowance
- PASS — GLOBAL transact standard allowance
- PASS — ID chat setup positive
- PASS — ID chat monthly positive
- PASS — ID chat included usage positive
- PASS — ID voice setup positive
- PASS — ID voice monthly positive
- PASS — ID voice included usage positive
- PASS — ID human setup positive
- PASS — ID human monthly positive
- PASS — ID human included usage positive
- PASS — ID act standard allowance
- PASS — ID transact standard allowance
- PASS — Example A setup
- PASS — Example A monthly
- PASS — Example B setup
- PASS — Example B monthly
- PASS — Example C setup
- PASS — Example C monthly
- PASS — Canonical ID Human Commerce Transact setup
- PASS — Canonical ID Human Commerce Transact monthly
- PASS — PRD A total
- PASS — PRD B base total
- PASS — PRD D total
- PASS — Agent A total
- PASS — Agent B total
- PASS — Agent C total
- PASS — quote validity 14
- PASS — annual discount 10

## Calculated reference quotes

```json
{
  "A_ID_chat_landing": {
    "fixed": 11800000,
    "discount_pct": 5.0,
    "integration": 0,
    "raw": 11210000.0,
    "setup": 11200000,
    "monthly": 1490000
  },
  "B_ID_voice_business_act_two_integrations": {
    "fixed": 24700000,
    "discount_pct": 10.0,
    "integration": 2500000,
    "raw": 24730000.0,
    "setup": 24700000,
    "monthly": 3240000
  },
  "C_GLOBAL_human_existing_transact_three_integrations": {
    "fixed": 8500,
    "discount_pct": 5.0,
    "integration": 750,
    "raw": 8825.0,
    "setup": 8850,
    "monthly": 898
  },
  "D_ID_human_commerce_transact": {
    "fixed": 39700000,
    "discount_pct": 10.0,
    "integration": 0,
    "raw": 35730000.0,
    "setup": 35700000,
    "monthly": 4990000
  }
}
```
