"""Summarize already validated map-observation receipts, without simulating outcomes.

``tools.map_observation`` owns run loading and integrity checks. This module only
projects its checked ``capture.evidence['observations']``. Report filters bind an
actor when it is first observed and follow that stable ID through ownership/type
changes within one load epoch. Quickload can reuse object IDs, so it starts a new
epoch. Assertion selectors are independent of report filters and bind exactly
one actor at ``bind_step`` (default L0); they never select a replacement actor.

Example expectation document::

    {"schema_version": "vera20k.map-observation-expectations.v1", "assertions": [
      {"name": "repair reached full health",
       "subject": {"actor": {"type_id": "GAPOWR", "owner": "Computer1"}},
       "eventually": {"from_step": 1, "through_step": 200},
       "field": "health", "op": "eq", "value": 750}]}

Subjects are ``actor`` (stable_id OR type_id/optional owner/cell, plus bind_step),
``house`` (owner name), ``super_weapon`` (owner and exact type name), or ``gesture``
(receipt ordinal, observed only at its issue_after_step). Exactly
one of ``at_step``, ``eventually`` or ``always`` selects completed-step samples.
Fields are literal dotted object keys; operators are eq/ne/lt/le/gt/ge. Actor
``present`` is the only synthetic field: false requires a missing_actor_ids
receipt, not a failed selector. Missing fields are different from JSON null.

An observed match establishes eventually; an observed counterexample disproves
always. Otherwise missing coverage yields UNOBSERVED, never a vacuous PASS.
These are assertions about retained samples, not command admission, causality,
in-between-step behavior, or native parity. Simulation clocks may rewind after
quickload, so all windows and timeline ordering use completed_steps.
"""
from __future__ import annotations

from bisect import bisect_left
from copy import deepcopy
import math
import re
from typing import Any, Mapping, Sequence

from tools.tactical_certification.core import ValidationError

EXPECTATIONS_SCHEMA = 'vera20k.map-observation-expectations.v1'
INSPECTION_SCHEMA = 'vera20k.map-observation-inspection.v1'
DEFAULT_ACTOR_FIELDS = ('owner', 'type_id', 'health', 'cell', 'active', 'in_limbo',
                        'dying', 'mission.effective', 'target', 'nav')
HOUSE_FIELDS = ('economy.credits', 'economy.spent_credits', 'economy.score')
SUPER_WEAPON_FIELDS = ('granted', 'ready', 'on_hold', 'charge_start', 'charge_duration')
MAX_ROWS = 10_000
MAX_ASSERTIONS = 128
MAX_ASSERTION_SAMPLES = 1_000_000
MAX_STEP = 100_000
_FIELD = re.compile(r'[A-Za-z][A-Za-z0-9_]*(?:\.[A-Za-z][A-Za-z0-9_]*)*\Z')
_ABSENT = object()


def _object(value: Any, label: str, required: set[str], optional=()) -> Mapping[str, Any]:
    if not isinstance(value, dict) or not required <= value.keys() or value.keys() - required - set(optional):
        raise ValidationError(f'{label} requires {sorted(required)}; optional keys: {sorted(optional)}')
    return value


def _integer(value: Any, label: str, minimum: int, maximum: int) -> int:
    if type(value) is not int or not minimum <= value <= maximum:
        raise ValidationError(f'{label} must be an integer in {minimum}..{maximum}')
    return value


def _string(value: Any, label: str) -> str:
    if not isinstance(value, str) or not value or len(value) > 160:
        raise ValidationError(f'{label} must be a nonempty string of at most 160 characters')
    return value


def _field(value: Any, label: str) -> str:
    result = _string(value, label)
    if not _FIELD.fullmatch(result) or len(result.split('.')) > 8:
        raise ValidationError(f'{label} must be a dotted object-key path, with at most eight components')
    return result


def _literal(value: Any, label: str, budget: list[int], depth=0) -> None:
    budget[0] -= 1
    if budget[0] < 0 or depth > 8:
        raise ValidationError(f'{label} exceeds the 256-value/eight-level literal limit')
    if value is None or type(value) is bool:
        return
    if type(value) in (int, float):
        if (type(value) is float and not math.isfinite(value)) or (type(value) is int and value.bit_length() > 64):
            raise ValidationError(f'{label} must contain finite numbers of at most 64 integer bits')
        return
    if isinstance(value, str):
        if len(value) > 1024:
            raise ValidationError(f'{label} string exceeds 1024 characters')
        return
    if isinstance(value, list):
        for item in value:
            _literal(item, label, budget, depth + 1)
        return
    if isinstance(value, dict) and all(isinstance(key, str) and len(key) <= 160 for key in value):
        for item in value.values():
            _literal(item, label, budget, depth + 1)
        return
    raise ValidationError(f'{label} must be a JSON literal')


def _subject(value: Any, label: str) -> None:
    if not isinstance(value, dict) or len(value) != 1:
        raise ValidationError(f'{label} must contain exactly one actor, house, super_weapon or gesture')
    if 'actor' in value:
        selector = value['actor']
        if isinstance(selector, dict) and 'stable_id' in selector:
            _object(selector, label + '.actor', {'stable_id'}, ('bind_step',))
            _integer(selector['stable_id'], label + '.actor.stable_id', 1, (1 << 64) - 1)
        else:
            selector = _object(selector, label + '.actor', {'type_id'}, ('owner', 'cell', 'bind_step'))
            _string(selector['type_id'], label + '.actor.type_id')
            if 'owner' in selector:
                _string(selector['owner'], label + '.actor.owner')
            if 'cell' in selector:
                cell = selector['cell']
                if not isinstance(cell, list) or len(cell) != 2:
                    raise ValidationError(f'{label}.actor.cell must contain two integer coordinates')
                for component in cell:
                    _integer(component, label + '.actor.cell', -(1 << 31), (1 << 31) - 1)
        if 'bind_step' in selector:
            _integer(selector['bind_step'], label + '.actor.bind_step', 0, MAX_STEP)
    elif 'house' in value:
        _string(value['house'], label + '.house')
    elif 'super_weapon' in value:
        selector = _object(value['super_weapon'], label + '.super_weapon', {'owner', 'type'})
        for key in selector:
            _string(selector[key], label + '.super_weapon.' + key)
    elif 'gesture' in value:
        _integer(value['gesture'], label + '.gesture', 0, (1 << 64) - 1)
    else:
        raise ValidationError(f'{label} must select actor, house, super_weapon or gesture')


def validate_expectations(document: Any) -> list[dict[str, Any]]:
    """Validate a strict, bounded expectation document; return independent rows.

    Invalid syntax raises the shared ValidationError. A valid query that asks for
    unavailable evidence is evaluated as UNOBSERVED, not a schema error.
    """
    document = _object(document, 'expectations', {'schema_version', 'assertions'})
    if document['schema_version'] != EXPECTATIONS_SCHEMA:
        raise ValidationError(f'expectations.schema_version must be {EXPECTATIONS_SCHEMA}')
    assertions = document['assertions']
    if not isinstance(assertions, list) or not 1 <= len(assertions) <= MAX_ASSERTIONS:
        raise ValidationError(f'expectations.assertions must contain 1..{MAX_ASSERTIONS} rows')
    names: set[str] = set()
    samples = 0
    for ordinal, assertion in enumerate(assertions):
        label = f'expectations.assertions[{ordinal}]'
        row = _object(assertion, label, {'name', 'subject', 'field', 'op', 'value'},
                      ('at_step', 'eventually', 'always'))
        name = _string(row['name'], label + '.name')
        if name in names:
            raise ValidationError(f'{label}.name is repeated')
        names.add(name)
        _subject(row['subject'], label + '.subject')
        _field(row['field'], label + '.field')
        if row['op'] not in ('eq', 'ne', 'lt', 'le', 'gt', 'ge'):
            raise ValidationError(f'{label}.op must be eq, ne, lt, le, gt or ge')
        _literal(row['value'], label + '.value', [256])
        if row['op'] not in ('eq', 'ne') and type(row['value']) not in (int, float):
            raise ValidationError(f'{label}.value must be numeric for an ordered comparison')
        windows = row.keys() & {'at_step', 'eventually', 'always'}
        if len(windows) != 1:
            raise ValidationError(f'{label} requires exactly one at_step, eventually or always window')
        mode = next(iter(windows))
        if mode == 'at_step':
            _integer(row[mode], label + '.at_step', 0, MAX_STEP)
            samples += 1
        else:
            window = _object(row[mode], label + '.' + mode, {'from_step', 'through_step'})
            start = _integer(window['from_step'], label + '.' + mode + '.from_step', 0, MAX_STEP)
            end = _integer(window['through_step'], label + '.' + mode + '.through_step', start, MAX_STEP)
            samples += end - start + 1
        if samples > MAX_ASSERTION_SAMPLES:
            raise ValidationError(f'expectations exceed {MAX_ASSERTION_SAMPLES} requested samples')
    return deepcopy(assertions)


def _read(row: Any, path: str) -> Any:
    for component in path.split('.'):
        if not isinstance(row, Mapping) or component not in row:
            return _ABSENT
        row = row[component]
    return row


def _observed(value: Any) -> dict[str, Any]:
    return {'observed': False} if value is _ABSENT else {'observed': True, 'value': deepcopy(value)}


def _equal(left: Any, right: Any) -> bool:
    if type(left) in (int, float) and type(right) in (int, float):
        return left == right
    if type(left) is not type(right):
        return False
    if isinstance(left, dict):
        return left.keys() == right.keys() and all(_equal(left[key], right[key]) for key in left)
    if isinstance(left, list):
        return len(left) == len(right) and all(_equal(a, b) for a, b in zip(left, right))
    return left == right


def _compare(actual: Any, operator: str, expected: Any) -> bool | None:
    if operator in ('eq', 'ne'):
        result = _equal(actual, expected)
        return result if operator == 'eq' else not result
    if type(actual) not in (int, float):
        return None
    if operator == 'lt':
        return actual < expected
    if operator == 'le':
        return actual <= expected
    if operator == 'gt':
        return actual > expected
    return actual >= expected


class _Index:
    def __init__(self, observations: Mapping[str, Any]):
        self.frames = {frame['completed_steps']: frame for frame in observations['frames']}
        self.actors = {step: {actor['stable_id']: actor for actor in frame['actors']}
                       for step, frame in self.frames.items()}
        self.missing = {step: set(frame['missing_actor_ids']) for step, frame in self.frames.items()}
        self.houses = {step: {house['owner']: house for house in frame.get('houses', [])}
                       for step, frame in self.frames.items()}
        self.restore_steps = [row['after_step'] for row in
                              observations.get('load_segments', {}).get('transitions', [])]
        self.gestures: dict[int, list[Mapping[str, Any]]] = {}
        for receipt in observations.get('gesture_input', {}).get('receipts', []):
            self.gestures.setdefault(receipt['ordinal'], []).append(receipt)

    def epoch(self, step: int) -> int:
        # sim/world/substrate.rs serializes the allocation counter with saves.
        # A boundary frame is retained before its quickload gesture; the next
        # frame belongs to the restored allocation history. Multiple restores
        # at one boundary still have no intervening retained actor sample.
        return bisect_left(self.restore_steps, step)

    def bind(self, subject: Mapping[str, Any]) -> tuple[dict[str, Any], str | None]:
        if 'gesture' in subject:
            matches = self.gestures.get(subject['gesture'], [])
            binding = dict(subject)
            if len(matches) != 1:
                return binding, 'ambiguous_gesture' if matches else 'gesture_not_observed'
            return {**binding, 'issue_after_step': matches[0]['issue_after_step']}, None
        if 'actor' not in subject:
            return dict(subject), None
        selector = subject['actor']
        step = selector.get('bind_step', 0)
        binding = {'bind_step': step, 'observation_epoch': self.epoch(step)}
        if step not in self.frames:
            return binding, 'binding_frame_not_observed'
        if 'stable_id' in selector:
            return {**binding, 'actor': selector['stable_id']}, None
        matches = [identity for identity, actor in self.actors[step].items()
                   if all(actor[key] == selector[key] for key in ('type_id', 'owner', 'cell') if key in selector)]
        binding['candidate_count'] = len(matches)
        if len(matches) != 1:
            binding['candidate_stable_ids'] = matches[:16]
            return binding, 'ambiguous_actor' if matches else 'actor_not_observed_at_binding'
        binding['actor'] = matches[0]
        return binding, None

    def value(self, step: int, binding: Mapping[str, Any], field: str) -> tuple[Any, str | None]:
        if step not in self.frames:
            return _ABSENT, 'frame_not_observed'
        if 'gesture' in binding:
            row = self.gestures[binding['gesture']][0]
            if step != row['issue_after_step']:
                return _ABSENT, 'gesture_not_observed_at_step'
        elif 'actor' in binding:
            if self.epoch(step) != binding['observation_epoch']:
                return _ABSENT, 'actor_identity_crosses_load'
            identity = binding['actor']
            row = self.actors[step].get(identity)
            if row is None:
                if identity in self.missing[step]:
                    return (False, None) if field == 'present' else (_ABSENT, 'actor_missing')
                return _ABSENT, 'actor_not_observed'
            if field == 'present':
                return True, None
        else:
            owner = binding['house'] if 'house' in binding else binding['super_weapon']['owner']
            row = self.houses[step].get(owner)
            if row is None:
                return _ABSENT, 'house_not_observed'
            if 'super_weapon' in binding:
                if 'super_weapons' not in row:
                    return _ABSENT, 'super_weapons_not_observed'
                matches = [weapon for weapon in row['super_weapons'] if weapon['type'] == binding['super_weapon']['type']]
                if len(matches) != 1:
                    return _ABSENT, 'ambiguous_super_weapon' if matches else 'super_weapon_not_observed'
                row = matches[0]
        value = _read(row, field)
        return value, 'field_not_observed' if value is _ABSENT else None


def _clock(frame: Mapping[str, Any]) -> dict[str, Any]:
    return {key: frame[key] for key in ('completed_steps', 'simulation_tick', 'binary_frame')}


def _assertions(index: _Index, assertions: Sequence[Mapping[str, Any]]) -> dict[str, Any]:
    results = []
    for assertion in assertions:
        mode = next(key for key in ('at_step', 'eventually', 'always') if key in assertion)
        start = assertion[mode] if mode == 'at_step' else assertion[mode]['from_step']
        end = start if mode == 'at_step' else assertion[mode]['through_step']
        binding, binding_error = index.bind(assertion['subject'])
        result = {**deepcopy(assertion), 'binding': binding, 'sample_count': end - start + 1,
                  'observed_count': 0, 'true_count': 0, 'false_count': 0, 'unobserved_count': 0,
                  'first_match': None, 'first_failure': None, 'first_unobserved': None}
        for step in range(start, end + 1):
            actual, error = (_ABSENT, binding_error) if binding_error else index.value(step, binding, assertion['field'])
            point = ({**_clock(index.frames[step]), 'observation_epoch': index.epoch(step)}
                     if step in index.frames else {'completed_steps': step})
            if not error:
                point['actual'] = deepcopy(actual)
                match = _compare(actual, assertion['op'], assertion['value'])
                if match is None:
                    error = 'comparison_requires_numeric_field'
            if error:
                result['unobserved_count'] += 1
                if result['first_unobserved'] is None:
                    result['first_unobserved'] = {**point, 'reason': error}
            else:
                result['observed_count'] += 1
                result['true_count' if match else 'false_count'] += 1
                first = 'first_match' if match else 'first_failure'
                if result[first] is None:
                    result[first] = point
        if mode == 'eventually' and result['true_count']:
            status = 'PASS'
        elif mode in ('always', 'at_step') and result['false_count']:
            status = 'FAIL'
        elif result['unobserved_count']:
            status = 'UNOBSERVED'
        else:
            status = 'PASS' if result['true_count'] else 'FAIL'
        result['status'] = status
        result['coverage_complete'] = result['unobserved_count'] == 0
        results.append(result)
    return {'status': ('NOT_REQUESTED' if not results else
                       'PASS' if all(row['status'] == 'PASS' for row in results) else 'FAIL'),
            'pass_count': sum(row['status'] == 'PASS' for row in results),
            'fail_count': sum(row['status'] == 'FAIL' for row in results),
            'unobserved_count': sum(row['status'] == 'UNOBSERVED' for row in results),
            'results': results}


def inspect_observations(observations: Mapping[str, Any], *, owners: Sequence[str] | None = None,
                         types: Sequence[str] | None = None, stable_ids: Sequence[int] | None = None,
                         fields: Sequence[str] | None = None, max_rows: int = 200,
                         expectations: Mapping[str, Any] | None = None) -> dict[str, Any]:
    """Return a bounded change timeline and independently evaluated assertions.

    The caller MUST validate the run first; this function does not load files or
    assert integrity. ``fields`` selects actor fields only. House economy and
    superweapon readiness/charge-start changes have fixed concise projections;
    assertions may address any retained object field. ``max_rows`` caps display,
    never assertion evaluation. Filters select initially observed identities and
    do not filter expectations; type/ID filters suppress House-only timeline rows.
    Each quickload starts a new identity epoch, including filter binding.
    """
    _integer(max_rows, 'max_rows', 0, MAX_ROWS)
    selected_fields = tuple(DEFAULT_ACTOR_FIELDS if fields is None else fields)
    if not 1 <= len(selected_fields) <= 32 or len(set(selected_fields)) != len(selected_fields):
        raise ValidationError('fields must contain 1..32 unique object-key paths')
    for field in selected_fields:
        _field(field, 'fields')
    owner_filter = set(owners or [])
    type_filter = set(types or [])
    id_filter = set(stable_ids or [])
    for name in owner_filter | type_filter:
        _string(name, 'filter')
    for identity in id_filter:
        _integer(identity, 'stable_ids', 1, (1 << 64) - 1)
    validated = validate_expectations(expectations) if expectations is not None else []
    index = _Index(observations)
    rows = []
    total_rows = 0

    def emit(frame, kind, event, subject, **detail):
        nonlocal total_rows
        total_rows += 1
        if len(rows) < max_rows:
            rows.append({**_clock(frame), 'observation_epoch': index.epoch(frame['completed_steps']),
                         'kind': kind, 'event': event,
                         'subject': deepcopy(subject), **deepcopy(detail)})

    def values(row, paths):
        return {path: _observed(_read(row, path)) for path in paths}

    def changes(before, after):
        return {key: {'before': before.get(key, {'observed': False}), 'after': value}
                for key, value in after.items() if not _equal(before.get(key), value)}

    seen: set[int] = set()
    selected: set[int] = set()
    selected_identities: set[tuple[int, int]] = set()
    previous_actors: dict[int, Any] = {}
    previous_houses: dict[str, Any] = {}
    previous_weapons: dict[tuple[str, str, int], Any] = {}
    commands: dict[int, list] = {}
    for command in observations['commands']:
        if not owner_filter or command['owner'] in owner_filter:
            commands.setdefault(command['issue_after_step'], []).append(command)
    transitions: dict[int, list] = {}
    for transition in observations.get('load_segments', {}).get('transitions', []):
        transitions.setdefault(transition['after_step'], []).append(transition)
    previous_epoch = 0
    for step, frame in index.frames.items():
        epoch = index.epoch(step)
        if epoch != previous_epoch:
            # Restoring also rewinds the object allocator. Neither an equal
            # numeric ID nor a matching type proves cross-load actor identity.
            seen.clear()
            selected.clear()
            previous_actors.clear()
            previous_houses.clear()
            previous_weapons.clear()
            previous_epoch = epoch
        for identity, actor in index.actors[step].items():
            if identity not in seen:
                seen.add(identity)
                if ((not owner_filter or actor['owner'] in owner_filter) and
                    (not type_filter or actor['type_id'] in type_filter) and
                    (not id_filter or identity in id_filter)):
                    selected.add(identity)
                    selected_identities.add((epoch, identity))
            if identity not in selected:
                continue
            subject = {key: actor[key] for key in ('stable_id', 'owner', 'type_id', 'category')}
            current = values(actor, selected_fields)
            if 'present' in selected_fields:
                current['present'] = _observed(True)
            if identity not in previous_actors:
                emit(frame, 'actor', 'added', subject, values=current)
            elif previous_actors[identity] is None:
                emit(frame, 'actor', 'reappeared', subject, values=current)
            elif delta := changes(previous_actors[identity], current):
                emit(frame, 'actor', 'changed', subject, changes=delta)
            previous_actors[identity] = current
        for identity in sorted(index.missing[step] & selected):
            if previous_actors.get(identity) is not None:
                emit(frame, 'actor', 'missing', {'stable_id': identity},
                     observation='absent_from_retained_actor_snapshot; cause_not_observed')
                previous_actors[identity] = None
        if not type_filter and not id_filter:
            for owner, house in index.houses[step].items():
                if owner_filter and owner not in owner_filter:
                    continue
                current = values(house, HOUSE_FIELDS)
                if owner not in previous_houses:
                    emit(frame, 'house', 'added', {'owner': owner}, values=current)
                elif delta := changes(previous_houses[owner], current):
                    emit(frame, 'house', 'changed', {'owner': owner}, changes=delta)
                previous_houses[owner] = current
                current_weapon_keys = set()
                for weapon in house.get('super_weapons', []):
                    key = (owner, weapon['type'], weapon['interned_id'])
                    current_weapon_keys.add(key)
                    subject = {'owner': owner, 'type': weapon['type'], 'interned_id': weapon['interned_id']}
                    current = values(weapon, SUPER_WEAPON_FIELDS)
                    if key not in previous_weapons:
                        emit(frame, 'super_weapon', 'added', subject, values=current)
                    elif delta := changes(previous_weapons[key], current):
                        emit(frame, 'super_weapon', 'changed', subject, changes=delta)
                    previous_weapons[key] = current
                for key in list(previous_weapons):
                    if key[0] == owner and key not in current_weapon_keys:
                        emit(frame, 'super_weapon', 'missing',
                             {'owner': owner, 'type': key[1], 'interned_id': key[2]},
                             observation='not_in_retained_super_weapon_list; cause_not_observed')
                        del previous_weapons[key]
        # Frame rows precede commands/gestures issued after this step, exactly as
        # the capture protocol specifies. No later outcome is invented here.
        for command in commands.get(step, []):
            emit(frame, 'command', 'queued', {'ordinal': command['ordinal'], 'owner': command['owner']},
                 receipt=command, outcome='not_observed')
        for transition in transitions.get(step, []):
            emit(frame, 'load', 'clock_restored', {'gesture_ordinal': transition['gesture_ordinal']},
                 before=transition['before'], after=transition['after'])
    steps = list(index.frames)
    return {'schema_version': INSPECTION_SCHEMA,
            'coverage': {'policy': observations['policy'], 'owners': list(observations['owners']),
                         'type_filter': deepcopy(observations.get('type_filter')),
                         'frame_count': len(steps), 'first_step': steps[0] if steps else None,
                         'last_step': steps[-1] if steps else None, 'time_axis': 'completed_steps',
                         'actor_identity': 'stable_id_within_load_epoch',
                         'scope': 'retained_samples_only', 'native_parity': 'NOT_ESTABLISHED',
                         'command_outcomes': 'NOT_OBSERVED'},
            'filters': {'owners': sorted(owner_filter), 'types': sorted(type_filter),
                        'stable_ids': sorted(id_filter), 'actor_fields': list(selected_fields),
                        'actor_binding': 'first_observed_identity_per_load_epoch', 'apply_to_assertions': False},
            'timeline': {'rows': rows, 'total_rows': total_rows, 'omitted_rows': total_rows - len(rows),
                         'truncated': total_rows > len(rows), 'selected_actor_count': len(selected_identities)},
            'assertions': _assertions(index, validated)}
