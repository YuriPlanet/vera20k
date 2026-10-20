"""Extract original Approach observations for production Rust comparisons.

This projection executes no game code and computes no expected gameplay result.
It copies fields from the independently executed native packets and preserves
both complete RNG states and the numeric candidate-geometry observations.
"""
from pathlib import Path

from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.fv_cell_attack.publication import HERE
from tools.spatial_oracle.shrapnel_repair.packet_io import digest, read_result

GROUPS = ('candidates', 'admission', 'queued')
SOURCES = {group: HERE / f'{group}.json.gz' for group in GROUPS}


def project_row(row, group):
    before, after = row['before'], row['after']
    approach = [
        event for event in row['flow'] if event['kind'] == 'unit_approach'
    ]
    assert len(approach) == 1
    approach_returned_eax = approach[0]['returned_eax']
    is_mission_dispatch = group == 'candidates'
    if not is_mission_dispatch:
        assert row['returned'] == approach_returned_eax

    def mission_state(snapshot):
        actor = snapshot['actor']
        return {
            key: actor[key]
            for key in ('mission', 'queued', 'status', 'mission_visit_count',
                        'dispatch', 'rearm')
        }

    before_target = before['actor']['target']
    after_target = after['actor']['target']
    result = dict(
        input=row['row'],
        source_xyz=row['source_xyz'],
        before=before['actor'],
        after=after['actor'],
        before_mission_state=mission_state(before),
        mission_state=mission_state(after),
        before_target_raw=before_target,
        target_raw=after_target,
        before_target_present=int(before_target, 16) != 0,
        target_present=int(after_target, 16) != 0,
        target_unchanged=after_target == before_target,
        before_nav_cell=before['nav_cell'],
        nav_cell=after['nav_cell'],
        before_drive_destination=before['drive_destination'],
        drive_destination=after['drive_destination'],
        before_drive_head=before['drive_head'],
        drive_head=after['drive_head'],
        before_movement_timer=before['movement_timer'],
        movement_timer=after['movement_timer'],
        before_blockage_timer=before['blockage_timer'],
        blockage_timer=after['blockage_timer'],
        rng_before=before['rng'],
        rng_after=after['rng'],
        entry_point='mission_dispatch' if is_mission_dispatch else 'unit_approach',
        entry_returned_eax=row['returned'],
        approach_returned_eax=approach_returned_eax,
        candidate_trace=row['candidates'],
        destination_calls=[
            event for event in row['flow']
            if event['kind'] in ('unit_destination', 'foot_destination', 'drive_destination')
        ],
    )
    if is_mission_dispatch:
        result['mission_dispatch_returned_eax'] = row['returned']
    for key in ('target_cell', 'nav_queue_cells'):
        if key in after:
            result[f'before_{key}'] = before[key]
            result[key] = after[key]
    for key in ('type_reader_before', 'type_reader_after', 'actual_target_486900',
                'approach_reset_multiplier', 'fpcw'):
        if key in row:
            result[key] = row[key]
    return result


def generate():
    packets = {group: read_result(source) for group, source in SOURCES.items()}
    native_sha256 = packets['candidates']['native_sha256']
    result = dict(
        native_sha256=native_sha256,
        sources={source.name: digest(source.read_bytes()) for source in SOURCES.values()},
        numeric=packets['candidates']['numeric'],
        original_constants=packets['candidates']['original_constants'],
    )
    for group, packet in packets.items():
        result[group] = [project_row(row, group) for row in packet['cases']]
    return result


def metadata():
    result = provenance(
        scope=__doc__,
        assumptions=[
            'Projection only: inputs, state, timers, full RNG states, callback '
            'arguments and geometry outputs are copied from separately executed '
            'original packets. No Rust output or calculated gameplay outcome '
            'contributes. The source packet limitations remain applicable.',
            'candidates contains full MissionDispatch controls; admission and '
            'queued contain complete UnitApproach controls at their declared '
            'input boundary. entry_returned_eax belongs to that declared '
            'entry point; approach_returned_eax is copied from the unique '
            'original UnitApproach flow record. The candidate entry return '
            'is MissionDispatch delay, not a returned Cell pointer.',
            'Mission, rearm and target pointer observations come from the '
            'original before/after snapshots. Target presence and pointer '
            'equality are projections only. Candidate rows did not capture '
            'target Cell coordinates; no coordinates are inferred from their '
            'input or chosen destination. Direct Approach can be compared '
            'with complete dispatch rows only for Approach-owned outputs: '
            'the mission cadence timer and Scenario RNG suffix occur later.',
            'Numeric candidate geometry is copied without recomputation from '
            'the original instruction-fragment controls; it does not claim '
            'source placement, map admission or complete Approach for those poses.',
        ],
        substitutions=[],
        entry_points={'projected_original_approach': 0x4D5690},
    )
    result['sources'] = {
        source.name: digest(source.read_bytes()) for source in SOURCES.values()
    }
    result['harness_sha256'] = digest(Path(__file__).read_bytes())
    return result


if __name__ == '__main__':
    finish_vectors(generate, HERE / 'approach_vectors.json', provenance=metadata)
