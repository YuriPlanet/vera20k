"""Literal native CABHUT composition projection; contains no gameplay algorithm."""
from pathlib import Path
import json,struct
from tools.native_oracle import finish_vectors,_canonical
from tools.spatial_oracle.shrapnel_repair.packet_io import read_result
from tools.spatial_oracle.fv_cell_attack.paid_vectors import actor,cell,rng,sha

HERE=Path(__file__).resolve().parent
SOURCE=HERE/'hut_joined_drive_crt.json.gz'
OUTPUT=HERE/'hut_drive_crt_vectors.json'

def project(packet):
 assert packet['failure']is None
 identities={x['ptr']:x['native_id']for s in packet['states']for x in s['logic_order']}
 def hut(row):return{k:v for k,v in row.items()if k not in('ptr','raw')}
 def boundary(row):
  return dict(huts=[hut(x)for x in row['huts']],hut_bytes_sha256=[sha(bytes.fromhex(x['raw']))for x in row['huts']],source_target_present=int(row['source_target'],16)!=0,rng={k:rng(v)for k,v in row['rng'].items()},counts={k:v for k,v in row.items()if k.endswith('_count')})
 def snapshot(row):
  return dict(next_frame=row['actor']['frame'],actor=actor(row['actor']),native_id=struct.unpack_from('<I',bytes.fromhex(row['actor_bytes']),0x10)[0],target_present=int(row['actor']['target'],16)!=0,nav_cell=row['nav_cell'],drive_destination=row['drive_destination'],drive_head=row['drive_head'],next_id=row['scenario_next_id'],logic_ids=[x['native_id']for x in row['logic_order']],huts=[hut(x)for x in row['huts']],hut_bytes_sha256=[sha(bytes.fromhex(x['raw']))for x in row['huts']],bullets=[dict(native_id=x['native_id'],xyz=x['xyz'],alive=x['alive'])for x in row['all_bullets']],bullet_count=row['bullet_count'],anim_count=row['anim_count'],deferred_count=row['deferred_count'],target_cell=cell(row['target_cell']),rng_sha256={k:sha(bytes.fromhex(v))for k,v in row['rng'].items()})
 callbacks=[]
 for e in packet['hut_events']:
  if e['kind']not in('notify_span','detach'):continue
  callbacks.append({**{k:e[k]for k in('kind','pc','caller','frame','phase')},'before':boundary(e['before']),'after':boundary(e['after'])})
 detach=[{k:v for k,v in e.items()if k not in('actor','target','tarcom')}|{'target_present':int(e['tarcom'],16)!=0,'target_matches':e['tarcom']==e['target']}for e in packet['hut_events']if e['kind'].startswith('detach_')]
 steps=[dict(name=e['name'],entry=e['entry'],eax=e['eax'],rng_before={k:rng(v)for k,v in e['rng_before'].items()},rng_after={k:rng(v)for k,v in e['rng_after'].items()})for e in packet['steps']]
 shots=[{**{k:v for k,v in e.items()if k not in('bullet','rng_after')},'native_id':identities[e['bullet'].removeprefix('0x')]}for e in packet['shots']]
 effects=[dict(frame=e['frame'],name=e['anim'],pc=e['pc'],caller=e['return_pc'],native_id=identities[e['this'].removeprefix('0x')])for e in packet['events']if e['kind']=='anim_ctor']
 draws=[{k:v for k,v in e.items()if k not in('this','logic_owner')}for e in packet['events']if e['kind']in('rng','raw')]
 repair={name:dict(snapshot=snapshot(s),span=[cell(x)for x in s['span']],rng={k:rng(v)for k,v in s['rng'].items()})for name,s in packet['repair'].items()}
 return dict(schema=1,boundary='Two actual physical CABHUT rows constructed after FV command at frame1; original live Logic, collapse callbacks, then admitted573540 repair. Full Scenario population/order and audio remain excluded.',structures=packet['inputs']['hut_structures'],csf_input=packet['inputs']['hut_csf_cache'],type_native_id=struct.unpack_from('<I',bytes.fromhex(packet['hut_type']),0x10)[0],neutral_house_native_id=struct.unpack_from('<I',bytes.fromhex(packet['house_bytes']),0x10)[0],before_join=snapshot(packet['before_join']),frames=[snapshot(s)for s in packet['states']],steps=steps,callbacks=callbacks,detach_order=detach,shots=shots,impacts=[{**{k:e[k]for k in('frame','position','damage','warhead')},'before':cell(e['before']),'after':cell(e['after'])}for e in packet['impacts']],effects=effects,rng_events=draws,repair=repair,initial_span=[cell(x)for x in packet['states'][0]['span']],initial_span_tags=packet['states'][0]['span_tags'],final_rng={k:rng(v)for k,v in packet['states'][-1]['rng'].items()},building_ai_visits=[{k:e[k]for k in('pc','frame','phase')}|{'native_id':identities[e['this']]}for e in packet['hut_events']if e['kind']=='building_ai'],tag_event_count=sum(e['kind']=='tag_event'for e in packet['hut_events']),hut_controller_count=sum(e['kind']in('hut_high_controller','hut_low_controller')for e in packet['hut_events']))

def generate():
 result=project(read_result(SOURCE))
 expected=json.loads((HERE/'promotion.json').read_bytes())['results']['hut_drive_crt_vectors.json']['published_payload_sha256']
 assert sha(_canonical(result))==expected,'Frozen literal native hut projection changed'
 return result

def metadata():
 source=SOURCE
 return dict(schema=1,source_sha256=sha(source.read_bytes()),
  source_meta_sha256=sha((HERE/'hut_joined_drive_crt.meta.json').read_bytes()),
  scope='Literal native CABHUT composition projection; no gameplay algorithm.')

if __name__=='__main__':finish_vectors(generate,OUTPUT,provenance=metadata)
