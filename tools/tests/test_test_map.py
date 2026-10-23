"""Pin the committed test maps to the commands that regenerate them.

Each entry in tools/run_test_map.py records its `tools.scale_map` invocation; this
runs that invocation and requires byte-identical output, so a hand-edited map
fails instead of silently diverging from the shape its entry documents.
"""

import unittest
from pathlib import Path
from unittest import mock

from tools import run_test_map
from tools.scale_map import build_map
from tools.run_test_map import MAPS

ROOT = Path(__file__).resolve().parents[2]


def regenerate(entry) -> bytes:
    """Run the entry's recorded regeneration command in-process."""
    command = list(entry.regenerate)
    size, starts = 250, None
    index = 0
    while index < len(command):
        if command[index] == "--size":
            size = int(command[index + 1])
            index += 2
        elif command[index] == "--starts":
            starts = int(command[index + 1])
            index += 2
        else:
            index += 1
    return build_map(size, starts).encode("ascii")


class TestMapRegistryTests(unittest.TestCase):
    def test_every_registered_map_matches_its_regeneration_command(self):
        for entry in MAPS:
            with self.subTest(map=entry.name):
                path = ROOT / entry.path
                self.assertTrue(path.is_file(), f"{entry.path} is missing")
                committed = path.read_bytes()
                committed.decode("ascii")  # scale_map writes ASCII
                self.assertEqual(
                    committed,
                    regenerate(entry),
                    f"{entry.path} does not match its recorded command "
                    f"({' '.join(entry.regenerate)})",
                )

    def test_map_names_are_unique(self):
        names = [entry.name for entry in MAPS]
        self.assertEqual(len(names), len(set(names)))

    def test_every_map_declares_a_scenario_and_a_regeneration(self):
        for entry in MAPS:
            self.assertTrue(entry.scenario, entry.name)
            self.assertIn("tools.scale_map", " ".join(entry.regenerate), entry.name)

    def test_documented_headless_command_forwards_scenario_and_ticks(self):
        with mock.patch.object(run_test_map, "headless", return_value=7) as headless:
            self.assertEqual(run_test_map.main(["match", "--ticks", "300"]), 7)
            headless.assert_called_once_with(MAPS[1], "match", True, ["--ticks", "300"])

    def test_documented_interactive_command_selects_client(self):
        with mock.patch.object(run_test_map, "interactive", return_value=0) as interactive:
            self.assertEqual(run_test_map.main(["flat", "--interactive"]), 0)
            interactive.assert_called_once_with(MAPS[0], [])

    def test_interactive_preview_has_a_base_unit_and_preserves_explicit_spawns(self):
        executable = ROOT / "target" / "recorded-game"
        for requested in (None, "SMCV:1,E2:2"):
            environment = {} if requested is None else {"RA2_DEBUG_SPAWN_UNITS": requested}
            with (
                self.subTest(spawns=requested),
                mock.patch.dict(run_test_map.os.environ, environment, clear=True),
                mock.patch.object(run_test_map.cargo_run, "invoke", return_value=0),
                mock.patch.object(
                    run_test_map.cargo_run, "resolve_binary", return_value=(executable, "fast")
                ),
                mock.patch.object(run_test_map.subprocess, "call", return_value=0) as launch,
            ):
                self.assertEqual(run_test_map.interactive(MAPS[0], []), 0)
                spawns = launch.call_args.kwargs["env"].get("RA2_DEBUG_SPAWN_UNITS")
                self.assertIsInstance(spawns, str)
                if requested is None:
                    self.assertIn("AMCV", [entry.split(":")[0] for entry in spawns.split(",")])
                else:
                    self.assertEqual(spawns, requested)

    def test_interactive_build_resolves_owned_binary_before_launch(self):
        executable = ROOT / "target" / "recorded-game"
        with (
            mock.patch.object(run_test_map.cargo_run, "invoke", return_value=0) as build,
            mock.patch.object(
                run_test_map.cargo_run, "resolve_binary", return_value=(executable, "fast")
            ) as resolve,
            mock.patch.object(run_test_map.subprocess, "call", return_value=7) as launch,
        ):
            self.assertEqual(run_test_map.interactive(MAPS[0], ["--features", "phobos"]), 7)
            build.assert_called_once_with(
                ["build", "-p", "vera20k", "--bin", "vera20k", "--profile", "fast",
                 "--features", "phobos"], cwd=ROOT,
            )
            resolve.assert_called_once_with(ROOT, "vera20k", "fast")
            self.assertEqual(launch.call_args.args[0], [str(executable)])
            self.assertEqual(launch.call_args.kwargs["env"]["RA2_QUICKPLAY"], str(MAPS[0].absolute))

    def test_failed_map_build_does_not_launch_an_old_binary(self):
        with (
            mock.patch.object(run_test_map.cargo_run, "invoke", return_value=101),
            mock.patch.object(run_test_map.cargo_run, "resolve_binary") as resolve,
            mock.patch.object(run_test_map.subprocess, "call") as launch,
        ):
            self.assertEqual(run_test_map.headless(MAPS[1], "match", True, []), 101)
            resolve.assert_not_called()
            launch.assert_not_called()
