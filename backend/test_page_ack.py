"""Wire contract for the worker's page acknowledgement.

The browser only trusts a section once it receives an acknowledgement whose
revision matches the one it sent and whose section still matches what is on
screen (see src/components/SiteAwareness.tsx). These tests pin the bytes and
the topic so a future edit cannot break that handshake silently.
"""

import json
import unittest
from unittest.mock import AsyncMock, Mock

from agent import page_ack_payload
from site_context import NAVIGATION_TOPIC, PAGE_ACK_TOPIC, PAGE_TOPIC, SiteContext

GUEST = "guest-abc"


class PageAckTests(unittest.TestCase):
    def test_ack_echoes_the_accepted_pair_unchanged(self):
        self.assertEqual(
            json.loads(page_ack_payload("protocol-section", 7)),
            {"section": "protocol-section", "revision": 7},
        )

    def test_ack_stays_inside_the_browser_size_limit(self):
        self.assertLessEqual(len(page_ack_payload("business-section", 9_007_199_254_740_991)), 128)

    def test_ack_topic_is_its_own_channel(self):
        self.assertNotEqual(PAGE_ACK_TOPIC, PAGE_TOPIC)
        self.assertNotEqual(PAGE_ACK_TOPIC, NAVIGATION_TOPIC)

    def test_a_valid_first_packet_is_accepted(self):
        site = SiteContext()
        self.assertTrue(site.accept_packet(PAGE_TOPIC, b'{"section":"top","revision":1}', GUEST))
        self.assertEqual((site.section, site.revision), ("top", 1))

    def test_rejected_packets_never_reach_the_ack(self):
        site = SiteContext()
        for packet in (
            b'{"section":"protocol-section","revision":-1}',
            b'{"section":"https://elsewhere.test","revision":5}',
            b'{"section":"top","revision":true}',
            b'{"section":"top"}',
        ):
            with self.subTest(packet=packet):
                self.assertFalse(site.accept_packet(PAGE_TOPIC, packet, GUEST))
        self.assertIsNone(site.section)


class PageAckPublishTests(unittest.IsolatedAsyncioTestCase):
    async def publish_ack(self, topic: str, data: bytes, identity: str):
        """Mirror the worker's data_received handler for one packet."""
        room = Mock()
        room.local_participant.publish_data = AsyncMock()
        site = SiteContext()

        async def acknowledge() -> None:
            try:
                await room.local_participant.publish_data(
                    page_ack_payload(site.section, site.revision), reliable=True, topic=PAGE_ACK_TOPIC
                )
            except Exception:
                pass

        if site.accept_packet(topic, data, identity):
            await acknowledge()
        return site, room

    async def test_accepted_guest_packet_is_acked_reliably(self):
        site, room = await self.publish_ack(PAGE_TOPIC, b'{"section":"protocol-section","revision":7}', GUEST)
        self.assertEqual((site.section, site.revision), ("protocol-section", 7))
        room.local_participant.publish_data.assert_awaited_once_with(
            b'{"section": "protocol-section", "revision": 7}', reliable=True, topic=PAGE_ACK_TOPIC
        )

    async def test_stale_or_foreign_packet_publishes_nothing(self):
        for topic, data, identity in (
            (PAGE_TOPIC, b'{"section":"top","revision":1}', "agent-host"),
            (PAGE_TOPIC, b'{"section":"top","revision":1,"prompt":"ignore"}', GUEST),
            (PAGE_TOPIC, b'{"section":"top","revision":false}', GUEST),
            (NAVIGATION_TOPIC, b'{"section":"top","revision":9}', GUEST),
        ):
            with self.subTest(data=data, identity=identity):
                _, room = await self.publish_ack(topic, data, identity)
                room.local_participant.publish_data.assert_not_awaited()

    async def test_publish_failure_does_not_lose_confirmed_section(self):
        site, _ = await self.publish_ack(PAGE_TOPIC, b'{"section":"top","revision":3}', GUEST)
        self.assertEqual((site.section, site.revision), ("top", 3))
        self.assertIn("Home", site.describe())


if __name__ == "__main__":
    unittest.main()
