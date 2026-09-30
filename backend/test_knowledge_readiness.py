"""Both states of every release flag in `knowledge.RELEASE_READINESS`.

A file on disk, a route answering on localhost, or a green suite is not evidence
that a customer-facing service is live, so the default state of the fact sheet
must stay the honest one and the only way to change it must be a deliberate,
reviewable flip. These tests hold both states at once: the default sheet, and
the sheet with a flag on, so a future orchestrator flip cannot silently invent
a feature or lose a limit that outlives the launch.
"""
import unittest
from unittest.mock import patch

import knowledge
from knowledge import product_facts, product_knowledge
from site_context import SECTIONS

PLANNED_API = "public customer API is not available yet"
PLANNED_RENTAL = "not connected to this demo"

# Claims no state is allowed to make. A mention of checkout or a pack is fine
# only in the negative, so the affirmative phrasings are what is banned here.
NEVER_CLAIMED = (
    "approved price",
    "official price",
    "published price",
    "Rp149,000",
    "Rp599,000",
    "Rp79,000",
    "Rp299,000",
    "connected minutes remaining",
    "24-hour rental",
    "checkout is live",
    "unlimited paid minutes",
)


class ReadinessDefaultTests(unittest.TestCase):
    def test_every_flag_defaults_to_off(self):
        readiness = knowledge.release_readiness()
        self.assertTrue(readiness, "at least one release flag should exist")
        for flag, value in readiness.items():
            self.assertIs(value, False, f"{flag} must default to off")

    def test_default_sheet_stays_on_the_truthful_public_status(self):
        notes = product_knowledge()
        self.assertIn(PLANNED_API, notes)
        self.assertIn(PLANNED_RENTAL, notes)

    def test_default_sheet_never_mentions_the_launch_wordings(self):
        notes = product_knowledge()
        self.assertNotIn("publicly reachable", notes)
        self.assertNotIn("verified server-side before a", notes)

    def test_the_flag_dict_is_a_copy_so_callers_cannot_flip_it(self):
        readiness = knowledge.release_readiness()
        readiness["customer_api_publicly_reachable"] = True
        self.assertIs(knowledge.RELEASE_READINESS["customer_api_publicly_reachable"], False)
        self.assertIn(PLANNED_API, product_facts("api"))

    def test_environment_variables_cannot_claim_a_live_service(self):
        with patch.dict(
            "os.environ",
            {
                "PACK_CONTRACT": "0x123",
                "API_KEYS_ENABLED": "true",
                "LIVEKIT_URL": "wss://example.invalid",
                "SPATIUS_API_KEY": "k",
            },
        ):
            notes = product_knowledge()
        self.assertIn(PLANNED_API, notes)
        self.assertIn(PLANNED_RENTAL, notes)
        self.assertIs(knowledge.RELEASE_READINESS["minute_packs_connected"], False)


class ApiReleasedTests(unittest.TestCase):
    def setUp(self):
        self.patch = patch.dict(
            knowledge.RELEASE_READINESS, {"customer_api_publicly_reachable": True}
        )
        self.patch.start()
        self.addCleanup(self.patch.stop)

    def test_the_live_wording_replaces_the_planned_one(self):
        self.assertNotIn(PLANNED_API, product_facts("api"))
        self.assertIn("publicly reachable", product_facts("api"))

    def test_the_live_wording_describes_the_flow_that_actually_ships(self):
        api = product_facts("api")
        self.assertIn("shown exactly once", api)
        self.assertIn("server-side", api)
        self.assertIn("short-lived token", api)
        self.assertIn("session's own ID", api)

    def test_the_live_warning_keeps_the_measured_metering_metric(self):
        self.assertIn("allocated room seconds", product_facts("api"))
        self.assertIn("deducted from the same wallet balance", product_facts("api"))

    def test_the_limit_that_outlives_the_launch_is_not_dropped(self):
        api = product_facts("api")
        self.assertIn("Ava cannot create, show, or verify a key", api)

    def test_other_topics_are_untouched_by_an_api_flip(self):
        self.assertIn(PLANNED_RENTAL, product_facts("rental"))
        self.assertIn("without a wallet", product_facts("preview"))

    def test_the_still_open_commercial_questions_survive_the_flip(self):
        self.assertIn("cannot create, show, or verify a key or quote", product_facts("api"))


class RentalReleasedTests(unittest.TestCase):
    def setUp(self):
        self.patch = patch.dict(knowledge.RELEASE_READINESS, {"minute_packs_connected": True})
        self.patch.start()
        self.addCleanup(self.patch.stop)

    def test_the_live_wording_replaces_the_planned_one(self):
        self.assertNotIn(PLANNED_RENTAL, product_facts("rental"))
        self.assertIn("verifies cumulative purchased minutes", product_facts("rental"))

    def test_the_live_wording_still_refuses_to_quote_commerce(self):
        rental = product_facts("rental")
        self.assertIn("live BNB quote from the contract", rental)
        self.assertIn("cannot quote its current amount, confirm payment", rental)

    def test_the_live_wording_describes_minute_packs(self):
        self.assertIn("60 and 300 minute packs", product_facts("rental"))

    def test_the_api_topic_stays_truthful_when_only_rental_is_connected(self):
        self.assertIn(PLANNED_API, product_facts("api"))


class SheetIntegrityTests(unittest.TestCase):
    """The sheet must stay whole in either state, not just the default one."""

    def assertSheetIntact(self, notes: str) -> None:
        self.assertIn("VERIFIED ARCHAVA FACTS", notes)
        self.assertIn("SITE SECTIONS AVA MAY OPEN", notes)
        self.assertIn("LIMITS", notes)
        for section_id, (title, _detail) in SECTIONS.items():
            self.assertIn(f"- {section_id} ({title})", notes)
        self.assertIn("Ava cannot inspect a wallet", notes)
        self.assertIn("issue an API key", notes)
        for claim in NEVER_CLAIMED:
            self.assertNotIn(claim, notes)

    def test_default_sheet_is_intact(self):
        self.assertSheetIntact(product_knowledge())

    def test_released_sheet_is_intact(self):
        with patch.dict(
            knowledge.RELEASE_READINESS,
            {
                "customer_api_publicly_reachable": True,
                "minute_packs_connected": True,
            },
        ):
            self.assertSheetIntact(product_knowledge())

    def test_a_released_api_never_contradicts_the_sheets_own_limits(self):
        with patch.dict(
            knowledge.RELEASE_READINESS, {"customer_api_publicly_reachable": True}
        ):
            notes = product_knowledge()
        # The sheet claims Ava cannot issue a key while the API fact says keys
        # are created in the dashboard. Both must be present, and neither may
        # read as if Ava can now issue one.
        self.assertIn("Ava cannot create, show, or verify a key", notes)
        self.assertIn("Ava cannot inspect a wallet", notes)
        self.assertNotIn("Ava can create an API key", notes)


if __name__ == "__main__":
    unittest.main()
