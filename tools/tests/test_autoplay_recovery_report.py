import unittest
from tools import autoplay_recovery_report as r


class RecoveryReportTests(unittest.TestCase):
    def test_old_log_is_unknown_and_marker_is_observed_zero(self):
        self.assertIsNone(r.asset_counts("old log"))
        self.assertEqual(r.asset_counts("auto play: asset_control_metrics tick=1 schema=1"),
                         dict(all=0, from_nonhostile=0, nonhostile_income=0))

    def test_only_observed_ownership_is_counted(self):
        text = "auto play: asset_control_metrics tick=1 schema=1\\n"
        text += "auto play: neutral_asset_purchase tick=9 free=500\\n"
        text += "auto play: asset_control_confirmed tick=20 target=7 type=CAOILD from_nonhostile=true income=true\\n"
        text += "auto play: asset_control_confirmed tick=30 target=8 type=GATECH from_nonhostile=false income=false"
        self.assertEqual(r.asset_counts(text), dict(all=2, from_nonhostile=1, nonhostile_income=1))

    def test_partial_smoke_does_not_pass(self):
        rows = {0: dict(outcome="won", tick=1200, seconds=2, opening_matches=True)}
        result = r.summarize(rows, set(range(120)))
        self.assertFalse(result["complete_dev"])
        self.assertFalse(result["formal_gate"])
        self.assertEqual(result["valid"], 1)
        self.assertEqual(len(result["missing"]), 119)
        self.assertIsNone(result["focus"][0]["outcome"])

    def test_paired_does_not_turn_missing_or_invalid_rows_into_losses(self):
        def row(outcome, opening=True):
            return dict(outcome=outcome, tick=1200, seconds=2, opening_matches=opening)
        old = {1: row("won"), 2: row("lost"), 3: row("won"), 4: row("won")}
        new = {1: row("lost"), 2: row("won"), 4: row("lost", False)}
        result = r.paired(old, new, {1, 2, 3, 4})
        self.assertEqual(result["paired"], 2)
        self.assertEqual(result["gained_wins"], [2])
        self.assertEqual(result["lost_wins"], [1])
