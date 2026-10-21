"""Focused original42D170/42C290 controls using the FV Continuation owner."""
from pathlib import Path
import json, struct, hashlib, gzip, sys
from unicorn import UC_HOOK_CODE
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.building_body_rules import dwords
HERE=Path(__file__).resolve().parent
from . import pursuit


def install_graph(q, records, start, goal, level_records=None):
 """Explicit tiny graph inputs over original constructed native records."""
 u=q.u;m=q.m
 for level in range(3):
  rows=level_records[level] if level_records is not None else records if level==0 else [dict(zone_type=7,parent=0,edges=[]),dict(zone_type=0,parent=0 if level==2 else 1,edges=[])]
  base=m.read32(0x87F878+level*24)
  for zid,r in enumerate(rows):
   p=base+zid*36;edges=r['edges']; ep=m.alloc(max(8,len(edges)*8))
   for i,(other,flag) in enumerate(edges):u.mem_write(ep+i*8,dwords(other,flag))
   u.mem_write(p+4,dwords(ep,len(edges),1,len(edges),10));u.mem_write(p+24,struct.pack('<H',r['parent']));u.mem_write(p+28,dwords(r['zone_type']))
   if 'threat_index' in r:u.mem_write(p+32,dwords(r['threat_index']))
  plane=m.read32(0x87F858)
  for xy,zid in [((87,49),start if level==0 else 1),((87,54),goal if level==0 else 1)]:
   if level_records is not None:zid=start if xy==(87,49)else goal
   u.mem_write(plane+(xy[1]*q.side+xy[0])*10+level*2,struct.pack('<H',zid))
 return level_records if level_records is not None else [records]+[[dict(zone_type=7,parent=0,edges=[]),dict(zone_type=0,parent=0 if level==2 else 1,edges=[])]for level in (1,2)]


def records(types, edges):
 return [dict(zone_type=7,parent=0,edges=[])]+[dict(zone_type=t,parent=1,edges=edges.get(i,[]))for i,t in enumerate(types,1)]


def equal_nominal_cost_branches():
 """Two eleven-edge routes, each with five flagged edges (nominal11.005)."""
 flags=[[1,1,0,0,0,1,0,0,1,1,0],[1,1,0,0,0,1,0,1,0,0,1]]
 n=len(flags[0]);goal=n*2;edges={1:[[2,flags[0][0]],[n+1,flags[1][0]]]}
 for branch,start in [(0,2),(1,n+1)]:
  for step in range(1,n):
   z=start+step-1;edges[z]=[[goal if step==n-1 else z+1,flags[branch][step]]]
 return records([0]*goal,edges)


def generate():
 q=pursuit.Continuation(dict(name='zone_cost_setup',stage='healthy'))
 q.plane();q.construct(True);q.pathfinder();q.graphs();q.checkpoint();out=[]
 controls=[
  ('native_heap_three_equal_branches',records([0]*5,{1:[[2,0],[3,0],[4,0]],3:[[5,0]],4:[[5,0]]}),1,5,0,False,False),
  ('same_impassable_zone',records([7],{}),1,1,0,False,False),
  ('leave_impassable_start',records([7,0],{1:[[2,0]]}),1,2,0,False,False),
  ('cannot_enter_impassable_goal',records([0,7],{1:[[2,0]]}),1,2,0,False,False),
  ('flagged_binary32_chain',records([0]*8,{i:[[i+1,i%2]]for i in range(1,8)}),1,8,0,False,False),
  ('binary32_spills_choose_between_equal_rational_cost_routes',equal_nominal_cost_branches(),1,22,0,False,False),
  ('explicit_bridge_flags_on_raw_ground',records([0]*5,{1:[[2,0]],2:[[3,0]],3:[[4,0]],4:[[5,0]]}),1,5,0,True,True),
 ]
 reached=[]
 def observe(u,a,n,d):
  if a in (0x4DC760,0x585F40,0x583820,0x583180,0x56DA10):reached.append(f'{a:08x}')
 q.u.hook_add(UC_HOOK_CODE,observe)
 for name,rs,start,goal,mz,sb,gb in controls:
  q.reset();graphs=install_graph(q,rs,start,goal);p=q.m.alloc(8);q.u.mem_write(p,struct.pack('<hhhh',87,49,87,54));q.phase='control';reached.clear()
  before_rng={k:pursuit.proof.state(q.m,v)for k,v in q.resident.rngs.items()};actor=bytes(q.u.mem_read(q.src,0x1000));answer=q.m.invoke(0x42D170,0x87E8B8,(p,p+4,q.src,int(sb),int(gb),mz));q.flow_pending.clear()
  assert before_rng=={k:pursuit.proof.state(q.m,v)for k,v in q.resident.rngs.items()};assert actor==bytes(q.u.mem_read(q.src,0x1000));assert '00585f40' not in reached
  row=dict(name=name,source=[87,49],target=[87,54],source_bridge=sb,target_bridge=gb,movement_zone=mz,source_id=start,target_id=goal,graphs=graphs,returned=answer,flow=q.flow,hierarchy=q.hierarchy,reached=reached.copy(),threat_scalar_bytes=actor[0x530:0x538].hex(),team_pointer=struct.unpack_from('<I',actor,0x5D4)[0]);out.append(row);print('PASS',name,answer,[f.get('paths')for f in q.flow if f['kind']=='hierarchy_preflight'],flush=True)
 q.close();return dict(schema=1,controls=out)


def metadata():
 d=provenance(scope=__doc__,assumptions=['Original FV constructor, complete physical Anytown setup, OS seams, hierarchy constructors and scratch initialization are reused from the independently checked pursuit owner.','Tiny ordered graph inputs replace only constructed node type,parent,edge fields and the two endpoint hierarchy IDs. These are synthetic controls, not claims about physical graph production.','FV Foot+530=0 and Team=NULL are checked; direct entry uses supplied movement row0. Original function instructions, projection, precheck, heap and float stores execute unmodified.'],substitutions=['Supplied graph and endpoint inputs; no returned admission,cost,path,heap,RNG or timer value is substituted.'],entry_points={'cost':0x42D170,'precheck':0x42C290})
 return d

def project_vectors(check=False):
 """Lossless native record/result projection for Rust's JSON-only test reader."""
 from tools.native_oracle import _canonical
 paths=[HERE/'pursuit.json.gz',pursuit.proof.FACTS,pursuit.proof.REPO/'tools/spatial_oracle/anytown_navigation_restore.json.gz']
 source,initial,restored=[json.loads(gzip.decompress(p.read_bytes()))for p in paths]
 graph_sets=[];known={};cases=[]
 for index,stage in enumerate(source['stages']):
  original=initial['initial']['graphs'] if index==0 else restored['cases'][index-1]['after']['graphs']
  digest=hashlib.sha256(_canonical(original)).hexdigest()
  if digest not in known:
   known[digest]=len(graph_sets);graph_sets.append([dict(records=g['records'])for g in original])
  for row in stage['results']:
   groups=[]
   for event in row['hierarchy']:
    if event['kind']=='level_begin' and event['level']==2:groups.append([])
    groups[-1].append(event)
   call_index=0
   for i,call in enumerate(row['flow']):
    if call['kind']!='path_cost':continue
    preflight=row['flow'][i+1];assert preflight['kind']=='hierarchy_preflight'
    src=call['from_cell'];dst=call['to_cell'];events=groups[call_index];call_index+=1
    width=row['plane_input']['width']
    cases.append(dict(name=row['row']['name']+':'+str(call_index),graph=known[digest],source=src,target=dst,source_ids=[g['ids'][src[1]*width+src[0]]for g in original],target_ids=[g['ids'][dst[1]*width+dst[0]]for g in original],source_bridge=bool(call['args'][3]),target_bridge=bool(call['args'][4]),cost=call['returned_signed'],preflight=preflight,hierarchy=events))
 data=dict(schema=1,source_sha256={str(p.relative_to(pursuit.proof.REPO)):hashlib.sha256(p.read_bytes()).hexdigest()for p in paths},graphs=graph_sets,cases=cases)
 out=HERE/'zone_cost_vectors.json'
 if check:
  assert json.loads(out.read_text())==data
  print('PASS native cost projection',len(cases),'cases',len(graph_sets),'graph sets')
 else:out.write_text(json.dumps(data,separators=(',',':'))+'\n');print('WROTE native cost projection',len(cases),'cases',len(graph_sets),'graph sets')

if __name__=='__main__':
 if '--threat' in sys.argv:
  from . import zone_threat
  finish_vectors(zone_threat.generate,HERE/'zone_threat.json',provenance=zone_threat.metadata,
                 argv=[arg for arg in sys.argv[1:]if arg!='--threat'])
 elif '--project' in sys.argv:project_vectors('--check' in sys.argv)
 else:finish_vectors(generate,HERE/'zone_cost.json',provenance=metadata)
