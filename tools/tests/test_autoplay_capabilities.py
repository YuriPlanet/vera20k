import unittest
from tools import autoplay_capabilities as c

def sample(tick, roster, aa=0, repair=0):
    return (f"auto play: v56_capabilities tick={tick} roster=[{roster}]"
            f" hulls=6 mobile_aa={aa} repair_mode={repair} garrisoned=0"
            " capture_mission=1 ready_attack=2 available_idle=3\n")

class CapabilityReportTests(unittest.TestCase):
    def test_old_logs_leave_capabilities_unknown(self):
        result = c.summarize_log("auto play: readiness tick=600 roster=1")
        self.assertIsNone(result["observed_type_peak"])
        self.assertIsNone(result["event_counts"]["rotation_extract"])
        self.assertIsNone(result["sampled_metric_peaks"])

    def test_observations_are_distinct_from_proposals(self):
        text = sample(600,"MTNK:5,FV:1",aa=1)
        text += "auto play: v56_purchase_proposal tick=800 type=SREF cost=1200 free=40\n"
        text += sample(1200,"MTNK:3,FV:2",aa=1,repair=1)
        result = c.summarize_log(text)
        self.assertEqual(result["observed_type_peak"], {"MTNK":5, "FV":2})
        self.assertNotIn("SREF", result["observed_type_first_tick"])
        self.assertEqual(result["purchase_proposals_by_type"], {"SREF":1})
        self.assertEqual(result["sampled_metric_first_positive_tick"]["repair_mode"],1200)
        self.assertEqual(result["sampled_metric_peaks"]["garrisoned"],0)
        self.assertEqual(result["event_counts"]["garrison_confirmed"],0)

    def test_partial_instrumentation_does_not_fake_roster(self):
        text = ("auto play: v56_quality_budget tick=600 research_bank=1000 unit_bank=0\n"
                "auto play: v56_rotation_extract tick=605 actor=3 target=4\n")
        result = c.summarize_log(text)
        self.assertEqual(result["event_counts"]["rotation_extract"],1)
        self.assertIsNone(result["event_counts"]["garrison_confirmed"])
        self.assertIsNone(result["observed_type_peak"])
        self.assertEqual(result["peak_research_hold"],1000)
        self.assertEqual(result["simultaneous_hold_samples"],0)

    def test_first_observed_uses_tick_order_and_rejects_malformed_roster(self):
        result = c.summarize_log(sample(1200,"SREF:2")+sample(600,"SREF:1"))
        self.assertEqual(result["observed_type_first_tick"]["SREF"],600)
        with self.assertRaises(ValueError):
            c.summarize_log(sample(600,"FV:no"))
