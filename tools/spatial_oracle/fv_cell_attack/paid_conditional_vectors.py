"""Literal native conditional-FV frame projection for production comparison."""
from pathlib import Path
import hashlib
import json
import struct
from tools.native_oracle import _canonical, finish_vectors, provenance
from tools.spatial_oracle.shrapnel_repair.packet_io import read_result

HERE = Path(__file__).resolve().parent
SOURCE = HERE / 'paid_conditional_v26.json.gz'
OUTPUT = HERE / 'paid_conditional_v26_vectors.json'
sha = lambda raw: hashlib.sha256(raw).hexdigest()


def cell(value):
    return {**{k:v for k,v in value.items() if k != 'ground_head'},
            'has_ground_object':value['ground_head'] != 0}


def bullet(value):
    raw = bytes.fromhex(value['raw'])
    return dict(native_id=value['native_id'], alive=value['alive'], position=value['xyz'],
        velocity=list(struct.unpack_from('<3Q', raw, 0xE8)),
        arm_timer=[struct.unpack_from('<i', raw, offset)[0] for offset in (0xC4, 0xCC)],
        course_locked=bool(raw[0x105]), course_frames=struct.unpack_from('<i', raw, 0x108)[0],
        closing_frames=struct.unpack_from('<i', raw, 0x118)[0],
        closing_accumulator_bits=struct.unpack_from('<Q', raw, 0x120)[0])


def anim(value):
    state = value['state']
    runtime = dict(state['runtime'])
    for key in ('constructor_reverse', 'first_ai_guard', 'inactive', 'paused'):
        runtime[key] = bool(runtime[key])
    return dict(native_id=state['native_id'], type_name=value['type_name'],
                alive=bool(state['alive']), position=state['location'], runtime=runtime)


def frame(native, index, identity):
    raw_actor = bytes.fromhex(native['actor_bytes'])
    raw_drive = bytes.fromhex(native['drive_bytes'])
    track_state = dict(
        movement_timer=[struct.unpack_from('<i', raw_actor, offset)[0] for offset in (0x640, 0x648)],
        blocked_timer=[struct.unpack_from('<i', raw_actor, offset)[0] for offset in (0x668, 0x670)],
        residual=struct.unpack_from('<i', raw_drive, 0x4C)[0],
        target_speed_bits=struct.unpack_from('<Q', raw_drive, 0x50)[0])
    passive_scan = dict(
        timer=[struct.unpack_from('<I', raw_actor, offset)[0] for offset in (0x180, 0x188)],
        last_frame=struct.unpack_from('<I', raw_actor, 0x4FC)[0],
        acquired=bool(raw_actor[0x50C]))
    actor = {k:v for k,v in native['actor'].items() if k not in ('frame','target')}
    mission = {k:actor[k] for k in ('mission','queued','status','mission_visit_count','dispatch','rearm')}
    return dict(completed_frame=native['actor']['frame'] - 1 if index else None,
        next_frame=native['actor']['frame'], actor=actor, mission=mission,
        native_id=identity, next_native_id=native['scenario_next_id'],
        game_speed=native['options']['game_speed'], passive_scan=passive_scan, track_state=track_state,
        nav_cell=native['nav_cell'], drive_destination=native['drive_destination'],
        drive_head=native['drive_head'], target_present=int(native['actor']['target'],16) != 0,
        bullets=[bullet(value) for value in native['all_bullets'] if value['alive']],
        anims=[anim(value) for value in native['anims']],
        bullet_count=native['bullet_count'], anim_count=native['anim_count'],
        deferred_count=native['deferred_count'],
        target_cell=cell(native['target_cell']), source_cell=cell(native['source_cell']),
        rng_sha256={k:sha(bytes.fromhex(value)) for k,value in native['rng'].items()})


def project(source):
    cases = []
    for case in source['cases']:
        frames = []
        identity = case['boundary']['source_native_id']
        for index, native in enumerate(case['states']):
            frames.append(frame(native, index, identity))
        cases.append(dict(stage=case['stage'], physical_stage=case.get('physical_stage', case['stage']),
            boundary=case['boundary'], seed_input=case.get('seed_input'),
            followup_commands=case.get('followup_commands', []),
            options=case['inputs']['options_constructor'],
            supplied_spawn=dict(xyz=frames[0]['actor']['position']), frames=frames,
            shots=[{k:v for k,v in shot.items() if k not in ('bullet','rng_after')} for shot in case['shots']],
            impacts=[dict(frame=row['frame'], position=row['position'], damage=row['damage'],
                          warhead=row['warhead'], before=cell(row['before']), after=cell(row['after']))
                     for row in case['impacts']],
            effects=[dict(frame=row['frame'], name=row['anim'], pc=row['pc'], caller=row['return_pc'])
                     for row in case['events'] if row['kind'] == 'anim_ctor'],
            logic_visits=[{k:row[k] for k in ('frame','index','native_id')}
                          for row in case['timeline'] if row['kind'] == 'logic_visit'],
            rng_passes=case['rng_passes'], all_selected_effects_drained=case['all_selected_effects_drained'],
            final_detached_trail_owners=[row['owner'] for row in case['states'][-1]['line_trails']],
            initial_span=[cell(row) for row in case['states'][0]['span']],
            final_span=[cell(row) for row in case['states'][-1]['span']]))
    frozen = json.loads((HERE / 'promotion.json').read_bytes())['results']
    result = dict(schema=1,
                  source_sha256=frozen['paid_conditional_v26.json']['frozen_source_sha256'],
                  input_sha256=source['input_sha256'], cases=cases)
    return result

def generate():
    result = project(read_result(SOURCE))
    frozen = json.loads((HERE / 'promotion.json').read_bytes())['results']
    assert sha(_canonical(result)) == frozen['paid_conditional_v26_vectors.json']['published_payload_sha256'], 'Frozen native conditional projection changed'
    return result


def metadata():
    result = provenance(scope=__doc__, entry_points={'source_live_logic':0x55B5FF},
        assumptions=['Every expected frame value comes from the executed native conditional packet, or a literal little-endian decode of its actor/Bullet/Anim bytes. Only live Bullets appear in the consumer frame list; the full source retains all constructed/retired bytes and full three-stream RNG.',
                     'The source input supplies declared constructor identity/options/RNG boundary and outside-world raw calls. This does not establish native whole-Scenario population or outside-producer scheduling parity.'],
        substitutions=['JSON selection and native raw-byte decoding only. No gameplay values are computed from Rust, and no native control flow is simulated here.'])
    result.update(source_sha256=sha(SOURCE.read_bytes()),
                  source_metadata_sha256=sha(SOURCE.with_suffix('').with_suffix('.meta.json').read_bytes()))
    return result


if __name__ == '__main__':
    finish_vectors(generate, OUTPUT, provenance=metadata)
