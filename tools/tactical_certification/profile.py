"""Strict v2 tactical profile and shared environment-contract validation."""

from __future__ import annotations

import os
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Mapping

from .core import (
    FileSnapshot,
    ValidationError,
    load_json_file,
    require_array,
    require_bool,
    require_exact_keys,
    require_int,
    require_number,
    require_object,
    require_sha256,
    require_string,
    require_value,
)


PROFILE_SCHEMA = "vera20k.tactical-profile.v2"
CONTRACT_SCHEMA = "vera20k.tactical-capture-contract.v2"
CHECKPOINT = "radar-online-v2"
ABSOLUTE_TIMEOUT_MAX_SECONDS = 900
CHILD_TIMEOUT_SECONDS = 720
POST_L0_TIMEOUT_SECONDS = 600
OVERALL_TICK_CAP = 8192
EVIDENCE_LIMITATIONS = (
    "Rust production-route regression evidence only",
    "native comparator NONE",
    "parity certification NONE",
    "visible-window pacing input audio and wider-map coverage unverified",
)

ENVIRONMENT_DENYLIST = (
    "RA2_QUICKPLAY",
    "RA2_DEV_SKIRMISH_SHELL",
    "RA2_DEBUG_SPAWN_UNITS",
    "RA2_DISABLE_LAT",
    "RA2_ENABLE_LAT",
    "RA2_DEBUG_CAMEO_PALETTES",
    "RA2_DEBUG_BRIDGE_RENDER_BUCKETS",
    "RA2_FORCE_TIB3_TO_TIB01",
    "RA2_TIB_ID_OFFSET",
    "RA2_FORCE_TIB_IMAGE",
    "RA2_DEBUG_MOUSE_CURSOR_SHEET",
    "RA2_NORMAL_COUNT",
    "RA2_NORMALS",
    "RA2_QUEUE_FRAME_MS",
    "RA2_DIR",
    "RA2_DUMP_SHROUD",
    "RA2_DEBUG_DEPTH_VIEW",
)

_TOP_KEYS = (
    "schema_version",
    "profile_id",
    "checkpoint",
    "fixture",
    "launch",
    "capture",
    "budgets",
    "pixel_inputs",
    "evidence_limitations",
)
_FIXTURE_KEYS = (
    "logical_map_name",
    "theater",
    "map_size",
    "local_size",
    "start_waypoint_count",
    "archive_name",
    "archive_byte_length",
    "archive_sha256",
    "mix_entry_id",
    "entry_payload_byte_length",
    "entry_payload_sha256",
    "entry_digest_authority",
    "battle_descriptor_id",
    "catalog_scen_index",
)
_MODE_KEYS = (
    "id",
    "ui_name_key",
    "tooltip_key",
    "override_file",
    "map_filter",
    "random_maps_allowed",
    "allies_allowed",
    "must_ally",
)
_LAUNCH_KEYS = (
    "mode",
    "seed",
    "input_delay_ticks",
    "player_name",
    "local",
    "opponents",
    "options",
)
_SLOT_KEYS = (
    "country",
    "country_random",
    "color_index",
    "color_random",
    "start_position",
    "team",
)
_AI_SLOT_KEYS = _SLOT_KEYS + ("difficulty",)
_OPTION_KEYS = (
    "starting_credits",
    "unit_count",
    "tech_level",
    "game_speed",
    "default_ai_difficulty",
    "short_game",
    "bases",
    "bridges_destroyable",
    "super_weapons",
    "build_off_ally",
    "crates",
    "mcv_redeploy",
    "fog_of_war",
    "shroud",
    "tiberium_grows",
    "multi_engineer",
    "harvester_truce",
    "ally_change_allowed",
)
_CAPTURE_KEYS = (
    "internal_width",
    "internal_height",
    "output_width",
    "output_height",
    "surface_formats",
    "vsync",
    "upscale",
    "extra_animations",
    "exact_step_hz",
    "sim_tick_ms",
    "app_ui_scale",
    "post_load_cursor",
    "cursor_id",
    "software_cursor_required",
    "placement_radius",
    "warm_frames",
    "build_targets",
)
_BUDGET_KEYS = (
    "stages",
    "overall_tick_cap",
    "post_l0_timeout_seconds",
    "child_timeout_seconds",
    "absolute_timeout_max_seconds",
    "expected_ledger",
)
_STAGE_NAMES = (
    "yard_active",
    "power_ready",
    "power_active",
    "refinery_ready",
    "refinery_active",
    "radar_ready",
    "radar_active",
    "radar_online",
    "readiness_and_warm_frames",
)
_STAGE_TICKS = (48, 640, 64, 2048, 64, 1024, 64, 4096, 18)
_STAGE_WALL = (15, 90, 15, 270, 15, 140, 15, 20, 10)
# A placed building completes one tick after its place command plus its
# type's build-up, so the Soviet and Yuri ledgers differ.
_LEDGERS = {
    "soviet": {
        "yard_active": 34,
        "power_ready": 619,
        "power_active": 672,
        "refinery_ready": 2635,
        "refinery_active": 2688,
        "radar_ready": 3644,
        "radar_active": 3676,
    },
    "yuri": {
        "yard_active": 30,
        "power_ready": 615,
        "power_active": 662,
        "refinery_ready": 2625,
        "refinery_active": 2676,
        "radar_ready": 3632,
        "radar_active": 3681,
    },
}
_OPTIONS: Mapping[str, Any] = {
    "starting_credits": 10000,
    "unit_count": 0,
    "tech_level": 10,
    "game_speed": 1,
    "default_ai_difficulty": 0,
    "short_game": True,
    "bases": True,
    "bridges_destroyable": True,
    "super_weapons": True,
    "build_off_ally": True,
    "crates": True,
    "mcv_redeploy": True,
    "fog_of_war": False,
    "shroud": True,
    "tiberium_grows": True,
    "multi_engineer": False,
    "harvester_truce": False,
    "ally_change_allowed": True,
}


@dataclass(frozen=True)
class ValidatedProfile:
    path: Path
    snapshot: FileSnapshot
    document: Mapping[str, Any]

    @property
    def profile_id(self) -> str:
        return str(self.document["profile_id"])

    @property
    def checkpoint(self) -> str:
        return str(self.document["checkpoint"])

    @property
    def fixture(self) -> Mapping[str, Any]:
        return require_object(self.document["fixture"], "fixture")

    @property
    def capture(self) -> Mapping[str, Any]:
        return require_object(self.document["capture"], "capture")

    @property
    def budgets(self) -> Mapping[str, Any]:
        return require_object(self.document["budgets"], "budgets")

    @property
    def pixel_inputs(self) -> Mapping[str, Any]:
        return require_object(self.document["pixel_inputs"], "pixel_inputs")


@dataclass(frozen=True)
class ValidatedContract:
    path: Path
    snapshot: FileSnapshot
    document: Mapping[str, Any]
    denylist: tuple[str, ...]


def repository_root() -> Path:
    return Path(__file__).absolute().parents[2]


def repository_contract_path() -> Path:
    return (
        repository_root()
        / "src"
        / "app"
        / "diagnostics"
        / "tactical_capture"
        / "contract.v2.json"
    )


def _exact_object(value: Any, keys: tuple[str, ...], field: str) -> Mapping[str, Any]:
    result = require_object(value, field)
    require_exact_keys(result, keys, field)
    return result


def _fixed(value: Any, expected: Any, field: str) -> None:
    require_value(value, expected, field)


def _positive_int(value: Any, field: str) -> int:
    parsed = require_int(value, field)
    if parsed <= 0:
        raise ValidationError(f"{field} must be positive")
    return parsed


def _validate_slot(
    value: Any,
    field: str,
    *,
    expected_country: str,
    expected_color: int,
    expected_start: int,
    ai: bool,
) -> None:
    slot = _exact_object(value, _AI_SLOT_KEYS if ai else _SLOT_KEYS, field)
    _fixed(slot["country"], expected_country, f"{field}.country")
    _fixed(slot["country_random"], False, f"{field}.country_random")
    _fixed(slot["color_index"], expected_color, f"{field}.color_index")
    _fixed(slot["color_random"], False, f"{field}.color_random")
    _fixed(slot["start_position"], expected_start, f"{field}.start_position")
    if slot["team"] is not None:
        raise ValidationError(f"{field}.team must be null")
    if ai:
        _fixed(slot["difficulty"], "Easy", f"{field}.difficulty")


def validate_profile_document(document: Mapping[str, Any]) -> None:
    require_exact_keys(document, _TOP_KEYS, "profile")
    if document["schema_version"] != PROFILE_SCHEMA:
        raise ValidationError("unsupported profile schema; use tactical-profile.v2 for the native 1x sidebar; v1 is historical")
    _fixed(document["checkpoint"], CHECKPOINT, "checkpoint")
    profile_id = require_string(document["profile_id"], "profile_id")
    if profile_id not in {"soviet-radar-online-v2", "yuri-radar-online-v2"}:
        raise ValidationError(f"unsupported tactical profile_id {profile_id!r}")

    fixture = _exact_object(document["fixture"], _FIXTURE_KEYS, "fixture")
    fixed_fixture = {
        "logical_map_name": "Fight.MAP",
        "theater": "NEWURBAN",
        "start_waypoint_count": 2,
        "archive_name": "multimd.mix",
        "archive_byte_length": 31_264_268,
        "archive_sha256": (
            "ff4138ba95f7efd8bded14342fc9082b99c47e43c25ab18236e4eea141b488e9"
        ),
        "mix_entry_id": 0x9306F050,
        "entry_payload_byte_length": 91_254,
        "entry_payload_sha256": (
            "d751dce7cd3611077e9228c33235f39c71681fff6ac08ca1f716d963ad6ce070"
        ),
        "entry_digest_authority": "DECLARED_FIXTURE_PROVENANCE",
        "battle_descriptor_id": 1,
        "catalog_scen_index": 12,
    }
    for key, expected in fixed_fixture.items():
        _fixed(fixture[key], expected, f"fixture.{key}")
    for key, expected in (
        ("map_size", {"width": 81, "height": 52}),
        ("local_size", {"width": 75, "height": 42}),
    ):
        dimensions = _exact_object(fixture[key], ("width", "height"), f"fixture.{key}")
        _fixed(dimensions["width"], expected["width"], f"fixture.{key}.width")
        _fixed(dimensions["height"], expected["height"], f"fixture.{key}.height")
    require_sha256(fixture["archive_sha256"], "fixture.archive_sha256")
    require_sha256(
        fixture["entry_payload_sha256"], "fixture.entry_payload_sha256"
    )

    launch = _exact_object(document["launch"], _LAUNCH_KEYS, "launch")
    mode = _exact_object(launch["mode"], _MODE_KEYS, "launch.mode")
    expected_mode = {
        "id": 1,
        "ui_name_key": "GUI:Battle",
        "tooltip_key": "STT:ModeBattle",
        "override_file": "MPBattleMD.ini",
        "map_filter": "standard",
        "random_maps_allowed": True,
        "allies_allowed": True,
        "must_ally": False,
    }
    for key, expected in expected_mode.items():
        _fixed(mode[key], expected, f"launch.mode.{key}")
    _fixed(launch["seed"], 0x12345678, "launch.seed")
    _fixed(launch["input_delay_ticks"], 2, "launch.input_delay_ticks")
    soviet = profile_id.startswith("soviet-")
    _fixed(
        launch["player_name"],
        "VERA-SOVIET" if soviet else "VERA-YURI",
        "launch.player_name",
    )
    _validate_slot(
        launch["local"],
        "launch.local",
        expected_country="Russia" if soviet else "Yuri",
        expected_color=0,
        expected_start=0,
        ai=False,
    )
    opponents = require_array(launch["opponents"], "launch.opponents")
    if len(opponents) != 1:
        raise ValidationError("launch.opponents must contain exactly one active AI")
    _validate_slot(
        opponents[0],
        "launch.opponents[0]",
        expected_country="Yuri" if soviet else "Russia",
        expected_color=1,
        expected_start=1,
        ai=True,
    )
    options = _exact_object(launch["options"], _OPTION_KEYS, "launch.options")
    for key, expected in _OPTIONS.items():
        _fixed(options[key], expected, f"launch.options.{key}")

    capture = _exact_object(document["capture"], _CAPTURE_KEYS, "capture")
    fixed_capture = {
        "internal_width": 800,
        "internal_height": 600,
        "output_width": 800,
        "output_height": 600,
        "vsync": True,
        "upscale": False,
        "extra_animations": True,
        "exact_step_hz": 45,
        "sim_tick_ms": 22,
        "cursor_id": "Default",
        "software_cursor_required": True,
        "placement_radius": 16,
        "warm_frames": 16,
    }
    for key, expected in fixed_capture.items():
        _fixed(capture[key], expected, f"capture.{key}")
    formats = require_array(capture["surface_formats"], "capture.surface_formats")
    if formats != ["Bgra8Unorm", "Bgra8UnormSrgb"]:
        raise ValidationError("capture.surface_formats has unsupported values/order")
    if require_number(capture["app_ui_scale"], "capture.app_ui_scale") != 1.0:
        raise ValidationError("current native sidebar requires capture.app_ui_scale exactly 1.0; half-scale profiles are historical")
    cursor = _exact_object(
        capture["post_load_cursor"], ("x", "y"), "capture.post_load_cursor"
    )
    _fixed(cursor["x"], 316, "capture.post_load_cursor.x")
    _fixed(cursor["y"], 284, "capture.post_load_cursor.y")
    targets = _exact_object(
        capture["build_targets"],
        ("power", "refinery", "radar", "refinery_spawned_harvester"),
        "capture.build_targets",
    )
    expected_targets = (
        {"power": "NAPOWR", "refinery": "NAREFN", "radar": "NARADR"}
        if soviet
        else {"power": "YAPOWR", "refinery": "YAREFN", "radar": "NAPSIS"}
    )
    for key, expected in expected_targets.items():
        _fixed(targets[key], expected, f"capture.build_targets.{key}")
    _fixed(
        targets["refinery_spawned_harvester"],
        "HARV" if soviet else None,
        "capture.build_targets.refinery_spawned_harvester",
    )

    budgets = _exact_object(document["budgets"], _BUDGET_KEYS, "budgets")
    stages = require_array(budgets["stages"], "budgets.stages")
    if len(stages) != len(_STAGE_NAMES):
        raise ValidationError("budgets.stages must contain exactly nine stages")
    for index, (name, tick_cap, wall) in enumerate(
        zip(_STAGE_NAMES, _STAGE_TICKS, _STAGE_WALL, strict=True)
    ):
        stage = _exact_object(
            stages[index], ("name", "tick_cap", "wall_seconds"), f"budgets.stages[{index}]"
        )
        _fixed(stage["name"], name, f"budgets.stages[{index}].name")
        _fixed(stage["tick_cap"], tick_cap, f"budgets.stages[{index}].tick_cap")
        _fixed(
            stage["wall_seconds"], wall, f"budgets.stages[{index}].wall_seconds"
        )
    _fixed(budgets["overall_tick_cap"], OVERALL_TICK_CAP, "budgets.overall_tick_cap")
    _fixed(
        budgets["post_l0_timeout_seconds"],
        POST_L0_TIMEOUT_SECONDS,
        "budgets.post_l0_timeout_seconds",
    )
    _fixed(
        budgets["child_timeout_seconds"],
        CHILD_TIMEOUT_SECONDS,
        "budgets.child_timeout_seconds",
    )
    _fixed(
        budgets["absolute_timeout_max_seconds"],
        ABSOLUTE_TIMEOUT_MAX_SECONDS,
        "budgets.absolute_timeout_max_seconds",
    )
    expected_ledger = _LEDGERS["soviet" if soviet else "yuri"]
    ledger = _exact_object(
        budgets["expected_ledger"], tuple(expected_ledger), "budgets.expected_ledger"
    )
    for key, expected in expected_ledger.items():
        _fixed(ledger[key], expected, f"budgets.expected_ledger.{key}")

    pixel_inputs = _exact_object(
        document["pixel_inputs"], ("font",), "pixel_inputs"
    )
    font = _exact_object(
        pixel_inputs["font"], ("path", "byte_length", "sha256"), "pixel_inputs.font"
    )
    _fixed(
        font["path"],
        r"C:\Windows\Fonts\verdana.ttf",
        "pixel_inputs.font.path",
    )
    _fixed(font["byte_length"], 243_304, "pixel_inputs.font.byte_length")
    _fixed(
        font["sha256"],
        "6a8481fe107ee547893c018b13dba291c2020bec3de5da6525d9ac09f6bc2105",
        "pixel_inputs.font.sha256",
    )
    limitations = require_array(
        document["evidence_limitations"], "evidence_limitations"
    )
    if limitations != list(EVIDENCE_LIMITATIONS):
        raise ValidationError(
            "evidence_limitations differ from the honest tactical v2 limits"
        )


def load_profile(path: str | os.PathLike[str]) -> ValidatedProfile:
    snapshot, document = load_json_file(path, "tactical profile")
    validate_profile_document(document)
    return ValidatedProfile(snapshot.path, snapshot, document)


def load_contract(
    path: str | os.PathLike[str],
    *,
    require_repository_bytes: bool = True,
) -> ValidatedContract:
    snapshot, document = load_json_file(path, "tactical contract")
    require_exact_keys(
        document,
        (
            "schema_version",
            "absolute_max_child_timeout_seconds",
            "environment_denylist",
        ),
        "contract",
    )
    _fixed(document["schema_version"], CONTRACT_SCHEMA, "contract.schema_version")
    _fixed(
        document["absolute_max_child_timeout_seconds"],
        ABSOLUTE_TIMEOUT_MAX_SECONDS,
        "contract.absolute_max_child_timeout_seconds",
    )
    denylist_values = require_array(
        document["environment_denylist"], "contract.environment_denylist"
    )
    denylist: tuple[str, ...] = tuple(
        require_string(value, f"contract.environment_denylist[{index}]")
        for index, value in enumerate(denylist_values)
    )
    if denylist != ENVIRONMENT_DENYLIST:
        raise ValidationError("contract.environment_denylist differs from v2")
    if len(set(denylist)) != len(denylist):
        raise ValidationError("contract.environment_denylist contains duplicates")
    if require_repository_bytes:
        repository_snapshot, _ = load_json_file(
            repository_contract_path(), "embedded tactical contract source"
        )
        if snapshot.raw != repository_snapshot.raw:
            raise ValidationError(
                "external tactical contract bytes differ from repository contract bytes"
            )
    return ValidatedContract(snapshot.path, snapshot, document, denylist)


def reject_denied_environment(
    contract: ValidatedContract, environment: Mapping[str, str] | None = None
) -> None:
    values = os.environ if environment is None else environment
    present = [name for name in contract.denylist if name in values]
    if present:
        raise ValidationError(
            f"tactical capture environment contains denied overrides: {present}"
        )


def scan_tactical_environment_names(root: Path | None = None) -> set[str]:
    """Discover literal RA2-prefixed names in Rust and tactical-wrapper sources."""

    repo = repository_root() if root is None else root
    candidates = [
        repo / "src",
        repo / "tools" / "tactical_certification",
    ]
    # Names are string values even when passed through a constant or helper.
    # Bare Rust constants with the same prefix are not environment inputs.
    pattern = re.compile(r'''["'](RA2_[A-Z0-9_]+)["']''')
    discovered: set[str] = set()
    for candidate in candidates:
        paths = [candidate] if candidate.is_file() else (
            sorted(candidate.rglob("*")) if candidate.is_dir() else []
        )
        for path in paths:
            if path.suffix.lower() not in {".rs", ".py", ".json"} or not path.is_file():
                continue
            try:
                text = path.read_text(encoding="utf-8")
            except (OSError, UnicodeDecodeError) as exc:
                raise ValidationError(f"cannot scan tactical source {path}: {exc}") from exc
            discovered.update(pattern.findall(text))
    return discovered


def validate_contract_source_coverage(contract: ValidatedContract) -> None:
    missing = sorted(scan_tactical_environment_names() - set(contract.denylist))
    if missing:
        raise ValidationError(
            f"tactical source references environment names absent from contract: {missing}"
        )
