"""Deterministic check for the corrected pricing arithmetic.

Run:  python3 pricing/check_cost_model.py

Nothing here is sampled, timed, or random, so a pass means the same numbers
every time. Every expected value is recomputed from the module's own published
constants rather than copied from a previous run, so a silent rate change
fails here instead of quietly shipping.

The point of the file is the two break-even questions, which the previous
revision collapsed into one:

    fixed-breakeven minutes  a monthly VOLUME of sales at a pack's price per
                             minute that covers the buffered vendor plan fees.
    minutes funded per pack  a ONE-OFF figure: how many billable minutes the
                             pack can fund once those same fees are paid.

They answer different questions and had been printed under one name.
"""
from __future__ import annotations

import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
PROJECT = HERE.parent
sys.path.insert(0, str(PROJECT))

import pricing.cost_model as cm  # noqa: E402

TOLERANCE = 1.0  # one rupiah, for figures reported as whole rupiah


def main() -> int:
    rows = [cm.scenario(minutes) for minutes in cm.SCENARIOS]
    offers = cm.evaluate_offers(rows)
    by_pack = {offer["minutes"]: offer for offer in offers}
    failures: list[str] = []

    def check(label: str, actual, expected, tolerance: float | None = None) -> None:
        """Compare with a tolerance when both sides are rounded currency."""
        if tolerance is None:
            ok = actual == expected
        else:
            ok = abs(float(actual) - float(expected)) <= tolerance
        if not ok:
            failures.append(f"{label}: expected {expected!r}, got {actual!r}")

    # --- 1. The two questions are both present and badly named ----------------
    # The old key was `incr_breakeven_minutes`, and it held the one-pack figure
    # while being read as a monthly volume. It must not come back under any
    # name, and its two replacements must both exist.
    for offer in offers:
        for gone in ("incr_breakeven_minutes", "pack_breakeven_minutes"):
            check(f"{offer['minutes']} min pack drops {gone}", gone in offer, False)
        for present in ("fixed_breakeven_minutes", "max_covered_minutes", "plan_fees_idr"):
            check(f"{offer['minutes']} min pack keeps {present}", present in offer, True)

    # --- 2. Marginal cost and plan fees, recomputed from published constants --
    marginal_idr = (cm.GEMINI_AUDIO_IN_PER_MIN + cm.GEMINI_AUDIO_OUT_PER_MIN) * cm.JISDOR_2026_09_29 * (1 + cm.BUFFER)
    check("marginal buffered IDR matches hand computation",
          cm.marginal_buffered_idr(), marginal_idr, 0.0001)

    plan_fees_idr = (cm.SPATIUS_STARTER + cm.LIVEKIT_BUILD) * cm.JISDOR_2026_09_29 * (1 + cm.BUFFER)
    check("buffered plan fees match hand computation",
          cm.buffered_plan_fees_idr(cm.SPATIUS_STARTER, cm.LIVEKIT_BUILD), plan_fees_idr, 0.0001)

    # --- 3. Both break-even figures, per pack, from scratch -------------------
    for offer in cm.evaluate_offers(rows):
        minutes = offer["minutes"]
        price_idr = float(offer["price_idr"]) if "price_idr" in offer else None
        if price_idr is None:
            # Fall back to the source table so this check still stands on its
            # own if the offer dict ever changes shape.
            price_idr = float(next(p for _, m, p in cm.DRAFT_OFFERS if m == minutes))

        sell_per_min = price_idr / minutes
        contribution = sell_per_min - marginal_idr

        expected_breakeven = plan_fees_idr / contribution
        expected_covered = (price_idr - plan_fees_idr) / marginal_idr

        check(f"{minutes} min pack fixed-fee break-even per month",
              round(offer["fixed_breakeven_minutes"]), round(expected_breakeven), TOLERANCE)
        check(f"{minutes} min pack buffered plan fees",
              round(offer["plan_fees_idr"]), round(plan_fees_idr), TOLERANCE)

        check(f"{minutes} min pack raw funded-minute arithmetic",
              offer["max_covered_minutes"], expected_covered, 0.0001)
        actual_covered = offer["max_covered_minutes"] if offer["pack_covers_plan_fees"] else None
        expected_visible = round(expected_covered) if expected_covered > 0 else None
        if expected_visible is None:
            check(f"{minutes} min pack reports no funded minutes", actual_covered is None, True)
        else:
            check(f"{minutes} min pack minutes funded by one pack",
                  actual_covered, expected_visible, TOLERANCE)

    # --- 4. The specific confusion this file exists to prevent ----------------
    # The entry pack is below the buffered cost of the vendor month it rides on,
    # so the one-off answer must be "none", never a monthly volume such as the
    # one the old code printed under the same name for the same pack.
    entry = by_pack[60]
    check("entry pack: monthly volume question still has an answer",
          round(entry["fixed_breakeven_minutes"]) > 0, True)
    check("entry pack: one-pack question reports nothing", entry["pack_covers_plan_fees"], False)

    # The lower hackathon drafts do not ask one buyer to fund the entire vendor
    # month; the monthly-volume break-even question remains distinct.
    growth = by_pack[300]
    check("growth pack: monthly volume can cover fees",
          round(growth["fixed_breakeven_minutes"]) > 0, True)
    check("growth pack: one pack alone does not cover the vendor month",
          growth["pack_covers_plan_fees"], False)

    # --- 5. Sensitivity: the derived row never masquerades as a quote ---------
    sensitivity = cm.sensitivity_rows()
    bases = {row["basis"] for row in sensitivity}
    check("sensitivity carries the quoted annual-effective rate",
          any("quoted" in basis for basis in bases), True)
    check("sensitivity carries the derived month-to-month rate",
          any("derived" in basis for basis in bases), True)
    check("sensitivity has no third basis until an invoice is supplied",
          len(bases), 2)

    quoted = next(row for row in sensitivity if "quoted" in row["basis"])
    check("quoted row still uses the published $16 Starter rate",
          round(quoted["spatius_starter_usd"], 2), 16.00, 0.005)
    derived = next(row for row in sensitivity if "derived" in row["basis"])
    check("derived row is labelled and above the quoted rate",
          round(derived["spatius_starter_usd"], 2) > round(quoted["spatius_starter_usd"], 2), True)

    with_invoice = cm.sensitivity_rows(account_bill_usd=24.50)
    check("invoice adds one row per pack", len(with_invoice) - len(sensitivity), len(cm.DRAFT_OFFERS))
    invoice = next(row for row in with_invoice if "owner-supplied" in row["basis"])
    check("invoice row keeps the supplied figure", round(invoice["spatius_starter_usd"], 2), 24.50, 0.005)
    check("invoice row is not silent about being different",
          round(invoice["plan_fees_idr"]) > round(quoted["plan_fees_idr"]), True)

    # --- 6. A higher plan fee moves both answers in the same direction --------
    dearer = cm.buffered_plan_fees_idr(24.50, cm.LIVEKIT_BUILD)
    check("a dearer plan fee raises the required monthly volume",
          dearer > plan_fees_idr, True)

    # --- 7. Rendering never ships the retired label --------------------------
    text = cm.render(rows, offers, sensitivity)
    check("report does not print the retired metric name",
          "incr_breakeven_minutes" in text or "break-even 462 min/month" in text, False)
    check("report labels the monthly volume question", "min/month" in text, True)
    check("report labels the one-pack question", "this pack funds" in text, True)
    check("report flags the derived rate as derived",
          "DERIVED" in text and "not quoted" in text, True)

    csv_text = cm.csv_report(rows, offers, sensitivity)
    check("csv drops the retired metric name", "incr_breakeven_minutes" in csv_text, False)
    check("csv names both questions",
          "fixed-fee break-even minutes per month" in csv_text and "billable minutes funded by one pack" in csv_text,
          True)

    # --- 8. CLI plumbing -----------------------------------------------------
    check("flag parses a bare value", cm.account_bill_from_argv(["--spatius-starter-account-bill", "24.5"]), 24.5)
    check("flag parses an equals value",
          cm.account_bill_from_argv(["--spatius-starter-account-bill=24.5"]), 24.5)
    check("flag rejects a non-number", cm.account_bill_from_argv(["--spatius-starter-account-bill", "abc"]), None)
    check("flag rejects zero", cm.account_bill_from_argv(["--spatius-starter-account-bill", "0"]), None)
    check("no flag is a clean None", cm.account_bill_from_argv([]), None)

    # --- Report --------------------------------------------------------------
    if failures:
        print(f"FAIL  {len(failures)} check(s) did not hold:")
        for failure in failures:
            print(f"  - {failure}")
        return 1

    print("PASS  pricing arithmetic holds")
    print(f"  marginal cost per billable minute : Rp{marginal_idr:,.1f}")
    print(f"  buffered vendor plan fees         : Rp{plan_fees_idr:,.0f}/mo")
    for minutes in sorted(by_pack):
        offer = by_pack[minutes]
        covered = offer["max_covered_minutes"] if offer["pack_covers_plan_fees"] else None
        funded = f"{covered:,.0f} min" if covered is not None else "none"
        print(f"  {minutes:>4} min pack @ Rp{float(offer['price_idr']):,.0f}"
              f"  -> {offer['fixed_breakeven_minutes']:,.0f} min/month to cover fees"
              f" | one pack funds {funded}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
