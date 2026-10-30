# Pricing regression data

`pricing.v1.json` is the retained pricebook reference from the former platform. The engine and pricebook regression tests embed it from this directory. Its original bytes were preserved when it moved from the root `config/` folder.

These values are test/reference data, not an approved current commercial price list. Runtime callers supply their own `Pricebook` or serialized pricebook to the Rust engine/native API. Changes to this fixture should be intentional regression-data updates.
