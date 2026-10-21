"""Native FV Approach retained-destination, permission and exhaustion controls.

Uses the frozen candidate/pursuit owner; all expected decisions are original
execution. New retained state and optional INI records are explicit inputs.
"""
from pathlib import Path
import gzip,json,struct,sys
HERE=Path(__file__).resolve().parent
from . import candidates
base=candidates.base
from .publication import finish_vectors
from tools.native_oracle import run_checked,RET_MAGIC
from tools.spatial_oracle.building_body_rules import SP,RULES,dwords
from unicorn.x86_const import UC_X86_REG_ESP,UC_X86_REG_ESI,UC_X86_REG_EBX,UC_X86_REG_EBP,UC_X86_REG_FPCW

class Admission(candidates.CandidateControls):
 def reset(self):
  super().reset()
  row=getattr(self,'active_row',{})
  if 'block_rectangle' in row:
   x0,y0,x1,y1=row['block_rectangle']
   for xy,cell in self.cells.items():
    if x0<=xy[0]<=x1 and y0<=xy[1]<=y1:
     self.u.mem_write(cell+0x124,b'\x40');self.u.mem_write(cell+0x128,b'\x40')
 def guard_cell(self,xy):
  box=getattr(self,'active_row',{}).get('block_rectangle')
  if box and box[0]<=xy[0]<=box[2] and box[1]<=xy[1]<=box[3]:
   # Actual4834A0 always rejects these raw masks before touching overlay/type
   # data; omitted Terrain actors cannot affect the reached admission result.
   cell=self.cells[xy]
   assert self.u.mem_read(cell+0x124,1)==b'\x40' and self.u.mem_read(cell+0x128,1)==b'\x40'
   return
  super().guard_cell(xy)
 def execute(self,row):
  self.active_row=row;self.reset();m=self.m;u=self.u
  self.prefix['input']=row
  self.phase='setup'
  # Join one independently native-established theater role used by486900.
  packet=json.loads(gzip.decompress(base.proof.FACTS.read_bytes()))
  cliff_base=packet['theater']['globals'][str(0xABC2C8)]
  inherited_cliff_base=m.read32(0xABC2C8);u.mem_write(0xABC2C8,dwords(cliff_base))
  reader_before={f'{off:x}':u.mem_read(self.typ+off,1)[0]for off in (0x695,0xD27,0xD33,0xD34,0xD6A,0xD29)}
  if 'type_ini' in row:
   m.rules_cache({'FV':row['type_ini']})
   for reg,value in ((UC_X86_REG_ESP,SP),(UC_X86_REG_ESI,RULES),(UC_X86_REG_EBX,self.typ+0x24),(UC_X86_REG_EBP,self.typ)):u.reg_write(reg,value)
   run_checked(u,0x7144A0,0x7144D4,count=100000,required_addresses=(0x5295F0,))
  reader_after={f'{off:x}':u.mem_read(self.typ+off,1)[0]for off in (0x695,0xD27,0xD33,0xD34,0xD6A,0xD29)}
  source_xy=tuple(row.get('source_cell',[87,48]));target_xy=tuple(row.get('target',[87,54]))
  p=m.alloc(12);m.invoke(0x486840,self.cells[source_xy],(p,));xyz=list(struct.unpack('<iii',u.mem_read(p,12)));xyz[1]+=row.get('source_y_delta',0)
  u.mem_write(self.src+0x9C,dwords(*xyz));u.mem_write(self.src+0x2B4,dwords(self.cells[target_xy]));u.mem_write(self.src+0x3D5,bytes((row.get('in_playfield',1),)))
  face=m.alloc(4);m.invoke(0x5F3DB0,self.src,(face,self.cells[target_xy]))
  for field in (0x388,0x3A0):m.invoke(0x4C9300,self.src+field,(face,))
  m.invoke(0x5B35E0,self.src,(row.get('mission',1),1));m.invoke(0x5B3570,self.src)
  if 'retained_nav' in row:m.invoke(0x741970,self.src,(self.cells[tuple(row['retained_nav'])],1))
  special=m.invoke(0x486900,self.cells[target_xy])&255
  self.events=[];self.pending={};self.writes=[];self.flow=[];self.flow_pending={};self.ranges=[];self.range_pending={};self.candidates=[];self.geometry=[];self.hierarchy=[];self.queries=[];self.phase='pursuit'
  result=self.run(entry=0x7414E0)
  if result['failure'] is not None:(base.HERE/'admission_failure.json').write_text(json.dumps(result,indent=2)+'\n')
  assert result['failure'] is None,result['failure']
  for e in self.flow_pending.pop(RET_MAGIC,[]):e.update(returned_eax=result['returned'],returned_al=result['returned']&255)
  self.pending.pop(RET_MAGIC,None)
  assert not self.flow_pending and not self.range_pending
  result.update(source_xyz=xyz,type_reader_before=reader_before,type_reader_after=reader_after,
                inherited_destroyable_cliffs=inherited_cliff_base,joined_destroyable_cliffs=cliff_base,
                actual_target_486900=special,approach_reset_multiplier=m.read32(m.read32(0x8871E0)+0xDF8),
                fpcw=u.reg_read(UC_X86_REG_FPCW))
  for name in ('before','after'):
   snap=result[name];actor=bytes.fromhex(snap['actor_bytes']);drive=bytes.fromhex(snap['drive_bytes']);nav=int(snap['nav'],16);target=struct.unpack_from('<I',actor,0x2B4)[0]
   snap['nav_cell']=list(struct.unpack('<hh',u.mem_read(nav+0x24,4))) if nav else None
   snap['target_cell']=list(struct.unpack('<hh',u.mem_read(target+0x24,4))) if target else None
   snap['drive_destination']=list(struct.unpack_from('<iii',drive,0x34));snap['drive_head']=list(struct.unpack_from('<iii',drive,0x40));snap['blockage_timer']=[struct.unpack_from('<i',actor,off)[0]for off in (0x668,0x670)];snap['movement_timer']=[struct.unpack_from('<i',actor,off)[0]for off in (0x640,0x648)]
  print('PASS',row['name'],'native_return',hex(result['returned']),'nav',result['after']['nav_cell'],'target',result['after']['target_cell'],'candidates',len(result['candidates']),flush=True)
  return result

def generate():
 q=Admission(dict(name='prepare_healthy_admission',stage='healthy'));q.plane();q.construct(True);q.pathfinder();q.graphs();q.checkpoint()
 # Raise only the bounded outer Approach budget; all native bytes/returns and
 # the existing invocation owner remain unchanged. Reset after this process use.
 invoke_globals=q.m.invoke.__func__.__globals__;original_run=invoke_globals['run_checked']
 def outer_budget(u,start,*args,**kwargs):
  if start==0x7414E0:kwargs['count']=20000000
  return original_run(u,start,*args,**kwargs)
 invoke_globals['run_checked']=outer_budget
 rows=[dict(name='outside_no_nav',source_y_delta=-2),
       dict(name='outside_retained_near_nav',source_y_delta=-2,retained_nav=[87,49]),
       dict(name='outside_retained_far_nav_reset',source_y_delta=-2,retained_nav=[87,47]),
       dict(name='outside_retained_far_nav_no_recalc',source_y_delta=-2,retained_nav=[87,47],type_ini={'CanRecalcApproachTarget':'no'}),
       dict(name='in_range_no_nav'),dict(name='in_range_no_nav_not_in_playfield',in_playfield=0),
       dict(name='in_range_retained_far_nav',retained_nav=[87,47]),
       dict(name='attack_can_approach_false',source_y_delta=-2,type_ini={'CanApproachTarget':'no'}),
       dict(name='guard_can_approach_false',source_y_delta=-2,mission=5,type_ini={'CanApproachTarget':'no'}),
       dict(name='guard_can_approach_true',source_y_delta=-2,mission=5),
       dict(name='exhaust_all_rings',source_y_delta=-2,block_rectangle=[80,44,97,62])]
 for row in rows:row['stage']='healthy'
 try:return dict(schema=1,cases=[q.execute(row)for row in rows])
 finally:invoke_globals['run_checked']=original_run;q.close()

def metadata():
 r=base.metadata();r['scope']=__doc__;r['assumptions']=[a for a in r['assumptions'] if not a.startswith('Each control restores')];r['outer_approach_instruction_limit']=20000000;r['assumptions'] += ['This packet calls completeUnitApproach7414E0 after actual QueueMission/Commence and optional UnitDestination setter, at the supplied frame; MissionDispatch itself is not run. Source position/in_playfield and retained destination selection are explicit boundary inputs.', 'Native type reader7144A0..7144D4 consumes optional supplied CanApproachTarget/CanRecalcApproachTarget INI records over retained retail/constructor fields; all absent keys retain native defaults. Physical DestroyableCliffs base572 is joined from frozen full native theater input, then actual486900 runs for the target.', 'Exhaustion supplies static40 raw occupation on both Cell planes inside the declared rectangle; no occupancy producer or Terrain actor lifecycle is claimed. Actual4834A0 rejects before omitted Terrain actors could affect its result. No result is replaced; original nearby-cell fallback may execute.'];return r

if __name__=='__main__':finish_vectors(generate,HERE/'admission.json.gz',provenance=metadata)
