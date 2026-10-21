"""Coverage, determinism, schema, and evidence-gate tests."""

from __future__ import annotations

from copy import deepcopy
import hashlib
from pathlib import Path
import tempfile
import unittest
from urllib.parse import urljoin

from tools.exact_shell_ui_matrix.catalog import (
    CONTROL_STATES,
    INPUT_STATES,
    LOADING_ART_VARIANTS,
    PAINT_STATES,
    POINTER_OWNERS,
    POINTER_STATES,
    RA2TS_CURRENT_POLICY,
    RESOLUTIONS,
    SCOPE_EXCLUSIONS,
    TRANSITIONS,
    resolution_token,
)
from tools.exact_shell_ui_matrix.generate import OUTPUT_ROOT, build_matrix, main as generate_main
from tools.exact_shell_ui_matrix.io import (
    MatrixError,
    canonical_json_bytes,
    load_json_path,
    load_json_strict,
)
from tools.exact_shell_ui_matrix.validation import (
    COMPARISON_RESULTS,
    EVIDENCE_KINDS,
    ROW_FAMILIES,
    ROW_STATUSES,
    VERIFICATION_POLICIES,
    validate_matrix,
)


TOOL_ROOT = Path(__file__).resolve().parents[1]
REPO_ROOT = TOOL_ROOT.parents[1]
TEST_ARTIFACT_BYTES = b'{"fixture":"exact-shell-ui-evidence"}\n'
TEST_ARTIFACT_SHA256 = hashlib.sha256(TEST_ARTIFACT_BYTES).hexdigest()


def _artifact() -> dict[str, str]:
    return {
        "id": "artifact:comparison-bundle",
        "path": "evidence/comparison-bundle.json",
        "sha256": TEST_ARTIFACT_SHA256,
    }


def _evidence(kind: str, *, resolutions: tuple[str, ...] = ()) -> dict[str, object]:
    if kind == "native-executable-differential":
        comparison = {
            "color_space": "sRGB8-unorm",
            "crop": "full-client-no-decoration",
            "cursor_policy": "captured-in-both",
            "frame_timing": "same-stable-frame-sequence",
            "resolutions": list(resolutions),
            "scaling": "one-native-pixel-to-one-output-pixel",
            "surface_region": "client-backbuffer",
        }
        proof_domain = None
    elif kind == "exhaustive-proof":
        comparison = None
        proof_domain = "Every input in the named matrix row state space"
    else:
        comparison = None
        proof_domain = None
    return {
        "artifacts": [_artifact()],
        "comparison_contract": comparison,
        "id": "evidence:test-bundle",
        "kind": kind,
        "proof_domain": proof_domain,
    }


def _manifest(
    evidence: dict[str, object],
    *,
    row_results: list[dict[str, object]] | None = None,
    blocker_resolutions: list[dict[str, str]] | None = None,
) -> dict[str, object]:
    return {
        "blocker_resolutions": blocker_resolutions or [],
        "evidence": [evidence],
        "row_results": row_results or [],
        "schema_version": 1,
    }


def _row_result(
    row: dict[str, object],
    *,
    comparison_id: str | None = None,
    comparison_result: str = "NOT_RUN",
    native_ids: tuple[str, ...] = (),
    owner: str | None = None,
    residuals: tuple[str, ...] = (),
    rust_ids: tuple[str, ...] = (),
    status: str = "UNVERIFIED",
) -> dict[str, object]:
    return {
        "comparison_id": comparison_id,
        "comparison_result": comparison_result,
        "native_ids": sorted(native_ids),
        "owner": owner,
        "residuals": sorted(residuals),
        "row_id": row["id"],
        "rust_ids": sorted(rust_ids),
        "status": status,
    }


class MatrixTests(unittest.TestCase):
    def setUp(self) -> None:
        self._artifact_temporary = tempfile.TemporaryDirectory()
        self.artifact_root = Path(self._artifact_temporary.name)
        self.artifact_path = self.artifact_root / _artifact()["path"]
        self.artifact_path.parent.mkdir(parents=True)
        self.artifact_path.write_bytes(TEST_ARTIFACT_BYTES)

    def tearDown(self) -> None:
        self._artifact_temporary.cleanup()

    def _build(self, manifest: dict[str, object]) -> dict[str, object]:
        return build_matrix(manifest, artifact_root=self.artifact_root)

    def test_default_matrix_is_deterministic_and_entirely_unverified(self) -> None:
        first = build_matrix()
        second = build_matrix()
        self.assertEqual(canonical_json_bytes(first), canonical_json_bytes(second))
        self.assertEqual(first["certification_state"], "IN_PROGRESS")
        self.assertEqual(first["coverage"]["total"], 624)
        self.assertEqual(first["coverage"]["by_status"]["UNVERIFIED"], 624)
        self.assertTrue(all(row["status"] == "UNVERIFIED" for row in first["rows"]))
        self.assertTrue(
            all(
                row["evidence"]
                == {"comparison_id": None, "native_ids": [], "rust_ids": []}
                for row in first["rows"]
            )
        )
        self.assertTrue(all(row["owner"] is None for row in first["rows"]))
        self.assertTrue(all(row["residuals"] == [] for row in first["rows"]))
        self.assertTrue(all(row["comparison_result"] == "NOT_RUN" for row in first["rows"]))
        self.assertEqual(
            {(item["width"], item["height"]) for item in first["resolutions"]},
            {(640, 480), (800, 600), (1024, 768)},
        )
        validate_matrix(first)

    def test_required_route_and_interaction_families_cover_every_resolution(self) -> None:
        matrix = build_matrix()
        rows = matrix["rows"]
        resolution_tokens = {resolution_token(item) for item in RESOLUTIONS}
        self.assertEqual({row["family"] for row in rows}, set(ROW_FAMILIES))
        for checkpoint, state, _source in PAINT_STATES:
            found = {
                resolution_token(row["resolution"])
                for row in rows
                if row["family"] == "paint"
                and row["checkpoint"] == checkpoint
                and row["state"] == state
            }
            self.assertEqual(found, resolution_tokens, (checkpoint, state))
        for owner, _source in POINTER_OWNERS:
            for state in POINTER_STATES:
                found = {
                    resolution_token(row["resolution"])
                    for row in rows
                    if row["family"] == "pointer"
                    and row["checkpoint"] == owner
                    and row["state"] == state
                }
                self.assertEqual(found, resolution_tokens, (owner, state))
        self.assertEqual(
            {
                row["state"]
                for row in rows
                if row["family"] == "input"
                and resolution_token(row["resolution"]) == "800x600"
            },
            set(INPUT_STATES),
        )
        self.assertEqual(
            {
                row["state"]
                for row in rows
                if row["family"] == "control"
                and resolution_token(row["resolution"]) == "800x600"
            },
            set(CONTROL_STATES),
        )
        self.assertEqual(
            {
                row["state"]
                for row in rows
                if row["family"] == "transition"
                and resolution_token(row["resolution"]) == "800x600"
            },
            set(TRANSITIONS),
        )

    def test_loading_rows_cover_ten_offline_art_variants_and_eight_colors(self) -> None:
        rows = [row for row in build_matrix()["rows"] if row["family"] == "loading-branch"]
        self.assertEqual(len(rows), 240)
        expected_variants = {item["key"] for item in LOADING_ART_VARIANTS}
        self.assertEqual(
            {row["variant"]["loading_art_variant"] for row in rows},
            expected_variants,
        )
        self.assertEqual(len(expected_variants), 10)
        self.assertNotIn("Random", expected_variants)
        self.assertNotIn("Observer", expected_variants)
        for resolution in RESOLUTIONS:
            scoped = [
                row
                for row in rows
                if row["resolution"] == resolution
            ]
            self.assertEqual(len(scoped), 80)
            self.assertEqual({row["variant"]["color_id"] for row in scoped}, set(range(8)))
        self.assertTrue(
            all(
                row["blocker_ids"]
                == [
                    "catalog:loading-country-art-palette-map",
                    "catalog:loading-first-renderer-localized-text",
                    "catalog:loading-player-color-pixel-map",
                ]
                for row in rows
            )
        )
        exclusion_ids = {item["id"] for item in SCOPE_EXCLUSIONS}
        self.assertIn("scope:observer-loading-art-offline-skirmish", exclusion_ids)
        self.assertIn("scope:random-country-not-loading-art-variant", exclusion_ids)
        control_states = {
            row["state"]
            for row in build_matrix()["rows"]
            if row["family"] == "control"
        }
        self.assertIn("skirmish-country-random-selection", control_states)
        self.assertIn(
            "skirmish-country-random-resolves-before-loading-art-selection",
            control_states,
        )

    def test_ra2ts_lifecycle_can_be_fixed_unverified_and_later_closed_by_proof(self) -> None:
        base = build_matrix()
        row = next(
            item
            for item in base["rows"]
            if item["verification_policy"] == RA2TS_CURRENT_POLICY
        )
        regression = _evidence("production-regression")
        fixed = self._build(
            _manifest(
                regression,
                row_results=[
                    _row_result(
                        row,
                        comparison_id=regression["id"],
                        comparison_result="DRIFT_FIXED_UNVERIFIED",
                        owner="checkpoint:ra2ts-restart",
                        residuals=("Native lifecycle comparison is still absent.",),
                        rust_ids=(regression["id"],),
                        status="DRIFT_FIXED_UNVERIFIED",
                    )
                ],
            )
        )
        updated = next(item for item in fixed["rows"] if item["id"] == row["id"])
        self.assertEqual(updated["status"], "DRIFT_FIXED_UNVERIFIED")
        self.assertEqual(updated["evidence"]["rust_ids"], [regression["id"]])
        self.assertEqual(updated["owner"], "checkpoint:ra2ts-restart")

        proof = _evidence("exhaustive-proof")
        closed = self._build(
            _manifest(
                proof,
                blocker_resolutions=[
                    {
                        "blocker_id": "catalog:ra2ts-process-lifecycle-comparability",
                        "evidence_id": proof["id"],
                    }
                ],
                row_results=[
                    _row_result(
                        row,
                        comparison_id=proof["id"],
                        comparison_result="MATCH",
                        native_ids=(proof["id"],),
                        rust_ids=(proof["id"],),
                        status="VERIFIED",
                    )
                ],
            )
        )
        updated = next(item for item in closed["rows"] if item["id"] == row["id"])
        self.assertEqual(updated["status"], "VERIFIED")

    def test_verified_requires_proof_grade_hashed_evidence_and_resolution_coverage(self) -> None:
        row = next(
            item
            for item in build_matrix()["rows"]
            if item["family"] == "paint"
            and item["verification_policy"] == "standard"
            and resolution_token(item["resolution"]) == "640x480"
        )
        static = _evidence("static-research")
        with self.assertRaisesRegex(MatrixError, "requires native differential"):
            self._build(
                _manifest(
                    static,
                    row_results=[
                        _row_result(
                            row,
                            comparison_id=static["id"],
                            comparison_result="MATCH",
                            native_ids=(static["id"],),
                            status="VERIFIED",
                        )
                    ],
                )
            )

        wrong_resolution = _evidence(
            "native-executable-differential", resolutions=("800x600",)
        )
        with self.assertRaisesRegex(MatrixError, "does not cover 640x480"):
            self._build(
                _manifest(
                    wrong_resolution,
                    row_results=[
                        _row_result(
                            row,
                            comparison_id=wrong_resolution["id"],
                            comparison_result="MATCH",
                            native_ids=(wrong_resolution["id"],),
                            rust_ids=(wrong_resolution["id"],),
                            status="VERIFIED",
                        )
                    ],
                )
            )

        native = _evidence(
            "native-executable-differential", resolutions=("640x480",)
        )
        matrix = self._build(
            _manifest(
                native,
                row_results=[
                    _row_result(
                        row,
                        comparison_id=native["id"],
                        comparison_result="MATCH",
                        native_ids=(native["id"],),
                        rust_ids=(native["id"],),
                        status="VERIFIED",
                    )
                ],
            )
        )
        self.assertEqual(
            next(item for item in matrix["rows"] if item["id"] == row["id"])["status"],
            "VERIFIED",
        )
        self.assertEqual(matrix["certification_state"], "IN_PROGRESS")

    def test_static_and_production_evidence_can_attach_without_claiming_verification(self) -> None:
        row = next(item for item in build_matrix()["rows"] if item["family"] == "input")
        static = _evidence("static-research")
        matrix = self._build(
            _manifest(
                static,
                row_results=[
                    _row_result(
                        row,
                        native_ids=(static["id"],),
                        owner="checkpoint:input-focus",
                        residuals=("Production route comparison has not run.",),
                    )
                ],
            )
        )
        updated = next(item for item in matrix["rows"] if item["id"] == row["id"])
        self.assertEqual(updated["status"], "UNVERIFIED")
        self.assertEqual(updated["comparison_result"], "NOT_RUN")
        self.assertEqual(updated["evidence"]["native_ids"], [static["id"]])

    def test_known_drift_comparison_cannot_be_hidden_by_unverified_status(self) -> None:
        row = next(item for item in build_matrix()["rows"] if item["family"] == "input")
        regression = _evidence("production-regression")
        for comparison_result in ("DRIFT", "DRIFT_FIXED_UNVERIFIED"):
            with self.subTest(comparison_result=comparison_result):
                with self.assertRaisesRegex(
                    MatrixError,
                    f"comparison_result {comparison_result} requires",
                ):
                    self._build(
                        _manifest(
                            regression,
                            row_results=[
                                _row_result(
                                    row,
                                    comparison_id=regression["id"],
                                    comparison_result=comparison_result,
                                    rust_ids=(regression["id"],),
                                    status="UNVERIFIED",
                                )
                            ],
                        )
                    )

    def test_loading_verification_requires_explicit_unknown_blocker_resolution(self) -> None:
        base = build_matrix()
        row = next(item for item in base["rows"] if item["family"] == "loading-branch")
        proof = _evidence("exhaustive-proof")
        result = _row_result(
            row,
            comparison_id=proof["id"],
            comparison_result="MATCH",
            native_ids=(proof["id"],),
            rust_ids=(proof["id"],),
            status="VERIFIED",
        )
        with self.assertRaisesRegex(MatrixError, "unresolved catalog blockers"):
            self._build(_manifest(proof, row_results=[result]))

        resolutions = [
            {"blocker_id": blocker_id, "evidence_id": proof["id"]}
            for blocker_id in row["blocker_ids"]
        ]
        matrix = self._build(
            _manifest(
                proof,
                row_results=[result],
                blocker_resolutions=resolutions,
            )
        )
        updated = next(item for item in matrix["rows"] if item["id"] == row["id"])
        self.assertEqual(updated["status"], "VERIFIED")
        self.assertTrue(
            all(
                blocker["status"] == "RESOLVED"
                for blocker in matrix["catalog_blockers"]
                if blocker["id"] in row["blocker_ids"]
            )
        )
        self.assertEqual(
            next(
                blocker
                for blocker in matrix["catalog_blockers"]
                if blocker["id"] == "catalog:ra2ts-process-lifecycle-comparability"
            )["status"],
            "UNKNOWN",
        )

    def test_evidence_artifacts_must_exist_and_match_declared_digest(self) -> None:
        row = next(item for item in build_matrix()["rows"] if item["family"] == "input")
        static = _evidence("static-research")
        manifest = _manifest(
            static,
            row_results=[
                _row_result(
                    row,
                    native_ids=(static["id"],),
                    status="UNVERIFIED",
                )
            ],
        )

        with self.assertRaisesRegex(MatrixError, "artifact_root is required"):
            build_matrix(manifest)

        self.artifact_path.unlink()
        with self.assertRaisesRegex(MatrixError, "artifact is missing or unreadable"):
            self._build(manifest)

        self.artifact_path.write_bytes(b"tampered\n")
        with self.assertRaisesRegex(MatrixError, "artifact digest mismatch"):
            self._build(manifest)

    def test_validator_detects_tampered_counts_and_strict_json_duplicates(self) -> None:
        matrix = build_matrix()
        tampered = deepcopy(matrix)
        tampered["coverage"]["total"] += 1
        with self.assertRaises(MatrixError):
            validate_matrix(tampered)
        with self.assertRaisesRegex(MatrixError, "duplicate JSON key"):
            load_json_strict('{"schema_version":1,"schema_version":1}')

    def test_validator_binds_immutable_rows_and_digest_to_current_catalog(self) -> None:
        matrix = build_matrix()

        tampered_digest = deepcopy(matrix)
        tampered_digest["catalog_digest"] = "0" * 64
        with self.assertRaisesRegex(MatrixError, r"\$\.catalog_digest"):
            validate_matrix(tampered_digest)

        tampered_row = deepcopy(matrix)
        tampered_row["rows"][0]["state"] = "invented-hand-edited-state"
        with self.assertRaisesRegex(MatrixError, "does not match the current catalog"):
            validate_matrix(tampered_row)

        tampered_sources = deepcopy(matrix)
        tampered_sources["sources"] = tampered_sources["sources"][1:]
        with self.assertRaisesRegex(MatrixError, r"\$\.sources"):
            validate_matrix(tampered_sources)

    def test_validator_recomputes_the_embedded_evidence_overlay_digest(self) -> None:
        row = next(item for item in build_matrix()["rows"] if item["family"] == "input")
        static = _evidence("static-research")
        manifest = _manifest(
            static,
            row_results=[
                _row_result(
                    row,
                    native_ids=(static["id"],),
                    owner="SLICE-TEST",
                    residuals=("Native comparison remains unavailable.",),
                )
            ],
        )
        matrix = self._build(manifest)
        validate_matrix(matrix, artifact_root=self.artifact_root)

        tampered = deepcopy(matrix)
        attached = next(item for item in tampered["rows"] if item["id"] == row["id"])
        attached["owner"] = "SLICE-HAND-EDITED"
        with self.assertRaisesRegex(MatrixError, r"\$\.evidence_digest"):
            validate_matrix(tampered, artifact_root=self.artifact_root)

    def test_empty_evidence_manifest_is_rejected_as_non_canonical(self) -> None:
        empty = {
            "blocker_resolutions": [],
            "evidence": [],
            "row_results": [],
            "schema_version": 1,
        }
        with self.assertRaisesRegex(MatrixError, "empty evidence manifest has no effect"):
            build_matrix(empty, artifact_root=self.artifact_root)

    def test_portable_schema_enums_match_runtime(self) -> None:
        matrix_schema = load_json_path(TOOL_ROOT / "matrix.v1.schema.json")
        evidence_schema = load_json_path(TOOL_ROOT / "evidence.v1.schema.json")
        evidence_ref = matrix_schema["properties"]["evidence"]["items"]["$ref"]
        self.assertEqual(
            urljoin(matrix_schema["$id"], evidence_ref.split("#", 1)[0]),
            evidence_schema["$id"],
        )
        self.assertEqual(
            set(matrix_schema["$defs"]["row"]["properties"]["family"]["enum"]),
            set(ROW_FAMILIES),
        )
        self.assertEqual(
            set(matrix_schema["$defs"]["row"]["properties"]["status"]["enum"]),
            set(ROW_STATUSES),
        )
        self.assertEqual(
            set(evidence_schema["$defs"]["evidence"]["properties"]["kind"]["enum"]),
            set(EVIDENCE_KINDS),
        )
        self.assertEqual(
            set(
                evidence_schema["properties"]["row_results"]["items"]["properties"][
                    "comparison_result"
                ]["enum"]
            ),
            set(COMPARISON_RESULTS),
        )
        self.assertEqual(
            set(matrix_schema["$defs"]["row"]["properties"]["verification_policy"]["enum"]),
            set(VERIFICATION_POLICIES),
        )

    def test_generator_writes_the_canonical_matrix(self) -> None:
        OUTPUT_ROOT.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=OUTPUT_ROOT) as temporary:
            output = Path(temporary).resolve() / "matrix.json"
            self.assertEqual(generate_main(["--output", str(output)]), 0)
            self.assertEqual(output.read_bytes(), canonical_json_bytes(build_matrix()))
            self.assertEqual(generate_main(["--output", str(output), "--check"]), 0)
            output.write_bytes(b"stale")
            self.assertEqual(generate_main(["--output", str(output), "--check"]), 2)


if __name__ == "__main__":
    unittest.main()
