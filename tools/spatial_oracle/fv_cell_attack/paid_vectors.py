"""Literal, pointer-independent projection of the executed native FV continuation.

No native decision is computed. This selects recorded native values for Rust
integration consumers; the complete source packet retains raw object/Drive bytes.
"""
from pathlib import Path
import hashlib,json,struct
from tools.native_oracle import finish_vectors,provenance,_canonical
from tools.spatial_oracle.shrapnel_repair.packet_io import read_result
HERE=Path(__file__).resolve().parent
SOURCE=HERE/'paid_world_drive_crt.json.gz'
OUTPUT=HERE/'paid_drive_crt_vectors.json'
sha=lambda raw:hashlib.sha256(raw).hexdigest()

def rng(raw):
 b=bytes.fromhex(raw)
 return dict(disabled=b[0],padding_hex=b[1:4].hex(),index_a=struct.unpack_from('<i',b,4)[0],index_b=struct.unpack_from('<i',b,8)[0],words=list(struct.unpack_from('<250I',b,12)))

def actor(value):
 return {k:v for k,v in value.items()if k not in('target','frame')}

def cell(value):
 return {**{k:v for k,v in value.items()if k!='ground_head'},'has_ground_object':value['ground_head']!=0}

def project(packet):
 cases=[]
 for case in packet['cases']:
  r=case['result'];assert r['failure'] is None
  identity=r['lifecycle'][0]['after']['native_id'];relative=lambda v:v-identity
  lifecycle=[]
  for event in r['lifecycle']:
   row=dict(kind=event['kind'],pc=event['pc'],caller=event['caller'])
   if event['kind']=='unlimbo':row['returned_al']=event['returned_eax']&255
   for phase in('before','after'):
    s=event[phase];row[phase]=dict(actor=actor(s['actor']),native_id=s['native_id'],next_id=s['scenario_next_id'],rng={k:rng(v)for k,v in s['rng'].items()})
   lifecycle.append(row)
  frames=[]
  for index,s in enumerate(r['states']):
   frames.append(dict(completed_frame=s['actor']['frame']-1 if index else None,next_frame=s['actor']['frame'],actor=actor(s['actor']),target_present=int(s['actor']['target'],16)!=0,nav_cell=s['nav_cell'],drive_destination=s['drive_destination'],drive_head=s['drive_head'],next_id_relative=relative(s['scenario_next_id']),logic_ids_relative=[relative(x['native_id'])for x in s['logic_order']],bullets=[dict(id_relative=relative(x['native_id']),position=x['xyz'],alive=x['alive'])for x in s['all_bullets']],bullet_count=s['bullet_count'],anim_count=s['anim_count'],deferred_count=s['deferred_count'],target_cell=cell(s['target_cell']),source_cell=cell(s['source_cell']),rng_sha256={k:sha(bytes.fromhex(v))for k,v in s['rng'].items()}))
  order=[]
  retained={'logic_visit','unit_ai','foot_ai','techno_ai','dispatch','attack_handler','approach','drive_process','drive_paid_points','firing_update','unit_fire','techno_fire','bullet_ctor','next_identity','logic_register','bullet_ai','detonate','area_damage','concrete_damage','anim_ctor','anim_ai','notify_expiry','uninit','deferred_drain','bullet_dtor','anim_dtor','line_trail_ctor','line_trail_detach','facing_update','commence','ready_before_foot','ready_after_firing','alive_after_warp_drive','warp_guard_result','tube_guard_result','alive_after_foot','ready_before_foot_result','ready_after_firing_result','facing_update_return'}
  for e in r['timeline']:
   if e['kind']not in retained or e['phase']not in('logic','drain'):continue
   row={k:e[k]for k in('kind','frame','phase','pc','caller','source_xyz','source_alive','mission','queued')if k in e}
   # At intraprocedural markers ESP names locals, not a function return PC.
   # The raw packet retains that stack word; only true entry calls expose it
   # as a caller in this consumer projection.
   if e['kind']in{'logic_visit','ready_before_foot','ready_after_firing','alive_after_warp_drive','warp_guard_result','tube_guard_result','alive_after_foot','ready_before_foot_result','ready_after_firing_result','facing_update_return'}:row.pop('caller',None)
   if e['kind']=='logic_visit':row.update(index=e['index'],id_relative=relative(e['native_id']))
   if e['kind'].endswith('_result'):row['returned_al']=e['eax']&255
   order.append(row)
  shots=[{k:v for k,v in s.items()if k not in('bullet','rng_after')}for s in r['shots']]
  impacts=[{**{k:e[k]for k in('frame','position','damage','warhead')},'before':cell(e['before']),'after':cell(e['after'])}for e in r['impacts']]
  draws=[{k:v for k,v in e.items()if k not in('this',)}for e in r['events']if e['kind']in('rng','raw')]
  placements=[{k:e[k]for k in('kind','pc','return_pc','frame')if k in e}for e in r['events']if e.get('phase')=='placement']
  effects=[dict(frame=e['frame'],name=e['anim'],pc=e['pc'],caller=e['return_pc'])for e in r['events']if e['kind']=='anim_ctor']
  cases.append(dict(stage=case['stage'],supplied_spawn=next(e for e in r['extra']if e['kind']=='spawn_coordinate_input'),rng_initialization=r['inputs']['rng_initialization'],options=r['inputs']['options_constructor'],physical_navigation=r['navigation_setup'],lifecycle=lifecycle,placement_calls=placements,frames=frames,order=order,shots=shots,impacts=impacts,effects=effects,rng_events=draws,initial_span=[cell(x)for x in r['states'][0]['span']],final_span=[cell(x)for x in r['last']['span']],final_rng={k:rng(v)for k,v in r['last']['rng'].items()},final_detached_trail_owners=[x['owner']for x in r['last']['line_trails']]))
 identity=json.loads((HERE/'promotion.json').read_bytes())['results']
 result=dict(schema=1,source_file_sha256=identity['paid_world_drive_crt.json']['frozen_source_sha256'],cases=cases)
 return result

def generate():
 result=project(read_result(SOURCE))
 identity=json.loads((HERE/'promotion.json').read_bytes())['results']
 assert sha(_canonical(result))==identity['paid_drive_crt_vectors.json']['published_payload_sha256'],'Frozen native paid projection changed'
 return result

def metadata():
 p=provenance(scope=__doc__,assumptions=['Every projected field is read from the executed native source packet; addresses become source-relative native IDs only where the native packet already records IDs. All constructor, placement and per-frame RNG values are preserved as native structs or byte hashes. No gameplay or RNG algorithm is reproduced by the projection.'],substitutions=['This file performs JSON selection and raw little-endian struct decoding only.'],entry_points={'source_live_logic':0x55B5FF})
 p.update(source_sha256=sha(SOURCE.read_bytes()),source_metadata_sha256=sha(SOURCE.with_suffix('').with_suffix('.meta.json').read_bytes()))
 return p

if __name__=='__main__':finish_vectors(generate,OUTPUT,provenance=metadata)
