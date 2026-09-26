from __future__ import annotations

import copy
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from collections.abc import Callable
from pathlib import Path
from unittest.mock import patch

from tools.tactical_certification import orchestrator as orchestrator_module
from tools.tactical_certification.core import (
    INVALID,
    VALID,
    ValidationError,
    require_regular_file,
    sha256_bytes,
)
from tools.tactical_certification.orchestrator import (
    CAPTURE_MANIFEST_NAME,
    CHILD_DIRECTORY_NAME,
    FRAME_NAME,
    EnvironmentInputs,
    build_capture_command,
    capture_once,
    validate_capture_bundle,
    validate_environment_inputs,
    validate_repeat,
)
from tools.tactical_certification.profile import (
    load_contract,
    load_profile,
    repository_contract_path,
    repository_root,
)


PROFILES = repository_root() / "tools" / "tactical_certification" / "profiles"


def _small_snapshot(directory: Path, name: str, data: bytes):
    path = directory / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    return require_regular_file(path.absolute(), name)


def _fake_environment(directory: Path) -> EnvironmentInputs:
    cwd = directory / "cwd"
    cwd.mkdir()
    retail = directory / "retail"
    retail.mkdir()
    return EnvironmentInputs(
        working_directory=cwd.absolute(),
        config=_small_snapshot(directory, "config.toml", b"[paths]\n"),
        executable=_small_snapshot(directory, "vera20k.exe", b"exe"),
        archive=_small_snapshot(directory, "archive.mix", b"archive"),
        font=_small_snapshot(directory, "verdana.ttf", b"font"),
        retail_root=retail.absolute(),
    )


def _nonuniform_frame(profile) -> bytes:
    pixel_count = (
        profile.capture["output_width"] * profile.capture["output_height"]
    )
    return b"\x00\x00\x00\xff" * (pixel_count - 1) + b"\x01\x02\x03\xff"


def _fixture_ledger(profile, online=3694):
    return {**profile.budgets["expected_ledger"], "radar_online": online,
            "second_readiness": online + 1, "capture": online + 1 + profile.capture["warm_frames"]}


def _observed_ledger(profile, *, capture_complete: int | None) -> dict[str, object]:
    expected = _fixture_ledger(profile)
    return {
        "capture_complete_tick": capture_complete,
        "capture_requested_tick": expected["capture"],
        "power_active_tick": expected["power_active"],
        "power_ready_tick": expected["power_ready"],
        "radar_active_tick": expected["radar_active"],
        "radar_online_tick": expected["radar_online"],
        "radar_ready_tick": expected["radar_ready"],
        "refinery_active_tick": expected["refinery_active"],
        "refinery_ready_tick": expected["refinery_ready"],
        "rust_l0_tick": 0,
        "second_readiness_tick": expected["second_readiness"],
        "yard_active_tick": expected["yard_active"],
    }


def _production_fixture(profile) -> dict[str, object]:
    expected = _fixture_ledger(profile)
    owner = profile.document["launch"]["player_name"]
    targets = profile.capture["build_targets"]
    bindings = {
        "power": {
            "cell": [46, 110],
            "stable_id": 122,
            "type_id": targets["power"],
        },
        "radar": {
            "cell": [46, 114],
            "stable_id": 125,
            "type_id": targets["radar"],
        },
        "refinery": {
            "cell": [49, 109],
            "stable_id": 123,
            "type_id": targets["refinery"],
        },
        "yard": {
            "cell": [48, 112],
            "stable_id": 121,
            "type_id": "NACNST",
        },
    }
    placements = [
        {
            "anchor_cell": [48, 112],
            "anchor_yard_id": 121,
            "candidate_index": 9,
            "cell": [46, 110],
            "foundation": [3, 2],
            "radius": 2,
            "type_id": targets["power"],
        },
        {
            "anchor_cell": [48, 112],
            "anchor_yard_id": 121,
            "candidate_index": 29,
            "cell": [49, 109],
            "foundation": [4, 3],
            "radius": 3,
            "type_id": targets["refinery"],
        },
        {
            "anchor_cell": [48, 112],
            "anchor_yard_id": 121,
            "candidate_index": 14,
            "cell": [46, 114],
            "foundation": [2, 2],
            "radius": 2,
            "type_id": targets["radar"],
        },
    ]

    def deploy(
        action_id: int,
        scheduled_tick: int,
        attempt: str,
        expected_result: dict[str, object],
        resolved_result: dict[str, object],
    ) -> dict[str, object]:
        return {
            "action_id": action_id,
            "execute_tick": scheduled_tick,
            "expected_result": expected_result,
            "owner": owner,
            "payload": {
                "DeployMcv": {"attempt": attempt, "entity_id": 119}
            },
            "resolved_result": resolved_result,
            "scheduled_tick": scheduled_tick,
        }

    def queue(
        action_id: int,
        scheduled_tick: int,
        role: str,
        type_id: str,
        rate_frames: int,
    ) -> dict[str, object]:
        return {
            "action_id": action_id,
            "execute_tick": scheduled_tick,
            "expected_result": {
                "QueueOrReady": {
                    "expected_rate_frames": rate_frames,
                    "type_id": type_id,
                }
            },
            "owner": owner,
            "payload": {
                "QueueExactType": {"role": role, "type_id": type_id}
            },
            "resolved_result": {
                "QueueObserved": {
                    "resolved_rate_frames": 0,
                    "type_id": type_id,
                }
            },
            "scheduled_tick": scheduled_tick,
        }

    def place(
        action_id: int,
        scheduled_tick: int,
        role: str,
        choice: dict[str, object],
        binding: dict[str, object],
    ) -> dict[str, object]:
        return {
            "action_id": action_id,
            "execute_tick": scheduled_tick,
            "expected_result": {
                "BuildingPlacedReadyConsumed": {
                    "cell": choice["cell"],
                    "type_id": choice["type_id"],
                }
            },
            "owner": owner,
            "payload": {
                "PlaceExactType": {
                    "choice": copy.deepcopy(choice),
                    "role": role,
                }
            },
            "resolved_result": {
                "BuildingObserved": {
                    "cell": binding["cell"],
                    "stable_id": binding["stable_id"],
                    "type_id": binding["type_id"],
                }
            },
            "scheduled_tick": scheduled_tick,
        }

    commands = [
        deploy(
            1,
            0,
            "First",
            {
                "McvTurnOrYard": {
                    "deploy_facing": 128,
                    "mcv_id": 119,
                    "yard_type_id": "NACNST",
                }
            },
            {"McvTurned": {"facing": 128, "mcv_id": 119}},
        ),
        deploy(
            2,
            1,
            "Second",
            {"YardCreated": {"mcv_id": 119, "yard_type_id": "NACNST"}},
            {
                "YardObserved": {
                    "cell": bindings["yard"]["cell"],
                    "stable_id": bindings["yard"]["stable_id"],
                }
            },
        ),
        queue(3, expected["yard_active"], "Power", targets["power"], 11),
        place(
            4,
            expected["power_ready"],
            "Power",
            placements[0],
            bindings["power"],
        ),
        queue(
            5,
            expected["power_active"],
            "Refinery",
            targets["refinery"],
            37,
        ),
        place(
            6,
            expected["refinery_ready"],
            "Refinery",
            placements[1],
            bindings["refinery"],
        ),
        queue(
            7,
            expected["refinery_active"],
            "Radar",
            targets["radar"],
            18,
        ),
        place(
            8,
            expected["radar_ready"],
            "Radar",
            placements[2],
            bindings["radar"],
        ),
    ]
    capture_tick = expected["capture"]
    return {
        "command_ledger": commands,
        "exact_step_count": capture_tick,
        "first_exact_step": {
            "binary_frame_after": 1,
            "binary_frame_before": 0,
            "tick_after": 1,
            "tick_before": 0,
        },
        "harvester": {
            "cell": [49, 119],
            "stable_id": 124,
            "type_id": targets["refinery_spawned_harvester"],
        },
        "last_exact_step": {
            "binary_frame_after": capture_tick,
            "binary_frame_before": capture_tick - 1,
            "tick_after": capture_tick,
            "tick_before": capture_tick - 1,
        },
        "observed_ledger": _observed_ledger(
            profile, capture_complete=capture_tick
        ),
        "placement_ledger": placements,
        "structure_bindings": bindings,
    }


def _render_fixture(profile) -> dict[str, object]:
    cursor = profile.capture["post_load_cursor"]
    render = {
        "app_has_radar": True,
        "bound_structures_ready": True,
        "cursor": {"x": float(cursor["x"]), "y": float(cursor["y"])},
        "cursor_id": profile.capture["cursor_id"],
        "egui_ready": True,
        "expected_theme": "Soviet",
        "no_modal_or_debug": True,
        "panel_contains_aperture": True,
        "power_ready": True,
        "production_render": {
            "instance_counts": {
                "minimap": 1,
                "radar_animation": 1,
                "viewport_rect": 1,
            },
            "minimap_aperture": {
                "height": 108.0,
                "width": 140.0,
                "x": 648.0,
                "y": 49.0,
            },
            "radar_content_insets": [16, 1, 12, 1],
            "sidebar_panel": {
                "height": 600.0,
                "width": 168.0,
                "x": 632.0,
                "y": 0.0,
            },
            "sidebar_view_present": True,
        },
        "radar_animation_source": {
            "actual_theme": "Soviet",
            "atlas_theme": "Soviet",
            "backgrounds": [
                {
                    "logical_name": "bkgdlg.shp",
                    "source_archive": "sidec02.mix",
                },
                {
                    "logical_name": "bkgdmd.shp",
                    "source_archive": "sidec02.mix",
                },
                {
                    "logical_name": "bkgdsm.shp",
                    "source_archive": "sidec02.mix",
                },
            ],
            "generic_palette": {
                "logical_name": "SIDEBAR.PAL",
                "source_archive": "sidec02.mix",
            },
            "parent_archive": "sidec02.mix",
            "radar": {
                "logical_name": "radar.shp",
                "source_archive": "sidec02.mix",
            },
            "requested_theme": "Soviet",
            "theme_palette": {
                "logical_name": "sidebar.pal",
                "source_archive": "sidec02.mix",
            },
        },
        "radar_authority_active": True,
        "radar_phase": "Online",
        "ready": True,
        "sidebar": {
            "credits": 6400,
            "layout": {
                "cameo_grid_bottom": 527.0,
                "cameo_grid_top": 227.0,
                "radar_y": 48.0,
                "side1_y": 158.0,
                "side2_tile_count": 6,
                "side3_y": 527.0,
                "sidebar_x": 632.0,
                "tabs_y": 197.0,
            },
            "low_power": False,
            "power_drained": 100,
            "power_produced": 150,
        },
        "sidebar_values_ready": True,
        "theme": "Soviet",
    }
    if profile.profile_id.startswith("yuri-"):
        render["theme"] = render["expected_theme"] = "Yuri"
        source = render["radar_animation_source"]
        for key in ("actual_theme", "requested_theme", "atlas_theme"):
            source[key] = "Yuri"
        source["parent_archive"] = "sidec02md.mix"
        source["radar"] = {"logical_name": "radary.shp", "source_archive": "sidec02md.mix"}
        source["theme_palette"] = {"logical_name": "radaryuri.pal", "source_archive": "sidec02md.mix"}
        source["backgrounds"] = [{"logical_name": name, "source_archive": "sidec02md.mix"}
            for name in ("bkgdlgy.shp", "bkgdmdy.shp", "bkgdsmy.shp")]
    return render


def _stable_fixture(
    profile, contract, environment: EnvironmentInputs
) -> dict[str, object]:
    production = _production_fixture(profile)
    render = _render_fixture(profile)
    capture_tick = _fixture_ledger(profile)["capture"]
    mix_entry_id = profile.fixture["mix_entry_id"]
    signed_entry_id = (
        mix_entry_id - (1 << 32)
        if mix_entry_id >= (1 << 31)
        else mix_entry_id
    )
    fingerprint_script = {
        "bindings": copy.deepcopy(production["structure_bindings"]),
        "commands": copy.deepcopy(production["command_ledger"]),
        "harvester": copy.deepcopy(production["harvester"]),
        "observed_ledger": _observed_ledger(
            profile, capture_complete=None
        ),
        "placements": copy.deepcopy(production["placement_ledger"]),
        "stage": "CaptureRequested",
    }
    return {
        "inputs": {
            "config": environment.config.public_identity(),
            "executable": environment.executable.public_identity(),
            "archive": environment.archive.public_identity(),
            "font": environment.font.public_identity(),
        },
        "map_source": {
            "archive_name": profile.fixture["archive_name"],
            "entry_digest_authority": profile.fixture[
                "entry_digest_authority"
            ],
            "loaded_source": {
                "entry_id": signed_entry_id,
                "kind": "mix",
                "logical_name": profile.fixture["logical_map_name"],
                "payload_len": profile.fixture["entry_payload_byte_length"],
                "source_archive": profile.fixture["archive_name"],
            },
            "logical_map_name": profile.fixture["logical_map_name"],
            "loose_shadow_rejected": True,
            "mix_entry_id": mix_entry_id,
            "payload_byte_length": profile.fixture[
                "entry_payload_byte_length"
            ],
            "payload_sha256": profile.fixture["entry_payload_sha256"],
            "post_load_resolve_entry_id": mix_entry_id,
            "post_load_resolve_source_archive": profile.fixture[
                "archive_name"
            ],
        },
        "lifecycle": {
            "focus_violations": 0,
            "input_violations": 0,
            "window_focused": False,
            "window_hidden": True,
        },
        "graphics": {
            "adapter": {
                "name": "adapter",
                "vendor": 1,
                "device": 2,
                "device_type": "DiscreteGpu",
                "driver": "driver",
                "driver_info": "driver info",
                "backend": "Dx12",
            },
            "surface_format": "Bgra8UnormSrgb",
            "width": profile.capture["output_width"],
            "height": profile.capture["output_height"],
            "window_scale_factor": 1.0,
            "app_ui_scale": profile.capture["app_ui_scale"],
            "egui_pixels_per_point": 1.0,
            "selected_font": environment.font.public_identity(),
        },
        "contract": {
            "schema_version": "vera20k.tactical-capture-contract.v2",
            "sha256": contract.snapshot.sha256,
            "embedded_bytes_equal": True,
        },
        "profile": {
            "checkpoint": profile.checkpoint,
            "fixture_entry_sha256": profile.fixture[
                "entry_payload_sha256"
            ],
            "profile_id": profile.profile_id,
        },
        "startup": {
            "classification": "AcceptedExplicitFixedBattle",
            "correlation": profile.fixture["battle_descriptor_id"],
            "seed": profile.document["launch"]["seed"],
            "seed_authority_certifying": True,
            "seed_source": "Controlled",
        },
        "production": production,
        "render": render,
        "final_fingerprint": {
            "core": {
                "binary_frame": _fixture_ledger(profile)["capture"],
                "deterministic_state_hash": 7168770358871354549,
                "simulation_tick": capture_tick,
                "total_simulation_ms": (
                    capture_tick * profile.capture["sim_tick_ms"]
                ),
            },
            "cursor": {
                "id": profile.capture["cursor_id"],
                "x": float(profile.capture["post_load_cursor"]["x"]),
                "y": float(profile.capture["post_load_cursor"]["y"]),
            },
            "power": {
                "blackout_remaining": 0,
                "drain": 100,
                "is_low_power": False,
                "output": 150,
            },
            "radar": {
                "app_has_radar": True,
                "authority_active": True,
                "phase": "Online",
            },
            "render": copy.deepcopy(render),
            "script": fingerprint_script,
            "wallet": {
                "credits": 6400,
                "harvested_credits": 0,
                "spent_credits": 3600,
            },
        },
        "known_residuals": [
            "Native pixels and whole-game parity remain unverified.",
        ],
    }


def _manifest(
    profile,
    contract,
    environment: EnvironmentInputs,
    frame: bytes,
    *,
    mutate: Callable[[dict[str, object]], None] | None = None,
) -> dict[str, object]:
    width = profile.capture["output_width"]
    height = profile.capture["output_height"]
    capture_tick = _fixture_ledger(profile)["capture"]
    manifest: dict[str, object] = {
        "schema_version": "vera20k.tactical-capture.v2",
        "status": "COMPLETE",
        "checkpoint": "radar-online-v2",
        "profile": {
            **profile.snapshot.public_identity(),
            "schema_version": "vera20k.tactical-profile.v2",
            "profile_id": profile.profile_id,
        },
        "contract": {
            **contract.snapshot.public_identity(),
            "schema_version": "vera20k.tactical-capture-contract.v2",
            "embedded_sha256": contract.snapshot.sha256,
            "bytes_equal": True,
        },
        "frame": {
            "file_name": FRAME_NAME,
            "width": width,
            "height": height,
            "row_stride": width * 4,
            "byte_length": len(frame),
            "sha256": sha256_bytes(frame),
            "surface_format": "Bgra8UnormSrgb",
            "pixel_layout": "BGRA8",
        },
        "evidence": {
            "stable": _stable_fixture(profile, contract, environment),
            "run": {
                "process_id": 123,
                "elapsed_ms": 111470,
                "render_frames": capture_tick + 1,
                "exact_steps": capture_tick,
            },
        },
        "failure": None,
        "native_comparator": "NONE",
        "parity_certification": "NONE",
        "evidence_limitations": list(profile.document["evidence_limitations"]),
    }
    if mutate is not None:
        mutate(manifest)
    return manifest


def _write_capture(
    directory: Path,
    profile,
    contract,
    environment: EnvironmentInputs,
    frame: bytes,
    *,
    mutate: Callable[[dict[str, object]], None] | None = None,
) -> None:
    directory.mkdir()
    (directory / FRAME_NAME).write_bytes(frame)
    (directory / CAPTURE_MANIFEST_NAME).write_text(
        json.dumps(
            _manifest(
                profile,
                contract,
                environment,
                frame,
                mutate=mutate,
            ),
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )


def _rewrite_capture_manifest(
    directory: Path,
    mutate: Callable[[dict[str, object]], None],
) -> None:
    path = directory / CAPTURE_MANIFEST_NAME
    manifest = json.loads(path.read_text(encoding="utf-8"))
    mutate(manifest)
    path.write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def _set_nested(
    manifest: dict[str, object],
    path: tuple[str, ...],
    value: object,
) -> None:
    target = manifest
    for key in path[:-1]:
        target = target[key]
    target[path[-1]] = value


class OrchestratorTests(unittest.TestCase):
    def setUp(self) -> None:
        self.profile = load_profile(PROFILES / "soviet-radar-online-v2.json")
        self.contract = load_contract(repository_contract_path())

    def test_current_theme_geometry_and_observed_online_time_classifier(self) -> None:
        from tools.tactical_certification.evidence_validation import _require_render, _require_observed_ledger
        for side in ("soviet", "yuri"):
            profile = load_profile(PROFILES / f"{side}-radar-online-v2.json")
            render = _render_fixture(profile)
            for count in (1, 2, 3, 4):
                render["production_render"]["instance_counts"]["viewport_rect"] = count
                _require_render({"render": render}, profile)
            for mutation in ("old_scale", "old_source", "zero_edges", "extra_edges"):
                bad = copy.deepcopy(render)
                if mutation == "old_scale": bad["production_render"]["sidebar_panel"]["width"] = 84
                elif mutation == "old_source": bad["radar_animation_source"]["actual_theme"] = "Allied"
                else: bad["production_render"]["instance_counts"]["viewport_rect"] = 0 if mutation == "zero_edges" else 5
                with self.subTest(side=side, mutation=mutation), self.assertRaises(ValidationError):
                    _require_render({"render": bad}, profile)
        for online in (3674, 3694, 3900, 7700):
            ledger = _observed_ledger(self.profile, capture_complete=online + 17)
            ledger.update(radar_online_tick=online, second_readiness_tick=online + 1, capture_requested_tick=online + 17)
            _require_observed_ledger(ledger, "ledger", self.profile, completed=True)
            ledger["capture_requested_tick"] += 1
            with self.assertRaisesRegex(ValidationError, "capture_requested_tick"):
                _require_observed_ledger(ledger, "ledger", self.profile, completed=True)

    def test_fingerprint_cannot_describe_a_different_valid_online_transition(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).absolute()
            environment = _fake_environment(root)
            capture = root / "child"
            def mutate(manifest):
                ledger = manifest["evidence"]["stable"]["final_fingerprint"]["script"]["observed_ledger"]
                for key in ("radar_online_tick", "second_readiness_tick", "capture_requested_tick"):
                    ledger[key] += 1
            _write_capture(capture, self.profile, self.contract, environment,
                           _nonuniform_frame(self.profile), mutate=mutate)
            with self.assertRaisesRegex(ValidationError, "final_fingerprint.script.observed_ledger"):
                validate_capture_bundle(capture, self.profile, self.contract, environment)

    def test_command_is_argument_list_with_exact_profile_contract_and_output(self) -> None:
        command = build_capture_command(
            Path("C:/build/vera20k.exe"),
            self.profile,
            self.contract,
            Path("C:/runs/run/capture"),
        )
        self.assertEqual(
            command,
            [
                "C:\\build\\vera20k.exe",
                "--tactical-capture",
                "radar-online-v2",
                "--profile",
                str(self.profile.path),
                "--contract",
                str(self.contract.path),
                "--output",
                "C:\\runs\\run\\capture",
            ],
        )

    def test_complete_bundle_validates_exact_artifacts_and_bgra(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).absolute()
            environment = _fake_environment(root)
            frame = _nonuniform_frame(self.profile)
            capture = root / "child"
            _write_capture(
                capture, self.profile, self.contract, environment, frame
            )
            validated = validate_capture_bundle(
                capture, self.profile, self.contract, environment
            )
            self.assertEqual(validated.frame_snapshot.sha256, sha256_bytes(frame))

            (capture / "extra.txt").write_text("unexpected", encoding="utf-8")
            with self.assertRaisesRegex(ValidationError, "artifact set"):
                validate_capture_bundle(
                    capture, self.profile, self.contract, environment
                )

    def test_formerly_accepted_partial_stable_object_is_invalid(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).absolute()
            environment = _fake_environment(root)
            capture = root / "child"
            _write_capture(
                capture,
                self.profile,
                self.contract,
                environment,
                _nonuniform_frame(self.profile),
            )

            def retain_old_partial_shape(manifest):
                stable = manifest["evidence"]["stable"]
                manifest["evidence"]["stable"] = {
                    key: stable[key]
                    for key in (
                        "inputs",
                        "map_source",
                        "lifecycle",
                        "graphics",
                        "contract",
                    )
                }

            _rewrite_capture_manifest(capture, retain_old_partial_shape)
            with self.assertRaisesRegex(
                ValidationError, "evidence.stable keys are invalid"
            ):
                validate_capture_bundle(
                    capture, self.profile, self.contract, environment
                )

    def test_load_bearing_stable_and_run_mutations_are_invalid(self) -> None:
        mutations = (
            (
                "missing sampled viewport edge",
                ("evidence", "stable", "render", "production_render",
                 "instance_counts", "viewport_rect"),
                0,
                "instance_counts.viewport_rect",
            ),
            (
                "impossible viewport edge count",
                ("evidence", "stable", "render", "production_render",
                 "instance_counts", "viewport_rect"),
                5,
                "instance_counts.viewport_rect",
            ),
            (
                "hidden window",
                (
                    "evidence",
                    "stable",
                    "lifecycle",
                    "window_hidden",
                ),
                False,
                "evidence.stable.lifecycle.window_hidden",
            ),
            (
                "map payload digest",
                (
                    "evidence",
                    "stable",
                    "map_source",
                    "payload_sha256",
                ),
                "0" * 64,
                "evidence.stable.map_source.payload_sha256",
            ),
            (
                "observed radar-online tick",
                (
                    "evidence",
                    "stable",
                    "production",
                    "observed_ledger",
                    "radar_online_tick",
                ),
                _fixture_ledger(self.profile)["radar_online"] + 1,
                "evidence.stable.production.observed_ledger.second_readiness_tick",
            ),
            (
                "minimap draw count",
                (
                    "evidence",
                    "stable",
                    "render",
                    "production_render",
                    "instance_counts",
                    "minimap",
                ),
                0,
                "evidence.stable.render.production_render.instance_counts.minimap",
            ),
            (
                "fingerprint render authority",
                (
                    "evidence",
                    "stable",
                    "final_fingerprint",
                    "render",
                    "radar_phase",
                ),
                "Offline",
                "evidence.stable.final_fingerprint.render",
            ),
            (
                "run exact-step count",
                ("evidence", "run", "exact_steps"),
                _fixture_ledger(self.profile)["capture"] - 1,
                "evidence.run.exact_steps",
            ),
        )
        for index, (label, path, replacement, expected_error) in enumerate(
            mutations
        ):
            with self.subTest(label=label), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary).absolute()
                environment = _fake_environment(root)
                capture = root / f"child-{index}"
                _write_capture(
                    capture,
                    self.profile,
                    self.contract,
                    environment,
                    _nonuniform_frame(self.profile),
                    mutate=lambda manifest: _set_nested(
                        manifest, path, replacement
                    ),
                )
                with self.assertRaisesRegex(ValidationError, expected_error):
                    validate_capture_bundle(
                        capture, self.profile, self.contract, environment
                    )

    def test_contradictory_production_receipts_are_invalid(self) -> None:
        def delayed_offline_command(manifest: dict[str, object]) -> None:
            stable = manifest["evidence"]["stable"]
            for commands in (
                stable["production"]["command_ledger"],
                stable["final_fingerprint"]["script"]["commands"],
            ):
                commands[0]["execute_tick"] = commands[0]["scheduled_tick"] + 2

        def prematurely_resolved_enqueue_rate(manifest: dict[str, object]) -> None:
            stable = manifest["evidence"]["stable"]
            for commands in (
                stable["production"]["command_ledger"],
                stable["final_fingerprint"]["script"]["commands"],
            ):
                commands[2]["resolved_result"]["QueueObserved"][
                    "resolved_rate_frames"
                ] = commands[2]["expected_result"]["QueueOrReady"][
                    "expected_rate_frames"
                ]

        def bogus_command_results(manifest: dict[str, object]) -> None:
            stable = manifest["evidence"]["stable"]
            for commands in (
                stable["production"]["command_ledger"],
                stable["final_fingerprint"]["script"]["commands"],
            ):
                commands[7]["expected_result"] = {
                    "BogusExpected": {"value": 1}
                }
                commands[7]["resolved_result"] = {
                    "BogusResolved": {"value": 2}
                }

        def detached_placement(manifest: dict[str, object]) -> None:
            stable = manifest["evidence"]["stable"]
            bad_choice = {
                "anchor_cell": [0, 0],
                "anchor_yard_id": 999999,
                "candidate_index": 999999,
                "cell": [60, 60],
                "foundation": [99, 99],
                "radius": 999,
                "type_id": self.profile.capture["build_targets"]["power"],
            }
            for placements in (
                stable["production"]["placement_ledger"],
                stable["final_fingerprint"]["script"]["placements"],
            ):
                placements[0] = copy.deepcopy(bad_choice)
            for commands in (
                stable["production"]["command_ledger"],
                stable["final_fingerprint"]["script"]["commands"],
            ):
                commands[3]["payload"]["PlaceExactType"]["choice"] = (
                    copy.deepcopy(bad_choice)
                )

        def wrong_ring_candidate(manifest: dict[str, object]) -> None:
            stable = manifest["evidence"]["stable"]
            for placements, commands in (
                (
                    stable["production"]["placement_ledger"],
                    stable["production"]["command_ledger"],
                ),
                (
                    stable["final_fingerprint"]["script"]["placements"],
                    stable["final_fingerprint"]["script"]["commands"],
                ),
            ):
                placements[0]["candidate_index"] = 10
                commands[3]["payload"]["PlaceExactType"]["choice"][
                    "candidate_index"
                ] = 10

        def duplicate_harvester_id(manifest: dict[str, object]) -> None:
            stable = manifest["evidence"]["stable"]
            yard_id = stable["production"]["structure_bindings"]["yard"][
                "stable_id"
            ]
            stable["production"]["harvester"]["stable_id"] = yard_id
            stable["final_fingerprint"]["script"]["harvester"][
                "stable_id"
            ] = yard_id

        def zero_final_binary_frame(manifest: dict[str, object]) -> None:
            stable = manifest["evidence"]["stable"]
            stable["production"]["last_exact_step"][
                "binary_frame_before"
            ] = 0
            stable["production"]["last_exact_step"][
                "binary_frame_after"
            ] = 0
            stable["final_fingerprint"]["core"]["binary_frame"] = 0

        def wrong_stock_foundation(manifest: dict[str, object]) -> None:
            stable = manifest["evidence"]["stable"]
            for placements, commands in (
                (
                    stable["production"]["placement_ledger"],
                    stable["production"]["command_ledger"],
                ),
                (
                    stable["final_fingerprint"]["script"]["placements"],
                    stable["final_fingerprint"]["script"]["commands"],
                ),
            ):
                placements[0]["foundation"] = [99, 99]
                commands[3]["payload"]["PlaceExactType"]["choice"][
                    "foundation"
                ] = [99, 99]

        def overlapping_structure_cells(manifest: dict[str, object]) -> None:
            stable = manifest["evidence"]["stable"]
            for placements, bindings, commands in (
                (
                    stable["production"]["placement_ledger"],
                    stable["production"]["structure_bindings"],
                    stable["production"]["command_ledger"],
                ),
                (
                    stable["final_fingerprint"]["script"]["placements"],
                    stable["final_fingerprint"]["script"]["bindings"],
                    stable["final_fingerprint"]["script"]["commands"],
                ),
            ):
                power_cell = copy.deepcopy(bindings["power"]["cell"])
                placements[2]["candidate_index"] = 9
                placements[2]["cell"] = copy.deepcopy(power_cell)
                placements[2]["radius"] = 2
                bindings["radar"]["cell"] = copy.deepcopy(power_cell)
                commands[7]["payload"]["PlaceExactType"]["choice"] = (
                    copy.deepcopy(placements[2])
                )
                commands[7]["expected_result"][
                    "BuildingPlacedReadyConsumed"
                ]["cell"] = copy.deepcopy(power_cell)
                commands[7]["resolved_result"]["BuildingObserved"][
                    "cell"
                ] = copy.deepcopy(power_cell)

        def harvester_inside_yard(manifest: dict[str, object]) -> None:
            stable = manifest["evidence"]["stable"]
            yard_cell = stable["production"]["structure_bindings"]["yard"][
                "cell"
            ]
            stable["production"]["harvester"]["cell"] = copy.deepcopy(
                yard_cell
            )
            stable["final_fingerprint"]["script"]["harvester"][
                "cell"
            ] = copy.deepcopy(yard_cell)

        def wrong_deploy_facing(manifest: dict[str, object]) -> None:
            stable = manifest["evidence"]["stable"]
            for commands in (
                stable["production"]["command_ledger"],
                stable["final_fingerprint"]["script"]["commands"],
            ):
                commands[0]["expected_result"]["McvTurnOrYard"][
                    "deploy_facing"
                ] = 0
                commands[0]["resolved_result"]["McvTurned"]["facing"] = 0

        mutations = (
            (
                "offline stamp incorrectly includes network delay",
                delayed_offline_command,
                r"command_ledger\[0\]\.execute_tick",
            ),
            (
                "enqueue receipt skips constructor rate",
                prematurely_resolved_enqueue_rate,
                r"command_ledger\[2\].*resolved_rate_frames",
            ),
            (
                "bogus command result enums",
                bogus_command_results,
                r"command_ledger\[7\]\.expected_result",
            ),
            (
                "placement detached from yard and binding",
                detached_placement,
                r"placement_ledger\[0\]\.anchor_yard_id",
            ),
            (
                "placement candidate-order contradiction",
                wrong_ring_candidate,
                r"placement_ledger\[0\]\.cell",
            ),
            (
                "harvester stable ID reused",
                duplicate_harvester_id,
                "structure/harvester stable IDs must be unique",
            ),
            (
                "binary frame contradicts exact simulation time",
                zero_final_binary_frame,
                r"last_exact_step\.binary_frame_before",
            ),
            (
                "foundation contradicts fixed stock type",
                wrong_stock_foundation,
                r"placement_ledger\[0\]\.foundation",
            ),
            (
                "two active structures overlap",
                overlapping_structure_cells,
                r"placement_ledger\[2\]\.foundation overlaps power",
            ),
            (
                "harvester occupies the yard footprint",
                harvester_inside_yard,
                r"harvester\.cell overlaps yard",
            ),
            (
                "deploy facing contradicts fixed stock yard rule",
                wrong_deploy_facing,
                r"command_ledger\[0\].*deploy_facing",
            ),
        )
        for index, (label, mutate, expected_error) in enumerate(mutations):
            with self.subTest(label=label), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary).absolute()
                environment = _fake_environment(root)
                capture = root / f"contradiction-{index}"
                _write_capture(
                    capture,
                    self.profile,
                    self.contract,
                    environment,
                    _nonuniform_frame(self.profile),
                    mutate=mutate,
                )
                with self.assertRaisesRegex(ValidationError, expected_error):
                    validate_capture_bundle(
                        capture,
                        self.profile,
                        self.contract,
                        environment,
                    )

    def test_uniform_tactical_frame_is_invalid(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).absolute()
            environment = _fake_environment(root)
            capture = root / "child"
            _write_capture(
                capture,
                self.profile,
                self.contract,
                environment,
                bytes(
                    self.profile.capture["output_width"]
                    * self.profile.capture["output_height"]
                    * 4
                ),
            )
            with self.assertRaisesRegex(ValidationError, "uniform"):
                validate_capture_bundle(
                    capture, self.profile, self.contract, environment
                )

    def test_failure_manifest_with_frame_and_link_artifacts_are_invalid(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).absolute()
            environment = _fake_environment(root)
            frame = _nonuniform_frame(self.profile)
            capture = root / "child"
            _write_capture(
                capture, self.profile, self.contract, environment, frame
            )
            manifest_path = capture / CAPTURE_MANIFEST_NAME
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["status"] = "FAILED"
            manifest["failure"] = {"stage": "test", "message": "failed"}
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            with self.assertRaisesRegex(ValidationError, "FAILED"):
                validate_capture_bundle(
                    capture, self.profile, self.contract, environment
                )

            manifest_path.unlink()
            try:
                manifest_path.symlink_to(capture / FRAME_NAME)
            except OSError:
                return
            with self.assertRaisesRegex(ValidationError, "non-link"):
                validate_capture_bundle(
                    capture, self.profile, self.contract, environment
                )

    def test_repeat_compares_full_stable_object_and_exact_frame(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).absolute()
            environment = _fake_environment(root)
            frame = _nonuniform_frame(self.profile)
            first = root / "first"
            second = root / "second"
            _write_capture(
                first, self.profile, self.contract, environment, frame
            )
            _write_capture(
                second, self.profile, self.contract, environment, frame
            )
            with (
                patch(
                    "tools.tactical_certification.orchestrator.validate_environment_inputs",
                    return_value=environment,
                ),
                patch(
                    "tools.tactical_certification.orchestrator.reject_denied_environment"
                ),
            ):
                report = validate_repeat(
                    first,
                    second,
                    self.profile.path,
                    self.contract.path,
                    executable_path=environment.executable.path,
                    working_directory=environment.working_directory,
                )
                self.assertEqual(report["status"], VALID)

                manifest_path = second / CAPTURE_MANIFEST_NAME
                manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
                manifest["evidence"]["stable"]["graphics"]["adapter"][
                    "name"
                ] = "second-adapter"
                manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
                report = validate_repeat(
                    first,
                    second,
                    self.profile.path,
                    self.contract.path,
                    executable_path=environment.executable.path,
                    working_directory=environment.working_directory,
                )
                self.assertEqual(report["status"], INVALID)
                self.assertIn(
                    "same-profile stable evidence differs", report["errors"]
                )

                first_manifest_path = first / CAPTURE_MANIFEST_NAME
                first_manifest = json.loads(
                    first_manifest_path.read_text(encoding="utf-8")
                )
                first_manifest["evidence"]["stable"]["graphics"]["adapter"][
                    "name"
                ] = "second-adapter"
                first_manifest_path.write_text(
                    json.dumps(first_manifest), encoding="utf-8"
                )
                manifest["evidence"]["stable"]["graphics"]["adapter"][
                    "vendor"
                ] = 2
                manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
                report = validate_repeat(
                    first,
                    second,
                    self.profile.path,
                    self.contract.path,
                    executable_path=environment.executable.path,
                    working_directory=environment.working_directory,
                )
                self.assertEqual(report["status"], INVALID)
                self.assertIn(
                    "same-profile stable evidence differs", report["errors"]
                )

    def test_repeat_rejects_same_directory_and_normalized_alias(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).absolute()
            environment = _fake_environment(root)
            capture = root / "capture"
            _write_capture(
                capture,
                self.profile,
                self.contract,
                environment,
                _nonuniform_frame(self.profile),
            )
            with (
                patch(
                    "tools.tactical_certification.orchestrator.validate_environment_inputs",
                    return_value=environment,
                ),
                patch(
                    "tools.tactical_certification.orchestrator.reject_denied_environment"
                ),
            ):
                for label, alias in (
                    ("same spelling", capture),
                    ("normalized alias", capture / ".." / capture.name),
                ):
                    with self.subTest(label=label):
                        report = validate_repeat(
                            capture,
                            alias,
                            self.profile.path,
                            self.contract.path,
                            executable_path=environment.executable.path,
                            working_directory=environment.working_directory,
                        )
                        self.assertEqual(report["status"], INVALID)
                        self.assertRegex(
                            "\n".join(report["errors"]),
                            "distinct|same capture director",
                        )

    def test_manifest_mutated_after_snapshot_fails_final_recheck(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).absolute()
            environment = _fake_environment(root)
            capture = root / "capture"
            _write_capture(
                capture,
                self.profile,
                self.contract,
                environment,
                _nonuniform_frame(self.profile),
            )
            manifest_path = capture / CAPTURE_MANIFEST_NAME
            mutated = False
            real_require_regular_file = require_regular_file

            def mutate_after_frame_snapshot(path, label, **kwargs):
                nonlocal mutated
                snapshot = real_require_regular_file(path, label, **kwargs)
                if label == "tactical frame" and not mutated:
                    manifest_path.write_bytes(manifest_path.read_bytes() + b"\n")
                    mutated = True
                return snapshot

            with patch(
                "tools.tactical_certification.orchestrator.require_regular_file",
                side_effect=mutate_after_frame_snapshot,
            ):
                with self.assertRaisesRegex(
                    ValidationError, "manifest changed during"
                ):
                    validate_capture_bundle(
                        capture, self.profile, self.contract, environment
                    )
            self.assertTrue(mutated)

    def test_inventory_mutated_during_validation_fails_final_recheck(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).absolute()
            environment = _fake_environment(root)
            capture = root / "capture"
            _write_capture(
                capture,
                self.profile,
                self.contract,
                environment,
                _nonuniform_frame(self.profile),
            )
            inventory_calls = 0
            real_inventory = orchestrator_module._artifact_inventory

            def mutate_after_initial_inventory(directory):
                nonlocal inventory_calls
                inventory = real_inventory(directory)
                inventory_calls += 1
                if inventory_calls == 1:
                    (capture / "late.txt").write_text("late", encoding="utf-8")
                return inventory

            with patch(
                "tools.tactical_certification.orchestrator._artifact_inventory",
                side_effect=mutate_after_initial_inventory,
            ):
                with self.assertRaisesRegex(
                    ValidationError, "artifact|inventory"
                ):
                    validate_capture_bundle(
                        capture, self.profile, self.contract, environment
                    )
            self.assertGreaterEqual(inventory_calls, 2)

    def test_real_preflight_hashes_archive_font_and_rejects_loose_shadow(self) -> None:
        project = repository_root()
        if not (project / "config.toml").is_file():
            primary = project.parent / "ra2-rust-game"
            if (primary / "config.toml").is_file():
                project = primary
        config = project / "config.toml"
        executable = Path(sys.executable).absolute()
        if not config.is_file():
            self.skipTest("local project config.toml is not available")
        environment = validate_environment_inputs(
            executable, project, self.profile
        )
        self.assertEqual(
            environment.archive.sha256,
            self.profile.fixture["archive_sha256"],
        )
        self.assertEqual(
            environment.font.sha256,
            self.profile.pixel_inputs["font"]["sha256"],
        )

        with tempfile.TemporaryDirectory() as temporary:
            working = Path(temporary).absolute()
            (working / "config.toml").write_bytes(config.read_bytes())
            (working / "Fight.MAP").write_bytes(b"shadow")
            with self.assertRaisesRegex(ValidationError, "loose map shadow"):
                validate_environment_inputs(executable, working, self.profile)

    def test_capture_launch_uses_no_shell_devnull_regular_files_and_720_seconds(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).absolute()
            environment = _fake_environment(root)
            calls: dict[str, object] = {}

            class Child:
                pid = 4242
                returncode = 0

                def wait(self, *, timeout):
                    calls["timeout"] = timeout
                    return 0

                def poll(self):
                    return self.returncode

                def kill(self):
                    self.returncode = -9

            def launch(command, **kwargs):
                calls["command"] = command
                calls["kwargs"] = kwargs
                return Child()

            valid_report = {
                "schema_version": "vera20k.tactical-validation.v1",
                "status": VALID,
                "errors": [],
                "checkpoint": "radar-online-v2",
                "profile_id": self.profile.profile_id,
                "capture": None,
            }
            with (
                patch(
                    "tools.tactical_certification.orchestrator.load_profile",
                    return_value=self.profile,
                ),
                patch(
                    "tools.tactical_certification.orchestrator.load_contract",
                    return_value=self.contract,
                ),
                patch(
                    "tools.tactical_certification.orchestrator.reject_denied_environment"
                ),
                patch(
                    "tools.tactical_certification.orchestrator.validate_environment_inputs",
                    return_value=environment,
                ),
                patch(
                    "tools.tactical_certification.orchestrator.subprocess.Popen",
                    side_effect=launch,
                ),
                patch(
                    "tools.tactical_certification.orchestrator.build_validation_report",
                    return_value=(valid_report, None),
                ),
            ):
                run, _ = capture_once(
                    environment.executable.path,
                    self.profile.path,
                    self.contract.path,
                    root / "run",
                    working_directory=environment.working_directory,
                )
            kwargs = calls["kwargs"]
            self.assertFalse(kwargs["shell"])
            self.assertEqual(kwargs["stdin"], subprocess.DEVNULL)
            self.assertNotEqual(kwargs["stdout"], subprocess.PIPE)
            self.assertNotEqual(kwargs["stderr"], subprocess.PIPE)
            self.assertEqual(kwargs["cwd"], environment.working_directory)
            self.assertEqual(calls["timeout"], 720.0)
            self.assertEqual(run["child"]["cleanup_scope"], "exact-child-pid-only")
            self.assertEqual(
                set(path.name for path in (root / "run").iterdir()),
                {
                    "profile.json",
                    "stdout.txt",
                    "stderr.txt",
                    "validation.json",
                    "run.json",
                },
            )

    def test_timeout_kills_only_still_live_popen_child_and_drains_files(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).absolute()
            environment = _fake_environment(root)
            calls = {"wait": 0, "kill": 0}

            class Child:
                pid = 5252
                returncode = None

                def wait(self, *, timeout):
                    calls["wait"] += 1
                    if calls["wait"] == 1:
                        raise subprocess.TimeoutExpired(["vera20k"], timeout)
                    self.returncode = -9
                    return -9

                def poll(self):
                    return self.returncode

                def kill(self):
                    calls["kill"] += 1
                    self.returncode = -9

            invalid_report = {
                "schema_version": "vera20k.tactical-validation.v1",
                "status": INVALID,
                "errors": ["timeout"],
                "checkpoint": "radar-online-v2",
                "profile_id": self.profile.profile_id,
                "capture": None,
            }
            with (
                patch(
                    "tools.tactical_certification.orchestrator.load_profile",
                    return_value=self.profile,
                ),
                patch(
                    "tools.tactical_certification.orchestrator.load_contract",
                    return_value=self.contract,
                ),
                patch(
                    "tools.tactical_certification.orchestrator.reject_denied_environment"
                ),
                patch(
                    "tools.tactical_certification.orchestrator.validate_environment_inputs",
                    return_value=environment,
                ),
                patch(
                    "tools.tactical_certification.orchestrator.subprocess.Popen",
                    return_value=Child(),
                ),
                patch(
                    "tools.tactical_certification.orchestrator.build_validation_report",
                    return_value=(invalid_report, None),
                ),
            ):
                run, _ = capture_once(
                    environment.executable.path,
                    self.profile.path,
                    self.contract.path,
                    root / "run",
                    working_directory=environment.working_directory,
                )
            self.assertEqual(calls, {"wait": 2, "kill": 1})
            self.assertTrue(run["child"]["timed_out"])
            self.assertEqual((root / "run" / "stdout.txt").read_bytes(), b"")
            self.assertEqual((root / "run" / "stderr.txt").read_bytes(), b"")
