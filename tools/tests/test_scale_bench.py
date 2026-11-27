"""Check the scale benchmark's reading of sim-bench and its launch folder."""

import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools import scale_bench, scale_bench_client

SIM_OUTPUT = """loaded battle30.map seed 0x5EED0001 in 1.0s: 32 houses (29 computer), 0 spawned, 404 live objects
simulation_tick_ms=22
worker_threads=4
progress tick=100 mean_ms=2.5 mobile=375 lost=29 new=0 objects_made=193 shots_live=0 live_objects=433
ticks=3000 total_s=90.00 mean_ms=30.00 p50_ms=29.00 p95_ms=40.00 max_ms=900.00
main_thread_cpu_mean_ms=25.00 p50_ms=24.00 p95_ms=33.00 max_ms=700.00
after_orders_mean_ms=31.50 (ticks 301..=3000)
measured ticks=2700 mean_ms=31.50 p50_ms=30.25 p95_ms=41.00 p99_ms=55.75 max_ms=120.00 process_cpu_ms=63.00 cores=2.00
live_objects start=404 end=21000
ares=off
final_hash=0123456789abcdef
idle_sleep asleep_now=10 visits_counted=20
battle fewest_mobile=19800 lost=27000 objects_made=510000
"""


class ScaleBenchTests(unittest.TestCase):
    def run_sim(self, stdout, returncode=0, threads=4):
        done = mock.Mock(stdout=stdout, stderr='', returncode=returncode)
        with mock.patch.object(scale_bench.subprocess, 'run', return_value=done) as run:
            result = scale_bench.sim_run(Path('sim-bench'), Path('battle.map'), threads, ['--ticks', '3000'])
        return result, run.call_args.args[0]

    def test_a_run_reads_the_measured_window_and_the_battle(self):
        result, command = self.run_sim(SIM_OUTPUT)
        self.assertEqual(command, ['sim-bench', '--map', 'battle.map', '--scenario', 'battle',
                                   '--ticks', '3000', '--threads', '4'])
        self.assertEqual(
            (result['ticks'], result['mean_ms'], result['p50_ms'], result['p99_ms'], result['max_ms']),
            (2700, 31.5, 30.25, 55.75, 120.0))
        self.assertEqual((result['cores'], result['final_hash']), (2.0, '0123456789abcdef'))
        self.assertEqual((result['fewest_mobile'], result['lost'], result['objects_made']), (19800, 27000, 510000))
        # Every CPU thread: no --threads.
        _, command = self.run_sim(SIM_OUTPUT, threads=None)
        self.assertNotIn('--threads', command)

    def test_a_run_that_fails_or_reports_no_battle_is_an_error(self):
        with self.assertRaisesRegex(scale_bench.BenchError, 'status 1'):
            self.run_sim(SIM_OUTPUT + 'Error: "the battle stopped"\n', returncode=1)
        with self.assertRaisesRegex(scale_bench.BenchError, 'failed'):
            self.run_sim(SIM_OUTPUT.replace('battle fewest_mobile', 'battle fewest'))

    def test_a_figure_is_the_median_and_repeats_within_the_spread(self):
        runs = [{'mean_ms': value, 'p99_ms': value * 2} for value in (30.0, 31.0, 30.5)]
        with mock.patch('builtins.print'):
            steady = scale_bench.summarize('tick', runs, ('mean_ms', 'p99_ms'), 5.0)
            noisy = scale_bench.summarize('tick', [*runs, {'mean_ms': 36.0, 'p99_ms': 70.0}], ('mean_ms', 'p99_ms'), 5.0)
        self.assertEqual((steady['mean_ms'], steady['p99_ms'], steady['spread_percent']), (30.5, 61.0, 3.3))
        self.assertTrue(steady['repeatable'])
        self.assertEqual((noisy['spread_percent'], noisy['repeatable']), (20.0, False))

    def test_pools_reads_every_thread_and_counts(self):
        self.assertEqual(scale_bench.pools('all,4'), (None, 4))
        self.assertEqual(scale_bench.pools('4'), (4,))
        for bad in ('', 'four', '0', 'all,-1'):
            with self.assertRaises(scale_bench.argparse.ArgumentTypeError):
                scale_bench.pools(bad)

    def test_the_launch_folder_is_one_human_against_29_computers_on_the_map(self):
        with tempfile.TemporaryDirectory() as scratch, \
                mock.patch.dict(scale_bench_client.os.environ, {'RA2_DIR': '/retail "yr"'}):
            source = Path(scratch) / 'battle.map'
            source.write_text('[Basic]\nName=Battle\n', encoding='ascii')
            folder = scale_bench_client.launch_folder(Path(scratch) / 'client', source)
            spawn = (folder / 'spawn.ini').read_text(encoding='ascii')
            self.assertEqual((folder / 'spawnmap.ini').read_text(encoding='ascii'), '[Basic]\nName=Battle\n')
            self.assertIn('ra2_dir = "/retail \\"yr\\""', (folder / 'config.toml').read_text(encoding='utf-8'))
            self.assertEqual((folder / 'VERA.INI').read_bytes(), b'')
        self.assertIn('PlayerCount=1\nAIPlayers=29\n', spawn)
        self.assertIn('Scenario=spawnmap.ini\n', spawn)
        for section in ('[HouseHandicaps]', '[HouseCountries]', '[HouseColors]'):
            rows = spawn.split(section + '\n')[1].split('[')[0].splitlines()
            self.assertEqual([row.split('=')[0] for row in rows], [f'Multi{n}' for n in range(2, 31)])

    def test_a_client_run_needs_its_frame_cost_up_to_the_last_frame(self):
        with tempfile.TemporaryDirectory() as scratch:
            folder = Path(scratch)

            def play(report, exit_code=0):
                def run(command, **kwargs):
                    self.assertEqual(command[1:], ['-SPAWN'])
                    self.assertEqual(
                        (kwargs['env']['VERA20K_SPAWN_HIDDEN_WINDOW'], kwargs['env']['VERA20K_SPAWN_FRAME_COST'],
                         kwargs['env']['VERA20K_SPAWN_LEAVE_AT_FRAME']), ('1', '300', '900'))
                    if report is not None:
                        (folder / 'spawn_result.json').write_text(report, encoding='utf-8')
                    return mock.Mock(returncode=exit_code)
                with mock.patch.object(scale_bench_client.subprocess, 'run', side_effect=run):
                    return scale_bench_client.client_run(Path('vera20k'), folder, (300, 900), 60.0)

            cost = play('{"result":"quit","exit_code":0,"frame_cost":{"frame_ms":12.5,"last_frame":903}}')
            self.assertEqual((cost['frame_ms'], cost['last_frame']), (12.5, 903))
            winner = '{"result":"won","exit_code":10,"frame_cost":{"last_frame":900}}'
            self.assertEqual(play(winner, exit_code=10)['last_frame'], 900)
            with self.assertRaisesRegex(scale_bench.BenchError, 'exited with -6'):
                play(winner, exit_code=-6)
            with self.assertRaisesRegex(scale_bench.BenchError, 'ended early'):
                play('{"result":"quit","exit_code":0,"frame_cost":{"frame_ms":12.5,"last_frame":640}}')
            with self.assertRaisesRegex(scale_bench.BenchError, 'no frame cost'):
                play('{"result":"quit"}')
            with self.assertRaisesRegex(scale_bench.BenchError, 'no frame cost'):
                play(None)


if __name__ == '__main__':
    unittest.main()
