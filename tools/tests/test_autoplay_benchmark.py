"""Focused tests of the manifest runner; no Rust binary or retail assets needed."""
from __future__ import annotations

from collections import Counter
from contextlib import redirect_stdout
import io
import json
from pathlib import Path
import sys
import tempfile
import threading
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from tools import autoplay_benchmark as b


def case(seed=20000):
    return dict(seed=seed, map="Arena.mmx", local="america", opponent="yuri",
                baseline=dict(outcome="lost", tick=1200))


def log(c, terminal="won at tick 2000", opening="10000 50000 0 1 1 0 0 0"):
    return (f"autoplay-match: Player on {c['map']}, seed {c['seed']:#x}, 200000 ticks, 512 per frame\n"
            f"0 {opening}\n"
            "1800 9000 40000 120 10 11 1 0 0\n"
            "[INFO] auto play: building=3 yard=1 fighters=4 pts=1500/1000\n"
            + (f"autoplay-match: {terminal}\n" if terminal else ""))


def fake_command(c, text, exit_code=0, delay=0):
    return [sys.executable, "-S", "-u", "-c",
            f"import time,sys; print({text!r},end='',flush=True); time.sleep({delay!r}); sys.exit({exit_code})"]


class ManifestTests(unittest.TestCase):
    def test_historical_counts_and_nested_tiers(self):
        data, cases = b.load_manifest(b.DEFAULT_MANIFEST, "all")
        self.assertEqual(len(cases), 1102)
        self.assertEqual([c['seed'] for c in cases], list(range(20000, 21102)))
        self.assertEqual(Counter(c['baseline']['outcome'] for c in cases),
                         {"won": 293, "lost": 709, "wallclock": 97, "timeout": 3})
        self.assertEqual(set(data['suites']['smoke']) <= set(data['suites']['dev']), True)
        self.assertEqual(len(data['suites']['smoke']), 24)
        self.assertEqual(len(data['suites']['dev']), 120)
        self.assertEqual(len(data['suites']['holdout']), 982)
        self.assertFalse(set(data['suites']['dev']) & set(data['suites']['holdout']))
        self.assertEqual(b.select_suites(cases), data['suites'])
        eb = next(c for c in cases if c['seed'] == 20145)
        self.assertEqual(eb['baseline']['wall_warnings'], 668)
        self.assertEqual(eb['baseline']['outcome'], 'wallclock')

    def test_all_manifest_cases_have_the_observed_opening(self):
        data, cases = b.load_manifest(b.DEFAULT_MANIFEST, "all")
        self.assertEqual({tuple(c['baseline']['opening']) for c in cases}, {(10000, 50000, 1, 1)})
        self.assertEqual(len({c['baseline']['log'] for c in cases}), 1102)
        self.assertEqual(len({c['baseline']['log_sha256'] for c in cases}), 1102)


class ClassificationTests(unittest.TestCase):
    def test_terminal_requires_clean_exit_and_exact_header(self):
        c = case()
        self.assertEqual(b.classify(log(c), c, b.CONFIG, 0, False)['outcome'], 'won')
        self.assertEqual(b.classify(log(c), c, b.CONFIG, 2, False)['outcome'], 'nonzero_exit')
        self.assertEqual(b.classify(log(c), c, b.CONFIG, -11, False)['outcome'], 'crash')
        self.assertEqual(b.classify(log(case(20001)), c, b.CONFIG, 0, False)['outcome'], 'protocol_error')
        self.assertEqual(b.classify(log(c, 'timeout at tick 1800'), c, b.CONFIG, 0, False)['outcome'], 'protocol_error')

    def test_opening_diagnostics_distinguish_missing_ready_and_deadline(self):
        self.assertIsNone(b.diagnostics('')['opening_purchase_requests'])
        self.assertIsNone(b.diagnostics('')['wave_ready_tick'])
        events = ('auto play: mobilization tick=45 active=true vehicles=0 points=0 floor=None\n'
                  'auto play: mobilization_purchase tick=900 type=MTNK free=700 floor=700\n'
                  'auto play: mobilization_exit tick=3000 reason=wave-ready vehicles=3 points=900\n'
                  'auto play: local_counter_purchase tick=4000 type=SNIPE\n')
        row = b.diagnostics(events)
        self.assertEqual(row['wave_ready_tick'], 3000)
        self.assertEqual(row['opening_purchase_requests'], 1)
        self.assertEqual(row['local_counter_requests'], 1)
        self.assertIsNone(row['opening_deadline_tick'])
        row = b.diagnostics('auto play: mobilization_exit tick=9000 reason=deadline\n')
        self.assertEqual(row['opening_deadline_tick'], 9000)
        self.assertIsNone(row['wave_ready_tick'])

    def test_safety_diagnostics_distinguish_unknown_from_zero(self):
        self.assertIsNone(b.diagnostics('')['safety_veto_reports'])
        events = 'auto play: safety_continuity tick=0\n'
        self.assertEqual(b.diagnostics(events)['casualty_entries'], 0)
        events += ('auto play: safety_veto tick=50 actor=4 engineer=true\n'
                   'auto play: casualty_enter tick=60 actor=8 hp=200\n'
                   'auto play: casualty_recovered tick=900 actor=8 hp=600\n')
        row = b.diagnostics(events)
        self.assertEqual((row['safety_veto_reports'], row['casualty_entries'], row['casualty_recovered']), (1,1,1))

    def test_readiness_and_siege_diagnostics_distinguish_absent_and_zero(self):
        old = b.diagnostics('')
        self.assertIsNone(old['readiness_samples'])
        self.assertIsNone(old['siege_post_requests'])
        initial = 'auto play: readiness tick=0 roster=0 available=0 raw_hp=0 available_hp=0 attack=0\n'
        self.assertEqual(b.diagnostics(initial)['siege_post_requests'], 0)
        self.assertIsNone(b.diagnostics(initial)['median_available_hp_pct'])
        row = b.diagnostics(initial +
            'auto play: readiness tick=600 roster=8 available=4 raw_hp=2000 available_hp=1000 attack=2\n'
            'auto play: readiness tick=1200 roster=10 available=8 raw_hp=3000 available_hp=2400 attack=5\n'
            'auto play: siege_post_request tick=750 actor=17 target=38\n')
        self.assertEqual(row['readiness_samples'], 3)
        self.assertEqual(row['peak_available_hp'], 2400)
        self.assertEqual(row['max_unavailable_hp'], 1000)
        self.assertEqual(row['median_available_hp_pct'], 65.0)
        self.assertEqual(row['siege_post_requests'], 1)

    def test_convergence_diagnostics_preserve_missing_and_count_events(self):
        self.assertIsNone(b.diagnostics('')['front_deferred_events'])
        initial = 'auto play: convergence tick=0 economy_rest=false\n'
        self.assertEqual(b.diagnostics(initial)['economy_rest_entries'], 0)
        row = b.diagnostics(initial +
            'auto play: economy_rest tick=300 active=true credits=200 income=Some(300)\n'
            'auto play: convergence tick=600 economy_rest=true\n'
            'auto play: local_contact tick=1200 target=8 cell=(3, 4)\n'
            'auto play: front_deferred tick=2400 lane=9 target=10 cell=(7, 8)\n')
        self.assertEqual(tuple(row[key] for key in b.CONVERGENCE_FIELDS), (1,1,1,1))

    def test_factory_handoff_reports_distinguish_observed_release_and_loss(self):
        self.assertTrue(all(b.diagnostics('')[k] is None for k in b.FACTORY_FIELDS))
        initial = 'auto play: factory_handoff tick=0 pending=0\n'
        self.assertEqual(b.diagnostics(initial)['factory_handoff_stalled'], 0)
        row = b.diagnostics(initial +
            'auto play: factory_handoff tick=600 pending=2\n'
            'auto play: factory_handoff_end tick=615 actor=7 factory=1 elapsed=15 orderable=true\n'
            'auto play: factory_handoff_end tick=617 actor=8 factory=1 elapsed=17 orderable=false\n'
            'auto play: factory_handoff_stalled tick=1050 actor=9 factory=1 elapsed=450\n'
            'auto play: factory_handoff_veto tick=1051 actor=9\n'
            'auto play: expansion_risk_veto tick=1100 actor=11 plot=Some((3, 4)) native=None exposed=false\n')
        self.assertEqual(tuple(row[k] for k in b.FACTORY_FIELDS), (2,2,2,1,1,1,1))

    def test_assault_diagnostics_are_reports_not_confirmed_kills(self):
        self.assertTrue(all(b.diagnostics('')[k] is None for k in b.ASSAULT_FIELDS))
        row = b.diagnostics('auto play: combat_doctrine tick=0\n'
            'auto play: readiness tick=600 roster=8 available=7 raw_hp=2400 available_hp=2100 attack=0\n'
            'auto play: readiness tick=1200 roster=8 available=7 raw_hp=2400 available_hp=2100 attack=5\n'
            'auto play: assault_commit tick=1220 target=8 count=5 hp=1500\n'
            'auto play: assault_end tick=1300 target=8 reason=forecast-changed start_hp=1500 remaining_hp=1200\n'
            'auto play: assault_end tick=1400 target=9 reason=target-not-visible\n'
            'auto play: assault_end tick=1600 target=10 reason=no-damage-progress\n'
            'auto play: front_guard_veto tick=1400 actor=3 infantry=true released=false\n')
        self.assertEqual(tuple(row[k] for k in b.ASSAULT_FIELDS), (1,3,1,1,1,1,1))
        old = b.diagnostics('auto play: readiness tick=600 roster=8 available=7 raw_hp=2400 available_hp=2100 attack=0\n')
        self.assertEqual(old['empty_fist_samples'], 1)
        self.assertIsNone(old['assault_commits'])

    def test_budget_and_cohort_feedback_keep_missing_distinct_from_zero(self):
        self.assertTrue(all(b.diagnostics("")[k] is None for k in (*b.ECONOMY_FIELDS,*b.FEEDBACK_FIELDS)))
        idle=b.diagnostics("auto play: combat_feedback tick=0\n")
        self.assertEqual(idle["assault_feedback_reports"],0)
        self.assertIsNone(idle["budget_samples"])
        self.assertIsNone(b.diagnostics("")["assault_feedback_reports"])
        row=b.diagnostics("auto play: economy_budget tick=0 cash=10000 spent=0 committed=0 bank=0 proposed=800 rest=false\n"
            "auto play: economy_budget tick=600 cash=900 spent=2000 committed=900 bank=2000 proposed=0 rest=true\n"
            "auto play: economy_budget tick=1200 cash=1400 spent=2100 committed=100 bank=0 proposed=200 rest=true\n"
            "auto play: assault_feedback tick=1200 target=8 missing_owned=1 removed_value=900 damage_value=450 target_damage=100 failures=2 cooldown=1800 visible=true\n")
        self.assertEqual(tuple(row[k] for k in b.ECONOMY_FIELDS),(3,1,2100,900,2000,1))
        self.assertEqual(tuple(row[k] for k in b.FEEDBACK_FIELDS),(1,1,900,450,100,1800))

    def test_tick_rate_is_observed_not_estimated_terminal_tick(self):
        self.assertEqual(b.observed_tick_rate(dict(tick=1800, seconds=9, tick_source='last_sample')), 200)
        self.assertIsNone(b.observed_tick_rate(dict(tick=1800, seconds=0)))
        self.assertIsNone(b.observed_tick_rate(dict(tick=None, seconds=20)))

    def test_wallclock_tick_is_last_sample_not_zero(self):
        c = case()
        row = b.classify(log(c, None), c, b.CONFIG, -9, True)
        self.assertEqual((row['outcome'], row['tick'], row['tick_source']), ('wallclock', 1800, 'last_sample'))
        self.assertEqual(b.classify('', c, b.CONFIG, -9, True)['outcome'], 'protocol_error')
        self.assertEqual(b.classify(log(c, None), c, b.CONFIG, 0, False)['outcome'], 'protocol_error')

    def test_opening_drift_and_observed_diagnostics(self):
        c = case()
        text = log(c, opening='10000 10000 0 1 1 0 0 0') + (
            'auto play: force_recovery tick=100 recent_peak=4000 fighting_points=1000\n'
            'auto play: force_recovery_exit tick=1900 elapsed=1800 recent_peak=4000 fighting_points=100 reason=timeout\n'
            'auto play: site_release tick=2000\n'
            'auto play: site_release_observed tick=2001 product_departed=false\n'
            'auto play: site_release_observed tick=2002 product_departed=true\n')
        row = b.classify(text, c, b.CONFIG, 0, False)
        self.assertFalse(row['opening_matches'])
        self.assertEqual((row['recovery_entries'], row['recovery_timeout_exits'], row['recovery_ready_exits']), (1, 1, 0))
        self.assertEqual((row['site_release_requests'], row['wall_product_departures']), (1, 1))


class ProcessTests(unittest.TestCase):
    def test_real_child_timeout_and_durable_receipt(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            c = case()
            # Allow the child interpreter to start on a loaded CI host;
            # the simulated match still sleeps well past the test-only cap.
            config = dict(b.CONFIG, wall_seconds=2.5)
            with patch.object(b, 'command', return_value=fake_command(c, log(c, None), delay=10)):
                row = b.run_case(Path(sys.executable), root, c, config, root, 'test', threading.Event())
            self.assertEqual(row['outcome'], 'wallclock')
            self.assertEqual(row['tick'], 1800)
            self.assertEqual(b.load_receipts(root, [c], 'test')[c['seed']], row)
            (root / row['log']).write_text('tampered', encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'changed log'):
                b.load_receipts(root, [c], 'test')

    def test_cancelled_child_preserves_log_not_a_result(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            c = case()
            stop = threading.Event()
            timer = threading.Timer(.3, stop.set)
            timer.start()
            try:
                with patch.object(b, 'command', return_value=fake_command(c, log(c, None), delay=5)):
                    row = b.run_case(Path(sys.executable), root, c, b.CONFIG, root, 'test', stop)
            finally:
                timer.cancel()
            self.assertIsNone(row)
            self.assertTrue((root / 'logs' / '20000.log').exists())
            self.assertFalse((root / 'results' / '20000.json').exists())

    def test_real_child_nonzero_exit_is_not_a_loss(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            c = case()
            with patch.object(b, 'command', return_value=fake_command(c, log(c), exit_code=2)):
                row = b.run_case(Path(sys.executable), root, c, b.CONFIG, root, 'test', threading.Event())
            self.assertEqual(row['outcome'], 'nonzero_exit')
            self.assertEqual(row['exit_code'], 2)


class RunnerTests(unittest.TestCase):
    def test_staged_resume_compare_and_identity_guards(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            retail = root / 'retail'; retail.mkdir()
            (retail / 'rulesmd.ini').write_text('[General]\n', encoding='utf-8')
            cases = [case(20000), case(20001), case(20002)]
            manifest = root / 'manifest.json'
            b.atomic_json(manifest, dict(schema=1, config=b.CONFIG, cases=cases,
                                        suites=dict(smoke=[20000], dev=[20000, 20001], all=[20000, 20001, 20002], holdout=[20002])))
            binary = root / 'test-binary'; binary.write_bytes(b'build one')
            args = SimpleNamespace(manifest=manifest, suite='all', stage='smoke', binary=binary, source_dir=root,
                                   ra2_dir=retail, run_dir=root / 'run', label='candidate', jobs=2)
            def child(binary, ra2, c, config):
                return fake_command(c, log(c))
            with patch.object(b, 'command', side_effect=child) as cmd, redirect_stdout(io.StringIO()):
                self.assertEqual(b.execute(args), 0)
                self.assertEqual(cmd.call_count, 1)
                first_log = (args.run_dir / 'results' / '20000.json').read_bytes()
                self.assertEqual(b.execute(args), 0)
                self.assertEqual(cmd.call_count, 1)
                args.stage = 'dev'
                self.assertEqual(b.execute(args), 0)
                self.assertEqual(cmd.call_count, 2)
                self.assertEqual((args.run_dir / 'results' / '20000.json').read_bytes(), first_log)
                compare_args = SimpleNamespace(manifest=manifest, suite='dev', run_dir=args.run_dir,
                                               before=None, output=root / 'paired.csv')
                self.assertEqual(b.compare(compare_args), 0)
                self.assertEqual(len((root / 'paired.csv').read_text().splitlines()), 3)
                compare_args.suite = 'all'
                self.assertEqual(b.compare(compare_args), 2)  # 1 missing
                args.jobs = 3
                with self.assertRaisesRegex(ValueError, 'jobs'):
                    b.execute(args)
                args.jobs = 2
                binary.write_bytes(b'build two')
                with self.assertRaisesRegex(ValueError, 'binary_sha256'):
                    b.execute(args)
                binary.write_bytes(b'build one')
                (retail / 'rulesmd.ini').write_text('[General]\nBuildSpeed=.1', encoding='utf-8')
                with self.assertRaisesRegex(ValueError, 'retail_inputs'):
                    b.execute(args)
                self.assertEqual(cmd.call_count, 2)


if __name__ == '__main__':
    unittest.main()
