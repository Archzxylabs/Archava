"""Allowlisted page context delivered by the browser to Ava."""

import json
import re
from dataclasses import dataclass


SECTIONS = {
    "top": (
        "Home",
        "The hero introduces the Archava avatar and the wallet-free preview the "
        "visitor is using right now.",
    ),
    "catalog-section": (
        "The avatar",
        "The portraits are concept art; the live session renders a separate Spatius "
        "avatar, which is what the visitor is speaking with now. Minute-pack "
        "checkout becomes available when the BNB Chain contract is connected.",
    ),
    "protocol-section": (
        "How it works",
        "This section explains the planned connect, buy minutes, and converse "
        "flow. The current preview works without a wallet; conversation and avatar "
        "rendering run offchain.",
    ),
    "business-section": (
        "For business",
        "This section presents custom Archava Platform use cases for hospitality, "
        "retail, education, events, and customer experience. It does not activate "
        "a custom integration from this page.",
    ),
}

PAID_SECTION_DETAILS = {
    "top": "The hero introduces Ava and the separate wallet-free preview. This visitor is already in a minute-verified live room.",
    "catalog-section": "The portraits are concept art; the visitor is speaking with the separately rendered Spatius avatar now. Purchased minutes and a remaining balance were verified for this room.",
    "protocol-section": "This section explains connect, buy minutes, and converse. This room has passed minute-balance verification; voice and avatar rendering run offchain.",
    "business-section": SECTIONS["business-section"][1],
}

PAGE_TOPIC = "archava.page"
PAGE_ACK_TOPIC = "archava.page.ack"
NAVIGATION_TOPIC = "archava.navigation"


@dataclass
class SiteContext:
    section: str | None = None
    revision: int = -1
    paid: bool = False

    def accept_packet(self, topic: str | None, data: bytes, identity: str | None) -> bool:
        """Accept ordered section IDs only from the room's LiveKit guest."""
        participant_allowed = bool(identity) and (
            bool(re.fullmatch(r"0x[a-fA-F0-9]{40}", identity)) if self.paid
            else identity.startswith("guest-")
        )
        if topic != PAGE_TOPIC or not participant_allowed or len(data) > 128:
            return False
        try:
            payload = json.loads(data)
        except (UnicodeDecodeError, json.JSONDecodeError):
            return False
        if not isinstance(payload, dict) or set(payload) != {"section", "revision"}:
            return False
        section = payload["section"]
        revision = payload["revision"]
        if (
            not isinstance(section, str)
            or section not in SECTIONS
            or isinstance(revision, bool)
            or not isinstance(revision, int)
            or revision <= self.revision
            or revision > 9_007_199_254_740_991
        ):
            return False
        self.section = section
        self.revision = revision
        return True

    def describe(self) -> str:
        if self.section is None:
            return "The visitor's current website section is not available yet. Do not guess."
        title, detail = SECTIONS[self.section]
        if self.paid:
            detail = PAID_SECTION_DETAILS[self.section]
        return (
            f"The browser last confirmed the '{title}' section ({self.section}). "
            f"This may change as the visitor scrolls. {detail}"
        )
