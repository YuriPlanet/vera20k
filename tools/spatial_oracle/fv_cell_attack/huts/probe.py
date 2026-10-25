"""Original CABHUT composition; reuses Paid and freshly executed Navigation."""
from pathlib import Path
import argparse,json,os,struct,traceback
from tools.spatial_oracle.fv_cell_attack import paid_world as owner
from tools.spatial_oracle.building_body_rules import RULES,INI,SP,dwords
from tools.native_oracle import run_checked, _canonical
from tools.spatial_oracle.shrapnel_repair import packet_io
from tools.spatial_oracle.fv_cell_attack.publication import publication_projection
from unicorn.x86_const import *

HERE=Path(__file__).resolve().parent
OUTPUT=HERE/'hut_joined_drive_crt.json.gz'
CTBHUT_SHA256='945216ab1eba7e34e6785fd54e916d096a6df4dee1df1052094e0b68f02630ff'

def hut_asset():
 root=os.environ.get('VERA20K_FV_HUT_ASSETS') or os.environ.get('VERA20K_ANYTOWN_INPUTS')
 if not root:raise ValueError('Set VERA20K_FV_HUT_ASSETS or VERA20K_ANYTOWN_INPUTS to an extracted retail asset directory')
 path=Path(root)/'CTBHUT.SHP';raw=path.read_bytes()
 assert owner.sha(raw)==CTBHUT_SHA256,('Physical CTBHUT.SHP identity changed',path)
 return raw

class Joined(owner.Paid):
 def __init__(self,world):
  self.hut_events=[];self.hut_steps=[];self.huts=[];self.logic_actor=None;self.hut_pending={}
  super().__init__(world)
 def observe(self,u,a,n,d):
  if a==0x55B610:self.logic_actor=u.reg_read(UC_X86_REG_ECX)
  if a in self.hut_pending:
   for row in self.hut_pending.pop(a):row.update(after=self.bridge_boundary(),returned_eax=u.reg_read(UC_X86_REG_EAX))
  if a in(0x70D4FD,0x70D50D,0x70D547,0x70D54D):
   actor=u.reg_read(UC_X86_REG_ESI);target=u.reg_read(UC_X86_REG_EBP)
   row=dict(kind={0x70D4FD:'detach_target_compare',0x70D50D:'detach_restore_mission',0x70D547:'detach_assign_target_null',0x70D54D:'detach_actor_after'}[a],pc=f'{a:08x}',frame=self.frame,actor=f'{actor:08x}',native_id=self.m.read32(actor+0x10),target=f'{target:08x}',tarcom=f'{self.m.read32(actor+0x2B4):08x}',is_hut=actor in self.huts,index=u.reg_read(UC_X86_REG_EBX))
   if a in(0x70D50D,0x70D547):row['callee']=f'{self.m.read32(self.m.read32(actor)+(0x1F8 if a==0x70D50D else 0x3C8)):08x}'
   self.hut_events.append(row)
  if a in(0x4A0535,0x4A0557):
   sp=u.reg_read(UC_X86_REG_ESP);p=self.m.read32(sp);old=self.m.read32(p);value=(old+(1 if a==0x4A0535 else -1))&0xffffffff
   self.hut_events.append(dict(kind='OS_Interlocked',pc=f'{a:08x}',ptr=f'{p:08x}',before=old,after=value));u.mem_write(p,dwords(value));u.reg_write(UC_X86_REG_EAX,value);u.reg_write(UC_X86_REG_ESP,sp+4);u.reg_write(UC_X86_REG_EIP,a+6);return
  if a in(0x4F64DC,0x4F5E3A):
   self.hut_events.append(dict(kind='house_probe',pc=f'{a:08x}',ebp=f'{u.reg_read(UC_X86_REG_EBP):08x}',house=f'{self.neutral_house:08x}',fields=bytes(u.mem_read(self.neutral_house+0x16050,0x60)).hex()))
  entries={0x45DD90:'building_type_ctor',0x45FE50:'building_type_read',0x4F54A0:'house_ctor',0x43B740:'building_ctor',0x440580:'building_unlimbo',0x43FB20:'building_ai',0x575EE0:'notify_span',0x70D4A0:'detach',0x574000:'hut_high_controller',0x574C20:'hut_low_controller',0x573540:'admitted_repair',0x6E53A0:'tag_event'}
  if a in entries:
   sp=u.reg_read(UC_X86_REG_ESP);self.hut_events.append(dict(kind=entries[a],pc=f'{a:08x}',caller=f'{self.m.read32(sp):08x}',this=f'{u.reg_read(UC_X86_REG_ECX):08x}',frame=self.frame,phase=self.phase))
   if a==0x43B740:self.huts.append(u.reg_read(UC_X86_REG_ECX))
   if a in(0x575EE0,0x70D4A0,0x573540):
    row=self.hut_events[-1];row['before']=self.bridge_boundary();self.hut_pending.setdefault(self.m.read32(sp),[]).append(row)
  count=len(self.events)
  super().observe(u,a,n,d)
  if self.phase=='logic' and self.logic_actor:
   for row in self.events[count:]:
    if row['kind']in('rng','raw'):row.update(logic_owner=f'{self.logic_actor:08x}',logic_owner_id=self.m.read32(self.logic_actor+0x10),logic_owner_kind='hut'if self.logic_actor in self.huts else'other')
 def written(self,u,access,a,size,value,data):
  if hasattr(self,'neutral_house') and self.neutral_house+0x16050<=a<self.neutral_house+0x160B0:
   self.hut_events.append(dict(kind='house_write',pc=f'{u.reg_read(UC_X86_REG_EIP):08x}',offset=a-self.neutral_house,size=size,value=value))
  super().written(u,access,a,size,value,data)
 def step(self,name,fn,this=0,args=()):
  m=self.m;u=self.u
  row=dict(name=name,entry=f'{fn:08x}',this=f'{this:08x}',args=list(args),rng_before={k:bytes(u.mem_read(p,1012)).hex()for k,p in self.resident.rngs.items()})
  self.hut_steps.append(row);print('BEGIN',name,hex(fn),flush=True)
  row['eax']=m.invoke(fn,this,args);row['rng_after']={k:bytes(u.mem_read(p,1012)).hex()for k,p in self.resident.rngs.items()}
  print('PASS',name,hex(row['eax']),flush=True);return row['eax']
 def hut_state(self):
  m=self.m;u=self.u
  rows=[]
  for p in self.huts:
   rows.append(dict(ptr=f'{p:08x}',native_id=m.read32(p+0x10),mission=struct.unpack('<i',u.mem_read(p+0xAC,4))[0],queued=struct.unpack('<i',u.mem_read(p+0xB4,4))[0],dispatch=[m.read32(p+0xC8),m.read32(p+0xD0)],target=f'{m.read32(p+0x2B4):08x}',xyz=list(struct.unpack('<3i',u.mem_read(p+0x9C,12))),health=m.read32(p+0x6C),alive=u.mem_read(p+0x90,1)[0],limbo=u.mem_read(p+0x81,1)[0],logic_registered=u.mem_read(p+0x98,1)[0],raw=bytes(u.mem_read(p,0x720)).hex()))
  return rows
 def full_state(self):
  result=super().full_state();result['huts']=self.hut_state();result['span_tags']=[dict(coord=[x,y],tag=f'{self.m.read32(self.cells[x,y]+0x3C):08x}')for y in range(50,60)for x in range(85,90)];return result
 def bridge_boundary(self):
  m=self.m;u=self.u
  return dict(huts=self.hut_state(),source_target=f'{m.read32(self.src+0x2B4):08x}',rng={k:bytes(u.mem_read(p,1012)).hex()for k,p in self.resident.rngs.items()},logic_count=m.read32(0x87F788),anim_count=m.read32(0xA8E9B8),bullet_count=m.read32(0xA8ED50),techno_count=m.read32(0xA8EC88),team_count=m.read32(0x8B40F8))
 def join_huts(self):
  self.phase='setup';m=self.m;u=self.u
  from tools.storage_oracle.keyboard_bindings import stock_csf
  physical,source_hash=stock_csf();names=['NAME:CABHUT','NAME:NEUTRAL'];records=m.alloc(len(names)*0x28);values=m.alloc(len(names)*4);extras=m.alloc(len(names)*4)
  for i,name in enumerate(names):
   raw=(physical[name]+'\0').encode('utf-16-le');p=m.alloc(len(raw));u.mem_write(p,raw);u.mem_write(values+i*4,dwords(p));u.mem_write(records+i*0x28,name.encode('ascii')+b'\0');u.mem_write(records+i*0x28+0x24,dwords(i))
  u.mem_write(0xB1CF6C,dwords(len(names)));u.mem_write(0xB1CF74,dwords(records,values,extras));self.inputs['hut_csf_cache']=dict(source_sha256=source_hash,entries={n:physical[n]for n in names},original_lookup='0x734E60',boundary='Existing keyboard oracle parses physical CSF text for native cached lookup; full archive/CSF load excluded.')
  m.assets['CTBHUT.SHP']=hut_asset()
  from tools.spatial_oracle.building_sale import OCCUPY_INIT
  self.step('Building foundation startup',OCCUPY_INIT)
  for fn in(0x4E6B60,0x4E7360,0x4E6CE0,0x725550,0x7254D0,0x725650,0x7253D0):self.step('static',fn)
  self.hut_type=m.alloc(0x1800);self.step('CABHUT ctor',0x45DD90,self.hut_type,(m.cstring('CABHUT'),))
  art,lines=owner.frozen.proof.lexical((owner.frozen.proof.assets_root()/'ARTMD.INI').read_bytes(),{'CABHUT'})
  m.make_ini(art);self.inputs['hut_art']=dict(sections=art,source_lines=lines)
  for name,path in owner.mission_owner.base.layers():
   if not path.exists():continue
   sec,lines=owner.frozen.proof.lexical(path.read_bytes(),{'CABHUT'});m.rules_cache(sec)
   self.step('CABHUT read '+name,0x45FE50,self.hut_type,(RULES,))
  self.neutral_country=self.countries['Neutral']
  for name,path in owner.mission_owner.base.layers():
   if not path.exists():continue
   sec,lines=owner.frozen.proof.lexical(path.read_bytes(),{'Neutral'});m.rules_cache(sec)
   self.step('Neutral read '+name,0x511850,self.neutral_country,(RULES,))
  self.neutral_house=m.alloc(0x17000);self.step('Neutral house ctor',0x4F54A0,self.neutral_house,(self.neutral_country,))
  from tools.rules_oracle.bridge_child_sound import Sound
  physical,lines=owner.frozen.proof.lexical(owner.frozen.proof.MAP.read_bytes(),{'Basic','Structures'})
  selected={'Basic':physical['Basic'],'Structures':{k:v for k,v in physical['Structures'].items()if v.split(',')[1]=='CABHUT' and tuple(map(int,v.split(',')[3:5]))in((85,58),(89,51))}}
  saved_art=bytes(u.mem_read(INI,0x40));proxy=Sound.__new__(Sound);proxy.__dict__=m.__dict__;Sound.make_ini(proxy,selected);map_ini=m.alloc(0x40);u.mem_write(map_ini,bytes(u.mem_read(INI,0x40)));u.mem_write(INI,saved_art)
  self.map_ini=map_ini;self.inputs['hut_structures']=selected
  # Original Basic NewINIFormat read/store; full Scenario loading remains excluded.
  u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EDI,map_ini);run_checked(u,0x68A13D,0x68A14B);run_checked(u,0x68A151,0x68A15B)
  self.step('Authored Structures read',0x44F820,map_ini)
  assert len(self.huts)==2,self.huts
  self.phase='logic'

def generate():
 original_heaps=(owner.lists_owner.HEAP,owner.reader_owner.HEAP)
 q=None;failure=None;states=[];repair=None;before_join=None
 try:
  worlds=owner.native_worlds()
  w=next(x for x in worlds if x['stage']=='damaged')
  owner.lists_owner.HEAP=owner.reader_owner.HEAP=0x28000000
  q=Joined(w);q.setup();before_join=q.full_state();q.join_huts();states.append(q.full_state())
  for _ in range(80):
   q.tick();states.append(q.full_state());print('TICK',q.frame,'shots',len(q.shots),'impacts',len(q.impacts),'huts',[(r['mission'],r['health'])for r in states[-1]['huts']],flush=True)
   if len(q.impacts)>=2 and states[-1]['bullet_count']==0 and states[-1]['anim_count']==0:break
  if states[-1]['target_cell']['overlay']==232:
   q.phase='repair';coord=q.m.alloc(4);q.u.mem_write(coord,struct.pack('<hh',*q.world_cache['case']['start']));repair={'before':q.full_state()};q.step('Admitted repair after joined collapse',0x573540,0x87F7E8,(coord,));repair['after']=q.full_state()
 except Exception as e:
  failure=dict(error=str(e),traceback=traceback.format_exc(),pc=f'{q.u.reg_read(UC_X86_REG_EIP):08x}'if q else None,trace=[f'{a:08x}'for a in q.trace]if q else [])
 finally:
  if q:q.close()
  owner.lists_owner.HEAP,owner.reader_owner.HEAP=original_heaps
 result=dict(schema=1,failure=failure,steps=q.hut_steps if q else [],hut_events=q.hut_events if q else [],events=q.events if q else [],timeline=q.timeline if q else [],writes=q.memwrites if q else [],extra=q.extra if q else [],world=q.world if q and hasattr(q,'world')else None,hut_type=bytes(q.u.mem_read(q.hut_type,0x1800)).hex()if q and hasattr(q,'hut_type')else None,house_bytes=bytes(q.u.mem_read(q.neutral_house,0x17000)).hex()if q and hasattr(q,'neutral_house')else None,states=states,repair=repair,shots=q.shots if q else [],impacts=q.impacts if q else [],inputs=q.inputs if q else None,before_join=before_join)
 if q:assert owner.sha(bytes(q.u.mem_read(0x401000,0x3E0000)))==w['code_hash']
 assert failure is None,failure
 assert len(result['shots'])==len(result['impacts'])==2
 assert result['repair']is not None
 return result

def metadata():
 historical=json.loads((HERE/'original_execution.meta.json').read_bytes())
 boundaries=list(historical['boundaries'])
 boundaries[-1]='Every replay freshly executes all four physical states via the existing original Navigation owner and compares all frozen cells/planes/graphs before selecting damaged. No external heap cache or pickle input is supported.'
 return dict(schema=1,
  original_execution_metadata_sha256=owner.sha((HERE/'original_execution.meta.json').read_bytes()),
  original_payload_sha256=historical['payload_sha256'],scope=historical['scope'],boundaries=boundaries,
  physical_asset_sha256={'CTBHUT.SHP':CTBHUT_SHA256},
  publication_changes=['Copied lexical INI dictionaries/lines become SHA256 through the shared FV publication owner. Original Drive CRT startup precedes construction; the historical image-zero packet remains preserved as superseded evidence.',
   'Drive startup is the gameplay prerequisite correction; historical execution metadata remains in original_execution.meta.json.'])

def canonical_generate():
 return json.loads(_canonical(generate()))

def main():
 parser=argparse.ArgumentParser(description=__doc__)
 mode=parser.add_mutually_exclusive_group();mode.add_argument('--write',action='store_true');mode.add_argument('--check',action='store_true')
 parser.add_argument('--fresh-world',action='store_true',help='compatibility flag: physical worlds are always freshly executed')
 parser.add_argument('--output',type=Path,default=OUTPUT);args=parser.parse_args()
 packet_io.finish_vectors(canonical_generate,OUTPUT,provenance=metadata,
  argv=['--write'if args.write else'--check','--output',str(args.output)],
  promotion_path=HERE/'promotion.json',projection=publication_projection)

if __name__=='__main__':main()
