"""Rust-facing projection of recorded original Shrapnel occupant executions.

No gameplay decisions are recomputed. Ordered callbacks, receiver arguments,
timer stores and complete RNG words are projected from the hash-pinned packet.
"""
import json,struct
from pathlib import Path
from tools.native_oracle import _canonical,finish_vectors,provenance
from tools.spatial_oracle.shrapnel_repair import packet_io

HERE=Path(__file__).resolve().parent


def rng_state(raw):
    disabled,a,b,*state=struct.unpack('<B3xii250I',bytes.fromhex(raw))
    return dict(disabled=disabled,index_a=a,index_b=b,state=state)


def project_cases(source, additional_event_kinds=()):
    """Shared extraction for request-boundary and joined native executions."""
    initial=source['cases'][0]['result'][0]['rng_before']['main']
    rows=[]
    keep={'occupants','recalc','driver_return','detach_target','resident_cached_next',
          'neighbor_live_next','bridge_overlay_write','cell_land_write','mark',
          'remove_content','logic_remove','display_remove','foot_restore','mission_restore',
          'assign_target','assign_destination','rng_return','ranged_raw_draw','sound_boundary',
          'anim_constructor_boundary','drive_at_coord','drive_at_coord_return',
          'can_enter','can_enter_return'}
    keep.update(additional_event_kinds)
    for case in source['cases']:
        assert all(v==initial for v in case['result'][0]['rng_before'].values())
        stages=[]
        for stage in case['result']:
            names={}
            for event in stage['events']:
                if event['kind']!='can_enter':continue
                call=next(c for c in stage['calls']if c['kind']=='unit_can_enter_entry'and c['visit']==event['visit'])
                names[call['this']]=event['actor']
            packets=[]
            for call in stage['calls']:
                if call['kind']=='unit_damage':
                    packets.append({k:call[k]for k in ('visit','damage','distance','warhead',
                        'attacker','ignore_defenses','arg6','house','health_alias')}|dict(actor=names[call['this']]))
            timers=[w for w in stage['writes']if w['region']in('actor','actor2')and
                    (0x174<=int(w['offset'],16)<0x180 or 0x640<=int(w['offset'],16)<0x64C)]
            stages.append(dict(stage=stage['stage'],before=stage['before'],after=stage['after'],
                span=stage['span'],damage_packets=packets,timer_writes=timers,
                events=[event for event in stage['events']if event['kind']in keep],
                rng_after={name:rng_state(raw)for name,raw in stage['rng_after'].items()}))
        rows.append(dict(input=case['input'],stages=stages))
    return dict(rng_seed0=rng_state(initial),cases=rows)


def generate():
    source=packet_io.read_result(HERE/'occupants.json.gz')
    promotion=json.loads((HERE/'occupants_promotion.json').read_bytes())['results']['occupants.json']
    identity=packet_io.digest(_canonical(source))
    assert identity==promotion['published_payload_sha256']
    return dict(schema=1,
        published_native_payload_sha256=identity,scene=source['scene'],
        native_actor_inputs=source['cases'][0]['native_inputs'],
        **project_cases(source))


def metadata():
    return provenance(scope=__doc__,assumptions=[
        'Pure projection of the immutable original occupants.json.gz packet; every native callback keeps its original instruction visit ordinal. No newly executed native or Rust behavior is claimed by this projection.',
        'RNG seed0 is the original logical0x3F4 structure without three padding bytes; each stage retains all250 words and both indices for all three streams. Physical world/actor/head, sound and navigation boundaries remain occupants.meta.json.',
        'Actor+178 padding is preserved as a raw native write; semantic RadarCombatFlash fields are+174 start/+17C duration. Pointer argument fields are retained for provenance, not stable engine entity identifiers.',
    ],substitutions=[],entry_points={'occupants':0x487A10,'damage':0x737C90,'detach':0x70D4A0})|dict(
        source_files={name:packet_io.digest((HERE/name).read_bytes())for name in
            ('occupants.json.gz','occupants_promotion.json')})


if __name__=='__main__':
    finish_vectors(generate,HERE/'occupants_test_vectors.json',provenance=metadata)
