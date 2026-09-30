"""Regression checks for the active-call framing.

The public bug: a visitor already talking to Ava was told they could call or try
the realtime avatar, as if the call had not started. The cause was pre-call
marketing wording in the prompt, the product facts, and the section
descriptions. These tests pin the fix so that wording cannot come back, while
leaving the frontend page-awareness protocol untouched.
"""

import json
import re
import unittest
from unittest.mock import Mock

from agent import INSTRUCTIONS, ArchavaHost, is_paid_room_metadata, make_instructions, page_ack_payload
from knowledge import product_facts, product_knowledge
from site_context import NAVIGATION_TOPIC, PAGE_ACK_TOPIC, PAGE_TOPIC, SECTIONS, SiteContext

GUEST = "guest-abc"
PACKET = lambda section, revision: json.dumps({"section": section, "revision": revision}).encode()


def flatten(text: str) -> str:
    return re.sub(r"\s+", " ", text)


def active_call_block() -> str:
    """The ACTIVE CALL section of the prompt, on its own."""
    match = re.search(r"^ACTIVE CALL:$(.*?)^LANGUAGE AND DELIVERY:$", INSTRUCTIONS, re.S | re.M)
    assert match is not None, "the prompt lost its ACTIVE CALL section"
    return flatten(match.group(1))


class ActiveCallPromptTests(unittest.TestCase):
    def test_every_utterance_is_treated_as_an_active_conversation(self):
        block = active_call_block()
        self.assertIn("live conversation that is already running", block)
        self.assertIn("speaking with you right now", block)
        self.assertIn("Never speak as if the call has not started yet", block)

    def test_the_caller_is_never_invited_to_start_the_call_again(self):
        block = active_call_block()
        self.assertIn('Do not invite this caller to click "Try Archava"', block)
        self.assertIn("start a call", block)
        self.assertIn("already running", block)

    def test_the_cta_is_explained_only_for_another_visitor_or_a_future_visit(self):
        block = active_call_block()
        self.assertIn("another visitor or a future visit", block)
        self.assertIn('"Try Archava" button', block)
        # The refusal and the exception live in the same section, in order:
        # refuse first, then explain on request.
        self.assertLess(
            block.index("Do not invite this caller"),
            block.index("another visitor or a future visit"),
        )

    def test_no_section_of_the_prompt_invites_the_caller_to_try_the_avatar(self):
        # Outside ACTIVE CALL, the only permitted mention is the future-visit rule.
        body = flatten(re.sub(r"^ACTIVE CALL:$.*?^LANGUAGE AND DELIVERY:$", "", INSTRUCTIONS, flags=re.S | re.M))
        self.assertNotIn("Try Archava", body)
        self.assertNotIn("start a call", body)
        self.assertNotIn("call the avatar", body)

    def test_identity_and_delivery_rules_survive(self):
        self.assertIn("You are Ava, the live host for the Archava Onchain website", INSTRUCTIONS)
        self.assertIn("Speak English by default", INSTRUCTIONS)
        self.assertIn("explicitly asks you to use", INSTRUCTIONS)
        self.assertIn("one or two short sentences", INSTRUCTIONS)
        self.assertIn("introduce yourself once", INSTRUCTIONS)
        self.assertIn(
            'Example: "Hi, I\'m Ava, Archava\'s live guide. You\'re speaking with me now."',
            INSTRUCTIONS,
        )

    def test_old_hospitality_demo_still_does_not_leak(self):
        for leak in ("White Rock", "Melasti", "Daybed", "check_room_availability", "trigger_web3_booking"):
            pattern = rf"(?<![A-Za-z]){re.escape(leak)}(?![A-Za-z])"
            self.assertIsNone(re.search(pattern, INSTRUCTIONS), leak)

    def test_paid_room_uses_paid_call_facts_instead_of_preview_facts(self):
        self.assertTrue(is_paid_room_metadata('{"product":"archava","preview":false}'))
        self.assertFalse(is_paid_room_metadata('{"product":"archava","preview":true}'))
        self.assertFalse(is_paid_room_metadata("broken"))
        paid_prompt = make_instructions(paid=True)
        self.assertIn("This room was opened after purchased minutes and a remaining balance were verified", paid_prompt)
        self.assertIn("not in that free preview", paid_prompt)
        self.assertNotIn("The current demo needs no wallet", paid_prompt)
        self.assertIn("The current demo needs no wallet", INSTRUCTIONS)
        self.assertIn("This room has passed minute-balance verification", product_knowledge(paid=True))


class StaleQuotaClaimTests(unittest.TestCase):
    def test_no_usage_quota_claim_survives_anywhere(self):
        sources = (INSTRUCTIONS, product_knowledge(), product_facts("preview"), product_facts("overview"))
        for source in sources:
            with self.subTest(source=source[:40]):
                self.assertNotIn("availability is limited", source.lower())
                self.assertNotIn("limited and shown by this site", source.lower())
                self.assertNotIn("quota", source.lower())

    def test_preview_fact_describes_the_live_call_and_its_timer(self):
        preview = product_facts("preview")
        self.assertIn("right now", preview)
        self.assertIn("120 seconds", preview)
        self.assertIn("without a wallet", preview)
        self.assertIn("Spatius-rendered avatar", preview)

    def test_preview_is_framed_as_running_not_as_something_to_start(self):
        preview = product_facts("preview").lower()
        self.assertIn("is the live voice call", preview)
        for stale in ("a visitor grants microphone access", "visitors can try", "the short live preview"):
            self.assertNotIn(stale, preview)

    def test_fact_sheet_is_still_injected_into_the_prompt(self):
        self.assertIn(product_knowledge(), INSTRUCTIONS)


class SectionDescriptionTests(unittest.TestCase):
    def test_section_details_do_not_invite_the_caller_to_start_anything(self):
        for section_id, (title, detail) in SECTIONS.items():
            with self.subTest(section=section_id):
                self.assertTrue(detail, f"{title} lost its description")
                lowered = detail.lower()
                self.assertNotIn("can start", lowered)
                self.assertNotIn("to start", lowered)
                self.assertNotIn("grants microphone", lowered)
                self.assertNotIn("you can try", lowered)

    def test_the_current_call_is_acknowledged_in_the_page_descriptions(self):
        site = SiteContext()
        expected = {"top": "using right now", "catalog-section": "speaking with now"}
        for section_id, phrase in expected.items():
            with self.subTest(section=section_id):
                self.assertTrue(site.accept_packet(PAGE_TOPIC, PACKET(section_id, site.revision + 1), GUEST))
                self.assertIn(phrase, site.describe())

    def test_minute_pack_and_business_wording_stays_accurate(self):
        self.assertIn("connect, buy minutes, and converse", SECTIONS["protocol-section"][1])
        self.assertIn("does not activate a custom integration", SECTIONS["business-section"][1])


class PageProtocolUnchangedTests(unittest.IsolatedAsyncioTestCase):
    async def test_topics_and_packet_shape_are_untouched(self):
        self.assertEqual(
            (PAGE_TOPIC, PAGE_ACK_TOPIC, NAVIGATION_TOPIC),
            ("archava.page", "archava.page.ack", "archava.navigation"),
        )
        self.assertEqual(
            json.loads(page_ack_payload("business-section", 42)),
            {"section": "business-section", "revision": 42},
        )

    async def test_paid_room_accepts_its_wallet_participant_and_describes_paid_context(self):
        site = SiteContext(paid=True)
        wallet = "0xCcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC"
        self.assertFalse(site.accept_packet(PAGE_TOPIC, PACKET("top", 1), GUEST))
        self.assertTrue(site.accept_packet(PAGE_TOPIC, PACKET("top", 1), wallet))
        self.assertIn("minute-verified live room", site.describe())
        self.assertNotIn("preview the visitor is using", site.describe())

    async def test_only_newer_allowlisted_guest_sections_are_accepted(self):
        site = SiteContext()
        self.assertTrue(site.accept_packet(PAGE_TOPIC, PACKET("business-section", 7), GUEST))
        # A repeated revision is stale, and an older revision is stale too.
        self.assertFalse(site.accept_packet(PAGE_TOPIC, PACKET("business-section", 7), GUEST))
        self.assertFalse(site.accept_packet(PAGE_TOPIC, PACKET("top", 6), GUEST))
        self.assertFalse(site.accept_packet(PAGE_TOPIC, PACKET("https://elsewhere.test", 9), GUEST))
        self.assertEqual((site.section, site.revision), ("business-section", 7))

    async def test_page_tool_still_reports_only_the_confirmed_section(self):
        site = SiteContext()
        host = ArchavaHost(site, Mock())
        self.assertIn("not available yet", await host.get_current_page())
        site.accept_packet(PAGE_TOPIC, PACKET("protocol-section", 4), GUEST)
        self.assertIn("connect, buy minutes, and converse", await host.get_current_page())


if __name__ == "__main__":
    unittest.main()
