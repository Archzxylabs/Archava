#!/usr/bin/env python3
"""DRAFT IDR cost calculator for the Archava avatar session.

Review tool only. Not imported by the app, server, or worker, and not wired
into any route. Every rate below is a planning assumption transcribed from the
vendor page cited on its line; the vendor invoice is the final source of cost.

Two hosting cases are computed and kept separate, because they are mutually
exclusive on any given deployment:

  local  -- the agent runs on the machine that runs this server (today's build).
            No LiveKit Cloud agent-session charge and no metered WebRTC charge
            while inside the included allowance. Hosting is a real cost but is
            not a vendor invoice, so it is carried as an explicit placeholder.
  cloud  -- the agent is deployed to LiveKit Cloud and consumes agent minutes
            plus metered WebRTC participation.

Gemini is billed directly to Archava, so LiveKit Inference is never added: that
is not a double count to be balanced, it is a different line that is absent.

Run:  python3 pricing/cost_model.py
Check: python3 pricing/check_cost_model.py
"""

from __future__ import annotations

import csv
import io
import sys

# --- FX ---------------------------------------------------------------------
# Bank Indonesia JISDOR, 29 September 2026: USD 1 = Rp17,998 (28 Sep: Rp17,965).
# The shared handoff doc instructs Rp18,000 for product math; both are shown so
# a reviewer can see the rounding direction (Rp18,000 is +0.01% conservative on
# cost, i.e. it overstates cost slightly, which is the safe direction).
JISDOR_2026_09_29 = 17_998
WORKING_FX = 18_000

# --- Vendor rates, per the cited official page ------------------------------
# Spatius pricing page (annual-billing toggle, "Save 20%"):
#   Free $0/mo, ~1,200 min/yr, 10 min max session
#   Starter $16/mo (billed $189/yr), ~2,000 min/mo included, overage $0.0072/min
#   Builder $39/mo (billed $469/yr), ~5,500 min/mo
#   Growth $119/mo, ~18,000 min/mo
#   Scale $239/mo, ~40,000 min/mo, overage $0.0056/min
# 10 credits per avatar-minute. Month-to-month rates are not printed on the
# page; the 20% saving implies about $20/mo Starter / $49/mo Builder, which is
# derived, not quoted. All figures below use the quoted annual-effective rate.
SPATIUS_FREE = 0.0
SPATIUS_STARTER = 16.0          # annual-effective monthly
SPATIUS_BUILDER = 39.0
SPATIUS_GROWTH = 119.0
SPATIUS_SCALE = 239.0
SPATIUS_STARTER_MIN = 2_000
SPATIUS_BUILDER_MIN = 5_500
SPATIUS_STARTER_OVERAGE = 0.0072
SPATIUS_SCALE_OVERAGE = 0.0056

# Sensitivity input, NOT a quoted rate. The page prints only the annual-
# effective figure, so the month-to-month figure is derived from it: $189/year
# presented as a 20% saving implies 189 / (12 x 0.8) = $19.69/mo month-to-month.
# It is carried so a reviewer can see how far the answer moves on a rate nobody
# has confirmed. The owner's Spatius invoice replaces it: set
# SPATIUS_STARTER_ACCOUNT_BILL below, or pass --spatius-starter-account-bill.
SPATIUS_STARTER_MONTH_TO_MONTH = 19.69    # derived, not quoted
SPATIUS_STARTER_ACCOUNT_BILL = None       # open input; None until the invoice

# Gemini 3.1 Flash Live Preview (Live API, native audio), paid tier:
#   audio input  $3.00 / 1M tokens, or $0.005 per audio minute
#   audio output $12.00 / 1M tokens, or $0.018 per audio minute
# 1 minute of input plus 1 minute of output per connected minute is the
# conservative planning assumption stated in the shared handoff.
GEMINI_AUDIO_IN_PER_MIN = 0.005
GEMINI_AUDIO_OUT_PER_MIN = 0.018

# LiveKit Cloud: Build $0 (1,000 agent min, 5,000 WebRTC min included);
# Ship from $50/mo (5,000 agent min, 150,000 WebRTC min included, agent overage
# $0.01/min, WebRTC overage $0.0005/min).
LIVEKIT_BUILD = 0.0
LIVEKIT_SHIP = 50.0
LIVEKIT_BUILD_AGENT_MIN = 1_000
LIVEKIT_SHIP_AGENT_MIN = 5_000
LIVEKIT_SHIP_WEBRTC_MIN = 150_000
LIVEKIT_AGENT_OVERAGE = 0.010
LIVEKIT_WEBRTC_OVERAGE = 0.0005

# Placeholder, not a vendor rate. Left explicit and zeroed so it cannot be
# mistaken for a measured number; the owner must supply the real host cost.
LOCAL_HOST_PER_MONTH_USD = 0.0

BUFFER = 0.25                  # uncertainty buffer on variable cost
SCENARIOS = (100, 1_000, 2_000)

# --- DRAFT hackathon offers under evaluation --------------------------------
# These replace the earlier high-markup 149k/599k placeholders. They are not
# contract quotes or approved retail prices, and assume the locally hosted
# agent and included LiveKit/Spatius allowances described above.
DRAFT_OFFERS = (
    ("DRAFT entry pack", 60, 79_000),
    ("DRAFT growth pack", 300, 299_000),
)


def tier(spatius_minutes: int) -> tuple[str, float, int, float]:
    """Cheapest Spatius plan whose included allowance covers the scenario."""
    if spatius_minutes <= 0:
        return "Free", SPATIUS_FREE, 0, 0.0
    if spatius_minutes <= SPATIUS_STARTER_MIN:
        return "Starter", SPATIUS_STARTER, SPATIUS_STARTER_MIN, 0.0
    if spatius_minutes <= SPATIUS_BUILDER_MIN:
        return "Builder", SPATIUS_BUILDER, SPATIUS_BUILDER_MIN, 0.0
    # Above Builder the page stops printing per-minute overage except on Scale.
    return "Growth", SPATIUS_GROWTH, 18_000, 0.0


def marginal_buffered_idr() -> float:
    """Marginal cost of one more billable minute inside a paid allowance.

    With the Spatius plan fee already sunk and LiveKit Build's included agent
    minutes unused, the only rate that moves with a minute is Gemini. This is
    the denominator of every break-even figure below, so it is stated once.
    """
    return (GEMINI_AUDIO_IN_PER_MIN + GEMINI_AUDIO_OUT_PER_MIN) * JISDOR_2026_09_29 * (1 + BUFFER)


def buffered_plan_fees_idr(spatius_fee_usd: float, livekit_fee_usd: float) -> float:
    """Monthly fixed vendor fees in IDR, carried at the same buffer as variable.

    The buffer is applied to a fixed fee only so it is treated the same way in
    both tests below; a reader who disagrees can set BUFFER to 0 and see the
    unbuffered figures, which is why the rate is a module constant and not a
    literal inside the arithmetic.
    """
    return (spatius_fee_usd + livekit_fee_usd) * JISDOR_2026_09_29 * (1 + BUFFER)


def scenario(minutes: int) -> dict:
    """Full monthly cost picture for one connected-minute volume."""
    spatius_plan, spatius_fee, spatius_included, _ = tier(minutes)
    spatius_over_min = max(0, minutes - spatius_included)
    # Overage is only published for Starter and Scale; the other tiers show
    # "Paid plans can flex" with no rate, so overage is flagged unknown.
    known_overage = {SPATIUS_STARTER: SPATIUS_STARTER_OVERAGE, SPATIUS_SCALE: SPATIUS_SCALE_OVERAGE}
    spatius_over_rate = known_overage.get(spatius_fee, None)

    gemini_usd = minutes * (GEMINI_AUDIO_IN_PER_MIN + GEMINI_AUDIO_OUT_PER_MIN)

    if minutes <= LIVEKIT_BUILD_AGENT_MIN:
        livekit_plan, livekit_fee = "Build", LIVEKIT_BUILD
        lk_agent_min, lk_webrtc_min = 0, 0
    else:
        livekit_plan, livekit_fee = "Ship", LIVEKIT_SHIP
        lk_agent_min = max(0, minutes - LIVEKIT_SHIP_AGENT_MIN)
        lk_webrtc_min = max(0, minutes - LIVEKIT_SHIP_WEBRTC_MIN)
    livekit_over_usd = lk_agent_min * LIVEKIT_AGENT_OVERAGE + lk_webrtc_min * LIVEKIT_WEBRTC_OVERAGE

    local_variable = spatius_fee + gemini_usd + LOCAL_HOST_PER_MONTH_USD
    cloud_variable = spatius_fee + gemini_usd + livekit_fee + livekit_over_usd

    def per_min(total_idr: float) -> float:
        return (total_idr / minutes) if minutes else 0.0

    local_total_idr = local_variable * JISDOR_2026_09_29
    cloud_total_idr = cloud_variable * JISDOR_2026_09_29

    return {
        "minutes": minutes,
        "spatius": f"{spatius_plan} ${spatius_fee:,.0f}/mo",
        "livekit_plan": livekit_plan,
        "livekit_fee_usd": livekit_fee,
        "spatius_included": spatius_included,
        "local_variable_usd": local_variable,
        "local_variable_idr": local_total_idr,
        "local_variable_per_min_idr": per_min(local_total_idr),
        "cloud_variable_usd": cloud_variable,
        "cloud_variable_idr": cloud_total_idr,
        "cloud_variable_per_min_idr": per_min(cloud_total_idr),
        "buffered_local_per_min_idr": per_min(local_total_idr * (1 + BUFFER)),
        "buffered_cloud_per_min_idr": per_min(cloud_total_idr * (1 + BUFFER)),
        "livekit_overage_flag": f"{livekit_plan}: +{lk_agent_min} agent min, +{lk_webrtc_min} WebRTC min",
        "spatius_overage_flag": (
            f"{spatius_over_min} min beyond plan"
            + ("" if spatius_over_rate is not None else " (overage rate unpublished)")
        ) if spatius_over_min else "within included allowance",
    }


def evaluate_offers(rows: list[dict]) -> list[dict]:
    """Margin and break-even for each draft offer.

    A pack is a sale, not a workload, so two honest bases are reported:

    incremental -- the buyer's minutes ride on top of a plan that is already
        paid for (the normal case for a repeat customer). If the pack fits
        inside an included allowance, the marginal cost is Gemini only. This is
        the basis a repeat customer's margin is actually earned on.
    solitary -- the pack's minutes are the entire month's volume, so the fixed
        Spatius and LiveKit plan fees land on those minutes alone. This is the
        basis that says whether a single-pack customer pays for the vendor
        month at all.

    Reporting only the first overstates margin; reporting only the second
    understates it.

    Break-even is split into two figures because the old single figure mixed a
    one-off pack sale with a monthly volume, so it answered neither question:

    fixed_breakeven_minutes -- a VOLUME per month. How many minutes must be sold
        each month at this pack's own price per minute for contribution to cover
        the buffered plan fees:  fees / (price per min - marginal cost per min).
        This is the figure a unit-economics or LTV/CAC model needs, and it is
        positive whenever the price clears the marginal cost at all.
    max_covered_minutes -- how many billable minutes THIS PACK's own revenue can
        fund once its plan fees are paid:
        (pack price - plan fees) / marginal cost per min. Negative means the pack
        does not cover the vendor month it depends on, so it funds nothing.

    The previous revision reported the second under the first's name, which made
    a one-pack shortfall read as "462 minutes per month" of cover. The two now
    print side by side with the units in their labels.
    """
    local_row = min(rows, key=lambda r: r["buffered_local_per_min_idr"])
    cloud_row = max(rows, key=lambda r: r["minutes"])
    incremental_buffered_idr = marginal_buffered_idr()

    results = []
    for name, offered_minutes, price_idr in DRAFT_OFFERS:
        sell_per_min = price_idr / offered_minutes

        # Incremental basis: minutes sit inside an already-paid allowance.
        incr_cost_total = incremental_buffered_idr * offered_minutes

        # Solitary basis: the smallest plan that covers the pack.
        # LiveKit Build (no fee) covers up to its included agent minutes; beyond
        # that the Ship fee applies.
        plan_plan, plan_fee_usd, plan_included, _ = tier(offered_minutes)
        if offered_minutes > LIVEKIT_BUILD_AGENT_MIN:
            lk_fee = LIVEKIT_SHIP
            livekit_name = "Ship"
        else:
            lk_fee = LIVEKIT_BUILD
            livekit_name = "Build"
        plan_fees_idr = buffered_plan_fees_idr(plan_fee_usd, lk_fee)
        marginal_gemini_usd = GEMINI_AUDIO_IN_PER_MIN + GEMINI_AUDIO_OUT_PER_MIN
        solitary_usd = plan_fee_usd + (offered_minutes * marginal_gemini_usd) + lk_fee
        solitary_idr = solitary_usd * JISDOR_2026_09_29 * (1 + BUFFER)

        # Break-even on the fixed plan fees, as two different questions.
        # The volume figure needs no guard: the sell price per minute is above
        # the marginal cost for both draft offers, so contribution per minute is
        # positive. It is still guarded, because a pack priced at or below
        # marginal cost can never cover a fixed fee and must not print a number.
        contribution_per_min = sell_per_min - incremental_buffered_idr
        fixed_breakeven_minutes = (plan_fees_idr / contribution_per_min) if contribution_per_min > 0 else None
        max_covered_minutes = (price_idr - plan_fees_idr) / incremental_buffered_idr

        results.append({
            "offer": name,
            "minutes": offered_minutes,
            "price_idr": price_idr,
            "effective_idr_per_min": sell_per_min,
            # incremental
            "incr_cost_per_min_idr": incremental_buffered_idr,
            "incr_cost_total_idr": incr_cost_total,
            "incr_contribution_per_min_idr": contribution_per_min,
            "incr_pack_margin_pct": ((price_idr - incr_cost_total) / price_idr) if price_idr else 0.0,
            # fixed fees
            "plan_fees_idr": plan_fees_idr,
            "pack_covers_plan_fees": price_idr >= plan_fees_idr,
            "fixed_breakeven_minutes": fixed_breakeven_minutes,   # volume, minutes/month
            "max_covered_minutes": max_covered_minutes,           # one-off pack
            # solitary
            "sol_plan": plan_plan,
            "sol_livekit": livekit_name,
            "sol_cost_total_idr": solitary_idr,
            "sol_cost_per_min_idr": solitary_idr / offered_minutes,
            "sol_pack_margin_pct": ((price_idr - solitary_idr) / price_idr) if price_idr else 0.0,
            "sol_pack_covers_vendor_month": price_idr >= solitary_idr,
            # reference scenarios
            "ref_local_scenario": local_row["minutes"],
            "ref_local_per_min_idr": local_row["buffered_local_per_min_idr"],
            "ref_cloud_scenario": cloud_row["minutes"],
            "ref_cloud_per_min_idr": cloud_row["buffered_cloud_per_min_idr"],
        })
    return results


def sensitivity_rows(account_bill_usd: float | None = None) -> list[dict]:
    """How far the fixed-fee answers move on the one rate nobody has quoted.

    Every rate in this file except the Spatius Starter month-to-month fee is
    printed on a vendor page. Starter is quoted only at its annual-effective
    $16/mo, so the month-to-month row is DERIVED ($189/yr as a 20% saving implies
    $19.69/mo), and the account-invoice row appears only when the owner passes a
    real figure. The point of the table is that a reviewer can see the spread
    without mistaking a derived row for a quoted one.
    """
    candidates = [
        (SPATIUS_STARTER, "annual-effective (quoted)"),
        (SPATIUS_STARTER_MONTH_TO_MONTH, "month-to-month (derived)"),
    ]
    if account_bill_usd:
        candidates.append((float(account_bill_usd), "account invoice (owner-supplied)"))

    rows = []
    marginal_idr = marginal_buffered_idr()
    for fee_usd, basis in candidates:
        plan_fees_idr = buffered_plan_fees_idr(fee_usd, LIVEKIT_BUILD)
        for _, offered_minutes, price_idr in DRAFT_OFFERS:
            sell_per_min = price_idr / offered_minutes
            rows.append({
                "spatius_starter_usd": fee_usd,
                "basis": basis,
                "plan_fees_idr": plan_fees_idr,
                "minutes": offered_minutes,
                "price_idr": price_idr,
                "fixed_breakeven_minutes": plan_fees_idr / (sell_per_min - marginal_idr),
                "max_covered_minutes": (price_idr - plan_fees_idr) / marginal_idr,
                "covers_plan_fees": price_idr >= plan_fees_idr,
            })
    return rows


def render(rows: list[dict], offers: list[dict], sensitivity: list[dict] | None = None) -> str:
    out = io.StringIO()
    w = out.write
    w("ARCHAVA AVATAR SESSION - DRAFT IDR COST MODEL\n")
    w("Status: DRAFT. Not approved for publication or sale.\n")
    w(f"FX: BI JISDOR 29 Sep 2026 = Rp{JISDOR_2026_09_29:,}/USD "
      f"(working Rp{WORKING_FX:,}). Buffer {BUFFER:.0%} on variable cost.\n")
    w("Rates: Spatius pricing page (annual-effective), Gemini 3.1 Flash Live "
      "Preview Live API, LiveKit Cloud pricing - all fetched 30 Sep 2026.\n\n")

    w("PER-CONNECTED-MINUTE COST\n")
    w(f"{'minutes':>7} | {'local var Rp/min':>16} | {'cloud var Rp/min':>16} | "
      f"{'buffered local':>13} | {'buffered cloud':>13}\n")
    for r in rows:
        w(f"{r['minutes']:>7,} | {r['local_variable_per_min_idr']:>16,.0f} | "
          f"{r['cloud_variable_per_min_idr']:>16,.0f} | "
          f"{r['buffered_local_per_min_idr']:>13,.0f} | {r['buffered_cloud_per_min_idr']:>13,.0f}\n")

    w("\nMONTHLY TOTALS AND FIXED FEES\n")
    for r in rows:
        w(f"- {r['minutes']:,} min: Spatius {r['spatius']}; LiveKit {r['livekit_overage_flag']}; "
          f"Spatius {r['spatius_overage_flag']}.\n")
        w(f"    local variable  ${r['local_variable_usd']:,.2f} = Rp{r['local_variable_idr']:,.0f}/mo\n")
        w(f"    cloud variable  ${r['cloud_variable_usd']:,.2f} = Rp{r['cloud_variable_idr']:,.0f}/mo "
          f"(includes the LiveKit Cloud plan fee)\n")

    w("\nDRAFT OFFER EVALUATION - two bases, because a pack is a sale, not a workload\n")
    w("incremental: pack rides on an already-paid plan -> only Gemini is marginal\n")
    w("solitary:    the pack is the entire month -> Spatius + LiveKit fees land on it\n")
    for o in offers:
        w(f"- {o['offer']}: Rp{o['price_idr']:,}/{o['minutes']} min = "
          f"Rp{o['effective_idr_per_min']:,.0f}/min\n")
        w(f"    incremental  cost Rp{o['incr_cost_per_min_idr']:,.0f}/min "
          f"-> pack cost Rp{o['incr_cost_total_idr']:,.0f}; contribution "
          f"Rp{o['incr_contribution_per_min_idr']:,.0f}/min; margin {o['incr_pack_margin_pct']:.0%}\n")
        # Fixed fees, answered as a volume. Always printable at these prices, but
        # guarded so a pack at or below marginal cost cannot print a number.
        if o["fixed_breakeven_minutes"] is not None:
            w(f"    break-even   {o['fixed_breakeven_minutes']:,.0f} min/month of sales at "
              f"Rp{o['effective_idr_per_min']:,.0f}/min covers the "
              f"Rp{o['plan_fees_idr']:,.0f} buffered plan fees\n")
        else:
            w(f"    break-even   none: Rp{o['effective_idr_per_min']:,.0f}/min does not clear the "
              f"Rp{o['incr_cost_per_min_idr']:,.0f}/min marginal cost, so no volume covers the "
              f"Rp{o['plan_fees_idr']:,.0f} plan fees\n")
        # Fixed fees, answered for the one pack being sold.
        if o["pack_covers_plan_fees"]:
            w(f"    this pack funds  {o['max_covered_minutes']:,.0f} more billable minutes once the "
              f"Rp{o['plan_fees_idr']:,.0f} plan fees are paid from its Rp{o['price_idr']:,}\n")
        else:
            w(f"    this pack funds  none: Rp{o['price_idr']:,} is below the "
              f"Rp{o['plan_fees_idr']:,.0f} buffered cost of the vendor month it depends on, so a customer\n"
              f"                 who buys only this pack is served at a loss on fixed fees.\n")
        w(f"    solitary     cost Rp{o['sol_cost_per_min_idr']:,.0f}/min "
          f"(Spatius {o['sol_plan']} + LiveKit {o['sol_livekit']}) "
          f"-> pack cost Rp{o['sol_cost_total_idr']:,.0f}; margin {o['sol_pack_margin_pct']:.0%}; "
          f"covers the vendor month: {'yes' if o['sol_pack_covers_vendor_month'] else 'NO'}\n")

    if sensitivity:
        w("\nSPATIUS STARTER FEE SENSITIVITY - the one rate nobody quoted\n")
        w("Only the Spatius Starter month-to-month fee is unquoted; the page prints the\n"
          "annual-effective rate. The month-to-month row is DERIVED from the annual\n"
          "saving ($189/yr x 20% off => 189 / (12 x 0.8) = $19.69/mo), not quoted.\n"
          "Replace it with the account invoice before any launch decision.\n")
        w(f"{'starter fee':>11} | {'plan fees':>10} | {'pack':>16} | "
          f"{'break-even min/mo':>17} | {'funded by pack':>15}\n")
        for s in sensitivity:
            pack = f"Rp{s['price_idr']:,}/{s['minutes']}m"
            funded = (f"{s['max_covered_minutes']:,.0f} min" if s["covers_plan_fees"] else "none")
            w(f"${s['spatius_starter_usd']:>10,.2f} | {s['plan_fees_idr']:>10,.0f} | {pack:>16} | "
              f"{s['fixed_breakeven_minutes']:>17,.0f} | {funded:>15}\n")

    w("\nLOW-VOLUME WARNING\n")
    smallest_row = min(rows, key=lambda r: r["minutes"])
    starter_fees = buffered_plan_fees_idr(SPATIUS_STARTER, LIVEKIT_BUILD)
    cheapest_pack = min(price for _, _, price in DRAFT_OFFERS)
    w(f"At {smallest_row['minutes']:,} min/month the Spatius plan fee alone dominates the bill "
      f"(Rp{smallest_row['buffered_local_per_min_idr']:,.0f}/min buffered local), and the\n"
      f"buffered Starter + LiveKit Build fees are Rp{starter_fees:,.0f}/mo. One "
      f"Rp{cheapest_pack:,} draft pack does not cover that month.\n")
    w("Local-worker hosting is a placeholder of Rp0 here. The owner must supply the real figure\n"
      "before this model is used for a launch decision.\n")
    return out.getvalue()


def csv_report(rows: list[dict], offers: list[dict], sensitivity: list[dict] | None = None) -> str:
    buf = io.StringIO()
    writer = csv.writer(buf, lineterminator="\n")
    writer.writerow(["section", "metric", "value_idr", "value_usd", "note"])
    for r in rows:
        writer.writerow(["cost", f"{r['minutes']} min local variable per minute",
                         round(r["local_variable_per_min_idr"]), round(r["local_variable_usd"], 4),
                         "Spatius + Gemini + local host placeholder"])
        writer.writerow(["cost", f"{r['minutes']} min cloud variable per minute",
                         round(r["cloud_variable_per_min_idr"]), round(r["cloud_variable_usd"], 4),
                         "adds LiveKit Cloud plan fee and overage"])
        writer.writerow(["cost", f"{r['minutes']} min buffered cloud per minute",
                         round(r["buffered_cloud_per_min_idr"]), "",
                         "25% buffer on variable cost"])
        writer.writerow(["cost", f"{r['minutes']} min monthly total cloud",
                         round(r["cloud_variable_idr"]), round(r["cloud_variable_usd"], 2),
                         "fixed plan fee included"])
    for o in offers:
        writer.writerow(["offer", f"{o['offer']} price per minute", round(o["effective_idr_per_min"]), "",
                         "DRAFT, not approved"])
        writer.writerow(["offer", f"{o['offer']} incremental margin percent",
                         round(o["incr_pack_margin_pct"] * 100), "",
                         "pack rides on an already-paid plan; Gemini is marginal"])
        writer.writerow(["offer", f"{o['offer']} solitary margin percent", round(o["sol_pack_margin_pct"] * 100), "",
                         "pack is the whole month; plan fees included"])
        writer.writerow(["offer", f"{o['offer']} solitary covers vendor month",
                         "yes" if o["sol_pack_covers_vendor_month"] else "no", "",
                         f"Spatius {o['sol_plan']} + LiveKit {o['sol_livekit']}"])
        # Volume question. Always answerable at these prices, so a spreadsheet
        # reading this column gets a monthly volume, not a one-pack figure.
        writer.writerow(["offer", f"{o['offer']} fixed-fee break-even minutes per month",
                         round(o["fixed_breakeven_minutes"]) if o["fixed_breakeven_minutes"] is not None else "", "",
                         "monthly sales volume at this pack's price per minute that covers the plan fees"])
        # One-off-sale question. Empty rather than negative, so a spreadsheet
        # cannot read a shortfall as a volume of minutes.
        funded = round(o["max_covered_minutes"]) if o["pack_covers_plan_fees"] else ""
        writer.writerow(["offer", f"{o['offer']} billable minutes funded by one pack",
                         funded, "",
                         "after paying the plan fees; empty when the pack is below them"])

    for s in sensitivity or []:
        basis = f"Spatius Starter {s['spatius_starter_usd']:,.2f} ({s['basis']})"
        writer.writerow(["sensitivity", f"{basis} buffered plan fees",
                         round(s["plan_fees_idr"]), s["spatius_starter_usd"],
                         "month-to-month row is DERIVED, not quoted, unless labelled owner-supplied"])
        writer.writerow(["sensitivity", f"{basis} {s['minutes']} min pack fixed-fee break-even per month",
                         round(s["fixed_breakeven_minutes"]), "",
                         "plan fees / (sale price per min - marginal cost per min)"])
    return buf.getvalue()


def account_bill_from_argv(argv: list[str]) -> float | None:
    """Optional owner-supplied Spatius Starter rate, as a monthly USD figure.

    The pricing page prints only the annual-effective Starter rate, so the
    real monthly invoice is an open input. Passing it here adds one labelled
    row to the sensitivity table; it never replaces the quoted annual rate.
    """
    flag = "--spatius-starter-account-bill"
    for index, argument in enumerate(argv):
        value: float | None = None
        if argument == flag and index + 1 < len(argv):
            value = _as_positive_float(argv[index + 1])
        elif argument.startswith(f"{flag}="):
            value = _as_positive_float(argument.split("=", 1)[1])
        if value is not None:
            return value
    return None


def _as_positive_float(text: str) -> float | None:
    try:
        number = float(text)
    except ValueError:
        return None
    return number if number > 0 else None


def main(argv: list[str] | None = None) -> None:
    arguments = list(argv) if argv is not None else sys.argv[1:]
    rows = [scenario(m) for m in SCENARIOS]
    offers = evaluate_offers(rows)
    sensitivity = sensitivity_rows(account_bill_from_argv(arguments))
    print(render(rows, offers, sensitivity))
    print("CSV (machine-readable, same numbers):")
    print(csv_report(rows, offers, sensitivity), end="")


if __name__ == "__main__":
    main()
