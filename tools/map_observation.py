"""Run and check a chosen-map production observation; never certify native parity.

The Rust map_observation profile and capture manifest own the runtime schema.
This wrapper binds their receipts to immutable inputs and actual frame bytes.
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path
import sys
from typing import Any, Mapping

from tools.cargo_run import resolve_binary, resolve_labeled_binary
from tools.child_process import run_child
from tools.tactical_certification.core import (
    FileSnapshot, ValidationError, assert_snapshot_unchanged,
    create_directory_exclusive, load_json_file, require_array, require_directory, require_int,
    require_object, require_regular_file, require_sha256, require_string,
    require_value, sha256_bytes, utc_now, write_bytes_exclusive, write_json_exclusive,
)
from tools.tactical_certification.profile import load_contract, reject_denied_environment

ROOT = Path(__file__).resolve().parents[1]
RUN_SCHEMA = 'vera20k.map-observation-run.v2'
LEGACY_RUN_SCHEMA = 'vera20k.map-observation-run.v1'
COPIES = {'profile': 'profile.json', 'config': 'config.toml', 'contract': 'contract.json'}


@dataclass(frozen=True)
class _Capture:
    evidence: dict[str, Any]
    manifest: FileSnapshot
    frame: FileSnapshot
    frame_format: str


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


def validate_capture(directory: Path, profile: Mapping[str, Any],
                     identities: Mapping[str, Mapping[str, Any]]) -> _Capture:
    """Check child semantics/bytes against independently checked input identities.

    Identities describe original runtime paths. Retained copies have their own real
    snapshots; offline callers check their bytes before passing these identities.
    """
    require_directory(directory, 'child output')
    manifest_snapshot, manifest = load_json_file(directory / 'capture.json', 'capture manifest')
    require_value(manifest.get('schema_version'), 'vera20k.map-observation.v2', 'schema_version')
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
    return _Capture(evidence, manifest_snapshot, frame_snapshot, frame['surface_format'])


def capture(*, profile_path: Path, contract_path: Path, output: Path,
            working_directory: Path, executable: Path | None = None,
            build_label: str | None = None) -> dict[str, Any]:
    profile_snapshot, profile = load_json_file(profile_path, 'map observation profile')
    require_value(profile.get('schema_version'), 'vera20k.map-observation-profile.v1',
                  'profile.schema_version')
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
    write_json_exclusive(run / 'run.json', report)
    return report


def _load_run(directory: Path, allow_legacy_inputs: bool) -> _CheckedRun:
    directory = require_directory(directory, 'observation run')
    run_snapshot, report = load_json_file(directory / 'run.json', 'observation run receipt')
    schema = report.get('schema_version')
    legacy = schema == LEGACY_RUN_SCHEMA
    if schema not in (RUN_SCHEMA, LEGACY_RUN_SCHEMA):
        raise ValidationError(f'unsupported observation wrapper schema: {schema!r}')
    if legacy and not allow_legacy_inputs:
        raise ValidationError('legacy run v1 has no sealed config/contract copies; '
                              'use --allow-legacy-inputs to revalidate the original files')
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
    require_value(profile.get('schema_version'), 'vera20k.map-observation-profile.v1',
                  'profile.schema_version')
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
    checked = validate_capture(directory / 'child-output', profile, identities)
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
                      input_provenance=provenance, capture=checked.evidence)
    result = _CheckedRun(validation, checked, inputs, tuple(snapshots))
    result.check_unchanged()
    return result


def _report(kind: str, status: str) -> dict[str, Any]:
    return {'schema_version': f'vera20k.map-observation-{kind}.v1', 'status': status,
            'checked_at_utc': utc_now(), 'errors': [],
            'native_comparator': 'NONE', 'parity_certification': 'NONE'}


def validate_run(directory: Path, *, allow_legacy_inputs: bool = False) -> dict[str, Any]:
    """Recheck retained bytes and the original executable, never trust a VALID label."""
    try:
        return _load_run(directory, allow_legacy_inputs).report
    except (OSError, ValueError) as exc:
        report = _report('validation', 'INVALID')
        report.update(run_path=str(directory), errors=[str(exc)])
        return report


def compare_runs(before: Path, after: Path, *,
                 allow_legacy_inputs: bool = False) -> dict[str, Any]:
    """Compare two checked production observations; MATCH is not native parity."""
    report = _report('comparison', 'INVALID')
    report.update(before_path=str(before), after_path=str(after), differences=[])
    try:
        left_dir = require_directory(before, 'before run')
        right_dir = require_directory(after, 'after run')
        if left_dir.samefile(right_dir):
            raise ValidationError('before and after must be distinct observation directories')
        left = _load_run(left_dir, allow_legacy_inputs)
        right = _load_run(right_dir, allow_legacy_inputs)
        report.update(before=left.report, after=right.report)
        # Compare actual inputs, not just their declared digest strings. Different
        # binaries are intentional; their original bytes were checked above.
        for name in COPIES:
            if left.inputs[name].raw != right.inputs[name].raw:
                raise ValidationError(f'{name} input bytes differ; observations are not comparable')
        compared_fields = ('initial', 'final', 'map_source', 'exact_step_count', 'unit_atlas')
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
                report = validate_run(args.run, allow_legacy_inputs=args.allow_legacy_inputs)
            else:
                report = compare_runs(args.before, args.after,
                                      allow_legacy_inputs=args.allow_legacy_inputs)
            result_path = write_json_exclusive(args.output, report)
            status = {'VALID': 0, 'MATCH': 0, 'MISMATCH': 1, 'INVALID': 2}[report['status']]
    except (OSError, ValueError) as exc:
        print(f'map observation: {exc}', file=sys.stderr)
        return 2
    print(f'{report["status"]}: {result_path}')
    return status


if __name__ == '__main__':
    raise SystemExit(main())
