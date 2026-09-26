from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from tools.tactical_certification.core import ValidationError
from tools.tactical_certification.profile import (
    EVIDENCE_LIMITATIONS,
    ENVIRONMENT_DENYLIST,
    load_contract,
    load_profile,
    reject_denied_environment,
    repository_contract_path,
    repository_root,
    scan_tactical_environment_names,
    validate_contract_source_coverage,
    validate_profile_document,
)


PROFILES = repository_root() / "tools" / "tactical_certification" / "profiles"


class ProfileTests(unittest.TestCase):
    def test_both_sealed_profiles_validate_with_exact_side_specific_fields(self) -> None:
        soviet = load_profile(PROFILES / "soviet-radar-online-v2.json")
        yuri = load_profile(PROFILES / "yuri-radar-online-v2.json")
        self.assertEqual(soviet.document["launch"]["player_name"], "VERA-SOVIET")
        self.assertEqual(yuri.document["launch"]["player_name"], "VERA-YURI")
        self.assertEqual(
            soviet.document["capture"]["build_targets"],
            {
                "power": "NAPOWR",
                "refinery": "NAREFN",
                "radar": "NARADR",
                "refinery_spawned_harvester": "HARV",
            },
        )
        self.assertIsNone(
            yuri.document["capture"]["build_targets"][
                "refinery_spawned_harvester"
            ]
        )
        for profile in (soviet, yuri):
            launch = profile.document["launch"]
            self.assertEqual(launch["seed"], 0x12345678)
            self.assertEqual(launch["options"]["unit_count"], 0)
            self.assertEqual(len(launch["options"]), 18)
            self.assertEqual(
                [stage["tick_cap"] for stage in profile.budgets["stages"]],
                [48, 640, 64, 2048, 64, 1024, 64, 4096, 18],
            )
            self.assertEqual(profile.budgets["child_timeout_seconds"], 720)
            self.assertEqual(
                profile.document["evidence_limitations"],
                list(EVIDENCE_LIMITATIONS),
            )

    def test_current_native_profile_rejects_historical_schema_and_half_scale(self) -> None:
        for side in ("soviet", "yuri"):
            with self.assertRaisesRegex(ValidationError, "v1 is historical"):
                load_profile(PROFILES / f"{side}-radar-online-v1.json")
            profile = load_profile(PROFILES / f"{side}-radar-online-v2.json")
            self.assertEqual(profile.capture["app_ui_scale"], 1.0)
            self.assertEqual(set(profile.pixel_inputs), {"font"})
            document = json.loads(json.dumps(profile.document))
            document["capture"]["app_ui_scale"] = 0.5
            with self.assertRaisesRegex(ValidationError, "half-scale profiles are historical"):
                validate_profile_document(document)

    def test_profile_rejects_unknown_key_boolean_integer_and_wrong_timeout(self) -> None:
        profile = load_profile(PROFILES / "soviet-radar-online-v2.json")
        for mutation in ("unknown", "boolean", "timeout"):
            document = json.loads(json.dumps(profile.document))
            if mutation == "unknown":
                document["unexpected"] = True
            elif mutation == "boolean":
                document["launch"]["seed"] = True
            else:
                document["budgets"]["child_timeout_seconds"] = 60
            with self.subTest(mutation=mutation), self.assertRaises(ValidationError):
                validate_profile_document(document)

    def test_profile_rejects_drifted_evidence_limitations(self) -> None:
        profile = load_profile(PROFILES / "soviet-radar-online-v2.json")
        document = json.loads(json.dumps(profile.document))
        document["evidence_limitations"] = [
            "This profile now claims everything is exact."
        ]
        with self.assertRaisesRegex(
            ValidationError,
            "evidence_limitations differ",
        ):
            validate_profile_document(document)

    def test_profile_file_rejects_duplicate_and_nonfinite_json(self) -> None:
        valid = (PROFILES / "soviet-radar-online-v2.json").read_text(
            encoding="utf-8"
        )
        with tempfile.TemporaryDirectory() as temporary:
            duplicate = Path(temporary).absolute() / "duplicate.json"
            duplicate.write_text(
                valid.replace(
                    '"schema_version": "vera20k.tactical-profile.v2",',
                    '"schema_version": "vera20k.tactical-profile.v2",'
                    '"schema_version": "vera20k.tactical-profile.v2",',
                    1,
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ValidationError, "duplicate"):
                load_profile(duplicate)

            nonfinite = Path(temporary).absolute() / "nonfinite.json"
            nonfinite.write_text('{"value": Infinity}', encoding="utf-8")
            with self.assertRaisesRegex(ValidationError, "non-finite"):
                load_profile(nonfinite)

    def test_source_scan_distinguishes_override_strings_from_numeric_constants(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "src").mkdir()
            override = "RA2_" + "FIXTURE_OVERRIDE"
            constant = "RA2_" + "FIXTURE_NUMBER"
            (root / "src" / "fixture.rs").write_text(
                f'const {constant}: u32 = 15;\n'
                f'const ENV_NAME: &str = "{override}";\n'
                'fn sample() { let _ = std::env::var(ENV_NAME); }\n',
                encoding="utf-8",
            )
            self.assertEqual(scan_tactical_environment_names(root), {override})

    def test_external_contract_is_byte_identical_and_covers_tactical_sources(self) -> None:
        contract = load_contract(repository_contract_path())
        self.assertEqual(contract.denylist, ENVIRONMENT_DENYLIST)
        validate_contract_source_coverage(contract)

        with tempfile.TemporaryDirectory() as temporary:
            drifted = Path(temporary).absolute() / "contract.json"
            drifted.write_bytes(contract.snapshot.raw + b"\n")
            with self.assertRaisesRegex(ValidationError, "bytes differ"):
                load_contract(drifted)

    def test_environment_denylist_rejects_presence_even_false_text(self) -> None:
        contract = load_contract(repository_contract_path())
        reject_denied_environment(contract, {})
        with self.assertRaisesRegex(ValidationError, "RA2_QUICKPLAY"):
            reject_denied_environment(contract, {"RA2_QUICKPLAY": "0"})
