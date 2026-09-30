"""Verified product facts for Ava, the Archava Onchain host.

Update these facts alongside a shipped feature. Environment flags alone do not
prove that customer-facing minute packs or API keys are operational, so the
release switches live in RELEASE_READINESS below and default to off.
"""

from site_context import PAID_SECTION_DETAILS, SECTIONS


PRODUCT_TOPICS = ("overview", "preview", "rental", "api", "business")

# Release readiness — the only switch that lets Ava describe a customer-facing
# feature as available. Everything here defaults to off, and no environment
# variable can set it: an unset variable is exactly the state a rushed deploy
# would try to claim. A file existing on disk, a route answering on localhost,
# or a green test suite is still not evidence of a live public service.
RELEASE_READINESS: dict[str, bool] = {
    # Turn on only after a live HTTP check of a public host that answers
    # /v1/sessions and /api/developer/* — not of a local server.
    "customer_api_publicly_reachable": False,
    # Turn on only after the minute-pack contract has been checked
    # against a live network, not merely configured in the environment.
    "minute_packs_connected": False,
}

FACTS = {
    "overview": (
        "Archava Onchain presents one ready-to-use conversational AI avatar. The "
        "visitor is speaking with that avatar live right now, and the site's "
        "wallet-free preview is how any visitor tries it, with no usage limit. "
        "BNB Testnet minute packs use demo mUSDT and become usable when the API is configured. Archava Platform is the "
        "separate custom integration offering for businesses."
    ),
    "preview": (
        "The preview is the live voice call the visitor is having right now, about "
        "two minutes (120 seconds) per call. It runs through Gemini realtime "
        "voice, a LiveKit room, and a Spatius-rendered avatar in this build, and "
        "it works without a wallet, account, signature, gas, or payment. The "
        "concept portraits on the page are not the live avatar."
    ),
    "rental": (
        "ArchavaRentalV2 and Archava Mock USDT are deployed on BNB Testnet, but "
        "checkout is not connected to this demo until the API has its QuickNode "
        "RPC and contract configuration. Packs contain 60 or 300 minutes and use "
        "test-only mUSDT. The API "
        "checks rental events and active access, then subtracts allocated room time "
        "from the shared wallet balance."
    ),
    "api": (
        "The current demo integrates Gemini, LiveKit, and Spatius internally. "
        "A wallet-signed API key dashboard and customer session API are built "
        "for local testing; the public customer API is not available yet. "
        "There is no SDK or public price. Ava cannot create or show a key."
    ),
    "business": (
        "Archava Platform is the custom integration path for businesses that want "
        "an avatar shaped around their brand, knowledge, and customer journey. "
        "The For business section gives examples such as hospitality, retail, "
        "education, events, and customer experience. This demo does not provision "
        "a custom integration or sell one automatically."
    ),
}

# The same worker receives wallet-free preview and minute-verified rooms. Room
# metadata selects the fact sheet; a paid caller must never be told the call they
# are already in is the free preview or that their minute pack is merely planned.
PAID_CALL_FACTS = {
    "overview": (
        "Archava Onchain presents one ready-to-use conversational AI avatar. "
        "The visitor is speaking with Ava live in a room opened after active "
        "purchased minutes and a remaining balance were verified. The website also has a separate wallet-free "
        "preview. Archava Platform is the custom business integration offering."
    ),
    "preview": (
        "The wallet-free preview is a separate short live call visitors can start "
        "from the landing page. This conversation is already running in a "
        "minute-verified room, not in that free preview. Gemini handles realtime "
        "voice, LiveKit carries the room, and Spatius renders the avatar."
    ),
    "rental": (
        "This room was created only after Archava verified purchased Ava minutes "
        "on BNB Chain and a remaining server-metered balance. The visitor is already "
        "speaking with Ava. The room reserves at most 30 minutes, and unused seconds "
        "return to the wallet balance when it closes. Ava cannot inspect the wallet, "
        "confirm a payment, or quote the current price or exact remaining balance."
    ),
}

# Wordings used only when the matching RELEASE_READINESS flag is on. Both keep
# the limits that stay true after a launch: Ava still issues no key, confirms no
# payment or quote the current onchain amount.
PACKS_CONNECTED = (
    "Onchain minute packs are connected. Archava verifies cumulative purchased "
    "minutes by summing rental events and checks active access, then subtracts "
    "server-metered room time before creating a paid room. The website reads a "
    "live mUSDT quote from the contract for 60 and 300 minute "
    "packs; Ava cannot quote its current amount, confirm payment, or buy a pack. "
    "No IDR retail price is approved."
)

API_REACHABLE = (
    "The customer API is publicly reachable. A developer connects a wallet, "
    "signs a dashboard challenge, and creates an Archava API key; the key's "
    "secret is shown exactly once and is never shown again. The customer's "
    "backend then uses that key server-side to open an Archava session, and the "
    "browser joins the returned LiveKit room with a short-lived token. A session "
    "is ended by the key that opened it, with the session's own ID. Usage is "
    "reported as allocated room seconds per session and deducted from the same "
    "wallet balance used by website calls. Ava cannot create, show, or verify a "
    "key or quote a current contract price."
)


def product_facts(topic: str, *, paid: bool = False) -> str:
    """Return only an allowlisted fact group, never arbitrary page content."""
    if topic not in FACTS:
        return "Unknown topic. Choose overview, preview, rental, api, or business."
    if paid and topic in PAID_CALL_FACTS:
        return PAID_CALL_FACTS[topic]
    released = _released_fact(topic)
    return FACTS[topic] if released is None else released


def _released_fact(topic: str) -> str | None:
    """Return the verified-launch wording, but only once its flag is on.

    Kept beside the planned wording rather than replacing it, so the default
    state of this module is still the honest one and turning a feature on is a
    single, reviewable line instead of a rewrite of the planned text.
    """
    if topic == "rental" and RELEASE_READINESS["minute_packs_connected"]:
        return PACKS_CONNECTED
    if topic == "api" and RELEASE_READINESS["customer_api_publicly_reachable"]:
        return API_REACHABLE
    return None


def release_readiness() -> dict[str, bool]:
    """Report the release flags, so a caller can show what is claimed live."""
    return dict(RELEASE_READINESS)


def product_knowledge(*, paid: bool = False) -> str:
    """Return the compact verified fact sheet injected into Ava's instructions."""
    facts = "\n".join(f"- {topic}: {product_facts(topic, paid=paid)}" for topic in PRODUCT_TOPICS)
    sections = "\n".join(
        f"- {section_id} ({title}): {PAID_SECTION_DETAILS[section_id] if paid else detail}"
        for section_id, (title, detail) in SECTIONS.items()
    )
    return (
        "VERIFIED ARCHAVA FACTS (describe planned features as planned):\n"
        f"{facts}\n\n"
        "SITE SECTIONS AVA MAY OPEN:\n"
        f"{sections}\n\n"
        "LIMITS: Ava receives only an allowlisted section ID, not the visitor's "
        "screen or page text. Ava cannot inspect a wallet, confirm a payment, "
        "quote a price, issue an API key, or guarantee a release date."
    )
