"""Run and check a chosen-map production observation; never certify native parity.

The Rust map_observation profile and capture manifest own the runtime schema.
This wrapper binds their receipts to immutable inputs and actual frame bytes.

Profiles may request retained sidebar snapshots with ``observe_sidebar_steps``.
A ``sidebar`` gesture names a current ``tab``, ``cameo`` (logical ``type_id``),
``scroll_up`` or ``scroll_down`` target. Rust resolves the retained hit area and
sends ordinary mouse edges; the receipt includes the resolved position and
before/after views. A nonzero run's L0 snapshot is retained-only; later requested
snapshots must match the actual view submitted to the renderer.

Literal ``key`` gestures traverse the shared in-game keyboard edge owner with
the loaded bindings. Their policy also observes local camera/mode/follow state
and pending selection voice requests without draining the audio owner.
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass
import math
import struct
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
SIDEBAR_GESTURE_POLICY = 'map-local-left-gesture-v2'
KEYBOARD_GESTURE_POLICY = 'map-local-gesture-v3'
COMMAND_BAR_GESTURE_POLICY = 'map-local-gesture-v4'
SIDEBAR_POLICY = 'map-retained-sidebar-observation-v1'
AUDIO_POLICY = 'map-device-pulled-player-pcm-v2'
PRIOR_AUDIO_POLICY = 'map-device-pulled-player-pcm-v1'
LOAD_SEGMENT_POLICY = 'map-literal-quickload-clock-segments-v1'
SIDEBAR_TABS = ('building', 'defense', 'infantry', 'vehicle')
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
EXTENSION_FIELDS = frozenset(('commands', 'gestures', 'observe_owners', 'observe_types',
                              'observe_projectiles', 'observe_anim_types', 'observe_action_line_inputs',
                              'observe_disguise_inputs', 'observe_lasers',
                              'observe_audio', 'allow_load_segments',
                              'camera_cell', 'cursor_position', 'terrain_cells',
                              'observe_super_weapons', 'observe_sidebar_steps'))
ASCII_UPPER = str.maketrans('abcdefghijklmnopqrstuvwxyz', 'ABCDEFGHIJKLMNOPQRSTUVWXYZ')
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


def _presentation_clock(value: Any, ticks: int, segments=None) -> dict[str, Any]:
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
        require_exact_keys(row, ('completed_steps', 'radar_ms', 'tooltip_ms', 'message_ms',
                                *(('simulation_tick',) if segments is not None else ())),
                           row_label)
        step = index + 1 if ticks else 0
        require_value(row.get('completed_steps'), step, f'{row_label}.completed_steps')
        tick, _ = _segment_clock(step, segments or [])
        if segments is not None:
            require_value(row['simulation_tick'], tick, f'{row_label}.simulation_tick')
        for key in ('radar_ms', 'tooltip_ms', 'message_ms'):
            require_value(row.get(key), tick * 22, f'{row_label}.{key}')
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
    elif gesture.get('kind') == 'key':
        require_exact_keys(gesture, ('kind', 'key', *(('modifiers',) if 'modifiers' in gesture else ())), label)
        key = require_string(gesture['key'], f'{label}.key')
        if key != 'Escape' and not (len(key) == 1 and key.isascii() and key.isalnum()):
            raise ValidationError(f'{label}.key must be one ASCII letter/digit or Escape')
        if 'modifiers' in gesture:
            modifiers = require_array(gesture['modifiers'], f'{label}.modifiers')
            if (len(modifiers) > 4 or any(type(value) is not str or value not in ('Ctrl', 'Shift', 'Alt', 'Super')
                                          for value in modifiers) or len(set(modifiers)) != len(modifiers)):
                raise ValidationError(f'{label}.modifiers must be unique literal Ctrl/Shift/Alt/Super keys')
    elif gesture.get('kind') == 'command_bar':
        require_exact_keys(gesture, ('kind', 'command'), label)
        command = require_string(gesture['command'], f'{label}.command')
        if not command.isascii() or not 1 <= len(command) <= 128:
            raise ValidationError(f'{label}.command must be a bounded nonempty ASCII identity')
        # Rust resolves the name from the existing command-bar registry. The
        # wrapper neither maintains another registry nor maps names to actions.
    elif gesture.get('kind') == 'sidebar':
        require_exact_keys(gesture, ('kind', 'target'), label)
        target = require_object(gesture['target'], f'{label}.target')
        kind = target.get('kind')
        if kind == 'tab':
            require_exact_keys(target, ('kind', 'tab'), f'{label}.target')
            if target['tab'] not in SIDEBAR_TABS:
                raise ValidationError(f'{label}.target.tab is unknown')
        elif kind == 'cameo':
            require_exact_keys(target, ('kind', 'type_id'), f'{label}.target')
            if not require_string(target['type_id'], f'{label}.target.type_id'):
                raise ValidationError(f'{label}.target.type_id is empty')
        elif kind in ('scroll_up', 'scroll_down', 'repair', 'sell'):
            require_exact_keys(target, ('kind',), f'{label}.target')
        else:
            raise ValidationError(f'{label}.target.kind must be tab, cameo, scroll_up, scroll_down, repair or sell')
    else:
        raise ValidationError(f'{label}.kind must be click, drag, sidebar, command_bar or key')


def _observes_local_input(profile: Mapping[str, Any]) -> bool:
    return any(row['gesture']['kind'] == 'key' for row in profile.get('gestures', []))


def _gesture_policy(profile: Mapping[str, Any]) -> str:
    if any(row['gesture']['kind'] == 'command_bar' for row in profile.get('gestures', [])):
        return COMMAND_BAR_GESTURE_POLICY
    if _observes_local_input(profile):
        return KEYBOARD_GESTURE_POLICY
    return (SIDEBAR_GESTURE_POLICY if any(row['gesture']['kind'] == 'sidebar'
            for row in profile.get('gestures', [])) else GESTURE_POLICY)


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
    if 'observe_audio' in profile:
        audio = require_object(profile['observe_audio'], 'profile.observe_audio')
        require_exact_keys(audio, ('sound_ids', 'max_events', 'max_samples_per_event', 'completion_tail_ms'),
                           'profile.observe_audio')
        ids = require_array(audio['sound_ids'], 'profile.observe_audio.sound_ids')
        if (not 1 <= len(ids) <= 16 or any(type(value) is not str or not value.isascii() or not 1 <= len(value) <= 128
                                           for value in ids) or len({value.upper() for value in ids}) != len(ids)):
            raise ValidationError('profile.observe_audio.sound_ids must contain 1..16 unique bounded ASCII IDs')
        for key, maximum in (('max_events', 16), ('max_samples_per_event', 262144), ('completion_tail_ms', 10000)):
            _bounded_int(audio[key], f'profile.observe_audio.{key}', 1, maximum)
    if 'allow_load_segments' in profile and type(profile['allow_load_segments']) is not bool:
        raise ValidationError('profile.allow_load_segments must be a boolean')
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
            if _is_quickload(row['gesture']) and not profile.get('allow_load_segments', False):
                raise ValidationError('literal quickload requires allow_load_segments')
    if 'observe_sidebar_steps' in profile:
        steps = require_array(profile['observe_sidebar_steps'], 'profile.observe_sidebar_steps')
        if len(steps) > 1024:
            raise ValidationError('profile.observe_sidebar_steps exceeds 1024 rows')
        previous = -1
        for index, value in enumerate(steps):
            step = _bounded_int(value, f'profile.observe_sidebar_steps[{index}]', 0, ticks)
            if step <= previous:
                raise ValidationError('profile.observe_sidebar_steps must be strictly increasing')
            previous = step
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
    if 'observe_projectiles' in profile and type(profile['observe_projectiles']) is not bool:
        raise ValidationError('profile.observe_projectiles must be a boolean')
    if 'observe_anim_types' in profile:
        names = require_array(profile['observe_anim_types'], 'profile.observe_anim_types')
        if not 1 <= len(names) <= 256:
            raise ValidationError('profile.observe_anim_types must contain 1..256 types')
        selected = set()
        for index, name in enumerate(names):
            name = require_string(name, f'profile.observe_anim_types[{index}]')
            if not name or name.translate(ASCII_UPPER) in selected:
                raise ValidationError('profile.observe_anim_types has an empty or duplicate type')
            selected.add(name.translate(ASCII_UPPER))
    if 'camera_cell' in profile:
        _coordinate(profile['camera_cell'], 'profile.camera_cell')
    if 'observe_action_line_inputs' in profile and type(profile['observe_action_line_inputs']) is not bool:
        raise ValidationError('profile.observe_action_line_inputs must be a boolean')
    if 'observe_disguise_inputs' in profile and type(profile['observe_disguise_inputs']) is not bool:
        raise ValidationError('profile.observe_disguise_inputs must be a boolean')
    if 'observe_lasers' in profile and type(profile['observe_lasers']) is not bool:
        raise ValidationError('profile.observe_lasers must be a boolean')
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
        _animation_runtime(anim['runtime'], f'{anim_label}.runtime')
    return len(slots)


def _animation_runtime(value: Any, label: str) -> None:
    runtime = require_object(value, label)
    require_exact_keys(runtime, ('current_frame', 'frame_step', 'delay_remaining', 'rate_reload',
                                 'frame_timer', 'loop_remaining', 'first_ai_guard',
                                 'constructor_reverse', 'inactive', 'paused'), label)
    for key in ('current_frame', 'frame_step'):
        _bounded_int(runtime[key], f'{label}.{key}', -(1 << 31), (1 << 31) - 1)
    for key in ('delay_remaining', 'rate_reload'):
        _bounded_int(runtime[key], f'{label}.{key}', 0, (1 << 16) - 1)
    _bounded_int(runtime['loop_remaining'], f'{label}.loop_remaining', 0, 255)
    _timer(runtime['frame_timer'], f'{label}.frame_timer')
    for key in ('first_ai_guard', 'constructor_reverse', 'inactive', 'paused'):
        if type(runtime[key]) is not bool:
            raise ValidationError(f'{label}.{key} must be a boolean')


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
            require_exact_keys(economy, ('credits', 'spent_credits', 'score'),
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


def _disguise_inputs(value: Any, category: str, label: str) -> None:
    if value is None:
        return
    row = require_object(value, label)
    require_exact_keys(row, ('active', 'creation_frame', 'type_id', 'house',
                             'reveal_start', 'reveal_duration', 'draw'), label)
    if type(row['active']) is not bool:
        raise ValidationError(f'{label}.active must be a boolean')
    _bounded_int(row['creation_frame'], f'{label}.creation_frame', 0, (1 << 32) - 1)
    for key in ('reveal_start', 'reveal_duration'):
        _bounded_int(row[key], f'{label}.{key}', -(1 << 31), (1 << 31) - 1)
    for key in ('type_id', 'house'):
        if row[key] is not None and not require_string(row[key], f'{label}.{key}'):
            raise ValidationError(f'{label}.{key} must be nonempty or null')
    if category != 'Unit':
        require_value(row['draw'], None, f'{label}.draw')
        return
    draw = require_object(row['draw'], f'{label}.draw')
    require_exact_keys(draw, ('type_id', 'voxel', 'shp_frame', 'terrain_pair_available',
                             'draw_state_visible', 'native_selector_bits'), f'{label}.draw')
    if not require_string(draw['type_id'], f'{label}.draw.type_id'):
        raise ValidationError(f'{label}.draw.type_id must be nonempty')
    for key in ('voxel', 'terrain_pair_available', 'draw_state_visible'):
        if type(draw[key]) is not bool:
            raise ValidationError(f'{label}.draw.{key} must be a boolean')
    if draw['shp_frame'] is not None:
        _bounded_int(draw['shp_frame'], f'{label}.draw.shp_frame', 0, 65535)
    _bounded_int(draw['native_selector_bits'], f'{label}.draw.native_selector_bits', 0, 14)


def _prism(value: Any, category: str, label: str) -> None:
    if category != 'Structure':
        require_value(value, None, label)
        return
    row = require_object(value, label)
    require_exact_keys(row, ('support_count', 'pending', 'rearm'), label)
    _bounded_int(row['support_count'], f'{label}.support_count', -(1 << 31), (1 << 31) - 1)
    rearm = require_object(row['rearm'], f'{label}.rearm')
    require_exact_keys(rearm, ('start', 'duration', 'remaining'), f'{label}.rearm')
    for key in rearm:
        _bounded_int(rearm[key], f'{label}.rearm.{key}', -(1 << 31), (1 << 31) - 1)
    if row['pending'] is not None:
        pending = require_object(row['pending'], f'{label}.pending')
        require_exact_keys(pending, ('mode', 'payload', 'remaining'), f'{label}.pending')
        mode = _bounded_int(pending['mode'], f'{label}.pending.mode', 1, 2)
        _bounded_int(pending['remaining'], f'{label}.pending.remaining', -(1 << 31), (1 << 31) - 1)
        payload = require_object(pending['payload'], f'{label}.pending.payload')
        require_exact_keys(payload, ('weapon',) if mode == 1 else ('to',), f'{label}.pending.payload')
        if mode == 1:
            if payload['weapon'] not in ('Primary', 'Secondary'):
                raise ValidationError(f'{label}.pending.payload.weapon is unknown')
        else:
            _coordinate(payload['to'], f'{label}.pending.payload.to', leptons=True)


def _lasers(value: Any, label: str) -> int:
    snapshot = require_object(value, label)
    require_exact_keys(snapshot, ('detail', 'live'), label)
    detail = require_object(snapshot['detail'], f'{label}.detail')
    require_exact_keys(detail, ('frame_rate', 'minimum', 'buffer', 'reduced', 'logic_visits',
                                'sample_start', 'sample_duration', 'initialized'), f'{label}.detail')
    for key in ('frame_rate', 'minimum', 'buffer', 'logic_visits'):
        _bounded_int(detail[key], f'{label}.detail.{key}', 0, (1 << 32) - 1)
    for key in ('sample_start', 'sample_duration'):
        _bounded_int(detail[key], f'{label}.detail.{key}', -(1 << 31), (1 << 31) - 1)
    for key in ('reduced', 'initialized'):
        if type(detail[key]) is not bool:
            raise ValidationError(f'{label}.detail.{key} must be a boolean')
    live = require_array(snapshot['live'], f'{label}.live')
    for index, value in enumerate(live):
        beam_label = f'{label}.live[{index}]'
        beam = require_object(value, beam_label)
        require_exact_keys(beam, ('birth_frame', 'from', 'to', 'z_adjust', 'width', 'supported',
                                 'house_color', 'rgb', 'duration', 'age', 'timer_start',
                                 'timer_duration'), beam_label)
        for key in ('birth_frame', 'z_adjust', 'width', 'duration', 'age', 'timer_start', 'timer_duration'):
            _bounded_int(beam[key], f'{beam_label}.{key}', -(1 << 31), (1 << 31) - 1)
        for key in ('from', 'to'):
            _coordinate(beam[key], f'{beam_label}.{key}', leptons=True)
        for key in ('supported', 'house_color'):
            if type(beam[key]) is not bool:
                raise ValidationError(f'{beam_label}.{key} must be a boolean')
        rgb = require_array(beam['rgb'], f'{beam_label}.rgb')
        if len(rgb) != 3:
            raise ValidationError(f'{beam_label}.rgb must contain three channels')
        for channel, value in enumerate(rgb):
            _bounded_int(value, f'{beam_label}.rgb[{channel}]', 0, 255)
    return 1 + len(live)


def _actor(value: Any, label: str, *, building_state: bool = True,
           docking_state: bool = True, walk_state: bool = True,
           action_line_inputs: bool = False, disguise_inputs: bool = False,
           lasers: bool = False) -> tuple[int, str]:
    actor = require_object(value, label)
    require_exact_keys(actor, ('stable_id', 'owner', 'type_id', 'category', 'cell',
                              'physical_leptons', 'on_bridge', 'health', 'active',
                              'in_limbo', 'dying', 'mission', 'target', 'archive', 'nav', 'foot',
                              *(('building',) if building_state else ()),
                              *(('unit',) if 'unit' in actor else ()),
                              *(('jumpjet',) if 'jumpjet' in actor else ()),
                              *(('cloak',) if 'cloak' in actor else ()),
                              *(('retask',) if 'retask' in actor else ()),
                              *(('action_line_inputs',) if action_line_inputs else ()),
                              *(('disguise_inputs',) if disguise_inputs else ()),
                              *(('prism',) if lasers else ()),
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
    if disguise_inputs:
        _disguise_inputs(actor['disguise_inputs'], category, f'{label}.disguise_inputs')
    if lasers:
        _prism(actor['prism'], category, f'{label}.prism')
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
    if 'retask' in actor:
        # Historical receipts omit this projection. Absence is not a claim
        # that either existing suspended-reference owner was empty.
        retask = require_object(actor['retask'], f'{label}.retask')
        require_exact_keys(retask, ('suspended_target', 'suspended_nav'), f'{label}.retask')
        _target_reference(retask['suspended_target'], f'{label}.retask.suspended_target')
        _target_reference(retask['suspended_nav'], f'{label}.retask.suspended_nav', navigation=True)
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


def _local_input_observation(value: Any, label: str) -> int:
    row = require_object(value, label)
    require_exact_keys(row, ('camera_top_left', 'camera_zoom', 'follow_target', 'repair_mode',
                             'sell_mode', 'targeting', 'main_rng_cursor', 'selection_voice_enabled',
                             'selection_voice_requests'), label)
    position = require_array(row['camera_top_left'], f'{label}.camera_top_left')
    if len(position) != 2:
        raise ValidationError(f'{label}.camera_top_left must contain exactly two coordinates')
    for index, number in enumerate((*position, row['camera_zoom'])):
        if type(number) not in (int, float) or not math.isfinite(number):
            raise ValidationError(f'{label} camera component {index} must be finite')
    if row['camera_zoom'] <= 0:
        raise ValidationError(f'{label}.camera_zoom must be positive')
    cursor = require_array(row['main_rng_cursor'], f'{label}.main_rng_cursor')
    if len(cursor) != 2:
        raise ValidationError(f'{label}.main_rng_cursor must contain exactly two indices')
    for index, number in enumerate(cursor):
        _bounded_int(number, f'{label}.main_rng_cursor[{index}]', -(1 << 31), (1 << 31) - 1)
    if row['follow_target'] is not None:
        _bounded_int(row['follow_target'], f'{label}.follow_target', 1, (1 << 64) - 1)
    for key in ('repair_mode', 'sell_mode', 'selection_voice_enabled'):
        if type(row[key]) is not bool:
            raise ValidationError(f'{label}.{key} must be boolean')
    if row['targeting'] is not None:
        targeting = require_object(row['targeting'], f'{label}.targeting')
        require_exact_keys(targeting, ('kind', 'type_id'), f'{label}.targeting')
        if targeting['kind'] not in ('building_placement', 'super_weapon'):
            raise ValidationError(f'{label}.targeting.kind is unknown')
        if not require_string(targeting['type_id'], f'{label}.targeting.type_id'):
            raise ValidationError(f'{label}.targeting.type_id is empty')
    voices = require_array(row['selection_voice_requests'], f'{label}.selection_voice_requests')
    for index, value in enumerate(voices):
        voice_label = f'{label}.selection_voice_requests[{index}]'
        voice = require_object(value, voice_label)
        require_exact_keys(voice, ('speaker_id', 'sound_id'), voice_label)
        _bounded_int(voice['speaker_id'], f'{voice_label}.speaker_id', 1, (1 << 64) - 1)
        require_string(voice['sound_id'], f'{voice_label}.sound_id')
    return len(voices)


def _input_observation(value: Any, label: str, *, local_input: bool = False) -> int:
    row = require_object(value, label)
    require_exact_keys(row, ('selected_ids', 'selection_pending', 'target_line_remaining',
                             'target_line_active', *(('local_input',) if local_input else ())), label)
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
    return len(selected) + (_local_input_observation(row['local_input'], f'{label}.local_input')
                            if local_input else 0)


def _sidebar_observation(value: Any, label: str) -> int:
    """Check retained view receipts without rebuilding production options or layout."""
    row = require_object(value, label)
    require_exact_keys(row, ('active_tab', 'scroll_rows', 'max_scroll_rows', 'tabs', 'items',
                             'scroll_up', 'scroll_down'), label)
    if row['active_tab'] not in SIDEBAR_TABS:
        raise ValidationError(f'{label}.active_tab is unknown')
    maximum = _bounded_int(row['max_scroll_rows'], f'{label}.max_scroll_rows', 0, (1 << 64) - 1)
    _bounded_int(row['scroll_rows'], f'{label}.scroll_rows', 0, maximum)

    def flag(value, name):
        if type(value) is not bool:
            raise ValidationError(f'{name} must be boolean')

    def finite(value, name):
        if type(value) not in (int, float) or not math.isfinite(value):
            raise ValidationError(f'{name} must be finite')

    def rect(value, name):
        coordinates = require_array(value, name)
        if len(coordinates) != 4:
            raise ValidationError(f'{name} must contain x, y, width and height')
        for index, number in enumerate(coordinates):
            finite(number, f'{name}[{index}]')
        if coordinates[2] < 0 or coordinates[3] < 0:
            raise ValidationError(f'{name} has a negative extent')

    tabs = require_array(row['tabs'], f'{label}.tabs')
    if len(tabs) != len(SIDEBAR_TABS):
        raise ValidationError(f'{label}.tabs must contain all four tabs')
    for index, (value, name) in enumerate(zip(tabs, SIDEBAR_TABS)):
        tab_label = f'{label}.tabs[{index}]'
        tab = require_object(value, tab_label)
        require_exact_keys(tab, ('tab', 'rect', 'active', 'disabled'), tab_label)
        require_value(tab['tab'], name, f'{tab_label}.tab')
        require_value(tab['active'], name == row['active_tab'], f'{tab_label}.active')
        flag(tab['disabled'], f'{tab_label}.disabled')
        rect(tab['rect'], f'{tab_label}.rect')
    items = require_array(row['items'], f'{label}.items')
    identities = set()
    for index, value in enumerate(items):
        item_label = f'{label}.items[{index}]'
        item = require_object(value, item_label)
        flags = ('enabled', 'is_building_this_type', 'is_ready', 'is_on_hold', 'is_armed', 'is_superweapon')
        require_exact_keys(item, ('slot', 'type_id', 'super_weapon_section', 'display_name', 'rect', 'cost',
                                  'queue_category', 'progress', 'queued_count', *flags), item_label)
        require_value(item['slot'], index, f'{item_label}.slot')
        for key in flags:
            flag(item[key], f'{item_label}.{key}')
        identity = require_string(item['type_id'], f'{item_label}.type_id')
        require_string(item['display_name'], f'{item_label}.display_name')
        if item['super_weapon_section'] is not None:
            identity = require_string(item['super_weapon_section'], f'{item_label}.super_weapon_section')
        if not identity or identity in identities:
            raise ValidationError(f'{item_label} has an empty or repeated cameo identity')
        identities.add(identity)
        require_value(item['is_superweapon'], item['super_weapon_section'] is not None,
                      f'{item_label}.is_superweapon')
        rect(item['rect'], f'{item_label}.rect')
        if item['cost'] is not None:
            _bounded_int(item['cost'], f'{item_label}.cost', -(1 << 31), (1 << 31) - 1)
        if item['queue_category'] not in ('Building', 'Defense', 'Infantry', 'Vehicle', 'Aircraft', 'Ship'):
            raise ValidationError(f'{item_label}.queue_category is unknown')
        finite(item['progress'], f'{item_label}.progress')
        _bounded_int(item['queued_count'], f'{item_label}.queued_count', 0, (1 << 64) - 1)
    for key in ('scroll_up', 'scroll_down'):
        control = require_object(row[key], f'{label}.{key}')
        require_exact_keys(control, ('rect', 'disabled'), f'{label}.{key}')
        rect(control['rect'], f'{label}.{key}.rect')
        flag(control['disabled'], f'{label}.{key}.disabled')
    return len(tabs) + len(items) + 2


def _sidebar_gesture(value: Any, target: Mapping[str, Any], label: str,
                     extent: tuple[int, int]) -> int:
    receipt = require_object(value, label)
    toggle_target = target['kind'] in ('repair', 'sell')
    require_exact_keys(receipt, ('resolved_position', 'before', 'after',
                                *(('toggle',) if toggle_target else ())), label)
    count = sum(_sidebar_observation(receipt[key], f'{label}.{key}') for key in ('before', 'after'))
    before = receipt['before']
    kind = target['kind']
    if toggle_target:
        toggle = require_object(receipt['toggle'], f'{label}.toggle')
        require_exact_keys(toggle, ('before', 'after'), f'{label}.toggle')
        for when in ('before', 'after'):
            row_label = f'{label}.toggle.{when}'
            row = require_object(toggle[when], row_label)
            require_exact_keys(row, ('rect', 'disabled', 'active'), row_label)
            rect = require_array(row['rect'], f'{row_label}.rect')
            if len(rect) != 4 or any(type(part) not in (int, float) or not math.isfinite(part)
                                     for part in rect) or rect[2] < 0 or rect[3] < 0:
                raise ValidationError(f'{row_label}.rect must be a finite rectangle')
            for flag in ('disabled', 'active'):
                if type(row[flag]) is not bool:
                    raise ValidationError(f'{row_label}.{flag} must be boolean')
        control = toggle['before']
        count += 2
    elif kind == 'tab':
        control = next(tab for tab in before['tabs'] if tab['tab'] == target['tab'])
    elif kind == 'cameo':
        found = [item for item in before['items']
                 if (item['super_weapon_section'] or item['type_id']) == target['type_id']]
        if len(found) != 1:
            raise ValidationError(f'{label} target must occur once in the current visible strip')
        control = found[0]
    else:
        control = before[kind]
    if control.get('disabled', False):
        raise ValidationError(f'{label} target is disabled')
    x, y, width, height = control['rect']
    if width <= 0 or height <= 0:
        raise ValidationError(f'{label} target has no hit area')
    expected = [math.floor(x + width / 2), math.floor(y + height / 2)]
    _screen_point(receipt['resolved_position'], f'{label}.resolved_position', extent)
    _require_equal(receipt['resolved_position'], expected, f'{label}.resolved_position')
    return count


def _sidebar_frames(value: Any, profile: Mapping[str, Any]) -> int:
    label = 'observations.sidebar'
    observation = require_object(value, label)
    require_exact_keys(observation, ('policy', 'frames'), label)
    require_value(observation['policy'], SIDEBAR_POLICY, f'{label}.policy')
    frames = require_array(observation['frames'], f'{label}.frames')
    requested = profile['observe_sidebar_steps']
    if len(frames) != len(requested):
        raise ValidationError(f'{label}.frames differs from requested sidebar observation count')
    count = 0
    for index, (value, step) in enumerate(zip(frames, requested)):
        row_label = f'{label}.frames[{index}]'
        row = require_object(value, row_label)
        require_exact_keys(row, ('completed_steps', 'rendered', 'sidebar'), row_label)
        require_value(row['completed_steps'], step, f'{row_label}.completed_steps')
        require_value(row['rendered'], step > 0 or profile['ticks'] == 0, f'{row_label}.rendered')
        count += _sidebar_observation(row['sidebar'], f'{row_label}.sidebar')
    return count


def _command_bar_gesture(value: Any, command: str, label: str, extent: tuple[int, int]) -> int:
    receipt = require_object(value, label)
    require_exact_keys(receipt, ('command', 'slot', 'gadget_id', 'rect', 'resolved_position'), label)
    require_value(receipt['command'], command, f'{label}.command')
    _bounded_int(receipt['slot'], f'{label}.slot', 0, extent[0] - 1)
    _bounded_int(receipt['gadget_id'], f'{label}.gadget_id', 1, (1 << 16) - 1)
    rect = require_array(receipt['rect'], f'{label}.rect')
    if (len(rect) != 4 or any(type(part) not in (int, float) or not math.isfinite(part) for part in rect)
            or rect[2] <= 0 or rect[3] <= 0):
        raise ValidationError(f'{label}.rect must have a finite positive hit area')
    center = [rect[0] + rect[2] / 2, rect[1] + rect[3] / 2]
    if not all(math.isfinite(value) for value in center):
        raise ValidationError(f'{label}.rect has a nonfinite center')
    expected = [math.floor(value) for value in center]
    _screen_point(receipt['resolved_position'], f'{label}.resolved_position', extent)
    _require_equal(receipt['resolved_position'], expected, f'{label}.resolved_position')
    return 1


def _is_quickload(gesture: Mapping[str, Any]) -> bool:
    return (gesture.get('kind') == 'key' and gesture.get('key', '').upper() == 'N'
            and set(gesture.get('modifiers', [])) == {'Ctrl', 'Shift'})


def _segment_clock(step: int, segments: list, *, after_inputs=False, gesture_ordinal=None) -> tuple[int, int]:
    for segment in reversed(segments):
        if (segment['after_step'] < step or (segment['after_step'] == step and after_inputs
                and (gesture_ordinal is None or segment['gesture_ordinal'] < gesture_ordinal))):
            delta = step - segment['after_step']
            return (segment['after']['simulation_tick'] + delta,
                    (segment['after']['binary_frame'] + delta) & 0xffffffff)
    return step, step


def _sound_state(value: Any, label: str) -> int:
    state = require_object(value, label)
    require_exact_keys(state, ('main_rng_cursor', 'scenario_rng_cursor', 'actors'), label)
    for key in ('main_rng_cursor', 'scenario_rng_cursor'):
        cursor = require_array(state[key], f'{label}.{key}')
        if len(cursor) != 2:
            raise ValidationError(f'{label}.{key} must contain two indices')
        for index in cursor:
            _bounded_int(index, f'{label}.{key}', 0, 249)
    actors = require_array(state['actors'], f'{label}.actors')
    previous = 0
    for actor in actors:
        row = require_object(actor, f'{label}.actors[]')
        require_exact_keys(row, ('stable_id', 'body_counter', 'active', 'countdown'), f'{label}.actors[]')
        previous = _bounded_int(row['stable_id'], f'{label}.stable_id', previous + 1, (1 << 64) - 1)
        _bounded_int(row['body_counter'], f'{label}.body_counter', -(1 << 31), (1 << 32) - 1)
        _bounded_int(row['countdown'], f'{label}.countdown', -(1 << 31), (1 << 31) - 1)
        if type(row['active']) is not bool:
            raise ValidationError(f'{label}.active must be a boolean')
    return len(actors) + 2


def _load_segments(value: Any, profile: Mapping[str, Any]) -> list:
    if not profile.get('allow_load_segments', False):
        if value is not None:
            raise ValidationError('load segment receipt was not requested')
        return []
    label = 'observations.load_segments'
    receipt = require_object(value, label)
    require_exact_keys(receipt, ('policy', 'transitions'), label)
    require_value(receipt['policy'], LOAD_SEGMENT_POLICY, f'{label}.policy')
    transitions = require_array(receipt['transitions'], f'{label}.transitions')
    requests = [(index, row) for index, row in enumerate(profile.get('gestures', [])) if _is_quickload(row['gesture'])]
    if len(transitions) != len(requests) or len(transitions) > 16:
        raise ValidationError('load transitions must match the requested quickload gestures (maximum 16)')
    previous = []
    for index, (value, (ordinal, request)) in enumerate(zip(transitions, requests)):
        row_label = f'{label}.transitions[{index}]'
        row = require_object(value, row_label)
        require_exact_keys(row, ('after_step', 'gesture_ordinal', 'before', 'after',
                                 *(('restored_audio_state',) if 'observe_audio' in profile else ())), row_label)
        require_value(row['after_step'], request['issue_after_step'], f'{row_label}.after_step')
        require_value(row['gesture_ordinal'], ordinal, f'{row_label}.gesture_ordinal')
        for key in ('before', 'after'):
            clock = require_object(row[key], f'{row_label}.{key}')
            require_exact_keys(clock, ('simulation_tick', 'binary_frame', 'total_simulation_ms'), f'{row_label}.{key}')
            for field in clock:
                _bounded_int(clock[field], f'{row_label}.{key}.{field}', 0, (1 << (32 if field == 'binary_frame' else 64)) - 1)
        expected = _segment_clock(row['after_step'], previous, after_inputs=True)
        for field, number in zip(('simulation_tick', 'binary_frame'), expected):
            require_value(row['before'][field], number, f'{row_label}.before.{field}')
        if any(row['after'][key] >= row['before'][key] for key in row['before']):
            raise ValidationError(f'{row_label} must retain an actual restored earlier clock')
        if 'observe_audio' in profile:
            _sound_state(row['restored_audio_state'], f'{row_label}.restored_audio_state')
        previous.append(row)
    return list(transitions)


def _audio_action(value: Any, label: str, profile: Mapping[str, Any], kinds: tuple[str, ...], earliest_ms: int) -> int:
    action = require_object(value, label)
    require_exact_keys(action, ('kind', 'service_ms', 'context'), label)
    if action['kind'] not in kinds:
        raise ValidationError('unknown audio action')
    milliseconds = _bounded_int(action['service_ms'], f'{label}.service_ms', earliest_ms, (1 << 64) - 1)
    context = require_object(action['context'], f'{label}.context')
    require_exact_keys(context, ('completed_steps', 'simulation_tick', 'binary_frame'), f'{label}.context')
    for key in context:
        _bounded_int(context[key], f'{label}.context.{key}', 0, profile['ticks'])
    return milliseconds


def _audio_observation(value: Any, profile: Mapping[str, Any]) -> None:
    label = 'observations.audio'
    audio = require_object(value, label)
    if audio.get('policy') not in (AUDIO_POLICY, PRIOR_AUDIO_POLICY):
        raise ValidationError('unknown audio observation policy')
    voices = audio['policy'] == AUDIO_POLICY
    require_exact_keys(audio, ('policy', 'point', 'completion_tail_ms', 'tail_draw_count',
                               'settled', 'truncated', 'outputs', *(('voice_actions',) if voices else ())), label)
    for key, expected in (('point', 'post_player_pre_device_mixer'),
                          ('settled', True), ('truncated', False)):
        require_value(audio[key], expected, f'{label}.{key}')
    _bounded_int(audio['completion_tail_ms'], f'{label}.completion_tail_ms', 0, profile['timeout_seconds'] * 1000)
    _bounded_int(audio['tail_draw_count'], f'{label}.tail_draw_count', 0, (1 << 32) - 1)
    config = profile['observe_audio']
    outputs = require_array(audio['outputs'], f'{label}.outputs')
    if len(outputs) > config['max_events']:
        raise ValidationError('audio event count exceeds requested bound')
    for index, value in enumerate(outputs):
        row_label = f'{label}.outputs[{index}]'
        row = require_object(value, row_label)
        require_exact_keys(row, ('submission', 'event', 'owner', 'sound_id', 'resolved_samples', 'source_sample_count', 'source_ended', 'actions', 'pcm', *(('owner_role',) if voices else ())), row_label)
        require_value(row['submission'], index, f'{row_label}.submission')
        require_value(row['source_ended'], True, f'{row_label}.source_ended')
        _bounded_int(row['event'], f'{row_label}.event', 0, (1 << 32) - 1)
        if row['owner'] is not None:
            _bounded_int(row['owner'], f'{row_label}.owner', 1, (1 << 64) - 1)
        if voices and (row['owner_role'] not in (None, 'positional', 'unit_voice')
                       or (row['owner_role'] is None) != (row['owner'] is None)):
            raise ValidationError('audio owner role differs from its typed owner')
        if require_string(row['sound_id'], f'{row_label}.sound_id').upper() not in {name.upper() for name in config['sound_ids']}:
            raise ValidationError('audio output is outside requested sound filter')
        names = require_array(row['resolved_samples'], f'{row_label}.resolved_samples')
        if len(names) > 128 or any(type(name) is not str or not name or len(name) > 128 for name in names):
            raise ValidationError('audio sample identities exceed bounds')
        _bounded_int(row['source_sample_count'], f'{row_label}.source_sample_count', 0, (1 << 64) - 1)
        actions = require_array(row['actions'], f'{row_label}.actions')
        if (not 2 <= len(actions) <= 64
                or require_object(actions[0], 'first audio action').get('kind') != 'submitted'
                or require_object(actions[-1], 'last audio action').get('kind') not in ('stopped', 'completed')):
            raise ValidationError('audio output requires bounded submission and terminal actions')
        previous_ms = 0
        for ordinal, action in enumerate(actions):
            previous_ms = _audio_action(action, f'{row_label}.actions[]', profile,
                ('submitted', 'started', 'release', 'detach', 'stopped', 'completed'), previous_ms)
            if ordinal < len(actions) - 1 and action['kind'] in ('stopped', 'completed'):
                raise ValidationError('audio output continued after terminal action')
        pcm = require_object(row['pcm'], f'{row_label}.pcm')
        require_exact_keys(pcm, ('encoding', 'sample_count', 'finite_count', 'nonzero_count', 'formats', 'sha256', 'hex', 'truncated'), f'{row_label}.pcm')
        require_value(pcm['encoding'], 'f32le', f'{row_label}.pcm.encoding')
        require_value(pcm['truncated'], False, f'{row_label}.pcm.truncated')
        count = _bounded_int(pcm['sample_count'], f'{row_label}.pcm.sample_count', 0, config['max_samples_per_event'])
        hex_value = require_string(pcm['hex'], f'{row_label}.pcm.hex')
        if len(hex_value) != count * 8 or any(char not in '0123456789abcdef' for char in hex_value):
            raise ValidationError('PCM hex length/encoding differs from its bounded sample count')
        payload = bytes.fromhex(hex_value)
        require_value(pcm['sha256'], sha256_bytes(payload), f'{row_label}.pcm.sha256')
        samples = [sample[0] for sample in struct.iter_unpack('<f', payload)]
        require_value(pcm['finite_count'], sum(math.isfinite(sample) for sample in samples), f'{row_label}.pcm.finite_count')
        require_value(pcm['nonzero_count'], sum(sample != 0 for sample in samples), f'{row_label}.pcm.nonzero_count')
        formats = require_array(pcm['formats'], f'{row_label}.pcm.formats')
        if len(formats) > 64 or bool(formats) != bool(count):
            raise ValidationError('PCM format spans do not cover the retained samples')
        previous_start = -1
        for format_index, span in enumerate(formats):
            require_exact_keys(require_object(span, 'PCM format'), ('first_sample', 'channels', 'sample_rate'), 'PCM format')
            previous_start = _bounded_int(span['first_sample'], 'PCM first_sample', previous_start + 1, count - 1)
            if format_index == 0:
                require_value(span['first_sample'], 0, 'PCM first format')
            _bounded_int(span['channels'], 'PCM channels', 1, 65535)
            _bounded_int(span['sample_rate'], 'PCM sample_rate', 1, (1 << 32) - 1)
    if voices:
        actions = require_array(audio['voice_actions'], f'{label}.voice_actions')
        if len(actions) > config['max_events'] * 64:
            raise ValidationError('voice action count exceeds requested bound')
        previous_ms = 0
        for ordinal, value in enumerate(actions):
            row_label = f'{label}.voice_actions[{ordinal}]'
            row = require_object(value, row_label)
            require_exact_keys(row, ('owner', 'action', 'before', 'after', 'live_event_before', 'submitted_event'), row_label)
            _bounded_int(row['owner'], f'{row_label}.owner', 1, (1 << 64) - 1)
            previous_ms = _audio_action(row['action'], f'{row_label}.action', profile,
                ('queued', 'reached_head', 'destroyed'), previous_ms)
            for key in ('live_event_before', 'submitted_event'):
                if row[key] is not None:
                    _bounded_int(row[key], f'{row_label}.{key}', 0, (1 << 32) - 1)
                    require_value(row['action']['kind'], 'reached_head', f'{row_label}.action.kind')
            names = []
            for phase in ('before', 'after'):
                state = require_object(row[phase], f'{row_label}.{phase}')
                require_exact_keys(state, ('pending', 'playing'), f'{row_label}.{phase}')
                for key, name in state.items():
                    if name is not None:
                        require_string(name, f'{row_label}.{phase}.{key}')
                        if not name or len(name) > 128 or not name.isascii():
                            raise ValidationError('voice latch identity exceeds bounds')
                        names.append(name.upper())
            if not set(names).intersection(name.upper() for name in config['sound_ids']):
                raise ValidationError('voice action is outside requested sound filter')


def _gesture_observations(value: Any, profile: Mapping[str, Any], segments=()) -> int:
    label = 'observations.gesture_input'
    observation = require_object(value, label)
    local_input = _observes_local_input(profile)
    require_exact_keys(observation, ('policy', 'equal_step_order', 'tactical_extent', 'receipts',
                                     *(('keyboard_bindings',) if local_input else ())), label)
    require_value(observation['policy'], _gesture_policy(profile), f'{label}.policy')
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
    if local_input:
        bindings = require_array(observation['keyboard_bindings'], f'{label}.keyboard_bindings')
        seen_commands = set()
        for index, value in enumerate(bindings):
            binding_label = f'{label}.keyboard_bindings[{index}]'
            binding = require_object(value, binding_label)
            require_exact_keys(binding, ('command', 'first_key'), binding_label)
            command = require_string(binding['command'], f'{binding_label}.command')
            if not command or command in seen_commands:
                raise ValidationError(f'{binding_label}.command is empty or repeated')
            seen_commands.add(command)
            if binding['first_key'] is not None:
                _bounded_int(binding['first_key'], f'{binding_label}.first_key', 1, (1 << 16) - 1)
        sample_count += len(bindings)
    for index, (value, request) in enumerate(zip(receipts, requested)):
        row_label = f'{label}.receipts[{index}]'
        row = require_object(value, row_label)
        sidebar = request['gesture']['kind'] == 'sidebar'
        keyboard = request['gesture']['kind'] == 'key'
        command_bar = request['gesture']['kind'] == 'command_bar'
        require_exact_keys(row, ('ordinal', 'issue_after_step', 'issued_simulation_tick',
                                 'issued_binary_frame', 'gesture', 'before', 'after',
                                 'left_press_captured', 'band_box_before_release',
                                 'neutral_input_restored', 'queued_commands',
                                 *(('sidebar',) if sidebar else ()),
                                 *(('command_bar',) if command_bar else ()),
                                 *(('keyboard',) if keyboard else ())), row_label)
        tick, frame = _segment_clock(request['issue_after_step'], segments, after_inputs=True, gesture_ordinal=index)
        for key, expected in (('ordinal', index), ('issue_after_step', request['issue_after_step']),
                              ('issued_simulation_tick', tick), ('issued_binary_frame', frame)):
            require_value(row[key], expected, f'{row_label}.{key}')
        _require_equal(row['gesture'], request['gesture'], f'{row_label}.gesture')
        _gesture(row['gesture'], f'{row_label}.gesture', extent)
        for key, expected in (('left_press_captured', not keyboard), ('neutral_input_restored', True),
                              ('band_box_before_release', request['gesture']['kind'] == 'drag')):
            require_value(row[key], expected, f'{row_label}.{key}')
        for key in ('before', 'after'):
            sample_count += _input_observation(row[key], f'{row_label}.{key}', local_input=local_input)
        if sidebar:
            sample_count += _sidebar_gesture(row['sidebar'], request['gesture']['target'],
                f'{row_label}.sidebar', (profile['width'], profile['height']))
        if command_bar:
            sample_count += _command_bar_gesture(row['command_bar'], request['gesture']['command'],
                f'{row_label}.command_bar', (profile['width'], profile['height']))
        if keyboard:
            key_label = f'{row_label}.keyboard'
            receipt = require_object(row['keyboard'], key_label)
            require_exact_keys(receipt, ('encoded_key', 'binding_command', 'press_held',
                                        'release_cleared'), key_label)
            _bounded_int(receipt['encoded_key'], f'{key_label}.encoded_key', 1, (1 << 16) - 1)
            if receipt['binding_command'] is not None and not require_string(
                    receipt['binding_command'], f'{key_label}.binding_command'):
                raise ValidationError(f'{key_label}.binding_command is empty')
            if receipt['binding_command'] is not None and receipt['binding_command'] not in seen_commands:
                raise ValidationError(f'{key_label}.binding_command is absent from registered bindings')
            for field in ('press_held', 'release_cleared'):
                require_value(receipt[field], True, f'{key_label}.{field}')
        commands = require_array(row['queued_commands'], f'{row_label}.queued_commands')
        sample_count += len(commands)
        for number, value in enumerate(commands):
            command_label = f'{row_label}.queued_commands[{number}]'
            command = require_object(value, command_label)
            require_exact_keys(command, ('owner', 'execute_tick', 'payload'), command_label)
            if not require_string(command['owner'], f'{command_label}.owner'):
                raise ValidationError(f'{command_label}.owner is empty')
            require_value(command['execute_tick'], tick, f'{command_label}.execute_tick')
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


def _observes_effects(profile: Mapping[str, Any]) -> bool:
    return profile.get('observe_projectiles', False) or 'observe_anim_types' in profile


def _effects(value: Any, profile: Mapping[str, Any], label: str) -> int:
    effects = require_object(value, label)
    with_projectiles = profile.get('observe_projectiles', False)
    with_anims = 'observe_anim_types' in profile
    require_exact_keys(effects, ('scenario_rng_cursor',
                                *(('projectiles',) if with_projectiles else ()),
                                *(('animations',) if with_anims else ())), label)
    cursor = require_array(effects['scenario_rng_cursor'], f'{label}.scenario_rng_cursor')
    if len(cursor) != 2:
        raise ValidationError(f'{label}.scenario_rng_cursor must contain two indices')
    for index, number in enumerate(cursor):
        _bounded_int(number, f'{label}.scenario_rng_cursor[{index}]', 0, 249)
    anim_types = {name.translate(ASCII_UPPER) for name in profile.get('observe_anim_types', [])}
    samples = 1
    for kind in ('projectiles', 'animations'):
        if kind not in effects:
            continue
        rows = require_array(effects[kind], f'{label}.{kind}')
        samples += len(rows)
        if samples > MAX_OBSERVATION_SAMPLES:
            raise ValidationError(f'{label} exceeds its retained sample budget')
        previous = 0
        for index, value in enumerate(rows):
            row_label = f'{label}.{kind}[{index}]'
            row = require_object(value, row_label)
            common = ('stable_id', 'native_id', 'type_id', 'physical_leptons', 'in_logic_vector')
            fields = (('weapon', 'source_id', 'awaiting_anim', 'visual_frame', 'visual_countdown')
                      if kind == 'projectiles' else
                      ('stored_leptons', 'owner_entity', 'completed', 'effective_end',
                       'effective_loop_end', 'draw_flags', 'z_adjust', 'hidden',
                       'translucency_ramp', 'runtime'))
            require_exact_keys(row, (*common, *fields), row_label)
            identity = _bounded_int(row['stable_id'], f'{row_label}.stable_id', 1, (1 << 64) - 1)
            if identity <= previous:
                raise ValidationError(f'{row_label}.stable_id is repeated or out of order')
            previous = identity
            _bounded_int(row['native_id'], f'{row_label}.native_id', -(1 << 31), (1 << 31) - 1)
            if kind == 'projectiles':
                _coordinate(row['physical_leptons'], f'{row_label}.physical_leptons', leptons=True)
                if not require_string(row['weapon'], f'{row_label}.weapon'):
                    raise ValidationError(f'{row_label}.weapon is empty')
                if row['type_id'] is not None and not require_string(row['type_id'], f'{row_label}.type_id'):
                    raise ValidationError(f'{row_label}.type_id is empty')
                _bounded_int(row['source_id'], f'{row_label}.source_id', 0, (1 << 64) - 1)
                for field in ('visual_frame', 'visual_countdown'):
                    _bounded_int(row[field], f'{row_label}.{field}', 0, 255)
                booleans = ('in_logic_vector', 'awaiting_anim')
            else:
                name = require_string(row['type_id'], f'{row_label}.type_id')
                if name.translate(ASCII_UPPER) not in anim_types:
                    raise ValidationError(f'{row_label}.type_id is outside the requested animation filter')
                if row['physical_leptons'] is not None:
                    _coordinate(row['physical_leptons'], f'{row_label}.physical_leptons', leptons=True)
                _coordinate(row['stored_leptons'], f'{row_label}.stored_leptons', leptons=True)
                if row['owner_entity'] is not None:
                    _bounded_int(row['owner_entity'], f'{row_label}.owner_entity', 1, (1 << 64) - 1)
                for field in ('effective_end', 'effective_loop_end', 'z_adjust'):
                    _bounded_int(row[field], f'{row_label}.{field}', -(1 << 31), (1 << 31) - 1)
                _bounded_int(row['draw_flags'], f'{row_label}.draw_flags', 0, (1 << 32) - 1)
                _bounded_int(row['translucency_ramp'], f'{row_label}.translucency_ramp', 0, 255)
                _animation_runtime(row['runtime'], f'{row_label}.runtime')
                booleans = ('in_logic_vector', 'completed', 'hidden')
            for field in booleans:
                if type(row[field]) is not bool:
                    raise ValidationError(f'{row_label}.{field} must be a boolean')
    return samples


def _observations(value: Any, profile: Mapping[str, Any], final: Mapping[str, Any], *,
                  building_state: bool = True, docking_state: bool = True,
                  walk_state: bool = True) -> dict[str, Any]:
    label = 'observations'
    observations = require_object(value, label)
    gesture_input = 'gestures' in profile
    sidebar = 'observe_sidebar_steps' in profile
    effects = _observes_effects(profile)
    if effects and not walk_state:
        raise ValidationError('effect observations require the current observation policy')
    if (gesture_input or sidebar) and not walk_state:
        raise ValidationError('gesture/sidebar observations require the current observation policy')
    require_exact_keys(observations, ('policy', 'owners', 'commands', 'frames',
                                     *(('audio',) if 'observe_audio' in profile else ()),
                                     *(('load_segments',) if profile.get('allow_load_segments', False) else ()),
                                     *(('gesture_input',) if gesture_input else ()),
                                     *(('sidebar',) if sidebar else ()),
                                     *(('type_filter',) if 'observe_types' in profile else ()),
                                     *(('rule_types',) if building_state else ())), label)
    segments = _load_segments(observations.get('load_segments'), profile)
    if 'observe_audio' in profile:
        _audio_observation(observations['audio'], profile)
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
        tick, _ = _segment_clock(request['issue_after_step'], segments)
        for key, expected in (('ordinal', index), ('issue_after_step', request['issue_after_step']),
                              ('issued_simulation_tick', tick),
                              ('envelope_execute_tick', tick), ('owner', request['owner'])):
            require_value(row[key], expected, f'{row_label}.{key}')
        _require_equal(row['payload'], request['payload'], f'{row_label}.payload')
    frames = require_array(observations['frames'], f'{label}.frames')
    ticks = profile['ticks']
    if len(frames) != ticks + 1:
        raise ValidationError(f'observations.frames must contain exactly {ticks + 1} rows including L0')
    seen = set()
    sample_count = _gesture_observations(observations['gesture_input'], profile, segments) if gesture_input else 0
    if 'observe_audio' in profile:
        sample_count += sum(_sound_state(row['restored_audio_state'], 'restored_audio_state')
                            for row in segments)
    if sidebar:
        sample_count += _sidebar_frames(observations['sidebar'], profile)
    previous_ms = -1
    expected_cells = profile.get('terrain_cells', [])
    for step, value in enumerate(frames):
        row_label = f'{label}.frames[{step}]'
        row = require_object(value, row_label)
        require_exact_keys(row, ('completed_steps', 'simulation_tick', 'binary_frame',
                                 'total_simulation_ms', 'actors', 'missing_actor_ids', 'terrain',
                                 *(('input',) if gesture_input else ()),
                                 *(('effects',) if effects else ()),
                                 *(('lasers',) if profile.get('observe_lasers', False) else ()),
                                 *(('audio_state',) if 'observe_audio' in profile else ()),
                                 *(('houses',) if docking_state else ())), row_label)
        tick, binary_frame = _segment_clock(step, segments)
        for key, expected in (('completed_steps', step), ('simulation_tick', tick), ('binary_frame', binary_frame)):
            require_value(row[key], expected, f'{row_label}.{key}')
        milliseconds = _integer(row, 'total_simulation_ms')
        for segment in segments:
            if segment['after_step'] == step - 1:
                previous_ms = segment['after']['total_simulation_ms']
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
                                     action_line_inputs=profile.get('observe_action_line_inputs', False),
                                     disguise_inputs=profile.get('observe_disguise_inputs', False),
                                     lasers=profile.get('observe_lasers', False))
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
        if effects:
            sample_count += _effects(row['effects'], profile, f'{row_label}.effects')
        if profile.get('observe_lasers', False):
            sample_count += _lasers(row['lasers'], f'{row_label}.lasers')
        if gesture_input:
            sample_count += _input_observation(row['input'], f'{row_label}.input',
                                                local_input=_observes_local_input(profile))
        if 'observe_audio' in profile:
            sample_count += _sound_state(row['audio_state'], f'{row_label}.audio_state')
            if any(actor['stable_id'] not in present for actor in row['audio_state']['actors']):
                raise ValidationError('sound-state actor is absent from the observed actor frame')
        if docking_state:
            sample_count += _houses(row['houses'], owners, f'{row_label}.houses',
                                    profile.get('observe_super_weapons', False))
        if building_state:
            sample_count += sum(len(actor['building']['animation_slots']) for actor in actors
                                if actor['category'] == 'Structure')
        if sample_count > MAX_OBSERVATION_SAMPLES:
            raise ValidationError('observations exceeds its retained sample budget')
    require_value(previous_ms, final['total_simulation_ms'], 'observations final total_simulation_ms')
    for index, segment in enumerate(segments):
        prior = (segments[index - 1]['after'] if index and segments[index - 1]['after_step'] == segment['after_step']
                 else frames[segment['after_step']])
        require_value(segment['before']['total_simulation_ms'], prior['total_simulation_ms'], 'load segment before time')
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
    segments = _load_segments(require_object(manifest.get('observations', {}), 'observations').get('load_segments'), profile)
    require_value(manifest.get('exact_step_count'), ticks, 'exact_step_count')
    initial = require_object(manifest.get('initial'), 'initial')
    final = require_object(manifest.get('final'), 'final')
    for name in ('simulation_tick', 'binary_frame', 'total_simulation_ms'):
        require_value(initial.get(name), 0, f'initial.{name}')
        _integer(final, name)
    for state in (initial, final):
        _integer(state, 'deterministic_state_hash')
    for name, expected in zip(('simulation_tick', 'binary_frame'), _segment_clock(ticks, segments)):
        require_value(final.get(name), expected, f'final.{name}')
    if ticks == 0:
        _require_equal(dict(final), dict(initial), 'zero-step final state')
        for name in ('first_exact_step', 'last_exact_step'):
            require_value(manifest.get(name), None, name)
    else:
        if final['total_simulation_ms'] <= initial['total_simulation_ms']:
            raise ValidationError('simulation time did not advance')
        for name, before in (('first_exact_step', 0), ('last_exact_step', ticks - 1)):
            receipt = require_object(manifest.get(name), name)
            for key, expected in zip(('tick', 'binary_frame'), _segment_clock(before, segments, after_inputs=True)):
                require_value(receipt.get(f'{key}_before'), expected, f'{name}.{key}_before')
                require_value(receipt.get(f'{key}_after'), expected + 1, f'{name}.{key}_after')

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
        clock = _presentation_clock(render.get('presentation_clock'), ticks,
                                    segments if profile.get('allow_load_segments', False) else None)
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


def export_audio(directory: Path, output: Path) -> dict[str, Any]:
    """Export checked device-pulled f32 PCM, without another asset decode/mix.

    Each observed Source format span becomes a float WAV. Any final incomplete
    channel frame stays in the original receipt and is counted in this export.
    """
    _check_output_outside_runs(output, [directory])
    checked = _load_run(directory, False, False)
    audio = checked.capture.evidence.get('observations', {}).get('audio')
    if audio is None:
        raise ValidationError('run did not request PCM observation')
    files = []
    payloads = []
    for event in audio['outputs']:
        pcm = event['pcm']
        raw = bytes.fromhex(pcm['hex'])
        for index, span in enumerate(pcm['formats']):
            end = (pcm['formats'][index + 1]['first_sample'] if index + 1 < len(pcm['formats'])
                   else pcm['sample_count'])
            count = end - span['first_sample']
            channels, rate = span['channels'], span['sample_rate']
            usable = count - count % channels
            data = raw[span['first_sample'] * 4:(span['first_sample'] + usable) * 4]
            if channels * 4 > 65535 or rate * channels * 4 > 0xffffffff:
                raise ValidationError('recorded PCM format exceeds WAVE field bounds')
            # RIFF WAVE_FORMAT_IEEE_FLOAT preserves every retained f32 bit.
            fmt = struct.pack('<HHIIHH', 3, channels, rate, rate * channels * 4, channels * 4, 32)
            chunks = b'fmt ' + struct.pack('<I', len(fmt)) + fmt
            chunks += b'fact' + struct.pack('<II', 4, usable // channels)
            chunks += b'data' + struct.pack('<I', len(data)) + data
            wav = b'RIFF' + struct.pack('<I', 4 + len(chunks)) + b'WAVE' + chunks
            name = f'submission-{event["submission"]}-event-{event["event"]}-span-{index}.wav'
            payloads.append((name, wav))
            files.append({'file_name': name, 'submission': event['submission'], 'event': event['event'], 'sound_id': event['sound_id'],
                          'resolved_samples': event['resolved_samples'], 'channels': channels, 'sample_rate': rate,
                          'sample_count': usable, 'unframed_tail_samples': count - usable,
                          'pcm_sha256': sha256_bytes(data), 'sha256': sha256_bytes(wav)})
    checked.check_unchanged()
    create_directory_exclusive(output, 'audio export')
    for name, wav in payloads:
        write_bytes_exclusive(output / name, wav)
    report = {'schema_version': 'vera20k.map-audio-export.v1', 'status': 'EXPORTED',
              'source_manifest': checked.capture.manifest.public_identity(), 'point': audio['point'],
              'files': files, 'native_comparator': 'NONE', 'parity_certification': 'NONE'}
    write_json_exclusive(output / 'audio.json', report)
    return report


def main(argv: list[str] | None = None) -> int:
    arguments = list(sys.argv[1:] if argv is None else argv)
    operation = arguments.pop(0) if arguments and arguments[0] in ('validate', 'compare', 'export-audio') else 'capture'
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
        if operation in ('validate', 'export-audio'):
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
            directories = [args.run] if operation in ('validate', 'export-audio') else [args.before, args.after]
            _check_output_outside_runs(args.output, directories)
            if operation == 'export-audio':
                report = export_audio(args.run, args.output)
                print(f'EXPORTED: {args.output}')
                return 0
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
