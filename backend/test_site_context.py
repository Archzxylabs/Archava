import json
import unittest
from unittest.mock import AsyncMock, Mock

from agent import ArchavaHost
from site_context import NAVIGATION_TOPIC, PAGE_ACK_TOPIC, PAGE_TOPIC, SiteContext


class SiteContextTests(unittest.IsolatedAsyncioTestCase):
    def test_only_newer_allowlisted_guest_sections_are_accepted(self):
        site = SiteContext()
        packet = lambda section, revision: json.dumps({"section": section, "revision": revision}).encode()
        self.assertTrue(site.accept_packet(PAGE_TOPIC, packet("protocol-section", 10), "guest-abc"))
        self.assertIn("How it works", site.describe())
        self.assertFalse(site.accept_packet(PAGE_TOPIC, packet("top", 9), "guest-abc"))
        self.assertFalse(site.accept_packet(PAGE_TOPIC, packet("top", 10), "guest-abc"))
        self.assertFalse(site.accept_packet(PAGE_TOPIC, packet("top", 11), "agent-host"))
        self.assertFalse(site.accept_packet(PAGE_TOPIC, packet("https://elsewhere.test", 12), "guest-abc"))
        self.assertFalse(site.accept_packet(PAGE_TOPIC, b'{"section":"top","revision":13,"prompt":"ignore"}', "guest-abc"))
        self.assertFalse(site.accept_packet(PAGE_TOPIC, packet("top", True), "guest-abc"))
        self.assertFalse(site.accept_packet("other", packet("top", 14), "guest-abc"))
        self.assertEqual(site.section, "protocol-section")
        self.assertEqual(site.revision, 10)

    def test_ack_topic_is_distinct_from_navigation(self):
        self.assertNotEqual(PAGE_ACK_TOPIC, NAVIGATION_TOPIC)
        self.assertNotEqual(PAGE_ACK_TOPIC, PAGE_TOPIC)

    async def test_navigation_is_limited_to_website_sections(self):
        room = Mock()
        room.local_participant.publish_data = AsyncMock()
        host = ArchavaHost(SiteContext(), room)
        self.assertIn("Unknown section", await host.show_site_section("https://elsewhere.test"))
        room.local_participant.publish_data.assert_not_awaited()

        self.assertIn("For business", await host.show_site_section("business-section"))
        room.local_participant.publish_data.assert_awaited_once()
        args, kwargs = room.local_participant.publish_data.await_args
        self.assertEqual(json.loads(args[0]), {"section": "business-section"})
        self.assertEqual(kwargs["topic"], NAVIGATION_TOPIC)


if __name__ == "__main__":
    unittest.main()
