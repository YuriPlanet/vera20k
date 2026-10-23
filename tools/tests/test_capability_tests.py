"""Pin the capability registry to Cargo.toml so a new flag cannot skip coverage.

The registry in tools/capability_tests.py is the single stated home of the
optional capability list: a feature that no build compiles is exactly the gap
issue #2798 describes, so adding one to the manifest without registering it
must fail here. The capability workflow runs this module before it compiles
anything; the wider Python suite (`python-tools.yml`) is manual.
"""

import os
import tomllib
import unittest
from pathlib import Path
from unittest import mock

from tools import capability_tests
from tools.capability_tests import BY_NAME, CAPABILITIES, COMBINATIONS, features_of

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "Cargo.toml"
CORE_MANIFEST = ROOT / "src" / "Cargo.toml"


def manifest_features() -> dict[str, list[str]]:
    with MANIFEST.open("rb") as handle:
        return tomllib.load(handle)["features"]


def uncovered_core_features(root: dict, core: dict[str, list[str]]) -> set[str]:
    forwarded = {
        value.removeprefix("vera20k-core/")
        for values in root["features"].values()
        for value in values
        if value.startswith("vera20k-core/")
    }
    # Client tests enable test-support through their core dev-dependency.
    for table in ("dependencies", "dev-dependencies"):
        forwarded.update(root.get(table, {}).get("vera20k-core", {}).get("features", ()))
    if "default" in core:
        forwarded.add("default")
    pending, covered = list(forwarded), set()
    while pending:
        name = pending.pop()
        if name not in covered:
            covered.add(name)
            pending.extend(core.get(name, ()))
    return set(core) - covered


class CapabilityRegistryTests(unittest.TestCase):
    def setUp(self):
        self.features = manifest_features()

    def test_every_optional_feature_is_registered(self):
        covered = set(self.features["default"])
        for spec in CAPABILITIES:
            covered.update(spec.features)
        uncovered = set(self.features) - {"default"} - covered
        self.assertEqual(
            uncovered,
            set(),
            "Cargo.toml features no build compiles; register them in "
            "tools/capability_tests.py (or add them to the default set)",
        )

    def test_every_registered_feature_exists_in_the_manifest(self):
        registered = {feature for spec in CAPABILITIES for feature in spec.features}
        unknown = registered - set(self.features)
        self.assertEqual(unknown, set(), "registered features that are not manifest features")

    def test_every_core_feature_is_covered_by_the_client_or_its_tests(self):
        with MANIFEST.open("rb") as handle:
            root = tomllib.load(handle)
        with CORE_MANIFEST.open("rb") as handle:
            core = tomllib.load(handle)["features"]
        self.assertEqual(
            uncovered_core_features(root, core), set(),
            "src/Cargo.toml features no build covers; forward them from a registered client capability",
        )

    def test_capability_names_are_unique_and_combination_names_known(self):
        names = [spec.name for spec in CAPABILITIES]
        self.assertEqual(len(names), len(set(names)), "duplicate capability names")
        for combination in COMBINATIONS:
            for name in combination:
                self.assertIn(name, BY_NAME, f"combination names unknown capability {name!r}")

    def test_combinations_enable_distinct_features(self):
        for combination in COMBINATIONS:
            features = features_of(combination)
            self.assertEqual(
                len(features),
                len(set(features)),
                f"combination {'+'.join(combination)} enables a feature twice",
            )

    def test_combination_features_exist_in_the_manifest(self):
        for combination in COMBINATIONS:
            for feature in features_of(combination):
                self.assertIn(feature, self.features, f"combination feature {feature!r}")

    def test_core_only_feature_cannot_skip_coverage(self):
        root = {"features": {"phobos": ["vera20k-core/phobos"]}}
        core = {"phobos": ["radiation"], "radiation": [], "unregistered": []}
        self.assertEqual(uncovered_core_features(root, core), {"unregistered"})


class CheckSequenceTests(unittest.TestCase):
    """The compile coverage blocks; a capability suite's known failures do not."""

    def test_suite_keeps_running_other_crates_after_a_test_binary_fails(self):
        with mock.patch.object(capability_tests, "run_cargo", return_value=101) as run:
            self.assertEqual(capability_tests.command_test("phobos", ()), 101)
        self.assertIn("--no-fail-fast", run.call_args.args[0])

    def run_check(self, test_status: int, soft_tests: bool) -> int:
        with (
            mock.patch.object(capability_tests, "command_build", return_value=0),
            mock.patch.object(capability_tests, "command_test", return_value=test_status),
            mock.patch.object(capability_tests, "run_cargo", return_value=0),
        ):
            return capability_tests.command_check([], soft_tests=soft_tests)

    def test_soft_tests_reports_suite_failures_without_failing(self):
        self.assertEqual(self.run_check(test_status=101, soft_tests=True), 0)

    def test_suite_failures_block_without_the_flag(self):
        self.assertEqual(self.run_check(test_status=101, soft_tests=False), 101)

    def test_compile_failures_block_even_with_soft_tests(self):
        with (
            mock.patch.object(capability_tests, "command_build", return_value=2),
            mock.patch.object(capability_tests, "command_test", return_value=0),
            mock.patch.object(capability_tests, "run_cargo", return_value=0),
        ):
            self.assertEqual(capability_tests.command_check([], soft_tests=True), 2)

    def test_test_compilation_failure_blocks_soft_tests_without_running_suite(self):
        with (
            mock.patch.object(capability_tests, "CAPABILITIES", CAPABILITIES[:1]),
            mock.patch.object(capability_tests, "COMBINATIONS", ()),
            mock.patch.object(capability_tests, "command_build", return_value=0),
            mock.patch.object(capability_tests, "command_test", return_value=0) as test,
            mock.patch.object(
                capability_tests,
                "run_cargo",
                side_effect=lambda args: 101 if "--no-run" in args else 0,
            ),
        ):
            self.assertEqual(capability_tests.command_check([], soft_tests=True), 101)
            test.assert_not_called()

    def test_only_github_actions_uses_direct_cargo(self):
        with mock.patch.object(capability_tests, "invoke", return_value=0) as invoke:
            with mock.patch.dict(os.environ, {"GITHUB_ACTIONS": ""}):
                self.assertEqual(capability_tests.run_cargo(["check", "--lib"]), 0)
                self.assertFalse(invoke.call_args.kwargs["isolated_ci"])
            with mock.patch.dict(os.environ, {"GITHUB_ACTIONS": "true"}):
                self.assertEqual(capability_tests.run_cargo(["check", "--lib"]), 0)
                self.assertTrue(invoke.call_args.kwargs["isolated_ci"])
