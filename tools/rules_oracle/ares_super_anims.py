"""Four synthetic Ares SuperAnim/availability histories using the sole EmpProbe VM.

Original hook exports and retail timer/slot-clear bodies execute intact.
PlayAnim is the existing superweapon_oracle recording boundary, not GPU/Anim
construction. SW/country/House registries and retained world fields are explicit
inputs. Actual extension constructors/lookup and parser/writer bodies execute.
The fourth history executes the successful-launch count writer leaf; full effect
handler Activate and its world are excluded (success-to-writer caller is static
10032ED0..10032F15 evidence). No original corpus is replayed by this producer.
"""
from pathlib import Path
import os,sys,struct,json,time
from tools.rules_oracle.ares_emp import EmpProbe,identified_bytes,ARES_SHA256,native
from tools.native_oracle import run_checked, finish_vectors
from tools.input_oracle.fast_scroll import return_from_sink
from unicorn.x86_const import *
FIXTURE=Path(__file__).with_suffix('.json')
class SuperAnimProbe(EmpProbe):
 def __init__(self,images):
  self.boundaries=[];self.recent=[];self.slot_events=[];self.candidates=[];super().__init__(images);self.native_crt=None
 def hook(self,u,pc,size,data):
  self.recent.append(f'{pc:08X}');self.recent=self.recent[-12:]
  sp=u.reg_read(UC_X86_REG_ESP)
  if pc in (0x7c8e17,0x100859b5):
   n=self.read(sp+4);p=self.alloc(n+64);p=(p+63)&~63 if pc==0x100859b5 else p
   self.boundaries.append(dict(service='heap_allocate',size=n));return return_from_sink(u,0,p)
  if pc in (0x7c8b3d,0x100033c0):return return_from_sink(u,0,0)
  if pc==0x7d140b:
   if self.native_crt is None:self.native_crt=self.alloc(0x1000)
   return return_from_sink(u,0,self.native_crt)
  if pc==0x100329e0:
   index=self.read(sp+4);self.candidates.append(index)
  if pc==0x451890:
   name=self.string(self.read(sp+4));slot=self.read(sp+8);damage=self.read(sp+12)&255;garrison=self.read(sp+16)&255;z=self.read(sp+20)
   self.slot_events.append(dict(action='play',name=name,slot=slot,damaged=bool(damage),garrisoned=bool(garrison),z_adjust=z))
   return return_from_sink(u,20,0)
  if pc in (0x30000320,0x30000330):return return_from_sink(u,0,0 if pc==0x30000320 else self.bt)
  if pc==0x30000340:return return_from_sink(u,0,1)
  if pc==0x30000350:
   self.slot_events.append(dict(action='delete',anim=f'{u.reg_read(UC_X86_REG_ECX):08X}'));return return_from_sink(u,4,0)
  return super().hook(u,pc,size,data)
 def setup(self):
  self.call(0x100576d0,ecx=0x100c1c54)
  self.types=[]
  for index in range(3):
   typ=self.alloc(0x120);self.write(typ+0x98,index);self.write(typ+0xb4,2)
   ext=self.alloc(0x500);self.call(0x10031460,typ,ecx=ext)
   self.call(0x10057cf0,typ,ext,ecx=0x100c1c54)
   assert self.call(0x10057c70,typ,ecx=0x100c1c54)==ext
   self.types.append((typ,ext))
  return [dict(index=i,defaults={f'{o:X}':self.read(e+o) for o in [0x1e4,0x1e8,0x25c,0x260,0x264,0x268,0x270,0x274,0x280]}) for i,(t,e) in enumerate(self.types)]
 def prepare_world(self):
  self.house=self.alloc(0x16100);country=self.alloc(0x200);self.write(country+0xb4,0);self.write(self.house+0x34,country)
  self.u.mem_write(self.house+0x1ec,b'\1');self.u.mem_write(self.house+0x1ed,b'\0');self.write(0xa8b238,0)
  items=self.alloc(12);self.write(self.house+0x258,items);self.write(self.house+0x264,3);self.supers=[]
  for i,(typ,ext) in enumerate(self.types):
   sup=self.alloc(0x80);self.write(items+i*4,sup);self.write(sup+0x28,typ);self.write(sup+0x30,-1);self.write(sup+0x38,899);self.supers.append(sup)
  self.building=self.alloc(0x800);self.bt=self.alloc(0x1800);self.bext=self.alloc(0x300);vt=self.alloc(0x500)
  self.write(self.bext,self.bt);self.write(self.bt+0xe24,self.bext);self.write(self.bt+0x16f0,0);self.write(self.bt+0x16f4,-1)
  self.write(self.building,vt);self.write(self.building+0x520,self.bt);self.write(self.building+0x21c,self.house)
  self.write(self.building+0x6c,100);self.write(self.bt+0xa0,100);self.write(self.bt+0x16e8,0x3f800000)
  for off,fn in [(0x408,0x30000320),(0x88,0x30000330),(0x184,0x30000340)]:self.write(vt+off,fn)
  rules=self.alloc(0x2000);self.write(0x8871e0,rules);self.u.mem_write(rules+0x1700,struct.pack('<d',.5));self.write(0xa8ed84,9000)
  names={14:(0x1304,0x1314,0x1324),15:(0x1348,0x1358),16:(0x138c,0x139c,0x13ac),17:(0x13d0,0x13e0)}
  for slot,offsets in names.items():
   for variant,offset in enumerate(offsets):self.u.mem_write(self.bt+offset,f'SLOT{slot}V{variant}'.encode()+b'\0')
  self.ini=self.alloc(4);self.write(self.ini,0x887048)
  self.counter=self.alloc(4);self.write(self.house+0x5554,self.counter);self.write(self.house+0x5558,1)
  self.aux=self.alloc(0x1000);self.write(self.aux+0xdf8,0)
 def snapshot(self):
  return dict(candidates=list(self.candidates),events=list(self.slot_events),slots={str(i):bool(self.read(self.building+0x55c+i*4)) for i in (14,15,16,17)})
 def clear_observation(self):self.candidates=[];self.slot_events=[]
 def select(self):
  self.clear_observation();p=self.call(0x1000d610,self.house,ecx=self.bext)
  return dict(selected=self.supers.index(p) if p else None,candidates=list(self.candidates))
 def open(self,left):
  self.clear_observation()
  for p in self.supers:self.write(p+0x38,left)
  frame=self.alloc(0x30);self.write(frame+0x10,self.building)
  resume=self.call(0x10034a60,frame);out=dict(resume=f'{resume:08X}',hook_result=self.supers.index(self.read(frame+0x24)) if self.read(frame+0x24) in self.supers else None)
  if resume==0x44643e:
   sp=0x2000e000;self.u.reg_write(UC_X86_REG_ESP,sp);self.u.reg_write(UC_X86_REG_EAX,self.read(frame+0x24));self.u.reg_write(UC_X86_REG_EDI,0xffffffff);self.u.reg_write(UC_X86_REG_EBP,self.building);self.u.reg_write(UC_X86_REG_EBX,0)
   run_checked(self.u,resume,0x44656d,count=100000)
  else:assert resume==0x446580
  self.check_code();return dict(left=left,**out,**self.snapshot())
 def update(self,left,occupied):
  self.clear_observation()
  for p in self.supers:self.write(p+0x38,left)
  for i in range(21):self.write(self.building+0x55c+i*4,0)
  for slot in occupied:
   anim=self.alloc(0x200);avt=self.alloc(0x40);self.write(anim,avt);self.write(avt+0x20,0x30000350);self.write(self.building+0x55c+slot*4,anim)
  frame=self.alloc(0x30);self.write(frame+0xc,self.building)
  resume=self.call(0x10034af0,frame)
  if resume==0x451030:
   self.u.reg_write(UC_X86_REG_ESP,0x2000e000);self.u.reg_write(UC_X86_REG_EAX,self.read(frame+0x24));self.u.reg_write(UC_X86_REG_EDI,self.read(frame+8));self.u.reg_write(UC_X86_REG_ESI,self.building)
   run_checked(self.u,resume,0x451132,count=100000)
  else:assert resume==0x451145
  self.check_code();return dict(left=left,occupied=list(occupied),resume=f'{resume:08X}',**self.snapshot())
 def allow_player(self,index,text):
  self.reads[('FIXTURE','SW.AllowPlayer')]=text
  self.call(0x1000ebf0,self.ini,self.text('FIXTURE'),self.text('SW.AllowPlayer'),ecx=self.types[index][1]+0x1e7)
 def histories(self):
  rows=[]
  rows.append(dict(name='array-index-not-dispatch-kind',input=dict(primary=0,secondary=-1,type_kind=2,per_type_array_indices=[0,1,2]),selection=self.select(),opening_charging=self.open(899),opening_ready=self.open(14),update_near=self.update(899,[14]),update_far=self.update(1800,[16])))
  self.write(self.types[0][0]+0xc8,self.aux);self.write(self.bt+0x16f4,1)
  b=dict(name='unavailable-primary-secondary-then-extra',input=dict(primary=0,secondary=1,native_aux_count=0),secondary=self.select(),secondary_open=self.open(899))
  self.allow_player(1,'no');extra=self.alloc(4);self.write(extra,2);self.write(self.bext+0x178,extra);self.write(self.bext+0x17c,extra+4)
  b['extra_after_secondary_reader_no']=self.select();b['extra_open']=self.open(899)
  self.write(self.counter,1);b['primary_recovers_first_in_order']=self.select();rows.append(b)
  self.write(self.counter,0);self.allow_player(2,'no')
  rows.append(dict(name='all-unavailable-primary-fallback',input=dict(primary=0,secondary=1,extra=[2],native_aux_count=0,secondary_allow_player=False,extra_allow_player=False),selection=self.select(),opening=self.open(899),update_occupied14=self.update(14,[14])))
  return rows
def fourth_history(images):
 m=None;began=time.time()
 try:
  m=SuperAnimProbe(images);m.setup();m.prepare_world()
  typ,ext=m.types[0];h=m.house;section=m.text('FIXTURE')
  def integer(key,value,offset):
   m.reads[('FIXTURE',key)]=value;m.call(0x1000a570,m.ini,section,m.text(key),ecx=ext+offset)
   return m.read(ext+offset)
  def boolean(key,value,offset):
   m.reads[('FIXTURE',key)]=value;m.call(0x1000ebf0,m.ini,section,m.text(key),ecx=ext+offset)
   return int(m.u.mem_read(ext+offset,1)[0])
  def mask(key,value,offset):
   m.reads[('FIXTURE',key)]=value;p=m.call(0x10033e30,section,m.text(key),m.read(ext+offset),ecx=m.ini);m.write(ext+offset,p);return p
  stages=[]
  stages.append(dict(stage='constructor-default-available',selected=m.select()))
  stages.append(dict(stage='required-random-rejects-country0',mask=mask('SW.RequiredHouses','<random>',0x25c),selected=m.select(),opening=m.open(899)))
  country=m.read(h+0x34);m.write(country+0xb4,30)
  stages.append(dict(stage='same-required-mask-country30-allowed',selected=m.select()))
  stages.append(dict(stage='forbidden-random-rejects-country30',mask=mask('SW.ForbiddenHouses','<random>',0x260),selected=m.select(),opening=m.open(899)))
  stages.append(dict(stage='forbidden-none-clears',mask=mask('SW.ForbiddenHouses','none',0x260),selected=m.select()))
  boolean('SW.AllowAI','no',0x1e8);m.u.mem_write(h+0x1ec,b'\0')
  stages.append(dict(stage='AI-no-rejects-AI',selected=m.select(),opening=m.open(899)))
  m.u.mem_write(h+0x1ec,b'\1');shots=integer('SW.Shots','2',0x280)
  house_ext=m.alloc(0x200);m.write(h+0x16084,house_ext);m.write(0xa8e340,3)
  def counts():
   out=m.alloc(8);m.call(0x10021480,out,0,ecx=house_ext);return dict(count=m.read(out),last_frame=m.read(out+4))
  stages.append(dict(stage='shot-limit2-initial',reader_value=shots,counter=counts(),selected=m.select()))
  m.call(0x100226d0,0,ecx=house_ext)
  stages.append(dict(stage='accepted-launch-writer-once',counter=counts(),selected=m.select()))
  m.write(0xa8ed84,9001);m.call(0x100226d0,0,ecx=house_ext)
  stages.append(dict(stage='accepted-launch-writer-twice-reaches-limit',counter=counts(),selected=m.select(),opening=m.open(899),update=m.update(14,[14])))
  m.check_code()
  return dict(status='fourth synthetic history executed',binary_identity=dict(ares_sha256=ARES_SHA256),stages=stages,boundaries=m.boundaries,code_guard=True,three_rng_unchanged=True,elapsed_seconds=time.time()-began,coverage_limit='100226D0 actual admitted-launch count/frame writer executes. Entire effect handler/common-launch world excluded; successful Activate to writer ordering is static original10032ED0..10032F15 evidence. Runtime slot PlayAnim is existing oracle recording boundary, not full Anim creation/GPU.')
 except Exception as e:return dict(status='boundary failure',error=str(e),recent=m.recent if m else [],elapsed_seconds=time.time()-began)

def project(first, fourth):
    assert first['code_guard'] and first['rng_unchanged']
    assert fourth['code_guard'] and fourth['three_rng_unchanged']
    # Lossless projection of behavior observations. Only run timing, image
    # identity (recorded once below), and declared transport logs are separate.
    return dict(schema=1,binary_identity=first['binary_identity'],defaults=first['defaults'],
                histories=first['histories'],availability_history=fourth['stages'],
                code_guard=True,three_rng_unchanged=True)

def generate(ares):
    images=(identified_bytes(ares,ARES_SHA256),native.image_bytes())
    m=SuperAnimProbe(images);defaults=m.setup();m.prepare_world();rows=m.histories();m.check_code()
    first=dict(binary_identity=dict(ares_sha256=ARES_SHA256),defaults=defaults,histories=rows,code_guard=True,rng_unchanged=True)
    fourth=fourth_history(images)
    if fourth.get('status')!='fourth synthetic history executed':raise RuntimeError(fourth)
    return project(first,fourth)

def metadata():
    result = native.provenance(
        scope="Four bounded installed-Ares SuperAnim and availability histories.",
        assumptions=[
            "Retained Super/Country/House registries and INI ReadString values are declared fixture inputs; native constructors, lookup, readers, count writer and timer/clear bodies execute.",
            "Three first-query histories and one availability/count history are separate executions. Their observation payload is retained without synthesizing values.",
            "No whole Activate payload, upgrade-provided Super enumeration, arbitrary stock-redirected success accounting or GPU/Anim rendering parity is established.",
        ],
        substitutions=[
            "Existing EmpProbe heap/CRT/TLS and Syringe transport; original code bytes remain guarded.",
            "PlayAnim records native slot/name/variant calls at the existing recording seam; it does not create or draw an Anim.",
        ],
        entry_points=dict(opening=0x10034a60, update=0x10034af0,
                          query=0x1000d610, availability=0x100329e0,
                          launch_writer=0x100226d0))
    result["ares_sha256"] = ARES_SHA256
    return result


def main():
    import argparse
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--ares-dll',required=True,type=Path)
    p.add_argument('--gamemd-exe',required=True,type=Path)
    a, remaining=p.parse_known_args()
    os.environ['VERA20K_GAMEMD_EXE']=str(a.gamemd_exe)
    finish_vectors(lambda: generate(a.ares_dll), FIXTURE, provenance=metadata,
                   argv=remaining,
                   source_paths={"producer": Path(__file__),
                                 "shared_vm": Path(sys.modules[EmpProbe.__module__].__file__),
                                 "runner": Path(native.__file__)})
if __name__=='__main__':main()
