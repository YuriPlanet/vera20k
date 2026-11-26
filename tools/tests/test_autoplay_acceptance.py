"""Development gate tests; no game executable required."""
import unittest
from tools import autoplay_acceptance as a


def rows():
    return {i:dict(outcome="won" if i<36 else "lost" if i<96 else "wallclock",
                   tick=50000,seconds=200,opening_matches=True)
            for i in range(120)}


class AcceptanceTests(unittest.TestCase):
    def test_boundary_pass_and_missing_receipt_fails_closed(self):
        r=rows()
        self.assertTrue(a.evaluate(r,range(120))["gate_pass"])
        r.pop(119)
        self.assertFalse(a.evaluate(r,range(120))["gate_pass"])

    def test_regression_in_wins_and_early_losses(self):
        r=rows()
        r[0]["outcome"]="lost"
        for i in range(36,61):
            r[i]["tick"]=20000
        report=a.evaluate(r,range(120))
        self.assertFalse(report["checks"]["wins"])
        self.assertFalse(report["checks"]["early_losses"])

    def test_missing_rate_and_protocol_drift_are_not_success(self):
        r=rows()
        r[0]["seconds"]=None
        self.assertFalse(a.evaluate(r,range(120))["checks"]["throughput"])
        r[1]["opening_matches"]=False
        self.assertFalse(a.evaluate(r,range(120))["checks"]["complete_valid_suite"])

    def test_missing_diagnostics_remain_unknown(self):
        r=rows()
        report=a.evaluate(r,range(120))
        self.assertIsNone(report["diagnostics"]["safety_veto_reports"]["total"])
        r[0]["safety_veto_reports"]=10
        d=a.evaluate(r,range(120))["diagnostics"]["safety_veto_reports"]
        self.assertEqual(d,dict(observed_cases=1,total=10,per_10000_ticks=2.0))
