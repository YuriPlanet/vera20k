"""Replay original stock human GAPILE construction and two-E1 output.

One public caller/comparison owner; Factory arithmetic, type/asset readers,
terrain, radio, Walk and runtime fixtures retain their existing native helpers.
See _factory_infantry_output/README.md for physical inputs and schedule limits.
"""
from pathlib import Path
import argparse
import json
import sys

from tools.spatial_oracle._factory_infantry_output import runtime as rt


def check(assets=None):
    from tools.spatial_oracle._factory_infantry_output.saved import check as saved_check
    if assets is not None:
        rt.configure_assets(assets)
    result = saved_check()
    from tools.spatial_oracle._factory_infantry_output.consumer_runtime import check_saved
    result['unit_ready_consumers'] = check_saved()
    from tools.spatial_oracle._factory_infantry_output.gate_runtime import check_saved as gate_check
    result['infantry_unlimbo_gate'] = gate_check()
    from tools.spatial_oracle._factory_infantry_output.saved import publication_phase_check
    result['publication_phase'] = publication_phase_check()
    return result


def _verify_loaded_helpers(checked):
    """Reuse the Gate census over its independently checked shared profiles."""
    from tools.spatial_oracle._factory_infantry_output import gate_runtime as gate
    profile = gate.verify_source()
    rt.require(profile == checked['infantry_unlimbo_gate']['gate_helpers'] and
               profile['shared_factory_profile'] == checked['shared_helpers'],
               'Loaded Gate/factory helper identities changed during replay')
    gate.census(profile)


def _replay_historical(control, assets=None):
    rt.require(sys.flags.optimize == 0,
        'Whole original emulation requires normal Python; shared input/runtime helpers retain assert guards. Use -O only with --check.')
    root = rt.configure_assets(assets)
    checked = check()
    import unicorn
    rt.require(unicorn.__version__ == rt.metadata()['native_runtime']['unicorn'],
        'Unsupported Unicorn version: ' + unicorn.__version__)
    from tools.spatial_oracle._factory_infantry_output import idle, fixture, projection
    primaries = []
    observer = idle.generate(control == 'rally', control, primaries.append)
    try:
        rt.require(observer['fault'] is None, 'Original emulation/observation failed: ' + str(observer['fault']))
        rt.require(len(primaries) == 1, 'Original primary boundary missing')
        primary = primaries[0]
        identity = rt.metadata()['controls'][control]
        rt.require(rt.canonical_sha(rt.normalize(observer)) == identity['native_private_full_sha256'],
            'Complete original GI private observation differs')
        selected = projection.projection(observer, primary)
        golden = rt.read_pinned('accepted/' + control + '.private.json.gz')
        from tools import native_oracle as native
        difference = native.first_difference(rt.normalize(golden), rt.normalize(selected))
        rt.require(difference is None, 'Selected original GI observation differs: ' + str(difference))
        actual_fixture = None
        if control == 'no_rally':
            accepted_fixture = rt.read_pinned('fixtures/ordinary-input-closure.json.gz')
            actual_fixture = fixture.generate(primary, observer, accepted_fixture['source_receipts'])
            difference = native.first_difference(rt.normalize(accepted_fixture), rt.normalize(actual_fixture))
            rt.require(difference is None, 'Native-derived milestone fixture differs: ' + str(difference))
        _verify_loaded_helpers(checked)
        return dict(schema_version=1, status='PASS', control='historical_'+control,
            scope='Historical supplied-prior control with omitted Walk CRT startup',
            native_sha256=rt.metadata()['native_sha256'],
            shared_helpers=checked['shared_helpers'], physical_input_root=str(root),
            complete_primary_native_comparison=observer['primary_comparison'],
            complete_gi_private_observation_equal=True, selected_gi_projection_equal=True,
            native_milestone_fixture_equal=actual_fixture is not None,
            full_original_primary=primary, full_original_gi_observer=observer,
            selected_native_projection=selected, native_milestone_fixture=actual_fixture,
            whole_object_completion_claimed=False,
            limits=rt.read_pinned('fixtures/ordinary-input-closure.json.gz')['limits'])
    except Exception as exc:
        return dict(schema_version=1, status='FAIL', control=control,
            native_sha256=rt.metadata()['native_sha256'],
            shared_helpers=checked['shared_helpers'], physical_input_root=str(root),
            failure=dict(type=type(exc).__name__, message=str(exc)),
            full_original_primary=primaries[0] if primaries else None,
            full_original_gi_observer=observer, whole_object_completion_claimed=False)


def _replay_publication_phase(order, assets=None):
    rt.require(sys.flags.optimize == 0, 'Original publication emulation requires normal Python; use -O only with --check.')
    root = rt.configure_assets(assets)
    checked = check()
    import unicorn
    rt.require(unicorn.__version__ == rt.metadata()['native_runtime']['unicorn'],
        'Unsupported Unicorn version: ' + unicorn.__version__)
    from tools.spatial_oracle._factory_infantry_output import phase, saved, fixture
    original = phase.generate(order, root)
    try:
        comparison = saved.compare_publication_phase(order, original)
        meta = rt.metadata()['publication_phase']
        selected = fixture.publication_phase_local_fixture({order: original}, meta['local_sources'],
                                                           meta['parent_manifest_sha256'])
        expected = json.loads((rt.REPO_ROOT / meta['rust_fixture']['path']).read_bytes())
        rt.require(selected['cases'][order] == expected['cases'][order],
            'Mechanically selected original publication route differs')
        _verify_loaded_helpers(checked)
        return dict(schema_version=1, status='PASS', control='publication_' + order,
            native_sha256=rt.metadata()['native_sha256'], shared_helpers=checked['shared_helpers'],
            physical_input_root=str(root), comparison=comparison,
            full_original_publication_control=original, selected_native_projection=selected,
            window_input_edge='Instruction-established only', whole_main_tick_parity_claimed=False,
            whole_object_completion_claimed=False, limits=meta['bounds'])
    except Exception as exc:
        return dict(schema_version=1, status='FAIL', control='publication_' + order,
            native_sha256=rt.metadata()['native_sha256'], shared_helpers=checked['shared_helpers'],
            physical_input_root=str(root), failure=dict(type=type(exc).__name__, message=str(exc)),
            full_original_publication_control=original, whole_main_tick_parity_claimed=False,
            whole_object_completion_claimed=False)


def replay(control, assets=None):
    if control.startswith('publication_'):
        return _replay_publication_phase(control.removeprefix('publication_'), assets)
    if control.startswith('historical_'):
        return _replay_historical(control.removeprefix('historical_'), assets)
    rt.require(sys.flags.optimize == 0,
        'Whole original emulation requires normal Python; use -O only with --check.')
    root = rt.configure_assets(assets)
    checked = check()
    import unicorn
    rt.require(unicorn.__version__ == rt.metadata()['native_runtime']['unicorn'],
        'Unsupported Unicorn version: ' + unicorn.__version__)
    from tools import native_oracle as native
    from tools.spatial_oracle._factory_infantry_output import initialized, fixture
    original = initialized.generate(control)
    try:
        rt.require(original['status'] == 'PASS', 'Initialized original emulation failed: ' + str(original['failure']))
        identity = rt.metadata()['initialized_controls'][control]
        primary, private = original['full_original_primary'], original['full_original_gi_observer']
        rt.require(rt.canonical_sha(rt.normalize(primary)) == identity['complete_primary_sha256'],
            'Complete initialized original primary differs')
        rt.require(rt.canonical_sha(rt.normalize(private)) == identity['complete_private_sha256'],
            'Complete initialized original GI observation differs')
        selected = fixture.initialized_projection(control, original)
        golden = rt.read_pinned(identity['projection'])
        difference = native.first_difference(rt.normalize(golden), rt.normalize(selected))
        rt.require(difference is None, 'Initialized native projection differs: ' + str(difference))
        _verify_loaded_helpers(checked)
        return dict(schema_version=2, status='PASS', control=control,
            scope='Original registered selected startup and two terminal native Infantry products',
            native_sha256=rt.metadata()['native_sha256'], shared_helpers=checked['shared_helpers'],
            physical_input_root=str(root), complete_primary_native_equal=True,
            complete_gi_private_equal=True, compact_native_projection_equal=True,
            complete_primary_native_sha256=identity['complete_primary_sha256'],
            complete_private_native_sha256=identity['complete_private_sha256'],
            final_frame=selected['measured_frames']['final'],
            full_original_initialized_control=original,
            selected_native_projection=selected, whole_object_completion_claimed=False,
            limits=rt.metadata()['initialized_limits'])
    except Exception as exc:
        return dict(schema_version=2, status='FAIL', control=control,
            native_sha256=rt.metadata()['native_sha256'], shared_helpers=checked['shared_helpers'],
            physical_input_root=str(root), failure=dict(type=type(exc).__name__, message=str(exc)),
            full_original_initialized_control=original, whole_object_completion_claimed=False)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    operation = parser.add_mutually_exclusive_group(required=True)
    operation.add_argument('--check', action='store_true', help='Saved native evidence and original byte identities; no emulation')
    operation.add_argument('--consumer-check', action='store_true', help='Saved registered UnitReady exports and original identities; no emulation')
    operation.add_argument('--gate-check', action='store_true', help='Saved original Infantry Unlimbo Gate/coordinate controls and fixture; no emulation')
    operation.add_argument('--phase-check', action='store_true', help='Saved original before/after Strip input interleavings and fixture; no emulation')
    operation.add_argument('--replay', choices=('no_rally', 'rally', 'historical_no_rally', 'historical_rally', 'unit_ready_consumer', 'infantry_unlimbo_gate', 'publication_before_strip', 'publication_after_strip'),
        help='Initialized original terminal controls, or explicitly historical missing-startup controls')
    parser.add_argument('--assets', type=Path, help='Existing exact physical-input root, or VERA20K_FACTORY_INFANTRY_OUTPUT_ASSETS')
    parser.add_argument('--output', type=Path, help='New receipt; .gz uses lossless deterministic gzip')
    parser.add_argument('--consumer-control', choices=('cadence','device','buffer','radar','all'), default='all')
    parser.add_argument('--consumer-assets', type=Path, help='Separate exact41-entry AnyTown-alias physical consumer input root')
    parser.add_argument('--wave', type=Path, help='Existing exact winning ceva062.wav; no retail files are written')
    parser.add_argument('--candidate-helper-profile', action='store_true',
        help='Explicit unregistered consumer candidate comparison; does not register helper compatibility')
    parser.add_argument('--candidate-gate-helper-profile', action='store_true',
        help='Explicit unregistered Gate caller comparison; does not register helper compatibility')
    args = parser.parse_args()
    rt.require(not args.output or not args.output.exists(), 'Refuse to overwrite saved evidence')
    rt.require(args.check or args.consumer_check or args.gate_check or args.phase_check or args.output is not None, '--replay requires --output to preserve the full result')
    rt.require(not args.candidate_gate_helper_profile or args.gate_check or args.replay == 'infantry_unlimbo_gate',
        'Gate candidate flag is restricted to Gate comparisons')
    if args.replay:
        # All branches replay this packet's historical file identity. In
        # particular the direct consumer/gate branches do not call saved.check.
        # Keep frozen callers intact; admitting a shared-loader build does not
        # authorize relabeling these exact historical receipts.
        from tools import native_oracle as native
        rt.require(native.image_sha256() == rt.metadata()['native_sha256'],
                   'Historical factory replay requires its recorded executable identity')
    if args.phase_check:
        rt.require(not args.candidate_helper_profile, 'Consumer candidate flag is restricted to consumer comparisons')
        from tools.spatial_oracle._factory_infantry_output.saved import publication_phase_check
        if args.assets is not None:
            rt.configure_assets(args.assets)
        result = publication_phase_check()
    elif args.gate_check or args.replay == 'infantry_unlimbo_gate':
        rt.require(not args.candidate_helper_profile, 'Consumer candidate flag is restricted to consumer comparisons')
        from tools.spatial_oracle._factory_infantry_output import gate_runtime as gate
        result = gate.check_saved(candidate=args.candidate_gate_helper_profile) if args.gate_check else \
            gate.replay(candidate=args.candidate_gate_helper_profile)
    elif args.consumer_check or args.replay == 'unit_ready_consumer':
        from tools.spatial_oracle._factory_infantry_output import consumer_runtime as consumer
        if args.consumer_check:
            result = consumer.check_saved(candidate=args.candidate_helper_profile)
        else:
            controls = ('cadence','device','buffer','radar') if args.consumer_control == 'all' else (args.consumer_control,)
            rows = {name: consumer.replay(name,args.consumer_assets,args.wave,candidate=args.candidate_helper_profile) for name in controls}
            result = dict(schema=1,status='PASS' if all(row['status']=='PASS' for row in rows.values()) else 'FAIL',
                control='unit_ready_consumer',controls=rows,whole_object_completion_claimed=False)
    else:
        rt.require(not args.candidate_helper_profile, 'Candidate flag is restricted to consumer comparisons')
        result = check(args.assets) if args.check else replay(args.replay, args.assets)
    if args.output:
        rt.write_new(args.output, result)
    if args.check or args.consumer_check or args.gate_check or args.phase_check:
        summary = result
    else:
        summary = {key: value for key, value in result.items() if key not in
            ('full_original_primary', 'full_original_gi_observer', 'selected_native_projection',
             'native_milestone_fixture', 'full_original_initialized_control', 'full_original_gate_control',
             'full_original_publication_control', 'limits')}
        summary['output'] = str(args.output)
        if result.get('control') == 'unit_ready_consumer':
            summary['controls'] = {name: {key:value for key,value in row.items() if key not in
                ('full_original_consumer','full_original_pcm_witness','partial_original_pcm_observation')}
                                   for name,row in result['controls'].items()}
    print(json.dumps(summary, indent=2))
    if result['status'] != 'PASS':
        raise SystemExit(1)


if __name__ == '__main__':
    main()
