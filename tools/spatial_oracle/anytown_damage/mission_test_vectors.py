"""Small Rust-facing projection of hash-pinned original mission executions.

This reads recorded native observations; it does not implement mission timing,
weapon arithmetic, RNG or bridge state transitions a second time.
"""
import json
from pathlib import Path

from tools.native_oracle import _canonical, finish_vectors, provenance
from tools.spatial_oracle.shrapnel_repair import packet_io

HERE = Path(__file__).resolve().parent
ACTOR_FIELDS = ('frame', 'mission', 'queued', 'mission_visit_count', 'dispatch',
                'rearm', 'position', 'health', 'primary_facing', 'turret_facing')


def actor(row):
    return {key: row[key] for key in ACTOR_FIELDS}


def generate():
    source = packet_io.read_result(HERE / 'mission.json.gz')
    identities = json.loads((HERE / 'mission_promotion.json').read_bytes())
    identity = identities['results']['mission.json']
    assert packet_io.digest(_canonical(source)) == identity['published_payload_sha256']
    counter = json.loads((HERE / 'mission_counter.json').read_bytes())
    assert packet_io.digest(_canonical(counter['comparison'])) == identities['counter']['comparison_projection_sha256']
    bridge_rolls = {e['frame']: e['returned_eax'] for e in source['events']
                    if e['kind'] == 'rng' and e['return_pc'] == '0x48a2a4'}
    impacts = [dict(frame=r['frame'], damage=r['damage'], position=r['position'],
                    roll=bridge_rolls[r['frame']], overlay_before=r['before']['overlay'],
                    overlay_after=r['after']['overlay'], land_after=r['after']['land'])
               for r in source['impacts']]
    visits = [dict(frame=e['frame'], delay=e['returned_eax']) for e in source['events']
              if e['kind'] == 'attack_mission']
    first_guard = next(r['state'] for r in source['frames']
                       if r['state']['frame'] > source['collapse_frame']
                       and r['state']['mission'] == 5)
    return dict(schema=1,
                original_native_payload_sha256=identity['original_payload_sha256'],
                published_native_payload_sha256=identity['published_payload_sha256'],
                seed0=dict(constructor_phase_raw_u16=source['after_ctor']['random_phase_raw_u16'],
                           after_command=actor(source['after_command']),
                           first_tick=actor(source['frames'][0]['state']),
                           shots=[{key: r[key] for key in ('frame', 'position', 'rearm', 'damage', 'velocity')}
                                  for r in source['shots']],
                           impacts=impacts, attack_visits=visits,
                           first_guard_after_collapse=actor(first_guard),
                           rng_after_command=source['rng_after_command'],
                           rng_final=source['rng_final']),
                supplied_v7=dict(boundary=actor(counter['comparison']['boundary']['native']),
                                 first_tick=actor(counter['comparison']['first_tick']['native']),
                                 promotion_order=counter['comparison']['first_tick']['promotion_order']))


def metadata():
    return provenance(scope=__doc__, assumptions=[
        'Pure projection of hash-pinned mission.json.gz and mission_counter.json. Seed0 results come from actual source constructor/Unlimbo/Event/native live Logic execution, including its constructor draw. Supplied-v7 counter data comes from the explicit retained production-input experiment; the input provenance is not whole-world native-load proof.',
        'Seed0 first_tick is recorded inside Logic before frame commit; supplied-v7 first_tick is the post-commit actor boundary. All shot/impact/Attack visit frames are Logic frames. first_guard_after_collapse is the same Logic pass whose second Commence promotes Guard.',
        'No new native execution or new Rust/production validation occurs in this projection. Native mechanism boundaries remain mission.meta.json and mission_counter.meta.json.'
    ], substitutions=[], entry_points={'unit_ai': 0x7360C0, 'commence': 0x5B3570,
                                      'techno_counter': 0x6FA646, 'attack': 0x4D4DC0}) | dict(
        source_files={name: packet_io.digest((HERE / name).read_bytes())
                      for name in ('mission.json.gz', 'mission_counter.json')})


if __name__ == '__main__':
    finish_vectors(generate, HERE / 'mission_test_vectors.json', provenance=metadata)
