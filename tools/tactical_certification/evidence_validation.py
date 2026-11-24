"""Strict v3 evidence-leaf validation for tactical certification.

This module interprets only the immutable manifest objects emitted by the
tactical child. Process launch, filesystem publication, and report ownership
remain in ``orchestrator.py``.
"""

from __future__ import annotations

from typing import Any, Mapping, Protocol

from .core import (
    FileSnapshot,
    ValidationError,
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
from .profile import (
    CHECKPOINT,
    CONTRACT_SCHEMA,
    ValidatedContract,
    ValidatedProfile,
)

# A frame is one native logic frame however fast the capture stepped it, so the
# ledger's total_simulation_ms counts frames at 1000 / 15 ms; the profile's
# sim_tick_ms only paces the capture (VERA20k issue #5107).
NATIVE_LOGIC_FRAMES_PER_SECOND = 15


STABLE_KEYS = (
    "inputs",
    "map_source",
    "lifecycle",
    "graphics",
    "contract",
    "profile",
    "startup",
    "production",
    "render",
    "final_fingerprint",
    "known_residuals",
)
RUN_KEYS = ("process_id", "elapsed_ms", "render_frames", "exact_steps")
KNOWN_RESIDUALS = ["Native pixels and whole-game parity remain unverified."]
ALLOWED_SURFACE_FORMATS = frozenset(("Bgra8Unorm", "Bgra8UnormSrgb"))
PRODUCTION_PROGRESS_INTERVALS = 53
STOCK_DEPLOY_FACING = 0x80
# Stock Foundation= values for the fixed profile types.
STOCK_FOUNDATIONS = {
    "NACNST": (4, 4),
    "YACNST": (4, 4),
    "NAPOWR": (3, 2),
    "YAPOWR": (2, 2),
    "NAREFN": (4, 3),
    "YAREFN": (2, 2),
    "NARADR": (2, 2),
    "NAPSIS": (2, 2),
}


class EnvironmentEvidence(Protocol):
    config: FileSnapshot
    executable: FileSnapshot
    archive: FileSnapshot
    font_archive: FileSnapshot


def _exact_object(value: Any, keys: tuple[str, ...], field: str) -> Mapping[str, Any]:
    result = require_object(value, field)
    require_exact_keys(result, keys, field)
    return result


def _positive_int(value: Any, field: str) -> int:
    parsed = require_int(value, field)
    if parsed <= 0:
        raise ValidationError(f"{field} must be positive")
    return parsed


def _nonnegative_int(value: Any, field: str) -> int:
    parsed = require_int(value, field)
    if parsed < 0:
        raise ValidationError(f"{field} must be nonnegative")
    return parsed


def _positive_number(value: Any, field: str) -> float:
    parsed = require_number(value, field)
    if parsed <= 0.0:
        raise ValidationError(f"{field} must be positive")
    return parsed


def _numeric_value(value: Any, expected: int | float, field: str) -> None:
    parsed = require_number(value, field)
    if parsed != float(expected):
        raise ValidationError(f"{field} is {parsed!r}, expected {float(expected)!r}")


def _int_pair(value: Any, field: str) -> tuple[int, int]:
    pair = require_array(value, field)
    if len(pair) != 2:
        raise ValidationError(f"{field} must contain exactly two integers")
    return (
        _nonnegative_int(pair[0], f"{field}[0]"),
        _nonnegative_int(pair[1], f"{field}[1]"),
    )


def _enum_variant(
    value: Any,
    variant: str,
    keys: tuple[str, ...],
    field: str,
) -> Mapping[str, Any]:
    enum_value = _exact_object(value, (variant,), field)
    return _exact_object(enum_value[variant], keys, f"{field}.{variant}")


def require_identity(
    value: Any,
    field: str,
    snapshot: FileSnapshot,
    *,
    extra: Mapping[str, Any],
) -> Mapping[str, Any]:
    identity = require_object(value, field)
    require_exact_keys(
        identity,
        ("path", "byte_length", "sha256", *extra.keys()),
        field,
    )
    require_value(identity["path"], str(snapshot.path), f"{field}.path")
    require_value(identity["byte_length"], snapshot.byte_length, f"{field}.byte_length")
    require_value(identity["sha256"], snapshot.sha256, f"{field}.sha256")
    for key, expected in extra.items():
        require_value(identity[key], expected, f"{field}.{key}")
    return identity


def _require_inputs(
    stable: Mapping[str, Any],
    environment: EnvironmentEvidence,
) -> None:
    inputs = _exact_object(
        stable.get("inputs"),
        ("config", "executable", "archive", "font_archive"),
        "evidence.stable.inputs",
    )
    for key, snapshot in (
        ("config", environment.config),
        ("executable", environment.executable),
        ("archive", environment.archive),
        ("font_archive", environment.font_archive),
    ):
        require_identity(
            inputs[key],
            f"evidence.stable.inputs.{key}",
            snapshot,
            extra={},
        )


def _require_map_source(
    stable: Mapping[str, Any],
    profile: ValidatedProfile,
) -> None:
    field = "evidence.stable.map_source"
    source = _exact_object(
        stable.get("map_source"),
        (
            "archive_name",
            "mix_entry_id",
            "payload_byte_length",
            "payload_sha256",
            "entry_digest_authority",
            "loose_shadow_rejected",
            "logical_map_name",
            "loaded_source",
        ),
        field,
    )
    fixture = profile.fixture
    expected_values = {
        "archive_name": fixture["archive_name"],
        "mix_entry_id": fixture["mix_entry_id"],
        "payload_byte_length": fixture["entry_payload_byte_length"],
        "payload_sha256": fixture["entry_payload_sha256"],
        "entry_digest_authority": fixture["entry_digest_authority"],
        "loose_shadow_rejected": True,
        "logical_map_name": fixture["logical_map_name"],
    }
    for key, expected in expected_values.items():
        require_value(source[key], expected, f"{field}.{key}")
    require_sha256(source["payload_sha256"], f"{field}.payload_sha256")

    loaded = _exact_object(
        source["loaded_source"],
        ("kind", "logical_name", "source_archive", "entry_id", "payload_len", "source_sha256"),
        f"{field}.loaded_source",
    )
    mix_entry_id = require_int(fixture["mix_entry_id"], "fixture.mix_entry_id")
    signed_entry_id = (
        mix_entry_id if mix_entry_id <= 0x7FFF_FFFF else mix_entry_id - 0x1_0000_0000
    )
    loaded_expected = {
        "kind": "mix",
        "logical_name": fixture["logical_map_name"],
        "source_archive": fixture["archive_name"],
        "entry_id": signed_entry_id,
        "payload_len": fixture["entry_payload_byte_length"],
        "source_sha256": fixture["entry_payload_sha256"],
    }
    for key, expected in loaded_expected.items():
        require_value(loaded[key], expected, f"{field}.loaded_source.{key}")
    require_sha256(loaded["source_sha256"], f"{field}.loaded_source.source_sha256")


def _require_lifecycle(stable: Mapping[str, Any]) -> None:
    field = "evidence.stable.lifecycle"
    lifecycle = _exact_object(
        stable.get("lifecycle"),
        ("window_hidden", "window_focused", "focus_violations", "input_violations"),
        field,
    )
    for key, expected in (
        ("window_hidden", True),
        ("window_focused", False),
        ("focus_violations", 0),
        ("input_violations", 0),
    ):
        require_value(lifecycle[key], expected, f"{field}.{key}")


def _require_graphics(
    stable: Mapping[str, Any],
    profile: ValidatedProfile,
    environment: EnvironmentEvidence,
) -> None:
    field = "evidence.stable.graphics"
    graphics = _exact_object(
        stable.get("graphics"),
        (
            "adapter",
            "surface_format",
            "width",
            "height",
            "window_scale_factor",
            "app_ui_scale",
            "bitmap_font",
        ),
        field,
    )
    adapter = _exact_object(
        graphics["adapter"],
        (
            "name",
            "vendor",
            "device",
            "device_type",
            "driver",
            "driver_info",
            "backend",
        ),
        f"{field}.adapter",
    )
    for key in ("name", "device_type", "driver", "driver_info", "backend"):
        require_string(adapter[key], f"{field}.adapter.{key}")
    for key in ("vendor", "device"):
        _nonnegative_int(adapter[key], f"{field}.adapter.{key}")

    capture = profile.capture
    require_value(graphics["width"], capture["output_width"], f"{field}.width")
    require_value(graphics["height"], capture["output_height"], f"{field}.height")
    surface = require_string(graphics["surface_format"], f"{field}.surface_format")
    if surface not in ALLOWED_SURFACE_FORMATS:
        raise ValidationError(f"{field}.surface_format is unsupported: {surface}")
    _positive_number(graphics["window_scale_factor"], f"{field}.window_scale_factor")
    _numeric_value(
        graphics["app_ui_scale"],
        capture["app_ui_scale"],
        f"{field}.app_ui_scale",
    )
    _require_bitmap_font(graphics["bitmap_font"], f"{field}.bitmap_font", profile)


def _require_bitmap_font(value: Any, field: str, profile: ValidatedProfile) -> None:
    font = _exact_object(
        value,
        ("source", "atlas_width", "atlas_height", "glyph_count", "cell_height",
         "bitmap_rows", "missing_glyph_present", "darken_texture_present"),
        field,
    )
    source = _exact_object(
        font["source"],
        ("logical_name", "source_archive", "entry_id", "payload_byte_length", "payload_sha256"),
        f"{field}.source",
    )
    expected = require_object(
        profile.pixel_inputs["bitmap_font"], "pixel_inputs.bitmap_font"
    )
    for key in source:
        require_value(source[key], expected[key], f"{field}.source.{key}")
    require_sha256(source["payload_sha256"], f"{field}.source.payload_sha256")
    for key in ("atlas_width", "atlas_height", "glyph_count"):
        _positive_int(font[key], f"{field}.{key}")
    require_value(font["cell_height"], 17, f"{field}.cell_height")
    require_value(font["bitmap_rows"], 16, f"{field}.bitmap_rows")
    require_value(font["missing_glyph_present"], True, f"{field}.missing_glyph_present")
    require_value(font["darken_texture_present"], True, f"{field}.darken_texture_present")


def _require_contract(
    stable: Mapping[str, Any],
    contract: ValidatedContract,
) -> None:
    field = "evidence.stable.contract"
    evidence = _exact_object(
        stable.get("contract"),
        ("schema_version", "sha256", "embedded_bytes_equal"),
        field,
    )
    require_value(evidence["schema_version"], CONTRACT_SCHEMA, f"{field}.schema_version")
    require_value(evidence["sha256"], contract.snapshot.sha256, f"{field}.sha256")
    require_value(evidence["embedded_bytes_equal"], True, f"{field}.embedded_bytes_equal")


def _require_profile(stable: Mapping[str, Any], profile: ValidatedProfile) -> None:
    field = "evidence.stable.profile"
    evidence = _exact_object(
        stable.get("profile"),
        ("profile_id", "checkpoint", "fixture_entry_sha256"),
        field,
    )
    require_value(evidence["profile_id"], profile.profile_id, f"{field}.profile_id")
    require_value(evidence["checkpoint"], CHECKPOINT, f"{field}.checkpoint")
    require_value(
        evidence["fixture_entry_sha256"],
        profile.fixture["entry_payload_sha256"],
        f"{field}.fixture_entry_sha256",
    )


def _require_startup(stable: Mapping[str, Any], profile: ValidatedProfile) -> None:
    field = "evidence.stable.startup"
    startup = _exact_object(
        stable.get("startup"),
        (
            "correlation",
            "seed",
            "seed_source",
            "seed_authority_certifying",
            "classification",
        ),
        field,
    )
    _positive_int(startup["correlation"], f"{field}.correlation")
    launch = require_object(profile.document["launch"], "launch")
    require_value(startup["seed"], launch["seed"], f"{field}.seed")
    require_value(startup["seed_source"], "Controlled", f"{field}.seed_source")
    require_value(
        startup["seed_authority_certifying"],
        True,
        f"{field}.seed_authority_certifying",
    )
    require_value(
        startup["classification"],
        "AcceptedExplicitFixedBattle",
        f"{field}.classification",
    )


def _require_observed_ledger(
    value: Any, field: str, profile: ValidatedProfile, *, completed: bool,
) -> Mapping[str, Any]:
    expected = profile.budgets["expected_ledger"]
    fixed = {"rust_l0_tick": 0, **{f"{key}_tick": value for key, value in expected.items()}}
    ledger = _exact_object(value, (*fixed, "radar_online_tick", "second_readiness_tick",
                                  "capture_requested_tick", "capture_complete_tick"), field)
    for key, value in fixed.items():
        require_value(ledger[key], value, f"{field}.{key}")
    online = _nonnegative_int(ledger["radar_online_tick"], f"{field}.radar_online_tick")
    active = expected["radar_active"]
    opening_cap = profile.budgets["stages"][7]["tick_cap"]
    if not active <= online <= active + opening_cap:
        raise ValidationError(f"{field}.radar_online_tick is outside the bounded opening interval")
    second = online + 1
    capture = second + profile.capture["warm_frames"]
    if capture > profile.budgets["overall_tick_cap"]:
        raise ValidationError(f"{field}.capture_requested_tick exceeds overall tick cap")
    require_value(ledger["second_readiness_tick"], second, f"{field}.second_readiness_tick")
    require_value(ledger["capture_requested_tick"], capture, f"{field}.capture_requested_tick")
    require_value(ledger["capture_complete_tick"], capture if completed else None,
                  f"{field}.capture_complete_tick")
    return ledger


def _observed_capture_tick(stable: Mapping[str, Any], profile: ValidatedProfile) -> int:
    production = require_object(stable["production"], "evidence.stable.production")
    ledger = _require_observed_ledger(production["observed_ledger"],
        "evidence.stable.production.observed_ledger", profile, completed=True)
    return ledger["capture_complete_tick"]


def _require_step_receipt(
    value: Any,
    field: str,
    *,
    expected_tick_before: int,
    expected_tick_after: int,
) -> Mapping[str, Any]:
    receipt = _exact_object(
        value,
        (
            "tick_before",
            "tick_after",
            "binary_frame_before",
            "binary_frame_after",
        ),
        field,
    )
    expected = {
        "tick_before": expected_tick_before,
        "tick_after": expected_tick_after,
    }
    for key, expected_value in expected.items():
        require_value(receipt[key], expected_value, f"{field}.{key}")
    before_frame = _nonnegative_int(receipt["binary_frame_before"], f"{field}.binary_frame_before")
    after_frame = _nonnegative_int(receipt["binary_frame_after"], f"{field}.binary_frame_after")
    # Exact-step production commits one native frame per simulation tick.
    # The selected wall-time quantum does not change the frame ordinal.
    expected_before_frame = expected_tick_before
    expected_after_frame = expected_tick_after
    require_value(
        before_frame,
        expected_before_frame,
        f"{field}.binary_frame_before",
    )
    require_value(
        after_frame,
        expected_after_frame,
        f"{field}.binary_frame_after",
    )
    return receipt


def _require_placement(value: Any, field: str, expected_type: str) -> Mapping[str, Any]:
    placement = _exact_object(
        value,
        (
            "anchor_cell",
            "anchor_yard_id",
            "candidate_index",
            "cell",
            "foundation",
            "radius",
            "type_id",
        ),
        field,
    )
    _int_pair(placement["anchor_cell"], f"{field}.anchor_cell")
    _positive_int(placement["anchor_yard_id"], f"{field}.anchor_yard_id")
    _nonnegative_int(placement["candidate_index"], f"{field}.candidate_index")
    _int_pair(placement["cell"], f"{field}.cell")
    foundation = _int_pair(placement["foundation"], f"{field}.foundation")
    if foundation[0] == 0 or foundation[1] == 0:
        raise ValidationError(f"{field}.foundation must be positive")
    radius = _positive_int(placement["radius"], f"{field}.radius")
    require_value(placement["type_id"], expected_type, f"{field}.type_id")
    return placement


def _require_binding(value: Any, field: str, expected_type: str | None) -> Mapping[str, Any]:
    binding = _exact_object(value, ("cell", "stable_id", "type_id"), field)
    _int_pair(binding["cell"], f"{field}.cell")
    _positive_int(binding["stable_id"], f"{field}.stable_id")
    type_id = require_string(binding["type_id"], f"{field}.type_id")
    if not type_id:
        raise ValidationError(f"{field}.type_id must be nonempty")
    if expected_type is not None:
        require_value(type_id, expected_type, f"{field}.type_id")
    return binding


def _ordered_square_ring_candidate(
    anchor: tuple[int, int],
    candidate_index: int,
    max_radius: int,
    grid_extent: int,
    field: str,
) -> tuple[tuple[int, int], int]:
    """Mirror the child's clipped square-ring order for the pinned map grid."""

    anchor_x, anchor_y = anchor
    if (
        anchor_x - max_radius < 0
        or anchor_y - max_radius < 0
        or anchor_x + max_radius >= grid_extent
        or anchor_y + max_radius >= grid_extent
    ):
        raise ValidationError(
            f"{field}.anchor_cell is too close to the pinned map-grid edge"
        )

    candidates: list[tuple[tuple[int, int], int]] = [((anchor_x, anchor_y), 0)]
    for radius in range(1, max_radius + 1):
        min_x = anchor_x - radius
        max_x = anchor_x + radius
        min_y = anchor_y - radius
        max_y = anchor_y + radius
        candidates.extend(
            ((x, min_y), radius) for x in range(min_x, max_x + 1)
        )
        candidates.extend(
            ((x, max_y), radius) for x in range(min_x, max_x + 1)
        )
        candidates.extend(
            ((min_x, y), radius) for y in range(min_y + 1, max_y)
        )
        candidates.extend(
            ((max_x, y), radius) for y in range(min_y + 1, max_y)
        )

    if candidate_index >= len(candidates):
        raise ValidationError(
            f"{field}.candidate_index exceeds the profile placement search"
        )
    return candidates[candidate_index]


def _expected_production_rate(
    expected_ledger: Mapping[str, Any],
    scheduled_key: str,
    ready_key: str,
    delay: int,
) -> int:
    scheduled = require_int(
        expected_ledger[scheduled_key],
        f"budgets.expected_ledger.{scheduled_key}",
    )
    ready = require_int(
        expected_ledger[ready_key],
        f"budgets.expected_ledger.{ready_key}",
    )
    progress_ticks = ready - scheduled - delay - 1
    if (
        progress_ticks <= 0
        or progress_ticks % PRODUCTION_PROGRESS_INTERVALS != 0
    ):
        raise ValidationError(
            f"budgets.expected_ledger cannot derive the {ready_key} production rate"
        )
    return progress_ticks // PRODUCTION_PROGRESS_INTERVALS


def _foundation_rect(
    cell: tuple[int, int],
    foundation: tuple[int, int],
    grid_extent: int,
    field: str,
) -> tuple[int, int, int, int]:
    x, y = cell
    width, height = foundation
    right = x + width
    bottom = y + height
    if right > grid_extent or bottom > grid_extent:
        raise ValidationError(f"{field} exceeds the pinned map grid")
    return (x, y, right, bottom)


def _rects_overlap(
    first: tuple[int, int, int, int],
    second: tuple[int, int, int, int],
) -> bool:
    return (
        first[0] < second[2]
        and second[0] < first[2]
        and first[1] < second[3]
        and second[1] < first[3]
    )


def _require_production(
    stable: Mapping[str, Any],
    profile: ValidatedProfile,
) -> Mapping[str, Any]:
    field = "evidence.stable.production"
    production = _exact_object(
        stable.get("production"),
        (
            "exact_step_count",
            "first_exact_step",
            "last_exact_step",
            "command_ledger",
            "placement_ledger",
            "structure_bindings",
            "harvester",
            "observed_ledger",
        ),
        field,
    )
    capture = profile.capture
    expected_ledger = require_object(
        profile.budgets["expected_ledger"],
        "budgets.expected_ledger",
    )
    capture_tick = _observed_capture_tick(stable, profile)
    sim_tick_ms = require_int(capture["sim_tick_ms"], "capture.sim_tick_ms")
    require_value(production["exact_step_count"], capture_tick, f"{field}.exact_step_count")
    first = _require_step_receipt(
        production["first_exact_step"],
        f"{field}.first_exact_step",
        expected_tick_before=0,
        expected_tick_after=1,
    )
    last = _require_step_receipt(
        production["last_exact_step"],
        f"{field}.last_exact_step",
        expected_tick_before=capture_tick - 1,
        expected_tick_after=capture_tick,
    )

    build_targets = require_object(capture["build_targets"], "capture.build_targets")
    roles = ("Power", "Refinery", "Radar")
    type_ids = (
        require_string(build_targets["power"], "capture.build_targets.power"),
        require_string(build_targets["refinery"], "capture.build_targets.refinery"),
        require_string(build_targets["radar"], "capture.build_targets.radar"),
    )
    role_keys = ("power", "refinery", "radar")
    expected_yard = (
        "NACNST" if profile.profile_id.startswith("soviet-") else "YACNST"
    )
    bindings = _exact_object(
        production["structure_bindings"],
        ("yard", *role_keys),
        f"{field}.structure_bindings",
    )
    validated_bindings = {
        "yard": _require_binding(
            bindings["yard"],
            f"{field}.structure_bindings.yard",
            expected_yard,
        ),
        **{
            role_key: _require_binding(
                bindings[role_key],
                f"{field}.structure_bindings.{role_key}",
                type_ids[index],
            )
            for index, role_key in enumerate(role_keys)
        },
    }

    harvester_type = build_targets["refinery_spawned_harvester"]
    validated_harvester: Mapping[str, Any] | None = None
    if harvester_type is None:
        require_value(production["harvester"], None, f"{field}.harvester")
    else:
        validated_harvester = _require_binding(
            production["harvester"],
            f"{field}.harvester",
            require_string(
                harvester_type,
                "capture.build_targets.refinery_spawned_harvester",
            ),
        )

    stable_ids = [
        binding["stable_id"] for binding in validated_bindings.values()
    ]
    if validated_harvester is not None:
        stable_ids.append(validated_harvester["stable_id"])
    if len(set(stable_ids)) != len(stable_ids):
        raise ValidationError(
            f"{field} structure/harvester stable IDs must be unique"
        )

    placements = require_array(production["placement_ledger"], f"{field}.placement_ledger")
    if len(placements) != 3:
        raise ValidationError(f"{field}.placement_ledger must contain exactly three placements")
    validated_placements = [
        _require_placement(
            placement,
            f"{field}.placement_ledger[{index}]",
            type_ids[index],
        )
        for index, placement in enumerate(placements)
    ]
    yard_binding = validated_bindings["yard"]
    yard_cell = _int_pair(
        yard_binding["cell"],
        f"{field}.structure_bindings.yard.cell",
    )
    max_radius = require_int(capture["placement_radius"], "capture.placement_radius")
    map_size = require_object(profile.fixture["map_size"], "fixture.map_size")
    grid_extent = (
        require_int(map_size["width"], "fixture.map_size.width")
        + require_int(map_size["height"], "fixture.map_size.height")
    )
    occupied_rects = [
        (
            "yard",
            _foundation_rect(
                yard_cell,
                STOCK_FOUNDATIONS[expected_yard],
                grid_extent,
                f"{field}.structure_bindings.yard",
            ),
        )
    ]
    for index, placement in enumerate(validated_placements):
        placement_field = f"{field}.placement_ledger[{index}]"
        require_value(
            placement["anchor_yard_id"],
            yard_binding["stable_id"],
            f"{placement_field}.anchor_yard_id",
        )
        require_value(
            placement["anchor_cell"],
            yard_binding["cell"],
            f"{placement_field}.anchor_cell",
        )
        radius = require_int(placement["radius"], f"{placement_field}.radius")
        if radius > max_radius:
            raise ValidationError(
                f"{placement_field}.radius exceeds capture.placement_radius"
            )
        candidate_index = require_int(
            placement["candidate_index"],
            f"{placement_field}.candidate_index",
        )
        expected_cell, expected_radius = _ordered_square_ring_candidate(
            yard_cell,
            candidate_index,
            max_radius,
            grid_extent,
            placement_field,
        )
        require_value(
            placement["cell"],
            list(expected_cell),
            f"{placement_field}.cell",
        )
        require_value(
            radius,
            expected_radius,
            f"{placement_field}.radius",
        )
        require_value(
            placement["cell"],
            validated_bindings[role_keys[index]]["cell"],
            f"{placement_field}.cell",
        )
        expected_foundation = STOCK_FOUNDATIONS[type_ids[index]]
        require_value(
            placement["foundation"],
            list(expected_foundation),
            f"{placement_field}.foundation",
        )
        placement_cell = _int_pair(
            placement["cell"],
            f"{placement_field}.cell",
        )
        placement_rect = _foundation_rect(
            placement_cell,
            expected_foundation,
            grid_extent,
            f"{placement_field}.foundation",
        )
        for occupied_role, occupied_rect in occupied_rects:
            if _rects_overlap(placement_rect, occupied_rect):
                raise ValidationError(
                    f"{placement_field}.foundation overlaps {occupied_role}"
                )
        occupied_rects.append((role_keys[index], placement_rect))

    if validated_harvester is not None:
        harvester_cell = _int_pair(
            validated_harvester["cell"],
            f"{field}.harvester.cell",
        )
        if (
            harvester_cell[0] >= grid_extent
            or harvester_cell[1] >= grid_extent
        ):
            raise ValidationError(f"{field}.harvester.cell is outside the map grid")
        for occupied_role, occupied_rect in occupied_rects:
            if (
                occupied_rect[0] <= harvester_cell[0] < occupied_rect[2]
                and occupied_rect[1] <= harvester_cell[1] < occupied_rect[3]
            ):
                raise ValidationError(
                    f"{field}.harvester.cell overlaps {occupied_role}"
                )

    commands = require_array(production["command_ledger"], f"{field}.command_ledger")
    if len(commands) != 8:
        raise ValidationError(f"{field}.command_ledger must contain exactly eight commands")
    launch = require_object(profile.document["launch"], "launch")
    owner = require_string(launch["player_name"], "launch.player_name")
    # Configured network delay is retained in the launch. Offline issue stamps
    # are current-frame ordinals; committed command effects are observed next frame.
    delay = 1
    scheduled_ticks = (
        0,
        delay,
        expected_ledger["yard_active"],
        expected_ledger["power_ready"],
        expected_ledger["power_active"],
        expected_ledger["refinery_ready"],
        expected_ledger["refinery_active"],
        expected_ledger["radar_ready"],
    )
    expected_rates = tuple(
        _expected_production_rate(
            expected_ledger,
            scheduled_key,
            ready_key,
            delay,
        )
        for scheduled_key, ready_key in (
            ("yard_active", "power_ready"),
            ("power_active", "refinery_ready"),
            ("refinery_active", "radar_ready"),
        )
    )
    mcv_id: int | None = None
    for index, command_value in enumerate(commands):
        command_field = f"{field}.command_ledger[{index}]"
        command = _exact_object(
            command_value,
            (
                "action_id",
                "scheduled_tick",
                "execute_tick",
                "owner",
                "payload",
                "expected_result",
                "resolved_result",
            ),
            command_field,
        )
        require_value(command["action_id"], index + 1, f"{command_field}.action_id")
        require_value(
            command["scheduled_tick"],
            scheduled_ticks[index],
            f"{command_field}.scheduled_tick",
        )
        require_value(
            command["execute_tick"],
            scheduled_ticks[index],
            f"{command_field}.execute_tick",
        )
        require_value(command["owner"], owner, f"{command_field}.owner")
        payload = require_object(command["payload"], f"{command_field}.payload")
        if index < 2:
            require_exact_keys(payload, ("DeployMcv",), f"{command_field}.payload")
            deploy = _exact_object(
                payload["DeployMcv"],
                ("attempt", "entity_id"),
                f"{command_field}.payload.DeployMcv",
            )
            require_value(
                deploy["attempt"],
                "First" if index == 0 else "Second",
                f"{command_field}.payload.DeployMcv.attempt",
            )
            entity_id = _positive_int(
                deploy["entity_id"],
                f"{command_field}.payload.DeployMcv.entity_id",
            )
            if mcv_id is None:
                mcv_id = entity_id
            elif entity_id != mcv_id:
                raise ValidationError("deploy commands refer to different MCVs")
            if index == 0:
                expected_result = _enum_variant(
                    command["expected_result"],
                    "McvTurnOrYard",
                    ("deploy_facing", "mcv_id", "yard_type_id"),
                    f"{command_field}.expected_result",
                )
                require_value(
                    expected_result["mcv_id"],
                    entity_id,
                    f"{command_field}.expected_result.McvTurnOrYard.mcv_id",
                )
                require_value(
                    expected_result["yard_type_id"],
                    expected_yard,
                    f"{command_field}.expected_result.McvTurnOrYard.yard_type_id",
                )
                deploy_facing = _nonnegative_int(
                    expected_result["deploy_facing"],
                    f"{command_field}.expected_result.McvTurnOrYard.deploy_facing",
                )
                require_value(
                    deploy_facing,
                    STOCK_DEPLOY_FACING,
                    f"{command_field}.expected_result.McvTurnOrYard.deploy_facing",
                )
                resolved_result = _enum_variant(
                    command["resolved_result"],
                    "McvTurned",
                    ("facing", "mcv_id"),
                    f"{command_field}.resolved_result",
                )
                require_value(
                    resolved_result["mcv_id"],
                    entity_id,
                    f"{command_field}.resolved_result.McvTurned.mcv_id",
                )
                require_value(
                    resolved_result["facing"],
                    deploy_facing,
                    f"{command_field}.resolved_result.McvTurned.facing",
                )
            else:
                expected_result = _enum_variant(
                    command["expected_result"],
                    "YardCreated",
                    ("mcv_id", "yard_type_id"),
                    f"{command_field}.expected_result",
                )
                require_value(
                    expected_result["mcv_id"],
                    entity_id,
                    f"{command_field}.expected_result.YardCreated.mcv_id",
                )
                require_value(
                    expected_result["yard_type_id"],
                    expected_yard,
                    f"{command_field}.expected_result.YardCreated.yard_type_id",
                )
                resolved_result = _enum_variant(
                    command["resolved_result"],
                    "YardObserved",
                    ("cell", "stable_id"),
                    f"{command_field}.resolved_result",
                )
                require_value(
                    resolved_result["cell"],
                    yard_binding["cell"],
                    f"{command_field}.resolved_result.YardObserved.cell",
                )
                require_value(
                    resolved_result["stable_id"],
                    yard_binding["stable_id"],
                    f"{command_field}.resolved_result.YardObserved.stable_id",
                )
            continue

        role_index = (index - 2) // 2
        role = roles[role_index]
        type_id = type_ids[role_index]
        role_binding = validated_bindings[role_keys[role_index]]
        if index % 2 == 0:
            require_exact_keys(payload, ("QueueExactType",), f"{command_field}.payload")
            queue = _exact_object(
                payload["QueueExactType"],
                ("role", "type_id"),
                f"{command_field}.payload.QueueExactType",
            )
            require_value(queue["role"], role, f"{command_field}.payload.QueueExactType.role")
            require_value(
                queue["type_id"],
                type_id,
                f"{command_field}.payload.QueueExactType.type_id",
            )
            expected_result = _enum_variant(
                command["expected_result"],
                "QueueOrReady",
                ("expected_rate_frames", "type_id"),
                f"{command_field}.expected_result",
            )
            require_value(
                expected_result["expected_rate_frames"],
                expected_rates[role_index],
                f"{command_field}.expected_result.QueueOrReady.expected_rate_frames",
            )
            require_value(
                expected_result["type_id"],
                type_id,
                f"{command_field}.expected_result.QueueOrReady.type_id",
            )
            resolved_result = _enum_variant(
                command["resolved_result"],
                "QueueObserved",
                ("resolved_rate_frames", "type_id"),
                f"{command_field}.resolved_result",
            )
            require_value(
                resolved_result["resolved_rate_frames"],
                0,  # Raw enqueue receipt; the first factory sweep resolves cadence.
                f"{command_field}.resolved_result.QueueObserved.resolved_rate_frames",
            )
            require_value(
                resolved_result["type_id"],
                type_id,
                f"{command_field}.resolved_result.QueueObserved.type_id",
            )
        else:
            require_exact_keys(payload, ("PlaceExactType",), f"{command_field}.payload")
            place = _exact_object(
                payload["PlaceExactType"],
                ("choice", "role"),
                f"{command_field}.payload.PlaceExactType",
            )
            require_value(place["role"], role, f"{command_field}.payload.PlaceExactType.role")
            require_value(
                place["choice"],
                validated_placements[role_index],
                f"{command_field}.payload.PlaceExactType.choice",
            )
            placement = validated_placements[role_index]
            expected_result = _enum_variant(
                command["expected_result"],
                "BuildingPlacedReadyConsumed",
                ("cell", "type_id"),
                f"{command_field}.expected_result",
            )
            require_value(
                expected_result["cell"],
                placement["cell"],
                f"{command_field}.expected_result.BuildingPlacedReadyConsumed.cell",
            )
            require_value(
                expected_result["type_id"],
                type_id,
                f"{command_field}.expected_result.BuildingPlacedReadyConsumed.type_id",
            )
            resolved_result = _enum_variant(
                command["resolved_result"],
                "BuildingObserved",
                ("cell", "stable_id", "type_id"),
                f"{command_field}.resolved_result",
            )
            require_value(
                resolved_result["cell"],
                role_binding["cell"],
                f"{command_field}.resolved_result.BuildingObserved.cell",
            )
            require_value(
                resolved_result["stable_id"],
                role_binding["stable_id"],
                f"{command_field}.resolved_result.BuildingObserved.stable_id",
            )
            require_value(
                resolved_result["type_id"],
                type_id,
                f"{command_field}.resolved_result.BuildingObserved.type_id",
            )

    if mcv_id in stable_ids:
        raise ValidationError(
            f"{field} reused the deployed MCV stable ID for a later entity"
        )
    _require_observed_ledger(
        production["observed_ledger"],
        f"{field}.observed_ledger",
        profile,
        completed=True,
    )
    return {
        **production,
        "_last_binary_frame": last["binary_frame_after"],
    }


def _require_rect(value: Any, field: str) -> Mapping[str, Any]:
    rect = _exact_object(value, ("x", "y", "width", "height"), field)
    require_number(rect["x"], f"{field}.x")
    require_number(rect["y"], f"{field}.y")
    _positive_number(rect["width"], f"{field}.width")
    _positive_number(rect["height"], f"{field}.height")
    return rect


def _require_asset(
    value: Any,
    field: str,
    *,
    logical_name: str,
    source_archive: str,
) -> None:
    asset = _exact_object(value, ("logical_name", "source_archive"), field)
    require_value(asset["logical_name"], logical_name, f"{field}.logical_name")
    require_value(asset["source_archive"], source_archive, f"{field}.source_archive")


def _require_render(
    stable: Mapping[str, Any],
    profile: ValidatedProfile,
) -> Mapping[str, Any]:
    field = "evidence.stable.render"
    render = _exact_object(
        stable.get("render"),
        (
            "app_has_radar",
            "radar_authority_active",
            "radar_phase",
            "expected_theme",
            "theme",
            "power_ready",
            "bound_structures_ready",
            "sidebar_values_ready",
            "bitmap_font_ready",
            "panel_contains_aperture",
            "no_modal_or_debug",
            "cursor_id",
            "cursor",
            "production_render",
            "radar_animation_source",
            "sidebar",
            "ready",
        ),
        field,
    )
    for key in (
        "app_has_radar",
        "radar_authority_active",
        "power_ready",
        "bound_structures_ready",
        "sidebar_values_ready",
        "bitmap_font_ready",
        "panel_contains_aperture",
        "no_modal_or_debug",
        "ready",
    ):
        require_value(render[key], True, f"{field}.{key}")
    require_value(render["radar_phase"], "Online", f"{field}.radar_phase")
    expected_theme = "Soviet" if profile.profile_id.startswith("soviet-") else "Yuri"
    require_value(render["expected_theme"], expected_theme, f"{field}.expected_theme")
    require_value(render["theme"], expected_theme, f"{field}.theme")
    capture = profile.capture
    require_value(render["cursor_id"], capture["cursor_id"], f"{field}.cursor_id")
    cursor = _exact_object(render["cursor"], ("x", "y"), f"{field}.cursor")
    cursor_profile = require_object(capture["post_load_cursor"], "capture.post_load_cursor")
    _numeric_value(cursor["x"], cursor_profile["x"], f"{field}.cursor.x")
    _numeric_value(cursor["y"], cursor_profile["y"], f"{field}.cursor.y")

    production_render = _exact_object(
        render["production_render"],
        (
            "sidebar_view_present",
            "minimap_aperture",
            "sidebar_panel",
            "radar_content_insets",
            "instance_counts",
            "sidebar_text_instances",
        ),
        f"{field}.production_render",
    )
    _positive_int(
        production_render["sidebar_text_instances"],
        f"{field}.production_render.sidebar_text_instances",
    )
    require_value(
        production_render["sidebar_view_present"],
        True,
        f"{field}.production_render.sidebar_view_present",
    )
    aperture = _require_rect(
        production_render["minimap_aperture"],
        f"{field}.production_render.minimap_aperture",
    )
    panel = _require_rect(
        production_render["sidebar_panel"],
        f"{field}.production_render.sidebar_panel",
    )
    if not (
        aperture["x"] >= panel["x"]
        and aperture["y"] >= panel["y"]
        and aperture["x"] + aperture["width"] <= panel["x"] + panel["width"]
        and aperture["y"] + aperture["height"] <= panel["y"] + panel["height"]
    ):
        raise ValidationError(f"{field}.production_render aperture is outside sidebar panel")
    insets = require_array(
        production_render["radar_content_insets"],
        f"{field}.production_render.radar_content_insets",
    )
    if len(insets) != 4:
        raise ValidationError(
            f"{field}.production_render.radar_content_insets must contain four integers"
        )
    for index, value in enumerate(insets):
        _nonnegative_int(value, f"{field}.production_render.radar_content_insets[{index}]")
    counts = _exact_object(
        production_render["instance_counts"],
        ("minimap", "radar_animation", "viewport_rect"),
        f"{field}.production_render.instance_counts",
    )
    require_value(aperture, {"x": 648.0, "y": 49.0, "width": 140.0, "height": 108.0},
                  f"{field}.production_render.minimap_aperture")
    require_value(panel, {"x": 632.0, "y": 0.0, "width": 168.0, "height": 600.0},
                  f"{field}.production_render.sidebar_panel")
    require_value(insets, [16, 1, 12, 1], f"{field}.production_render.radar_content_insets")
    for key in ("minimap", "radar_animation"):
        require_value(counts[key], 1, f"{field}.production_render.instance_counts.{key}")
    # Up to four native rectangle edges survive actual camera/source clipping.
    # This observation alone cannot certify their pixel positions or coverage.
    edges = require_int(counts["viewport_rect"], f"{field}.production_render.instance_counts.viewport_rect")
    if not 1 <= edges <= 4:
        raise ValidationError(f"{field}.production_render.instance_counts.viewport_rect must be between 1 and 4")

    source_field = f"{field}.radar_animation_source"
    source = _exact_object(
        render["radar_animation_source"],
        (
            "actual_theme",
            "requested_theme",
            "atlas_theme",
            "parent_archive",
            "radar",
            "backgrounds",
            "generic_palette",
            "theme_palette",
        ),
        source_field,
    )
    for key in ("actual_theme", "requested_theme", "atlas_theme"):
        require_value(source[key], expected_theme, f"{source_field}.{key}")
    parent_archive = "sidec02.mix" if expected_theme == "Soviet" else "sidec02md.mix"
    require_value(source["parent_archive"], parent_archive, f"{source_field}.parent_archive")
    _require_asset(
        source["radar"],
        f"{source_field}.radar",
        logical_name="radar.shp" if expected_theme == "Soviet" else "radary.shp",
        source_archive=parent_archive,
    )
    backgrounds = require_array(source["backgrounds"], f"{source_field}.backgrounds")
    expected_backgrounds = (("bkgdlg.shp", "bkgdmd.shp", "bkgdsm.shp") if expected_theme == "Soviet"
                            else ("bkgdlgy.shp", "bkgdmdy.shp", "bkgdsmy.shp"))
    if len(backgrounds) != len(expected_backgrounds):
        raise ValidationError(f"{source_field}.backgrounds has the wrong length")
    for index, logical_name in enumerate(expected_backgrounds):
        _require_asset(
            backgrounds[index],
            f"{source_field}.backgrounds[{index}]",
            logical_name=logical_name,
            source_archive=parent_archive,
        )
    _require_asset(
        source["generic_palette"],
        f"{source_field}.generic_palette",
        logical_name="SIDEBAR.PAL",
        source_archive="sidec02.mix",
    )
    _require_asset(
        source["theme_palette"],
        f"{source_field}.theme_palette",
        logical_name="sidebar.pal" if expected_theme == "Soviet" else "radaryuri.pal",
        source_archive=parent_archive,
    )

    sidebar = _exact_object(
        render["sidebar"],
        ("credits", "low_power", "power_drained", "power_produced", "layout"),
        f"{field}.sidebar",
    )
    _nonnegative_int(sidebar["credits"], f"{field}.sidebar.credits")
    require_value(sidebar["low_power"], False, f"{field}.sidebar.low_power")
    drained = _nonnegative_int(sidebar["power_drained"], f"{field}.sidebar.power_drained")
    produced = _nonnegative_int(sidebar["power_produced"], f"{field}.sidebar.power_produced")
    if produced <= drained:
        raise ValidationError(f"{field}.sidebar must have positive spare power")
    layout = _exact_object(
        sidebar["layout"],
        (
            "sidebar_x",
            "radar_y",
            "side1_y",
            "tabs_y",
            "cameo_grid_top",
            "cameo_grid_bottom",
            "side3_y",
            "side2_tile_count",
        ),
        f"{field}.sidebar.layout",
    )
    for key in (
        "sidebar_x",
        "radar_y",
        "side1_y",
        "tabs_y",
        "cameo_grid_top",
        "cameo_grid_bottom",
        "side3_y",
    ):
        require_number(layout[key], f"{field}.sidebar.layout.{key}")
    require_value(layout, {
        "sidebar_x": 632.0, "radar_y": 48.0, "side1_y": 158.0, "tabs_y": 197.0,
        "cameo_grid_top": 227.0, "cameo_grid_bottom": 527.0, "side3_y": 527.0,
        "side2_tile_count": 6,
    }, f"{field}.sidebar.layout")
    return render


def _require_final_fingerprint(
    stable: Mapping[str, Any],
    profile: ValidatedProfile,
    production: Mapping[str, Any],
    render: Mapping[str, Any],
) -> None:
    field = "evidence.stable.final_fingerprint"
    fingerprint = _exact_object(
        stable.get("final_fingerprint"),
        ("core", "cursor", "power", "radar", "render", "script", "wallet"),
        field,
    )
    capture_tick = _observed_capture_tick(stable, profile)
    require_int(profile.capture["sim_tick_ms"], "capture.sim_tick_ms")
    core = _exact_object(
        fingerprint["core"],
        ("simulation_tick", "binary_frame", "total_simulation_ms", "deterministic_state_hash"),
        f"{field}.core",
    )
    require_value(core["simulation_tick"], capture_tick, f"{field}.core.simulation_tick")
    require_value(
        core["total_simulation_ms"],
        capture_tick * 1000 // NATIVE_LOGIC_FRAMES_PER_SECOND,
        f"{field}.core.total_simulation_ms",
    )
    require_value(
        core["binary_frame"],
        production["_last_binary_frame"],
        f"{field}.core.binary_frame",
    )
    require_int(core["deterministic_state_hash"], f"{field}.core.deterministic_state_hash")

    cursor = _exact_object(fingerprint["cursor"], ("x", "y", "id"), f"{field}.cursor")
    require_value(cursor["id"], render["cursor_id"], f"{field}.cursor.id")
    _numeric_value(cursor["x"], render["cursor"]["x"], f"{field}.cursor.x")
    _numeric_value(cursor["y"], render["cursor"]["y"], f"{field}.cursor.y")

    power = _exact_object(
        fingerprint["power"],
        ("output", "drain", "is_low_power", "blackout_remaining"),
        f"{field}.power",
    )
    require_value(power["output"], render["sidebar"]["power_produced"], f"{field}.power.output")
    require_value(power["drain"], render["sidebar"]["power_drained"], f"{field}.power.drain")
    require_value(power["is_low_power"], False, f"{field}.power.is_low_power")
    require_value(power["blackout_remaining"], 0, f"{field}.power.blackout_remaining")

    radar = _exact_object(
        fingerprint["radar"],
        ("app_has_radar", "authority_active", "phase"),
        f"{field}.radar",
    )
    require_value(radar["app_has_radar"], True, f"{field}.radar.app_has_radar")
    require_value(radar["authority_active"], True, f"{field}.radar.authority_active")
    require_value(radar["phase"], "Online", f"{field}.radar.phase")
    require_value(fingerprint["render"], render, f"{field}.render")

    script = _exact_object(
        fingerprint["script"],
        ("stage", "commands", "placements", "bindings", "harvester", "observed_ledger"),
        f"{field}.script",
    )
    require_value(script["stage"], "CaptureRequested", f"{field}.script.stage")
    require_value(script["commands"], production["command_ledger"], f"{field}.script.commands")
    require_value(
        script["placements"],
        production["placement_ledger"],
        f"{field}.script.placements",
    )
    require_value(
        script["bindings"],
        production["structure_bindings"],
        f"{field}.script.bindings",
    )
    require_value(script["harvester"], production["harvester"], f"{field}.script.harvester")
    _require_observed_ledger(
        script["observed_ledger"],
        f"{field}.script.observed_ledger",
        profile,
        completed=False,
    )
    # The fingerprint is frozen before readback completion, but it must describe
    # the same observed transition/capture as the completed production ledger.
    require_value(script["observed_ledger"],
                  {**production["observed_ledger"], "capture_complete_tick": None},
                  f"{field}.script.observed_ledger")

    wallet = _exact_object(
        fingerprint["wallet"],
        ("credits", "score", "spent_credits"),
        f"{field}.wallet",
    )
    credits = _nonnegative_int(wallet["credits"], f"{field}.wallet.credits")
    _nonnegative_int(wallet["score"], f"{field}.wallet.score")
    spent = _nonnegative_int(wallet["spent_credits"], f"{field}.wallet.spent_credits")
    require_value(credits, render["sidebar"]["credits"], f"{field}.wallet.credits")
    launch = require_object(profile.document["launch"], "launch")
    options = require_object(launch["options"], "launch.options")
    # The profile deposits no ore, so cash and spending account for the start.
    require_value(
        credits + spent,
        options["starting_credits"],
        f"{field}.wallet.balance",
    )


def require_stable_evidence(
    stable: Mapping[str, Any],
    profile: ValidatedProfile,
    contract: ValidatedContract,
    environment: EnvironmentEvidence,
) -> None:
    """Require the full child-emitted stable v2 evidence contract."""

    require_exact_keys(stable, STABLE_KEYS, "evidence.stable")
    _require_inputs(stable, environment)
    _require_map_source(stable, profile)
    _require_lifecycle(stable)
    _require_graphics(stable, profile, environment)
    _require_contract(stable, contract)
    _require_profile(stable, profile)
    _require_startup(stable, profile)
    production = _require_production(stable, profile)
    render = _require_render(stable, profile)
    _require_final_fingerprint(stable, profile, production, render)
    require_value(stable["known_residuals"], KNOWN_RESIDUALS, "evidence.stable.known_residuals")


def require_run_evidence(value: Any, profile: ValidatedProfile, *, stable: Mapping[str, Any]) -> None:
    """Require typed completion counters while keeping run values non-repeatable."""

    field = "evidence.run"
    run = _exact_object(value, RUN_KEYS, field)
    _positive_int(run["process_id"], f"{field}.process_id")
    _nonnegative_int(run["elapsed_ms"], f"{field}.elapsed_ms")
    capture_tick = _observed_capture_tick(stable, profile)
    require_value(run["exact_steps"], capture_tick, f"{field}.exact_steps")
    require_value(run["render_frames"], capture_tick + 1, f"{field}.render_frames")


def require_nonuniform_bgra(raw: bytes) -> None:
    """Reject a solid-color frame that cannot be the scoped tactical render."""

    if len(raw) < 8 or len(raw) % 4 != 0:
        raise ValidationError("tactical frame is not complete BGRA pixels")
    first = raw[:4]
    if all(raw[offset : offset + 4] == first for offset in range(4, len(raw), 4)):
        raise ValidationError("tactical frame is uniform and cannot evidence the checkpoint")
