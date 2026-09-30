import re
import unittest
from unittest.mock import AsyncMock, Mock

from agent import INSTRUCTIONS, ArchavaHost
from knowledge import product_facts, product_knowledge
from site_context import SiteContext


class InstructionTests(unittest.TestCase):
    def flat_instructions(self):
        return re.sub(r"\s+", " ", INSTRUCTIONS)

    def test_ava_has_an_english_default_and_explicit_language_switch(self):
        self.assertIn("You are Ava, the live host for the Archava Onchain website", INSTRUCTIONS)
        self.assertIn("Speak English by default", INSTRUCTIONS)
        self.assertIn("explicitly asks you to use", INSTRUCTIONS)
        self.assertIn("one or two short sentences", INSTRUCTIONS)
        self.assertIn("introduce yourself once", INSTRUCTIONS)

    def test_product_and_page_answers_use_different_sources(self):
        self.assertIn(product_knowledge(), INSTRUCTIONS)
        self.assertIn("call get_current_page", INSTRUCTIONS)
        self.assertIn("call get_product_facts", INSTRUCTIONS)
        self.assertIn("use both get_current_page and get_product_facts", self.flat_instructions())
        self.assertIn("Do not infer product availability", self.flat_instructions())

    def test_unavailable_features_are_not_sold_as_live(self):
        self.assertIn("Never imply that a planned feature is already available", self.flat_instructions())
        self.assertIn("Never ask", INSTRUCTIONS)
        self.assertIn("customer API key", INSTRUCTIONS)

    def test_old_hospitality_demo_does_not_leak_into_archava(self):
        for leak in ("White Rock", "Melasti", "Daybed", "check_room_availability", "trigger_web3_booking"):
            pattern = rf"(?<![A-Za-z]){re.escape(leak)}(?![A-Za-z])"
            self.assertIsNone(re.search(pattern, INSTRUCTIONS), leak)


class HostTests(unittest.IsolatedAsyncioTestCase):
    async def test_page_tool_reports_only_confirmed_section(self):
        site = SiteContext()
        host = ArchavaHost(site, Mock())
        self.assertIn("not available yet", await host.get_current_page())
        site.accept_packet("archava.page", b'{"section":"protocol-section","revision":1}', "guest-abc")
        self.assertIn("planned connect, buy minutes, and converse", await host.get_current_page())

    async def test_product_tool_returns_verified_facts(self):
        host = ArchavaHost(SiteContext(), Mock())
        self.assertEqual(await host.get_product_facts("api"), product_facts("api"))
        self.assertIn("not available yet", await host.get_product_facts("api"))
        self.assertIn("Unknown topic", await host.get_product_facts("unknown"))

    async def test_navigation_publishes_only_allowed_sections(self):
        room = Mock()
        room.local_participant.publish_data = AsyncMock()
        host = ArchavaHost(SiteContext(), room)
        self.assertIn("For business", await host.show_site_section("business-section"))
        room.local_participant.publish_data.assert_awaited_once()
        self.assertIn("Unknown section", await host.show_site_section("https://elsewhere.test"))
        self.assertEqual(room.local_participant.publish_data.await_count, 1)


if __name__ == "__main__":
    unittest.main()
