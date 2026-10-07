"""Run and check a chosen-map production observation; never certify native parity.

The Rust map_observation profile and capture manifest own the runtime schema.
This wrapper binds their receipts to immutable inputs and actual frame bytes.
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass
import math
from pathlib import Path
import sys
from typing import Any, Mapping

from tools.cargo_run import resolve_binary, resolve_labeled_binary
from tools.child_process import run_child
from tools.tactical_certification.core import (
    FileSnapshot, ValidationError, assert_snapshot_unchanged,
    create_directory_exclusive, load_json_file, require_array, require_directory, require_exact_keys, require_int,
    require_object, require_regular_file, require_sha256, require_string,
    require_value, sha256_bytes, utc_now, write_bytes_exclusive, write_json_exclusive,
)
from tools.tactical_certification.profile import load_contract, reject_denied_environment

ROOT = Path(__file__).resolve().parents[1]
RUN_SCHEMA = 'vera20k.map-observation-run.v7'
DOCKING_RUN_SCHEMA = 'vera20k.map-observation-run.v6'
BUILDING_RUN_SCHEMA = 'vera20k.map-observation-run.v5'
TRAJECTORY_RUN_SCHEMA = 'vera20k.map-observation-run.v4'
PRIOR_RUN_SCHEMA = 'vera20k.map-observation-run.v3'
LEGACY_CLOCK_RUN_SCHEMA = 'vera20k.map-observation-run.v2'
LEGACY_RUN_SCHEMA = 'vera20k.map-observation-run.v1'
CHILD_SCHEMA = 'vera20k.map-observation.v7'
DOCKING_CHILD_SCHEMA = 'vera20k.map-observation.v6'
BUILDING_CHILD_SCHEMA = 'vera20k.map-observation.v5'
TRAJECTORY_CHILD_SCHEMA = 'vera20k.map-observation.v4'
PRIOR_CHILD_SCHEMA = 'vera20k.map-observation.v3'
LEGACY_CHILD_SCHEMA = 'vera20k.map-observation.v2'
CLOCK_POLICY = 'map-exact-step-presentation-v1'
OBSERVATION_POLICY = 'map-ordinary-command-observation-v4'
GESTURE_POLICY = 'map-tactical-left-gesture-v1'
DOCKING_OBSERVATION_POLICY = 'map-ordinary-command-observation-v3'
BUILDING_OBSERVATION_POLICY = 'map-ordinary-command-observation-v2'
TRAJECTORY_OBSERVATION_POLICY = 'map-ordinary-command-observation-v1'
PROFILE_V1 = 'vera20k.map-observation-profile.v1'
PROFILE_V2 = 'vera20k.map-observation-profile.v2'
MAX_OBSERVATION_SAMPLES = 100_000
MAX_RECEIPT_BYTES = 128 * 1024 * 1024
ORDER_VARIANTS = frozenset(('Select', 'Move', 'Stop', 'Attack', 'ForceAttack', 'Guard',
                            'DeployMcv', 'ForceAttackCell', 'CaptureBuilding', 'ToggleRepair',
                            'EnterTransport', 'UnloadPassengers', 'RepairAtDepot', 'SellBuilding', 'SetRally',
                            'LaunchSuperWeapon'))
PRODUCTION_VARIANTS = frozenset(('QueueProduction', 'PlaceReadyBuilding'))
EXTENSION_FIELDS = frozenset(('commands', 'gestures', 'observe_owners', 'observe_types', 'observe_action_line_inputs', 'camera_cell',
                              'cursor_position', 'terrain_cells', 'observe_super_weapons'))
COPIES = {'profile': 'profile.json', 'config': 'config.toml', 'contract': 'contract.json'}


@dataclass(frozen=True)
class _Capture:
    evidence: dict[str, Any]
    manifest: FileSnapshot
    frame: FileSnapshot
    frame_format: str
    clock: dict[str, Any]


@dataclass(frozen=True)
class _CheckedRun:
    report: dict[str, Any]
    capture: _Capture
    inputs: Mapping[str, FileSnapshot]
    snapshots: tuple[FileSnapshot, ...]

    def check_unchanged(self) -> None:
        for snapshot in self.snapshots:
            assert_snapshot_unchanged(snapshot, str(snapshot.path))


def _integer(document: Mapping[str, Any], key: str, minimum: int = 0) -> int:
    value = require_int(document.get(key), key)
    if value < minimum:
        raise ValidationError(f'{key} must be at least {minimum}')
    return value


def _identity(value: Any, expected_identity: Mapping[str, Any], label: str) -> None:
    document = require_object(value, label)
    for key, expected in expected_identity.items():
        require_value(document.get(key), expected, f'{label}.{key}')


def _differences(before: Any, after: Any, field: str) -> list[dict[str, Any]]:
    """Exact typed JSON comparison, including nested bool-versus-int differences."""
    if type(before) is type(after):
        if isinstance(before, dict):
            differences = []
            for key in sorted(before.keys() | after.keys()):
                path = f'{field}.{key}' if field else key
                if key not in before or key not in after:
                    differences.append({'field': path, 'before_present': key in before,
                                        'after_present': key in after,
                                        'before': before.get(key), 'after': after.get(key)})
                else:
                    differences.extend(_differences(before[key], after[key], path))
            return differences
        if isinstance(before, list) and len(before) == len(after):
            return [difference for index, (left, right) in enumerate(zip(before, after))
                    for difference in _differences(left, right, f'{field}[{index}]')]
        if before == after:
            return []
    return [{'field': field, 'before': before, 'after': after}]


def _require_equal(value: Any, expected: Any, label: str) -> None:
    differences = _differences(expected, value, label)
    if differences:
        difference = differences[0]
        raise ValidationError(f"{difference['field']} differs: {difference}")


def _recorded_identity(value: Any, label: str) -> Mapping[str, Any]:
    identity = require_object(value, label)
    if set(identity) != {'path', 'byte_length', 'sha256'}:
        raise ValidationError(f'{label} must contain path, byte_length and sha256')
    path = require_string(identity.get('path'), f'{label}.path')
    if not Path(path).is_absolute():
        raise ValidationError(f'{label}.path must be absolute')
    _integer(identity, 'byte_length')
    require_sha256(identity.get('sha256'), f'{label}.sha256')
    return identity


def _content_identity(identity: Mapping[str, Any], snapshot: FileSnapshot, label: str) -> None:
    # Paths intentionally differ for sealed copies. Never replace a snapshot's
    # path with the original path to manufacture a supposedly live identity.
    for key in ('byte_length', 'sha256'):
        require_value(identity.get(key), getattr(snapshot, key), f'{label}.{key}')


def _unit_atlas(value: Any) -> dict[str, Any]:
    """Check the renderer's allocation receipt without reconstructing packing."""
    atlas = require_object(value, 'render.unit_atlas')
    for key in ('resident_sprite_count', 'last_build_rasterized_sprite_count',
                'total_texel_payload_bytes'):
        _integer(atlas, key)
    pages = require_array(atlas.get('pages'), 'render.unit_atlas.pages')
    total = 0
    for index, value in enumerate(pages):
        label = f'render.unit_atlas.pages[{index}]'
        page = require_object(value, label)
        extent = require_array(page.get('extent'), f'{label}.extent')
        if len(extent) != 3:
            raise ValidationError(f'{label}.extent must have width, height and layers')
        width, height = (require_int(extent[axis], f'{label}.extent[{axis}]')
                         for axis in (0, 1))
        if width <= 0 or height <= 0:
            raise ValidationError(f'{label}.extent width and height must be positive')
        require_value(extent[2], 1, f'{label}.extent[2]')
        for key, expected in (('format', 'R8Uint'), ('dimension', 'D2'),
                              ('mip_level_count', 1), ('sample_count', 1),
                              ('texel_payload_bytes', width * height)):
            require_value(page.get(key), expected, f'{label}.{key}')
        total += width * height
    require_value(atlas.get('total_texel_payload_bytes'), total,
                  'render.unit_atlas.total_texel_payload_bytes')
    return dict(atlas)


def _presentation_clock(value: Any, ticks: int) -> dict[str, Any]:
    """Validate the versioned diagnostic transcript, not native wall cadence."""
    label = 'render.presentation_clock'
    clock = require_object(value, label)
    require_exact_keys(clock, ('policy', 'origin_ms', 'interval_ms', 'draws'), label)
    for key, expected in (('policy', CLOCK_POLICY), ('origin_ms', 0), ('interval_ms', 22)):
        require_value(clock.get(key), expected, f'{label}.{key}')
    if not 0 <= ticks <= 100_000:
        raise ValidationError('presentation clock tick budget must be in 0..100000')
    draws = require_array(clock.get('draws'), f'{label}.draws')
    if len(draws) != max(ticks, 1):
        raise ValidationError(f'{label}.draws must contain exactly {max(ticks, 1)} rows')
    for index, value in enumerate(draws):
        row_label = f'{label}.draws[{index}]'
        row = require_object(value, row_label)
        require_exact_keys(row, ('completed_steps', 'radar_ms', 'tooltip_ms', 'message_ms'),
                           row_label)
        step = index + 1 if ticks else 0
        require_value(row.get('completed_steps'), step, f'{row_label}.completed_steps')
        for key in ('radar_ms', 'tooltip_ms', 'message_ms'):
            require_value(row.get(key), step * 22, f'{row_label}.{key}')
    return dict(clock)


def _bounded_int(value: Any, label: str, minimum: int, maximum: int) -> int:
    number = require_int(value, label)
    if not minimum <= number <= maximum:
        raise ValidationError(f'{label} must be in {minimum}..{maximum}')
    return number


def _coordinate(value: Any, label: str, *, leptons: bool = False,
                packed_cell: bool = False) -> list[int]:
    coordinate = require_array(value, label)
    size = 3 if leptons else 2
    if len(coordinate) != size:
        raise ValidationError(f'{label} must contain exactly {size} coordinates')
    limits = ((-(1 << 31), (1 << 31) - 1) if leptons else
              (-(1 << 15), (1 << 15) - 1) if packed_cell else (0, (1 << 16) - 1))
    return [_bounded_int(item, f'{label}[{index}]', *limits)
            for index, item in enumerate(coordinate)]


def _screen_point(value: Any, label: str, extent: tuple[int, int]) -> list[int]:
    point = require_array(value, label)
    if len(point) != 2:
        raise ValidationError(f'{label} must contain exactly two coordinates')
    for axis, dimension in enumerate(extent):
        _bounded_int(point[axis], f'{label}[{axis}]', 1, dimension - 2)
    return point


def _gesture(value: Any, label: str, extent: tuple[int, int]) -> None:
    gesture = require_object(value, label)
    if gesture.get('kind') == 'click':
        require_exact_keys(gesture, ('kind', 'position'), label)
        _screen_point(gesture['position'], f'{label}.position', extent)
    elif gesture.get('kind') == 'drag':
        require_exact_keys(gesture, ('kind', 'from', 'to'), label)
        start = _screen_point(gesture['from'], f'{label}.from', extent)
        end = _screen_point(gesture['to'], f'{label}.to', extent)
        if start == end:
            raise ValidationError(f'{label} drag endpoints must differ')
    else:
        raise ValidationError(f'{label}.kind must be click or drag')


def _profile_extensions(profile: Mapping[str, Any], *, production_commands: bool = True) -> None:
    """Check diagnostic syntax/budgets; Rust still owns Command/launch admission."""
    schema = profile.get('schema_version')
    if schema not in (PROFILE_V1, PROFILE_V2):
        raise ValidationError(f'unsupported profile.schema_version: {schema!r}')
    required = {'schema_version', 'launch', 'seed', 'input_delay_ticks', 'ticks',
                'width', 'height', 'timeout_seconds'}
    allowed = required | (EXTENSION_FIELDS if schema == PROFILE_V2 else frozenset())
    if not required <= profile.keys() or profile.keys() - allowed:
        raise ValidationError('profile fields differ from its declared schema_version')
    ticks = _bounded_int(profile.get('ticks'), 'profile.ticks', 0, 100_000)
    commands = require_array(profile.get('commands', []), 'profile.commands')
    if len(commands) > 1024:
        raise ValidationError('profile.commands exceeds 1024 rows')
    previous = 0
    for index, value in enumerate(commands):
        label = f'profile.commands[{index}]'
        command = require_object(value, label)
        require_exact_keys(command, ('issue_after_step', 'owner', 'payload'), label)
        step = _bounded_int(command.get('issue_after_step'), f'{label}.issue_after_step', 0, 100_000)
        if step < previous or step >= ticks:
            raise ValidationError(f'{label}.issue_after_step must be ordered before the final step')
        previous = step
        if not require_string(command.get('owner'), f'{label}.owner'):
            raise ValidationError(f'{label}.owner is empty')
        # Preserve the existing serde Command payload opaquely. Rust validates
        # exact argument fields/types; this wrapper does not translate orders.
        payload = require_object(command.get('payload'), f'{label}.payload')
        variants = ORDER_VARIANTS | (PRODUCTION_VARIANTS if production_commands else frozenset())
        if len(payload) != 1 or next(iter(payload)) not in variants:
            raise ValidationError(f'{label}.payload is outside ordinary order coverage')
    if 'gestures' in profile:
        gestures = require_array(profile['gestures'], 'profile.gestures')
        if len(gestures) > 1024:
            raise ValidationError('profile.gestures exceeds 1024 rows')
        if 'cursor_position' not in profile:
            raise ValidationError('profile.gestures requires a sealed cursor_position')
        # Rust's shared tactical viewport owner applies the stricter sidebar /
        # bottom-strip bounds. The wrapper checks render coordinates here and
        # the actual tactical extent in the child receipt, without copying that
        # presentation geometry into a second owner.
        extent = tuple(_bounded_int(profile.get(key), f'profile.{key}', 1, (1 << 32) - 1)
                       for key in ('width', 'height'))
        previous = 0
        for index, value in enumerate(gestures):
            label = f'profile.gestures[{index}]'
            row = require_object(value, label)
            require_exact_keys(row, ('issue_after_step', 'gesture'), label)
            step = _bounded_int(row['issue_after_step'], f'{label}.issue_after_step', 0, 100_000)
            if step < previous or step >= ticks:
                raise ValidationError(f'{label}.issue_after_step must be ordered before the final step')
            previous = step
            _gesture(row['gesture'], f'{label}.gesture', extent)
    owners = require_array(profile.get('observe_owners', []), 'profile.observe_owners')
    if len(owners) > 30:
        raise ValidationError('profile.observe_owners exceeds 30 Houses')
    selected = set()
    for index, value in enumerate(owners):
        owner = require_string(value, f'profile.observe_owners[{index}]')
        if not owner or owner in selected:
            raise ValidationError('profile.observe_owners has an empty or duplicate House')
        selected.add(owner)
    if 'observe_types' in profile:
        types = require_array(profile['observe_types'], 'profile.observe_types')
        if not 1 <= len(types) <= 256:
            raise ValidationError('profile.observe_types must contain 1..256 types')
        selected_types = set()
        for index, value in enumerate(types):
            name = require_string(value, f'profile.observe_types[{index}]')
            if not name or name in selected_types:
                raise ValidationError('profile.observe_types has an empty or duplicate type')
            selected_types.add(name)
    if 'camera_cell' in profile:
        _coordinate(profile['camera_cell'], 'profile.camera_cell')
    if 'observe_action_line_inputs' in profile and type(profile['observe_action_line_inputs']) is not bool:
        raise ValidationError('profile.observe_action_line_inputs must be a boolean')
    if 'observe_super_weapons' in profile and type(profile['observe_super_weapons']) is not bool:
        raise ValidationError('profile.observe_super_weapons must be a boolean')
    if 'cursor_position' in profile:
        position = require_array(profile['cursor_position'], 'profile.cursor_position')
        if len(position) != 2:
            raise ValidationError('profile.cursor_position must contain exactly two coordinates')
        for axis, extent in enumerate(('width', 'height')):
            dimension = _bounded_int(profile.get(extent), f'profile.{extent}', 1, (1 << 32) - 1)
            _bounded_int(position[axis], f'profile.cursor_position[{axis}]', 1, dimension - 2)
    cells = require_array(profile.get('terrain_cells', []), 'profile.terrain_cells')
    if len(cells) > 256:
        raise ValidationError('profile.terrain_cells exceeds 256 cells')
    seen = set()
    for index, value in enumerate(cells):
        cell = tuple(_coordinate(value, f'profile.terrain_cells[{index}]'))
        if cell in seen:
            raise ValidationError('profile.terrain_cells has duplicate coordinates')
        seen.add(cell)


def _target_reference(value: Any, label: str, *, navigation: bool = False) -> None:
    if value is None:
        return
    target = require_object(value, label)
    if len(target) != 1:
        raise ValidationError(f'{label} must be one tagged target reference')
    tag, payload = next(iter(target.items()))
    if navigation:
        payload = require_object(payload, label)
        if tag == 'Cell':
            require_exact_keys(payload, ('rx', 'ry'), label)
            _coordinate([payload['rx'], payload['ry']], label)
        elif tag in ('Entity', 'Object', 'Building'):
            require_exact_keys(payload, ('id',), label)
            _bounded_int(payload['id'], label, 1, (1 << 64) - 1)
        else:
            raise ValidationError(f'{label} has an unknown navigation reference tag')
    elif tag == 'Cell':
        _coordinate(payload, label)
    elif tag == 'Entity':
        _bounded_int(payload, label, 1, (1 << 64) - 1)
    else:
        raise ValidationError(f'{label} has an unknown target reference tag')


def _timer(value: Any, label: str) -> None:
    timer = require_object(value, label)
    require_exact_keys(timer, ('start_frame', 'duration'), label)
    for key in timer:
        _bounded_int(timer[key], f'{label}.{key}', -(1 << 31), (1 << 31) - 1)


def _building(value: Any, label: str) -> int:
    building = require_object(value, label)
    keys = ('body_state', 'queued_body_state', 'construction_control',
            'stage', 'ready_latch', 'actually_placed', 'last_operational', 'animation_slots')
    # Optional complete extension preserves previously sealed building captures.
    if 'voxel_gun' in building:
        keys += ('voxel_gun',)
        gun = require_object(building['voxel_gun'], f'{label}.voxel_gun')
        require_exact_keys(gun, ('facing', 'elevation', 'hva_counter', 'recoil', 'recoil_active'), f'{label}.voxel_gun')
        for key in ('facing', 'elevation'):
            _bounded_int(gun[key], f'{label}.voxel_gun.{key}', 0, 65535)
        _bounded_int(gun['hva_counter'], f'{label}.voxel_gun.hva_counter', -(1 << 31), (1 << 31) - 1)
        travel = require_array(gun['recoil'], f'{label}.voxel_gun.recoil')
        if len(travel) != 2 or any(type(v) not in (int, float) or not math.isfinite(v) for v in travel):
            raise ValidationError(f'{label}.voxel_gun.recoil must contain two finite numbers')
        if type(gun['recoil_active']) is not bool:
            raise ValidationError(f'{label}.voxel_gun.recoil_active must be boolean')
    require_exact_keys(building, keys, label)
    for key in ('body_state', 'queued_body_state'):
        if building[key] is not None:
            _bounded_int(building[key], f'{label}.{key}', -(1 << 31), (1 << 31) - 1)
    control = require_array(building['construction_control'], f'{label}.construction_control')
    if len(control) != 3:
        raise ValidationError(f'{label}.construction_control must have exactly three entries')
    for index, number in enumerate(control):
        _bounded_int(number, f'{label}.construction_control[{index}]', -(1 << 31), (1 << 31) - 1)
    stage = require_object(building['stage'], f'{label}.stage')
    require_exact_keys(stage, ('value', 'changed', 'timer', 'rate', 'increment'), f'{label}.stage')
    for key in ('value', 'rate', 'increment'):
        _bounded_int(stage[key], f'{label}.stage.{key}', -(1 << 31), (1 << 31) - 1)
    _bounded_int(stage['changed'], f'{label}.stage.changed', 0, 255)
    _timer(stage['timer'], f'{label}.stage.timer')
    _bounded_int(building['ready_latch'], f'{label}.ready_latch', 0, 255)
    for key in ('actually_placed', 'last_operational'):
        if type(building[key]) is not bool:
            raise ValidationError(f'{label}.{key} must be a boolean')
    slots = require_array(building['animation_slots'], f'{label}.animation_slots')
    previous = -1
    for index, value in enumerate(slots):
        slot_label = f'{label}.animation_slots[{index}]'
        slot = require_object(value, slot_label)
        require_exact_keys(slot, ('slot', 'anim_id', 'animation'), slot_label)
        number = _bounded_int(slot['slot'], f'{slot_label}.slot', 0, 20)
        if number <= previous:
            raise ValidationError(f'{slot_label}.slot is repeated or out of order')
        previous = number
        identity = _bounded_int(slot['anim_id'], f'{slot_label}.anim_id', 1, (1 << 64) - 1)
        if slot['animation'] is None:
            continue  # A retained slot with no live Anim is explicit evidence.
        anim_label = f'{slot_label}.animation'
        anim = require_object(slot['animation'], anim_label)
        require_exact_keys(anim, ('stable_id', 'native_id', 'type_id', 'interned_type_id',
                                 'physical_leptons', 'in_logic_vector', 'owner_entity',
                                 'building_slot', 'runtime'), anim_label)
        require_value(anim['stable_id'], identity, f'{anim_label}.stable_id')
        _bounded_int(anim['native_id'], f'{anim_label}.native_id', -(1 << 31), (1 << 31) - 1)
        if not require_string(anim['type_id'], f'{anim_label}.type_id'):
            raise ValidationError(f'{anim_label}.type_id is empty')
        _bounded_int(anim['interned_type_id'], f'{anim_label}.interned_type_id', 0, (1 << 32) - 1)
        _coordinate(anim['physical_leptons'], f'{anim_label}.physical_leptons', leptons=True)
        if type(anim['in_logic_vector']) is not bool:
            raise ValidationError(f'{anim_label}.in_logic_vector must be a boolean')
        if anim['owner_entity'] is not None:
            _bounded_int(anim['owner_entity'], f'{anim_label}.owner_entity', 1, (1 << 64) - 1)
        if anim['building_slot'] is not None:
            owner_slot = require_array(anim['building_slot'], f'{anim_label}.building_slot')
            if len(owner_slot) != 2:
                raise ValidationError(f'{anim_label}.building_slot must contain owner and slot')
            _bounded_int(owner_slot[0], f'{anim_label}.building_slot[0]', 1, (1 << 64) - 1)
            _bounded_int(owner_slot[1], f'{anim_label}.building_slot[1]', 0, 20)
        runtime = require_object(anim['runtime'], f'{anim_label}.runtime')
        require_exact_keys(runtime, ('current_frame', 'frame_step', 'delay_remaining', 'rate_reload',
                                     'frame_timer', 'loop_remaining', 'first_ai_guard',
                                     'constructor_reverse', 'inactive', 'paused'), f'{anim_label}.runtime')
        for key in ('current_frame', 'frame_step'):
            _bounded_int(runtime[key], f'{anim_label}.runtime.{key}', -(1 << 31), (1 << 31) - 1)
        for key in ('delay_remaining', 'rate_reload'):
            _bounded_int(runtime[key], f'{anim_label}.runtime.{key}', 0, (1 << 16) - 1)
        _bounded_int(runtime['loop_remaining'], f'{anim_label}.runtime.loop_remaining', 0, 255)
        _timer(runtime['frame_timer'], f'{anim_label}.runtime.frame_timer')
        for key in ('first_ai_guard', 'constructor_reverse', 'inactive', 'paused'):
            if type(runtime[key]) is not bool:
                raise ValidationError(f'{anim_label}.runtime.{key} must be a boolean')
    return len(slots)


def _rule_types(value: Any) -> None:
    for index, value in enumerate(require_array(value, 'observations.rule_types')):
        label = f'observations.rule_types[{index}]'
        row = require_object(value, label)
        require_exact_keys(row, ('type_id', 'interned_id', 'category'), label)
        if not require_string(row['type_id'], f'{label}.type_id'):
            raise ValidationError(f'{label}.type_id is empty')
        _bounded_int(row['interned_id'], f'{label}.interned_id', 0, (1 << 32) - 1)
        if row['category'] not in ('Infantry', 'Unit', 'Aircraft', 'Structure'):
            raise ValidationError(f'{label}.category is unknown')


def _unit(value: Any, label: str) -> None:
    unit = require_object(value, label)
    keys = ('deployed_6e0', 'deploying_6e1', 'undeploying_6e2',
            'landing_for_deploy_134', 'deploy_anim_130', 'stage_f8', 'body_counter_538')
    # The paired selection extension preserves older sealed unit receipts.
    selection = ('current_weapon_138', 'current_turret_124')
    if any(key in unit for key in selection):
        keys += selection
        for key in selection:
            _bounded_int(unit.get(key), f'{label}.{key}', -(1 << 31), (1 << 31) - 1)
    require_exact_keys(unit, keys, label)
    for key in ('deployed_6e0', 'deploying_6e1', 'undeploying_6e2'):
        _bounded_int(unit[key], f'{label}.{key}', 0, 255)
    if type(unit['landing_for_deploy_134']) is not bool:
        raise ValidationError(f'{label}.landing_for_deploy_134 must be a boolean')
    _bounded_int(unit['stage_f8'], f'{label}.stage_f8', -(1 << 31), (1 << 31) - 1)
    _bounded_int(unit['body_counter_538'], f'{label}.body_counter_538', 0, (1 << 32) - 1)
    if unit['deploy_anim_130'] is None:
        return
    anim_label = f'{label}.deploy_anim_130'
    anim = require_object(unit['deploy_anim_130'], anim_label)
    require_exact_keys(anim, ('stable_id', 'live'), anim_label)
    _bounded_int(anim['stable_id'], f'{anim_label}.stable_id', 1, (1 << 64) - 1)
    if anim['live'] is None:
        return  # A retained identity with no live Anim remains an observation.
    live_label = f'{anim_label}.live'
    live = require_object(anim['live'], live_label)
    require_exact_keys(live, ('type_id', 'frame', 'owner_entity'), live_label)
    if not require_string(live['type_id'], f'{live_label}.type_id'):
        raise ValidationError(f'{live_label}.type_id is empty')
    _bounded_int(live['frame'], f'{live_label}.frame', -(1 << 31), (1 << 31) - 1)
    if live['owner_entity'] is not None:
        _bounded_int(live['owner_entity'], f'{live_label}.owner_entity', 1, (1 << 64) - 1)


def _docking_state(actor: Mapping[str, Any], label: str) -> None:
    radio = require_object(actor['radio'], f'{label}.radio')
    require_exact_keys(radio, ('contacts', 'dock_entered_with'), f'{label}.radio')
    contacts = require_array(radio['contacts'], f'{label}.radio.contacts')
    if not contacts:
        raise ValidationError(f'{label}.radio.contacts must retain at least one slot')
    for index, identity in enumerate(contacts):
        if identity is not None:
            _bounded_int(identity, f'{label}.radio.contacts[{index}]', 1, (1 << 64) - 1)
    if radio['dock_entered_with'] is not None:
        _bounded_int(radio['dock_entered_with'], f'{label}.radio.dock_entered_with', 1, (1 << 64) - 1)
    miner = actor['miner']
    if miner is None:
        return
    miner = require_object(miner, f'{label}.miner')
    require_exact_keys(miner, ('cargo_bales', 'capacity_bales', 'unload_active', 'harvesting'),
                       f'{label}.miner')
    _bounded_int(miner['cargo_bales'], f'{label}.miner.cargo_bales', 0, (1 << 64) - 1)
    _bounded_int(miner['capacity_bales'], f'{label}.miner.capacity_bales', 0, (1 << 16) - 1)
    for key in ('unload_active', 'harvesting'):
        if type(miner[key]) is not bool:
            raise ValidationError(f'{label}.miner.{key} must be a boolean')


def _jumpjet(value: Any, label: str) -> None:
    if value is None:
        return
    runtime = require_object(value, label)
    require_exact_keys(runtime, ('destination_leptons', 'moving', 'phase', 'landing_latched',
                                 'params', 'flight'), label)
    _coordinate(runtime['destination_leptons'], f'{label}.destination_leptons', leptons=True)
    _bounded_int(runtime['phase'], f'{label}.phase', -(1 << 31), (1 << 31) - 1)
    for key in ('moving', 'landing_latched'):
        if type(runtime[key]) is not bool:
            raise ValidationError(f'{label}.{key} must be a boolean')
    params = require_object(runtime['params'], f'{label}.params')
    require_exact_keys(params, ('turn_rate', 'speed', 'climb_bits', 'crash_bits', 'height',
                                'accel_bits', 'wobbles_bits', 'deviation', 'no_wobbles'),
                       f'{label}.params')
    for key in ('turn_rate', 'speed', 'height', 'deviation'):
        _bounded_int(params[key], f'{label}.params.{key}', -(1 << 31), (1 << 31) - 1)
    for key in ('climb_bits', 'crash_bits', 'accel_bits', 'wobbles_bits'):
        _bounded_int(params[key], f'{label}.params.{key}', 0, (1 << 32) - 1)
    if type(params['no_wobbles']) is not bool:
        raise ValidationError(f'{label}.params.no_wobbles must be a boolean')
    flight = require_object(runtime['flight'], f'{label}.flight')
    require_exact_keys(flight, ('facing', 'current_speed_bits', 'target_speed_bits',
                                'target_height', 'bob_phase_bits'), f'{label}.flight')
    for key in ('current_speed_bits', 'target_speed_bits', 'bob_phase_bits'):
        _bounded_int(flight[key], f'{label}.flight.{key}', 0, (1 << 64) - 1)
    _bounded_int(flight['target_height'], f'{label}.flight.target_height',
                 -(1 << 31), (1 << 31) - 1)
    facing = require_object(flight['facing'], f'{label}.flight.facing')
    require_exact_keys(facing, ('current', 'prev', 'start_frame', 'duration_frames',
                                'rot_per_frame'), f'{label}.flight.facing')
    for key in ('current', 'prev', 'duration_frames', 'rot_per_frame'):
        _bounded_int(facing[key], f'{label}.flight.facing.{key}', 0, (1 << 16) - 1)
    if facing['start_frame'] is not None:
        _bounded_int(facing['start_frame'], f'{label}.flight.facing.start_frame',
                     0, (1 << 32) - 1)


def _foot_air(value: Any, label: str) -> None:
    air = require_object(value, label)
    holders = ('current_cell_slot_holder', 'tracker_cell_slot_holder', 'slot_cell_slot_holder')
    require_exact_keys(air, ('tracker_cell_560', 'slot_cell_564', 'spatial_bucket',
                             'spatial_enter_order', *holders), label)
    for key in ('tracker_cell_560', 'slot_cell_564'):
        _coordinate(air[key], f'{label}.{key}', packed_cell=True)
    if air['spatial_bucket'] is not None:
        _bounded_int(air['spatial_bucket'], f'{label}.spatial_bucket', 0, (1 << 16) - 1)
    _bounded_int(air['spatial_enter_order'], f'{label}.spatial_enter_order', 0, (1 << 64) - 1)
    for key in holders:
        if air[key] is not None:
            _bounded_int(air[key], f'{label}.{key}', 1, (1 << 64) - 1)


def _houses(value: Any, owners: list[str], label: str, super_weapons: bool = False) -> int:
    houses = require_array(value, label)
    if len(houses) != len(owners):
        raise ValidationError(f'{label} must contain one row per requested House')
    count = len(houses)
    for index, (value, owner) in enumerate(zip(houses, owners)):
        row_label = f'{label}[{index}]'
        house = require_object(value, row_label)
        require_exact_keys(house, ('owner', 'economy', *(('super_weapons',) if super_weapons else ())),
                           row_label)
        require_value(house['owner'], owner, f'{row_label}.owner')
        if house['economy'] is not None:
            economy = require_object(house['economy'], f'{row_label}.economy')
            require_exact_keys(economy, ('credits', 'spent_credits', 'harvested_credits'),
                               f'{row_label}.economy')
            for key in economy:
                _bounded_int(economy[key], f'{row_label}.economy.{key}', -(1 << 31), (1 << 31) - 1)
        if super_weapons:
            count += _super_weapons(house['super_weapons'], f'{row_label}.super_weapons')
    return count


def _super_weapons(value: Any, label: str) -> int:
    rows = require_array(value, label)
    previous = -1
    for index, value in enumerate(rows):
        row_label = f'{label}[{index}]'
        row = require_object(value, row_label)
        require_exact_keys(row, ('type', 'interned_id', 'granted', 'ready', 'on_hold', 'charge_start',
                                 'charge_duration', 'remaining', 'fade_countdown', 'fade_coords'),
                           row_label)
        if not require_string(row['type'], f'{row_label}.type'):
            raise ValidationError(f'{row_label}.type is empty')
        identity = _bounded_int(row['interned_id'], f'{row_label}.interned_id', 0, (1 << 32) - 1)
        if identity <= previous:
            raise ValidationError(f'{row_label}.interned_id is repeated or out of order')
        previous = identity
        for key in ('granted', 'ready', 'on_hold'):
            if type(row[key]) is not bool:
                raise ValidationError(f'{row_label}.{key} must be a boolean')
        for key in ('charge_start', 'charge_duration', 'remaining', 'fade_countdown'):
            _bounded_int(row[key], f'{row_label}.{key}', -(1 << 31), (1 << 31) - 1)
        coords = require_array(row['fade_coords'], f'{row_label}.fade_coords')
        if len(coords) != 3:
            raise ValidationError(f'{row_label}.fade_coords must hold three coordinates')
        for axis, coord in enumerate(coords):
            _bounded_int(coord, f'{row_label}.fade_coords[{axis}]', -(1 << 31), (1 << 31) - 1)
    return len(rows)


def _action_line_inputs(value: Any, category: str, label: str) -> None:
    if category == 'Structure':
        require_value(value, None, label)
        return
    inputs = require_object(value, label)
    require_exact_keys(inputs, ('body_facing', 'turret_facing', 'locomotor', 'is_moving',
                               'applied_speed_fraction_fixed_bits', 'crate_speed_multiplier_f64_bits',
                               'house_speed_bonus_f32_bits', 'current_speed', 'veterancy',
                               'current_weapon', 'turret_offset', 'rocking_angles_fixed_bits'), label)
    _bounded_int(inputs['body_facing'], f'{label}.body_facing', 0, (1 << 16) - 1)
    if inputs['turret_facing'] is not None:
        _bounded_int(inputs['turret_facing'], f'{label}.turret_facing', 0, (1 << 16) - 1)
    if inputs['locomotor'] not in (None, 'Drive', 'Hover', 'Walk', 'Fly', 'Teleport', 'Ship', 'Jumpjet', 'Rocket'):
        raise ValidationError(f'{label}.locomotor is unknown')
    if inputs['is_moving'] is not None and type(inputs['is_moving']) is not bool:
        raise ValidationError(f'{label}.is_moving must be boolean or null')
    _bounded_int(inputs['applied_speed_fraction_fixed_bits'],
                 f'{label}.applied_speed_fraction_fixed_bits', 0, 1 << 16)
    _bounded_int(inputs['crate_speed_multiplier_f64_bits'],
                 f'{label}.crate_speed_multiplier_f64_bits', 0, (1 << 64) - 1)
    _bounded_int(inputs['house_speed_bonus_f32_bits'],
                 f'{label}.house_speed_bonus_f32_bits', 0, (1 << 32) - 1)
    _bounded_int(inputs['veterancy'], f'{label}.veterancy', 0, (1 << 16) - 1)
    for key in ('current_speed', 'turret_offset'):
        _bounded_int(inputs[key], f'{label}.{key}', -(1 << 31), (1 << 31) - 1)
    if inputs['current_weapon'] is not None and not require_string(inputs['current_weapon'], f'{label}.current_weapon'):
        raise ValidationError(f'{label}.current_weapon is empty')
    if inputs['rocking_angles_fixed_bits'] is not None:
        angles = require_array(inputs['rocking_angles_fixed_bits'], f'{label}.rocking_angles_fixed_bits')
        if len(angles) != 2:
            raise ValidationError(f'{label}.rocking_angles_fixed_bits must contain two angles')
        for index, bits in enumerate(angles):
            _bounded_int(bits, f'{label}.rocking_angles_fixed_bits[{index}]', -(1 << 31), (1 << 31) - 1)


def _cloak(value: Any, label: str) -> None:
    if value is None:
        return
    cloak = require_object(value, label)
    require_exact_keys(cloak, ('state_i32', 'progress_i32', 'cloaking_stages_i32',
                         'voxel', 'no_shadow'), label)
    for key in ('state_i32', 'progress_i32', 'cloaking_stages_i32'):
        _bounded_int(cloak[key], f'{label}.{key}', -(1 << 31), (1 << 31) - 1)
    for key in ('voxel', 'no_shadow'):
        if type(cloak[key]) is not bool:
            raise ValidationError(f'{label}.{key} must be a boolean')


def _actor(value: Any, label: str, *, building_state: bool = True,
           docking_state: bool = True, walk_state: bool = True,
           action_line_inputs: bool = False) -> tuple[int, str]:
    actor = require_object(value, label)
    require_exact_keys(actor, ('stable_id', 'owner', 'type_id', 'category', 'cell',
                              'physical_leptons', 'on_bridge', 'health', 'active',
                              'in_limbo', 'dying', 'mission', 'target', 'archive', 'nav', 'foot',
                              *(('building',) if building_state else ()),
                              *(('unit',) if 'unit' in actor else ()),
                              *(('jumpjet',) if 'jumpjet' in actor else ()),
                              *(('cloak',) if 'cloak' in actor else ()),
                              *(('action_line_inputs',) if action_line_inputs else ()),
                              *(('miner', 'radio') if docking_state else ())), label)
    identity = _bounded_int(actor['stable_id'], f'{label}.stable_id', 1, (1 << 64) - 1)
    owner = require_string(actor['owner'], f'{label}.owner')
    if not owner or not require_string(actor['type_id'], f'{label}.type_id'):
        raise ValidationError(f'{label} has an empty owner/type identity')
    category = actor['category']
    if category not in ('Unit', 'Infantry', 'Aircraft', 'Structure'):
        raise ValidationError(f'{label}.category is unknown')
    if action_line_inputs:
        _action_line_inputs(actor['action_line_inputs'], category, f'{label}.action_line_inputs')
    if docking_state:
        _docking_state(actor, label)
    if building_state:
        if category == 'Structure':
            _building(actor['building'], f'{label}.building')
        else:
            require_value(actor['building'], None, f'{label}.building')
    if 'unit' in actor:
        if category == 'Unit':
            _unit(actor['unit'], f'{label}.unit')
        else:
            require_value(actor['unit'], None, f'{label}.unit')
    # Additive instance projections preserve historical sealed v6 receipts.
    # The diagnostic never supplies an absent runtime or derives either cache.
    if 'jumpjet' in actor:
        _jumpjet(actor['jumpjet'], f'{label}.jumpjet')
    if 'cloak' in actor:
        _cloak(actor['cloak'], f'{label}.cloak')
    _coordinate(actor['cell'], f'{label}.cell')
    _coordinate(actor['physical_leptons'], f'{label}.physical_leptons', leptons=True)
    _bounded_int(actor['health'], f'{label}.health', -(1 << 31), (1 << 31) - 1)
    for key in ('on_bridge', 'active', 'in_limbo', 'dying'):
        if type(actor[key]) is not bool:
            raise ValidationError(f'{label}.{key} must be a boolean')
    mission = require_object(actor['mission'], f'{label}.mission')
    require_exact_keys(mission, ('current', 'queued', 'suspended', 'effective', 'handler_state',
                                'start_frame', 'ai_counter', 'dispatch_timer'), f'{label}.mission')
    for key in ('current', 'queued', 'suspended', 'effective'):
        _bounded_int(mission[key], f'{label}.mission.{key}', -(1 << 31), (1 << 31) - 1)
    for key in ('handler_state', 'start_frame', 'ai_counter'):
        _bounded_int(mission[key], f'{label}.mission.{key}', 0, (1 << 32) - 1)
    timer = require_object(mission['dispatch_timer'], f'{label}.mission.dispatch_timer')
    require_exact_keys(timer, ('start_frame', 'delay'), f'{label}.mission.dispatch_timer')
    for key in timer:
        _bounded_int(timer[key], f'{label}.mission.dispatch_timer.{key}', -(1 << 31), (1 << 31) - 1)
    for key in ('target', 'archive', 'nav'):
        _target_reference(actor[key], f'{label}.{key}', navigation=key == 'nav')
    foot = actor['foot']
    if category == 'Structure':
        require_value(foot, None, f'{label}.foot')
    else:
        foot = require_object(foot, f'{label}.foot')
        require_exact_keys(foot, ('retarget_after_stop_688', 'firing_sequence_latch_68d',
                                  'infantry_doing', 'navigation_leptons', 'navigation_unavailable',
                                  *(('air',) if 'air' in foot else ()),
                                  *(('walk_head_leptons', 'walk_destination_leptons',
                                     'walk_is_moving') if walk_state else ()),
                                  *(('track',) if 'track' in foot else ()),
                                  *(('pending_entry_500',) if 'pending_entry_500' in foot else ())),
                           f'{label}.foot')
        if 'air' in foot:
            _foot_air(foot['air'], f'{label}.foot.air')
        # Additive read-only projection: previously sealed v6 receipts omit it.
        # A missing field carries no information about the pending-entry owner.
        if 'pending_entry_500' in foot and foot['pending_entry_500'] is not None:
            _bounded_int(foot['pending_entry_500'], f'{label}.foot.pending_entry_500',
                         1, (1 << 64) - 1)
        if type(foot['retarget_after_stop_688']) is not bool:
            raise ValidationError(f'{label}.foot.retarget_after_stop_688 must be a boolean')
        _bounded_int(foot['firing_sequence_latch_68d'], f'{label}.foot.firing_sequence_latch_68d', 0, 255)
        if category == 'Infantry':
            _bounded_int(foot['infantry_doing'], f'{label}.foot.infantry_doing', -(1 << 31), (1 << 31) - 1)
        else:
            require_value(foot['infantry_doing'], None, f'{label}.foot.infantry_doing')
        if foot['navigation_leptons'] is None:
            if not require_string(foot['navigation_unavailable'], f'{label}.foot.navigation_unavailable'):
                raise ValidationError(f'{label}.foot.navigation_unavailable is empty')
        else:
            _coordinate(foot['navigation_leptons'], f'{label}.foot.navigation_leptons', leptons=True)
            require_value(foot['navigation_unavailable'], None, f'{label}.foot.navigation_unavailable')
        if walk_state:
            for key in ('walk_head_leptons', 'walk_destination_leptons'):
                if foot[key] is not None:
                    _coordinate(foot[key], f'{label}.foot.{key}', leptons=True)
            moving = foot['walk_is_moving']
            if moving is None:
                # No active Walk instance: its coordinates are unavailable too.
                for key in ('walk_head_leptons', 'walk_destination_leptons'):
                    require_value(foot[key], None, f'{label}.foot.{key}')
            elif type(moving) is not bool:
                raise ValidationError(f'{label}.foot.walk_is_moving must be boolean or null')
        # An additive immutable projection of the installed Drive/Ship owner.
        # Historical sealed receipts omit it; absence cannot prove no track.
        if 'track' in foot and foot['track'] is not None:
            track_label = f'{label}.foot.track'
            track = require_object(foot['track'], track_label)
            require_exact_keys(track, ('family', 'destination_leptons', 'head_leptons',
                                       'selector', 'cursor', 'valid'), track_label)
            if track['family'] not in ('Drive', 'Ship'):
                raise ValidationError(f'{track_label}.family must be Drive or Ship')
            for key in ('destination_leptons', 'head_leptons'):
                if track[key] is not None:
                    _coordinate(track[key], f'{track_label}.{key}', leptons=True)
            for key in ('selector', 'cursor'):
                _bounded_int(track[key], f'{track_label}.{key}', -(1 << 31), (1 << 31) - 1)
            if type(track['valid']) is not bool:
                raise ValidationError(f'{track_label}.valid must be a boolean')
    return identity, owner


def _terrain(value: Any, expected_cell: Any, label: str) -> None:
    cell = require_object(value, label)
    fields = ('final_tile_index', 'final_sub_tile', 'presentation_tile', 'level', 'slope',
              'raw_bridge_flags', 'bridge_state', 'has_deck', 'deck_level', 'walkable', 'transition')
    extension = ('overlay', 'terrain_object') if 'overlay' in cell or 'terrain_object' in cell else ()
    visibility_extension = ('local_visibility',) if 'local_visibility' in cell else ()
    require_exact_keys(cell, ('cell', 'allocated', *fields, *extension, *visibility_extension), label)
    if visibility_extension and cell['local_visibility'] is not None:
        visibility = require_object(cell['local_visibility'], f'{label}.local_visibility')
        require_exact_keys(visibility, ('owner', 'revealed', 'visible', 'gap_covered'),
                           f'{label}.local_visibility')
        require_string(visibility['owner'], f'{label}.local_visibility.owner')
        if not visibility['owner']:
            raise ValidationError(f'{label}.local_visibility.owner must be nonempty')
        for key in ('revealed', 'visible', 'gap_covered'):
            if type(visibility[key]) is not bool:
                raise ValidationError(f'{label}.local_visibility.{key} must be boolean')
    if extension:
        overlay = cell['overlay']
        if overlay is not None:
            require_exact_keys(require_object(overlay, f'{label}.overlay'), ('id', 'density'), f'{label}.overlay')
            if overlay['id'] is not None:
                _bounded_int(overlay['id'], f'{label}.overlay.id', 0, 255)
            _bounded_int(overlay['density'], f'{label}.overlay.density', 0, 255)
        obj = cell['terrain_object']
        if obj is not None:
            require_exact_keys(require_object(obj, f'{label}.terrain_object'), ('name', 'frame', 'active'), f'{label}.terrain_object')
            if not isinstance(obj['name'], str) or not obj['name']:
                raise ValidationError(f'{label}.terrain_object.name must be nonempty')
            if obj['frame'] is not None:
                _bounded_int(obj['frame'], f'{label}.terrain_object.frame', -(1 << 31), (1 << 31) - 1)
            if obj['active'] is not None and type(obj['active']) is not bool:
                raise ValidationError(f'{label}.terrain_object.active must be boolean or null')
    _require_equal(cell['cell'], expected_cell, f'{label}.cell')
    if type(cell['allocated']) is not bool:
        raise ValidationError(f'{label}.allocated must be a boolean')
    if not cell['allocated']:
        for key in fields:
            require_value(cell[key], None, f'{label}.{key}')
        return
    _bounded_int(cell['final_tile_index'], f'{label}.final_tile_index', -(1 << 31), (1 << 31) - 1)
    for key in ('final_sub_tile', 'level', 'slope', 'bridge_state', 'deck_level'):
        _bounded_int(cell[key], f'{label}.{key}', 0, 255)
    _bounded_int(cell['raw_bridge_flags'], f'{label}.raw_bridge_flags', 0, (1 << 32) - 1)
    tile = _coordinate(cell['presentation_tile'], f'{label}.presentation_tile')
    _bounded_int(tile[1], f'{label}.presentation_tile[1]', 0, 255)
    for key in ('has_deck', 'walkable', 'transition'):
        if type(cell[key]) is not bool:
            raise ValidationError(f'{label}.{key} must be a boolean')


def _input_observation(value: Any, label: str) -> int:
    row = require_object(value, label)
    require_exact_keys(row, ('selected_ids', 'selection_pending', 'target_line_remaining',
                             'target_line_active'), label)
    selected = require_array(row['selected_ids'], f'{label}.selected_ids')
    for index, identity in enumerate(selected):
        _bounded_int(identity, f'{label}.selected_ids[{index}]', 1, (1 << 64) - 1)
    if len(set(selected)) != len(selected):
        raise ValidationError(f'{label}.selected_ids contains duplicate identities')
    for key in ('selection_pending', 'target_line_active'):
        if type(row[key]) is not bool:
            raise ValidationError(f'{label}.{key} must be boolean')
    remaining = _bounded_int(row['target_line_remaining'], f'{label}.target_line_remaining',
                             -(1 << 31), (1 << 31) - 1)
    if row['target_line_active'] and remaining <= 0:
        raise ValidationError(f'{label}.target_line_active requires positive remaining frames')
    return len(selected)


def _gesture_observations(value: Any, profile: Mapping[str, Any]) -> int:
    label = 'observations.gesture_input'
    observation = require_object(value, label)
    require_exact_keys(observation, ('policy', 'equal_step_order', 'tactical_extent', 'receipts'), label)
    require_value(observation['policy'], GESTURE_POLICY, f'{label}.policy')
    require_value(observation['equal_step_order'], 'commands_then_gestures', f'{label}.equal_step_order')
    dimensions = require_array(observation['tactical_extent'], f'{label}.tactical_extent')
    if len(dimensions) != 2:
        raise ValidationError(f'{label}.tactical_extent must contain exactly two dimensions')
    extent = tuple(_bounded_int(value, f'{label}.tactical_extent[{axis}]', 1, profile[key])
                   for axis, (value, key) in enumerate(zip(dimensions, ('width', 'height'))))
    receipts = require_array(observation['receipts'], f'{label}.receipts')
    requested = profile['gestures']
    if len(receipts) != len(requested):
        raise ValidationError(f'{label}.receipts differs from requested gesture count')
    sample_count = 0
    for index, (value, request) in enumerate(zip(receipts, requested)):
        row_label = f'{label}.receipts[{index}]'
        row = require_object(value, row_label)
        require_exact_keys(row, ('ordinal', 'issue_after_step', 'issued_simulation_tick',
                                 'issued_binary_frame', 'gesture', 'before', 'after',
                                 'left_press_captured', 'band_box_before_release',
                                 'neutral_input_restored', 'queued_commands'), row_label)
        for key, expected in (('ordinal', index), ('issue_after_step', request['issue_after_step']),
                              ('issued_simulation_tick', request['issue_after_step']),
                              ('issued_binary_frame', request['issue_after_step'])):
            require_value(row[key], expected, f'{row_label}.{key}')
        _require_equal(row['gesture'], request['gesture'], f'{row_label}.gesture')
        _gesture(row['gesture'], f'{row_label}.gesture', extent)
        for key, expected in (('left_press_captured', True), ('neutral_input_restored', True),
                              ('band_box_before_release', request['gesture']['kind'] == 'drag')):
            require_value(row[key], expected, f'{row_label}.{key}')
        for key in ('before', 'after'):
            sample_count += _input_observation(row[key], f'{row_label}.{key}')
        commands = require_array(row['queued_commands'], f'{row_label}.queued_commands')
        sample_count += len(commands)
        for number, value in enumerate(commands):
            command_label = f'{row_label}.queued_commands[{number}]'
            command = require_object(value, command_label)
            require_exact_keys(command, ('owner', 'execute_tick', 'payload'), command_label)
            if not require_string(command['owner'], f'{command_label}.owner'):
                raise ValidationError(f'{command_label}.owner is empty')
            require_value(command['execute_tick'], request['issue_after_step'], f'{command_label}.execute_tick')
            # These are observations of actual Command values, not profile
            # orders. Preserve their serde payload without translating input
            # actions or maintaining a second gameplay-command whitelist.
            payload = require_object(command['payload'], f'{command_label}.payload')
            if len(payload) != 1 or not next(iter(payload)):
                raise ValidationError(f'{command_label}.payload must be one tagged Command')
            require_object(next(iter(payload.values())), f'{command_label}.payload arguments')
    if sample_count > MAX_OBSERVATION_SAMPLES:
        raise ValidationError(f'{label} exceeds its retained sample budget')
    return sample_count


def _observations(value: Any, profile: Mapping[str, Any], final: Mapping[str, Any], *,
                  building_state: bool = True, docking_state: bool = True,
                  walk_state: bool = True) -> dict[str, Any]:
    label = 'observations'
    observations = require_object(value, label)
    gesture_input = 'gestures' in profile
    if gesture_input and not walk_state:
        raise ValidationError('gesture observations require the current observation policy')
    require_exact_keys(observations, ('policy', 'owners', 'commands', 'frames',
                                     *(('gesture_input',) if gesture_input else ()),
                                     *(('type_filter',) if 'observe_types' in profile else ()),
                                     *(('rule_types',) if building_state else ())), label)
    policy = (OBSERVATION_POLICY if walk_state else DOCKING_OBSERVATION_POLICY if docking_state else
              BUILDING_OBSERVATION_POLICY if building_state else TRAJECTORY_OBSERVATION_POLICY)
    require_value(observations['policy'], policy, f'{label}.policy')
    if building_state:
        _rule_types(observations['rule_types'])
    type_filter = profile.get('observe_types')
    if type_filter is not None:
        if not (building_state and docking_state):
            raise ValidationError('type-filtered observations require the current observation policy')
        _require_equal(observations['type_filter'], type_filter, f'{label}.type_filter')
        known_types = {row['type_id'] for row in observations['rule_types']}
        if any(name not in known_types for name in type_filter):
            raise ValidationError('profile.observe_types contains a type absent from rule_types')
    owners = profile.get('observe_owners', [])
    _require_equal(observations['owners'], owners, f'{label}.owners')
    commands = require_array(observations['commands'], f'{label}.commands')
    requested = profile.get('commands', [])
    if len(commands) != len(requested):
        raise ValidationError('observations.commands differs from requested command count')
    for index, (value, request) in enumerate(zip(commands, requested)):
        row_label = f'{label}.commands[{index}]'
        row = require_object(value, row_label)
        require_exact_keys(row, ('ordinal', 'issue_after_step', 'issued_simulation_tick',
                                 'envelope_execute_tick', 'owner', 'payload'), row_label)
        for key, expected in (('ordinal', index), ('issue_after_step', request['issue_after_step']),
                              ('issued_simulation_tick', request['issue_after_step']),
                              ('envelope_execute_tick', request['issue_after_step']), ('owner', request['owner'])):
            require_value(row[key], expected, f'{row_label}.{key}')
        _require_equal(row['payload'], request['payload'], f'{row_label}.payload')
    frames = require_array(observations['frames'], f'{label}.frames')
    ticks = profile['ticks']
    if len(frames) != ticks + 1:
        raise ValidationError(f'observations.frames must contain exactly {ticks + 1} rows including L0')
    seen = set()
    sample_count = _gesture_observations(observations['gesture_input'], profile) if gesture_input else 0
    previous_ms = -1
    expected_cells = profile.get('terrain_cells', [])
    for step, value in enumerate(frames):
        row_label = f'{label}.frames[{step}]'
        row = require_object(value, row_label)
        require_exact_keys(row, ('completed_steps', 'simulation_tick', 'binary_frame',
                                 'total_simulation_ms', 'actors', 'missing_actor_ids', 'terrain',
                                 *(('input',) if gesture_input else ()),
                                 *(('houses',) if docking_state else ())), row_label)
        for key in ('completed_steps', 'simulation_tick', 'binary_frame'):
            require_value(row[key], step, f'{row_label}.{key}')
        milliseconds = _integer(row, 'total_simulation_ms')
        if milliseconds <= previous_ms:
            raise ValidationError(f'{row_label}.total_simulation_ms did not increase')
        if step == 0:
            require_value(milliseconds, 0, f'{row_label}.total_simulation_ms')
        previous_ms = milliseconds
        actors = require_array(row['actors'], f'{row_label}.actors')
        present = set()
        previous_id = 0
        for index, value in enumerate(actors):
            actor_label = f'{row_label}.actors[{index}]'
            identity, owner = _actor(value, actor_label, building_state=building_state,
                                     docking_state=docking_state, walk_state=walk_state,
                                     action_line_inputs=profile.get('observe_action_line_inputs', False))
            if identity <= previous_id:
                raise ValidationError(f'{actor_label}.stable_id is repeated or out of order')
            if identity not in seen and owner not in owners:
                raise ValidationError(f'{actor_label}.owner is outside the requested Houses')
            if identity not in seen and type_filter is not None and value['type_id'] not in type_filter:
                raise ValidationError(f'{actor_label}.type_id is outside the requested type filter')
            previous_id = identity
            present.add(identity)
        seen.update(present)
        missing = require_array(row['missing_actor_ids'], f'{row_label}.missing_actor_ids')
        for index, identity in enumerate(missing):
            _bounded_int(identity, f'{row_label}.missing_actor_ids[{index}]', 1, (1 << 64) - 1)
        if list(sorted(set(missing))) != list(missing) or present & set(missing):
            raise ValidationError(f'{row_label}.missing_actor_ids must be sorted, unique and absent')
        if present | set(missing) != seen:
            raise ValidationError(f'{row_label} omitted a previously observed stable actor')
        terrain = require_array(row['terrain'], f'{row_label}.terrain')
        if len(terrain) != len(expected_cells):
            raise ValidationError(f'{row_label}.terrain count differs from the requested cells')
        for index, (value, expected) in enumerate(zip(terrain, expected_cells)):
            _terrain(value, expected, f'{row_label}.terrain[{index}]')
        sample_count += len(actors) + len(missing) + len(terrain)
        if gesture_input:
            sample_count += _input_observation(row['input'], f'{row_label}.input')
        if docking_state:
            sample_count += _houses(row['houses'], owners, f'{row_label}.houses',
                                    profile.get('observe_super_weapons', False))
        if building_state:
            sample_count += sum(len(actor['building']['animation_slots']) for actor in actors
                                if actor['category'] == 'Structure')
        if sample_count > MAX_OBSERVATION_SAMPLES:
            raise ValidationError('observations exceeds its retained sample budget')
    require_value(previous_ms, final['total_simulation_ms'], 'observations final total_simulation_ms')
    return dict(observations)


def validate_capture(directory: Path, profile: Mapping[str, Any],
                     identities: Mapping[str, Mapping[str, Any]], *,
                     legacy_clock: bool = False, prior_observations: bool = False,
                     prior_trajectory: bool = False, building_only: bool = False,
                     docking_only: bool = False) -> _Capture:
    """Check child semantics/bytes against independently checked input identities.

    Identities describe original runtime paths. Retained copies have their own real
    snapshots; offline callers check their bytes before passing these identities.
    """
    _profile_extensions(profile, production_commands=not (legacy_clock or prior_observations or prior_trajectory))
    if (legacy_clock or prior_observations) and profile['schema_version'] != PROFILE_V1:
        raise ValidationError('historical child schemas require profile v1')
    require_directory(directory, 'child output')
    manifest_snapshot, manifest = load_json_file(directory / 'capture.json', 'capture manifest',
                                                maximum_length=MAX_RECEIPT_BYTES)
    expected_schema = (LEGACY_CHILD_SCHEMA if legacy_clock else
                       PRIOR_CHILD_SCHEMA if prior_observations else
                       TRAJECTORY_CHILD_SCHEMA if prior_trajectory else
                       BUILDING_CHILD_SCHEMA if building_only else
                       DOCKING_CHILD_SCHEMA if docking_only else CHILD_SCHEMA)
    require_value(manifest.get('schema_version'),
                  expected_schema, 'schema_version')
    if manifest.get('status') != 'COMPLETE':
        raise ValidationError(f'child did not complete: {manifest.get("failure", manifest.get("status"))}')
    if {path.name for path in directory.iterdir()} != {'capture.json', 'frame.bgra'}:
        raise ValidationError('child output must contain exactly capture.json and frame.bgra')
    for key in ('native_comparator', 'parity_certification'):
        require_value(manifest.get(key), 'NONE', key)
    child_profile = require_object(manifest.get('profile'), 'profile')
    require_value(child_profile.get('sha256'), identities['profile']['sha256'], 'profile.sha256')
    _require_equal(child_profile.get('request'), dict(profile), 'profile.request')
    inputs = require_object(manifest.get('inputs'), 'inputs')
    contract = require_object(manifest.get('contract'), 'contract')
    require_value(contract.get('sha256'), identities['contract']['sha256'], 'contract.sha256')
    for name in ('config', 'executable'):
        _identity(inputs.get(name), identities[name], f'inputs.{name}')

    ticks = _integer(profile, 'ticks')
    require_value(manifest.get('exact_step_count'), ticks, 'exact_step_count')
    initial = require_object(manifest.get('initial'), 'initial')
    final = require_object(manifest.get('final'), 'final')
    for name in ('simulation_tick', 'binary_frame', 'total_simulation_ms'):
        require_value(initial.get(name), 0, f'initial.{name}')
        _integer(final, name)
    for state in (initial, final):
        _integer(state, 'deterministic_state_hash')
    for name in ('simulation_tick', 'binary_frame'):
        require_value(final.get(name), ticks, f'final.{name}')
    if ticks == 0:
        _require_equal(dict(final), dict(initial), 'zero-step final state')
        for name in ('first_exact_step', 'last_exact_step'):
            require_value(manifest.get(name), None, name)
    else:
        if final['total_simulation_ms'] <= initial['total_simulation_ms']:
            raise ValidationError('simulation time did not advance')
        for name, before in (('first_exact_step', 0), ('last_exact_step', ticks - 1)):
            receipt = require_object(manifest.get(name), name)
            for key in ('tick', 'binary_frame'):
                require_value(receipt.get(f'{key}_before'), before, f'{name}.{key}_before')
                require_value(receipt.get(f'{key}_after'), before + 1, f'{name}.{key}_after')

    startup = require_object(manifest.get('startup'), 'startup')
    for key, expected in (('seed', profile.get('seed')), ('seed_source', 'Controlled'),
                          ('seed_authority_certifying', True),
                          ('classification', 'AcceptedExplicitFixedBattle')):
        require_value(startup.get(key), expected, f'startup.{key}')
    _integer(startup, 'correlation', 1)
    source = require_object(manifest.get('map_source'), 'map_source')
    require_sha256(source.get('source_sha256'), 'map_source.source_sha256')
    _integer(source, 'payload_len', 1)
    if source.get('kind') == 'loose':
        path = require_string(source.get('path'), 'map_source.path')
        if not path:
            raise ValidationError('empty map source path')
    elif source.get('kind') == 'mix':
        for name in ('logical_name', 'source_archive'):
            if not require_string(source.get(name), f'map_source.{name}'):
                raise ValidationError(f'empty map_source.{name}')
        require_int(source.get('entry_id'), 'map_source.entry_id')
    else:
        raise ValidationError('map source must be loaded loose or MIX bytes')
    lifecycle = require_object(manifest.get('lifecycle'), 'lifecycle')
    for key, expected in (('window_hidden', True), ('window_focused', False),
                          ('focus_violations', 0), ('input_violations', 0)):
        require_value(lifecycle.get(key), expected, f'lifecycle.{key}')
    render = require_object(manifest.get('render'), 'render')
    if 'cursor_position' in profile:
        _require_equal(render.get('cursor_position'), [float(value) for value in profile['cursor_position']],
                       'render.cursor_position')
    elif 'cursor_position' in render:
        raise ValidationError('render.cursor_position requires profile.cursor_position')
    if 'frame_wall_mean_ms' in render:
        mean = render['frame_wall_mean_ms']
        if type(mean) not in (int, float) or not 0 <= mean <= 3.4028234663852886e38 or not math.isfinite(mean):
            raise ValidationError('render.frame_wall_mean_ms must be finite and nonnegative')
    if legacy_clock or prior_observations:
        if 'observations' in manifest or 'camera' in render:
            raise ValidationError('historical child cannot declare v4 observations/camera')
        observations = None
    else:
        observations = _observations(manifest.get('observations'), profile, final,
                                     building_state=not prior_trajectory,
                                     docking_state=not (prior_trajectory or building_only),
                                     walk_state=not (prior_trajectory or building_only or docking_only))
        camera = require_object(render.get('camera'), 'render.camera')
        require_exact_keys(camera, ('requested_cell', 'top_left', 'zoom'), 'render.camera')
        _require_equal(camera['requested_cell'], profile.get('camera_cell'), 'render.camera.requested_cell')
        top_left = require_array(camera['top_left'], 'render.camera.top_left')
        if len(top_left) != 2:
            raise ValidationError('render.camera.top_left must contain exactly two coordinates')
        for key, value in (('top_left[0]', top_left[0]), ('top_left[1]', top_left[1]), ('zoom', camera['zoom'])):
            if (type(value) not in (int, float) or abs(value) > 3.4028234663852886e38
                    or not math.isfinite(value)):
                raise ValidationError(f'render.camera.{key} must be a finite number')
        if camera['zoom'] <= 0:
            raise ValidationError('render.camera.zoom must be positive')
    if legacy_clock:
        if 'presentation_clock' in render or 'neutral_input' in render:
            raise ValidationError('legacy child v2 cannot declare presentation_clock or neutral_input')
        clock = {'policy': 'legacy-wall-clock'}
    else:
        clock = _presentation_clock(render.get('presentation_clock'), ticks)
        neutral = require_object(render.get('neutral_input'), 'render.neutral_input')
        require_exact_keys(neutral, ('static_default_cursor', 'camera_input_idle'),
                           'render.neutral_input')
        for key in neutral:
            require_value(neutral[key], True, f'render.neutral_input.{key}')
    for key in ('ready', 'sidebar_view_present'):
        require_value(render.get(key), True, f'render.{key}')
    width, height = _integer(profile, 'width', 1), _integer(profile, 'height', 1)
    for key in ('internal_extent', 'surface_extent'):
        _require_equal(render.get(key), [width, height], f'render.{key}')
    unit_atlas = _unit_atlas(render.get('unit_atlas'))
    frame = require_object(manifest.get('frame'), 'frame')
    frame_snapshot = require_regular_file(directory / 'frame.bgra', 'frame',
                                          exact_length=width * height * 4)
    for key, expected in (('file_name', 'frame.bgra'), ('width', width), ('height', height),
                          ('row_stride', width * 4), ('byte_length', frame_snapshot.byte_length),
                          ('sha256', frame_snapshot.sha256), ('pixel_layout', 'BGRA8')):
        require_value(frame.get(key), expected, f'frame.{key}')
    if frame.get('surface_format') not in ('Bgra8Unorm', 'Bgra8UnormSrgb'):
        raise ValidationError('unsupported frame.surface_format')
    assert_snapshot_unchanged(manifest_snapshot, 'capture manifest')
    assert_snapshot_unchanged(frame_snapshot, 'frame')
    evidence = {'manifest': manifest_snapshot.public_identity(),
                'frame': frame_snapshot.public_identity(), 'map_source': dict(source),
                'initial': dict(initial), 'final': dict(final), 'exact_step_count': ticks,
                'unit_atlas': unit_atlas}
    for metadata in ('cursor_position', 'frame_wall_mean_ms'):
        if metadata in render:
            evidence[metadata] = render[metadata]
    if not legacy_clock:
        evidence['presentation_clock'] = clock
        evidence['neutral_input'] = dict(neutral)
    if observations is not None:
        evidence['observations'] = observations
        evidence['camera'] = dict(camera)
    return _Capture(evidence, manifest_snapshot, frame_snapshot, frame['surface_format'], clock)


def capture(*, profile_path: Path, contract_path: Path, output: Path,
            working_directory: Path, executable: Path | None = None,
            build_label: str | None = None) -> dict[str, Any]:
    profile_snapshot, profile = load_json_file(profile_path, 'map observation profile')
    _profile_extensions(profile)
    # Only wrapper resource budgets are interpreted here. Rust owns launch admission.
    timeout = _integer(profile, 'timeout_seconds', 1)
    contract = load_contract(contract_path)
    reject_denied_environment(contract)
    if timeout > contract.document['absolute_max_child_timeout_seconds']:
        raise ValidationError('profile timeout exceeds checked contract maximum')
    cwd = require_directory(working_directory, 'working directory')
    if executable is not None and build_label is not None:
        raise ValidationError('executable and build_label are mutually exclusive')
    if build_label is not None:
        executable, _ = resolve_labeled_binary(ROOT, build_label, 'vera20k', 'release')
        if executable is None:
            raise ValidationError(f'no verified release vera20k in build label {build_label!r}')
    if executable is None:
        executable, _ = resolve_binary(ROOT, 'vera20k', 'release')
        if executable is None:
            raise ValidationError('no verified release vera20k; build with tools.cargo_run first')
    snapshots = {'profile': profile_snapshot, 'contract': contract.snapshot,
                 'config': require_regular_file(cwd / 'config.toml', 'config'),
                 'executable': require_regular_file(executable, 'executable')}
    run = create_directory_exclusive(output, 'observation output')
    for name, filename in COPIES.items():
        write_bytes_exclusive(run / filename, snapshots[name].raw)
    child_output = run / 'child-output'
    command = [str(snapshots['executable'].path), '--tactical-capture', 'map-observe-v1',
               '--profile', str(profile_snapshot.path), '--contract', str(contract.path),
               '--output', str(child_output)]
    started = utc_now()
    child = run_child(command, cwd=cwd, temporary_directory=run, timeout_seconds=timeout)
    errors = list(child.errors)
    if child.timed_out:
        errors.append('observation child timed out')
    if child.exit_status != 0:
        errors.append(f'observation child exit status: {child.exit_status}')
    for name, snapshot in snapshots.items():
        try:
            assert_snapshot_unchanged(snapshot, name)
        except ValidationError as exc:
            errors.append(str(exc))
    capture_evidence = None
    try:
        capture_evidence = validate_capture(
            child_output, profile, {name: snapshot.public_identity()
                                    for name, snapshot in snapshots.items()}).evidence
    except (ValidationError, OSError) as exc:
        errors.append(str(exc))
    unexpected = {path.name for path in run.iterdir()} - {*COPIES.values(), 'child-output'}
    if unexpected:
        errors.append(f'unexpected wrapper output: {sorted(unexpected)}')
    # Runtime still consumes the originals; retained copies seal later validation.
    for name, filename in COPIES.items():
        try:
            copy = require_regular_file(run / filename, f'{name} copy')
            _content_identity(snapshots[name].public_identity(), copy, f'{name} copy')
        except ValidationError as exc:
            errors.append(str(exc))
    artifacts = {}
    for name, raw in (('stdout.log', child.stdout), ('stderr.log', child.stderr)):
        write_bytes_exclusive(run / name, raw)
        artifacts[name] = {'byte_length': len(raw), 'sha256': sha256_bytes(raw)}
    report = {'schema_version': RUN_SCHEMA,
              'status': 'VALID' if not errors else 'INVALID', 'errors': errors,
              'started_at_utc': started, 'finished_at_utc': utc_now(),
              'command': command, 'working_directory': str(cwd),
              'inputs': {name: value.public_identity() for name, value in snapshots.items()},
              'child': {'pid': child.pid, 'exit_status': child.exit_status,
                        'timed_out': child.timed_out, 'timeout_seconds': timeout,
                        'cleanup_scope': 'exact-child-pid-only'},
              'logs': artifacts, 'capture': capture_evidence,
              'native_comparator': 'NONE', 'parity_certification': 'NONE'}
    write_json_exclusive(run / 'run.json', report, maximum_length=MAX_RECEIPT_BYTES,
                         compact=True)
    return report


def _load_run(directory: Path, allow_legacy_inputs: bool,
              allow_legacy_clock: bool = False) -> _CheckedRun:
    directory = require_directory(directory, 'observation run')
    run_snapshot, report = load_json_file(directory / 'run.json', 'observation run receipt',
                                         maximum_length=MAX_RECEIPT_BYTES)
    schema = report.get('schema_version')
    legacy = schema == LEGACY_RUN_SCHEMA
    if schema not in (RUN_SCHEMA, DOCKING_RUN_SCHEMA, BUILDING_RUN_SCHEMA, TRAJECTORY_RUN_SCHEMA, PRIOR_RUN_SCHEMA,
                      LEGACY_CLOCK_RUN_SCHEMA, LEGACY_RUN_SCHEMA):
        raise ValidationError(f'unsupported observation wrapper schema: {schema!r}')
    if legacy and not allow_legacy_inputs:
        raise ValidationError('legacy run v1 has no sealed config/contract copies; '
                              'use --allow-legacy-inputs to revalidate the original files')
    legacy_clock = schema in (LEGACY_CLOCK_RUN_SCHEMA, LEGACY_RUN_SCHEMA)
    prior_observations = schema == PRIOR_RUN_SCHEMA
    prior_trajectory = schema == TRAJECTORY_RUN_SCHEMA
    building_only = schema == BUILDING_RUN_SCHEMA
    docking_only = schema == DOCKING_RUN_SCHEMA
    if legacy_clock and not allow_legacy_clock:
        raise ValidationError('legacy wall-clock evidence requires --allow-legacy-clock; '
                              'it has no deterministic presentation schedule')
    copies = {'profile': 'profile.json'} if legacy else COPIES
    expected_files = {*copies.values(), 'run.json', 'stdout.log', 'stderr.log', 'child-output'}
    if {path.name for path in directory.iterdir()} != expected_files:
        raise ValidationError(f'observation run must contain exactly {sorted(expected_files)}')
    require_value(report.get('status'), 'VALID', 'run.status')
    require_value(report.get('errors'), [], 'run.errors')
    for key in ('native_comparator', 'parity_certification'):
        require_value(report.get(key), 'NONE', f'run.{key}')
    recorded = require_object(report.get('inputs'), 'run.inputs')
    identities = {name: _recorded_identity(recorded.get(name), f'run.inputs.{name}')
                  for name in (*COPIES, 'executable')}
    inputs = {}
    provenance = {}
    for name, identity in identities.items():
        if name in copies:
            path = directory / copies[name]
            provenance[name] = 'SEALED_COPY'
        else:
            path = Path(identity['path'])
            provenance[name] = ('EXTERNALLY_REVALIDATED_UNSEALED' if name != 'executable'
                                else 'EXTERNALLY_REVALIDATED')
        inputs[name] = require_regular_file(path, f'{name} evidence')
        _content_identity(identity, inputs[name], f'run.inputs.{name}')
    # The current environment is irrelevant to an offline check. The recorded
    # contract is checked structurally, without requiring today's checkout bytes.
    contract = load_contract(inputs['contract'].path, require_repository_bytes=False)
    profile_snapshot, profile = load_json_file(inputs['profile'].path, 'retained profile')
    _profile_extensions(profile)
    timeout = _integer(profile, 'timeout_seconds', 1)
    if timeout > contract.document['absolute_max_child_timeout_seconds']:
        raise ValidationError('profile timeout exceeds checked contract maximum')
    child = require_object(report.get('child'), 'run.child')
    for key, expected in (('exit_status', 0), ('timed_out', False),
                          ('timeout_seconds', timeout), ('cleanup_scope', 'exact-child-pid-only')):
        require_value(child.get(key), expected, f'run.child.{key}')
    _integer(child, 'pid', 1)
    cwd = require_string(report.get('working_directory'), 'run.working_directory')
    if not Path(cwd).is_absolute():
        raise ValidationError('run.working_directory must be absolute')
    require_value(identities['config']['path'], str(Path(cwd) / 'config.toml'),
                  'run.inputs.config.path')
    expected_command = [identities['executable']['path'], '--tactical-capture', 'map-observe-v1',
                        '--profile', identities['profile']['path'],
                        '--contract', identities['contract']['path'],
                        '--output', str(directory / 'child-output')]
    _require_equal(report.get('command'), expected_command, 'run.command')
    checked = validate_capture(directory / 'child-output', profile, identities,
                               legacy_clock=legacy_clock, prior_observations=prior_observations,
                               prior_trajectory=prior_trajectory, building_only=building_only,
                               docking_only=docking_only)
    _require_equal(report.get('capture'), checked.evidence, 'run.capture')
    logs = require_object(report.get('logs'), 'run.logs')
    snapshots = [run_snapshot, *inputs.values(), contract.snapshot, profile_snapshot,
                 checked.manifest, checked.frame]
    for name in ('stdout.log', 'stderr.log'):
        snapshot = require_regular_file(directory / name, name)
        expected = {'byte_length': snapshot.byte_length, 'sha256': snapshot.sha256}
        _require_equal(logs.get(name), expected, f'run.logs.{name}')
        snapshots.append(snapshot)
    validation = _report('validation', 'VALID')
    validation.update(run=run_snapshot.public_identity(), inputs=dict(identities),
                      input_provenance=provenance, capture=checked.evidence,
                      presentation_clock=checked.clock)
    result = _CheckedRun(validation, checked, inputs, tuple(snapshots))
    result.check_unchanged()
    return result


def _report(kind: str, status: str) -> dict[str, Any]:
    return {'schema_version': f'vera20k.map-observation-{kind}.v1', 'status': status,
            'checked_at_utc': utc_now(), 'errors': [],
            'native_comparator': 'NONE', 'parity_certification': 'NONE'}


def validate_run(directory: Path, *, allow_legacy_inputs: bool = False,
                 allow_legacy_clock: bool = False) -> dict[str, Any]:
    """Recheck retained bytes and the original executable, never trust a VALID label."""
    try:
        return _load_run(directory, allow_legacy_inputs, allow_legacy_clock).report
    except (OSError, ValueError) as exc:
        report = _report('validation', 'INVALID')
        report.update(run_path=str(directory), errors=[str(exc)])
        return report


def compare_runs(before: Path, after: Path, *,
                 allow_legacy_inputs: bool = False,
                 allow_legacy_clock: bool = False) -> dict[str, Any]:
    """Compare two checked production observations; MATCH is not native parity."""
    report = _report('comparison', 'INVALID')
    report.update(before_path=str(before), after_path=str(after), differences=[])
    try:
        left_dir = require_directory(before, 'before run')
        right_dir = require_directory(after, 'after run')
        if left_dir.samefile(right_dir):
            raise ValidationError('before and after must be distinct observation directories')
        left = _load_run(left_dir, allow_legacy_inputs, allow_legacy_clock)
        right = _load_run(right_dir, allow_legacy_inputs, allow_legacy_clock)
        # Trajectories remain sealed in the source bundles. Do not duplicate
        # two potentially large transcripts into the comparison report.
        comparison_runs = []
        for checked in (left, right):
            evidence = dict(checked.report)
            if 'observations' in checked.capture.evidence:
                evidence['capture'] = {key: value for key, value in checked.report['capture'].items()
                                       if key != 'observations'}
                trajectory = checked.capture.evidence['observations']
                evidence['observation_transcript'] = {
                    'manifest': checked.capture.manifest.public_identity(),
                    'policy': trajectory['policy'], 'frame_count': len(trajectory['frames']),
                    'command_count': len(trajectory['commands']),
                }
            comparison_runs.append(evidence)
        report.update(before=comparison_runs[0], after=comparison_runs[1])
        # Compare actual inputs, not just their declared digest strings. Different
        # binaries are intentional; their original bytes were checked above.
        for name in COPIES:
            if left.inputs[name].raw != right.inputs[name].raw:
                raise ValidationError(f'{name} input bytes differ; observations are not comparable')
        require_value(right.capture.clock['policy'], left.capture.clock['policy'],
                      'presentation_clock.policy')
        _require_equal(right.capture.clock, left.capture.clock, 'presentation_clock')
        compared_fields = ('initial', 'final', 'map_source', 'exact_step_count', 'unit_atlas')
        if ('observations' in left.capture.evidence) != ('observations' in right.capture.evidence):
            raise ValidationError('observation policies differ between child generations')
        if 'observations' in left.capture.evidence:
            if left.capture.evidence['observations']['policy'] != right.capture.evidence['observations']['policy']:
                raise ValidationError('observation policies differ between child generations')
            compared_fields += ('observations', 'camera')
        differences = [difference for name in compared_fields
                       for difference in _differences(left.capture.evidence[name],
                                                      right.capture.evidence[name], name)]
        differences.extend(_differences(left.capture.frame_format, right.capture.frame_format,
                                        'frame.surface_format'))
        if left.capture.frame.raw != right.capture.frame.raw:
            differences.append({'field': 'frame.bytes',
                                'before': left.capture.frame.sha256,
                                'after': right.capture.frame.sha256})
        # Catch mutations while the other bundle was being checked as well.
        left.check_unchanged()
        right.check_unchanged()
        report.update(status='MISMATCH' if differences else 'MATCH', differences=differences)
    except (OSError, ValueError) as exc:
        report['errors'].append(str(exc))
    return report


def _check_output_outside_runs(output: Path, directories: list[Path]) -> None:
    # Evidence directories are immutable, including when a requested check fails.
    for directory in directories:
        if output.resolve().is_relative_to(directory.resolve()):
            raise ValidationError('validation output must be outside observation directories')


def main(argv: list[str] | None = None) -> int:
    arguments = list(sys.argv[1:] if argv is None else argv)
    operation = arguments.pop(0) if arguments and arguments[0] in ('validate', 'compare') else 'capture'
    parser = argparse.ArgumentParser(description=__doc__,
                                     epilog='Offline commands: validate --run DIR; '
                                            'compare --before DIR --after DIR (both need --output JSON).')
    parser.add_argument('--output', type=Path, required=True)
    if operation == 'capture':
        parser.add_argument('--profile', type=Path, required=True)
        parser.add_argument('--contract', type=Path, required=True)
        parser.add_argument('--cwd', type=Path, default=Path.cwd())
        binary = parser.add_mutually_exclusive_group()
        binary.add_argument('--executable', type=Path)
        binary.add_argument('--build-label')
    else:
        parser.add_argument('--allow-legacy-inputs', action='store_true')
        parser.add_argument('--allow-legacy-clock', action='store_true',
                            help='Allow offline wall-clock v2 children; not comparable with diagnostic-clock children')
        if operation == 'validate':
            parser.add_argument('--run', type=Path, required=True)
        else:
            parser.add_argument('--before', type=Path, required=True)
            parser.add_argument('--after', type=Path, required=True)
    args = parser.parse_args(arguments)
    try:
        if operation == 'capture':
            report = capture(profile_path=args.profile, contract_path=args.contract,
                             output=args.output, working_directory=args.cwd,
                             executable=args.executable, build_label=args.build_label)
            result_path = args.output / 'run.json'
            status = 0 if report['status'] == 'VALID' else 1
        else:
            directories = [args.run] if operation == 'validate' else [args.before, args.after]
            _check_output_outside_runs(args.output, directories)
            if operation == 'validate':
                report = validate_run(args.run, allow_legacy_inputs=args.allow_legacy_inputs,
                                      allow_legacy_clock=args.allow_legacy_clock)
            else:
                report = compare_runs(args.before, args.after,
                                      allow_legacy_inputs=args.allow_legacy_inputs,
                                      allow_legacy_clock=args.allow_legacy_clock)
            result_path = write_json_exclusive(args.output, report, maximum_length=MAX_RECEIPT_BYTES,
                                              compact=True)
            status = {'VALID': 0, 'MATCH': 0, 'MISMATCH': 1, 'INVALID': 2}[report['status']]
    except (OSError, ValueError) as exc:
        print(f'map observation: {exc}', file=sys.stderr)
        return 2
    print(f'{report["status"]}: {result_path}')
    return status


if __name__ == '__main__':
    raise SystemExit(main())
