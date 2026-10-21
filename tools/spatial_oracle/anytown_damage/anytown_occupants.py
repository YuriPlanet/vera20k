"""Original concrete-low damage occupants and actual Cell-target Detach continuation.

Bounded physical Anytown projection; initialized MTNK/Drive, supplied world
membership and retained head. No full scenario/order/flight or animation/audio
engine claim. Original path admission, receiver and class cleanup execute.
"""
import sys
from pathlib import Path
from collections import Counter
from tools.spatial_oracle.naval_occupants import *
from tools.rules_oracle.bridge_anim_inputs import crc
from tools.rules_oracle.bridge_child_sound import sections as sound_sections
HERE=Path(__file__).resolve().parent
from .anytown_resident import Resident,rules as resident_rules,identity,inputs

class Occupied(Native):
 def alloc(self,n):return super().alloc(0x1000 if n==0x800 else n)
 def make_ini(self,sections):
  super().make_ini(sections);index=self.read32(INI+0x28)
  for i in range(self.read32(INI+0x2C)):
   sec=self.read32(index+i*8+4);keys=sections[self.string(self.read32(sec+0xC))];table=self.read32(sec+0x2C)
   entries={self.read32(table+j*8):self.read32(table+j*8+4) for j in range(len(keys))}
   ordered=[entries[crc(k)] for k in keys];end=self.alloc(0x20)
   if ordered:self.u.mem_write(sec+0x18,dwords(ordered[0]))
   for j,p in enumerate(ordered):self.u.mem_write(p+4,dwords(ordered[j+1] if j+1<len(ordered) else end,ordered[j-1] if j else end))
 def __init__(self,donor,t,case,*,scene=None):
  # Scene data only: other ordinary bridge families reuse the same native
  # actor/Drive/receiver/lifetime owner. Defaults preserve the frozen Anytown
  # execution, including its separately declared physical layer boundaries.
  self.scene=scene or dict(impact_cell=[87,54],ground_z=416,
   map_path=identity.ASSETS/'XMP03T4.MAP',area_entry=0x48A2B4,area_stop=0x48A2C4,
   span=[(x,y) for y in range(51,58) for x in range(86,89)])
  impact=self.scene['impact_cell'];ground_z=self.scene['ground_z']
  self.visit=0;self.events2=[];self.return_events=[];self.samples=[];self.symbols={};self.active_stage='setup';self.case2=case
  super().__init__(dict(type='MTNK',current=[impact[0]*256+128,impact[1]*256+128,ground_z],target=[115,59],member=[115,59]))
  u=self.u;self.phase='setup'
  self.read_extra()
  self.inputs['type']=self.type_state();self.inputs.pop('ship_vtable');self.inputs.pop('ship_at_coord')
  # Construct the actual physical MTNK locomotor family; no vtable substitution.
  self.invoke(0x4AF540,self.loco);u.mem_write(self.loco+0xC,dwords(self.actor));u.mem_write(self.loco+0x14,dwords(1));u.mem_write(self.actor+0x674,dwords(self.loco+4))
  assert self.read32(self.loco+4)==0x7E7EB0
  for start,end,perms in donor.u.mem_regions():
   if start>=0x40000000:u.mem_map(start,end-start+1,perms);u.mem_write(start,bytes(donor.u.mem_read(start,end-start+1)))
  for p,n in ((MAP,0x200),(0xC00000,0x100000),(0xABDC50,0x200),(0xA8ED2C,4),(0xA8ED38,4),(0xA83D84,4),(0x89EA40,12*36)):
   u.mem_write(p,bytes(donor.u.mem_read(p,n)))
  self.cells=dict(donor.ptrs);self.current=tuple(case['current_cell']);self.current_cell=self.cells[self.current];self.selected=self.cells[tuple(impact)]
  for p in self.cells.values():u.mem_write(p,dwords(0x7E4EEC));u.mem_write(p+0xE4,dwords(0,0));u.mem_write(p+0x122,b'\x10')
  for p,v in t['globals'].items():u.mem_write(p,dwords(v))
  u.mem_write(RULES+0x664,bytes(donor.u.mem_read(0x44400664,1)))
  self.invoke(0x4AF4A0,0)
  for a in (0x561710,0x5617A0,0x5617C0,0x5617E0):self.invoke(a,0)
  self.invoke(0x6D1C20,self.alloc(0x2000));self.invoke(0x5F5B90,self.actor,(0,self.alloc(0x80)))
  u.mem_write(self.actor+0x9C,dwords(self.current[0]*256+128,self.current[1]*256+128,ground_z))
  u.mem_write(self.actor+0x55C,packed(*self.current));u.mem_write(self.current_cell+0xE4,dwords(self.actor));u.mem_write(self.current_cell+0x124,dwords(0x20));u.mem_write(self.actor+0x74,b'\1')
  u.mem_write(self.actor+0x8C,b'\0');u.mem_write(self.actor+0xAC,dwords(5));u.mem_write(self.actor+0xB0,dwords(-1))
  if case.get('head_cell'):
   x,y=case['head_cell'];u.mem_write(self.loco+0x40,dwords(x*256+128,y*256+128,ground_z+case.get('head_dz',0)))
   u.mem_write(self.loco+0x58,dwords(-1,0));u.mem_write(self.loco+0x63,b'\1');u.mem_write(self.actor+0x8D,b'\1');u.mem_write(self.loco+0x50,struct.pack('<d',0.75))
  u.mem_write(self.actor+0x2B4,dwords(self.selected if case.get('target_impact') else 0))
  u.mem_write(0xB0F698,dwords(0x7E91EC,self.alloc(128),32,1,0,10))
  self.invoke(0x40CB80,0);self.invoke(0x4A8630,0);assert self.invoke(0x55BAA0,0x87F778,(self.actor,0))&255;self.invoke(0x4A9720,0,(self.actor,))
  techno_items=self.alloc(64);u.mem_write(techno_items,dwords(self.actor));u.mem_write(0xA8EC78,dwords(0x7EB6D4,techno_items,16,1,1,10))
  self.counter=self.alloc(64);u.mem_write(self.counter,dwords(1));u.mem_write(self.house+0x5564,dwords(0,self.counter,16,1,1))
  u.mem_write(self.current_cell+0x120,b'\x10')
  self.symbols={0:'null',self.actor:'mtnk',self.loco:'drive',self.loco+4:'drive_interface',self.typ:'MTNK',self.house:'house',**{p:f'cell_{x}_{y}' for (x,y),p in self.cells.items()}}
  if case.get('restore_target'):
   u.mem_write(self.actor+0xB0,dwords(1));u.mem_write(self.actor+0x2B8,dwords(self.cells[tuple(case['restore_target'])]))
  self.read_ranges.extend([('current_cell',self.current_cell,0x144),('impact_cell',self.selected,0x144),('logic_vector',0x87F778,24),('display_vectors',0x8A0360,120),('deferred_vector',0xB0F698,24),('owner_type_counter',self.counter,64)])
  self.initializers=dict(drive_vtable=hex(self.read32(self.loco+4)),drive_at_coord=hex(self.read32(self.read32(self.loco+4)+0xA0)),type_jumpjet=u.mem_read(self.typ+0xD94,1)[0],shake_screen=signed(self.read32(RULES+0x5C8)))
  if case.get('second_member'):self.add_second_member()
  # Independent original-seeded streams; the donor geometry's bootstrap does
  # not establish a loaded-game stream for this object composition.
  for p in self.rngs.values():self.invoke(0x65C6D0,p,(case.get('seed',0),))
  u.mem_write(0xA8ED84,dwords(1000));self.code_hash=sha(bytes(u.mem_read(0x401000,0x3E0000)));assert self.code_hash=='4cd5557a7490debc493ff965afc4483d8d2f1065f434f6b665cbb8fc4835b0cc';self.phase='measure'
 def add_second_member(self):
  u=self.u;p=self.alloc(0x800);loco=self.alloc(0x100)
  u.mem_write(SP,dwords(RET_MAGIC,self.typ,0));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,p);run_checked(u,0x7353C0,0x7354CE,count=500000)
  self.invoke(0x4AF540,loco);u.mem_write(loco+0xC,dwords(p));u.mem_write(loco+0x14,dwords(1));u.mem_write(p+0x674,dwords(loco+4))
  for off,size in ((0x6C,4),(0x74,1),(0x81,1),(0x8C,1),(0x8D,1),(0x90,1),(0x9C,12),(0xAC,4),(0xB0,4),(0xB4,4),(0x21C,4),(0x338,4),(0x3D5,1),(0x55C,4),(0x5E0,1),(0x684,1),(0x6D8,4)):
   u.mem_write(p+off,bytes(u.mem_read(self.actor+off,size)))
  for off,size in ((0x40,12),(0x50,8),(0x58,8),(0x63,1)):
   u.mem_write(loco+off,bytes(u.mem_read(self.loco+off,size)))
  self.invoke(0x5F5B90,p,(0,self.alloc(0x80)));u.mem_write(self.actor+0x30,dwords(p))
  assert self.invoke(0x55BAA0,0x87F778,(p,0))&255;self.invoke(0x4A9720,0,(p,));u.mem_write(self.counter,dwords(2))
  self.second=p;self.second_loco=loco;self.symbols[p]='mtnk2';self.symbols[loco]='drive2';self.symbols[loco+4]='drive2_interface'
  self.read_ranges.extend([('actor2',p,0x800),('drive2',loco,0x100)])
 def read_extra(self):
  u=self.u
  self.block(0x665E21,0x665E2B,{UC_X86_REG_ESI:RULES})
  physical=sound_sections((ASSETS/'SOUNDMD.INI').read_bytes());wanted={k:v for k,v in physical.items() if k in ('Defaults','GenVehicleDie')};wanted['SoundList']={k:v for k,v in physical['SoundList'].items() if v=='GenVehicleDie'}
  self.make_ini(wanted);u.mem_write(0x87E2A0,dwords(1));u.mem_write(0x87E294,dwords(self.alloc(0x100)));self.invoke(0x4072C0,0x87E250);u.mem_write(0xB1D378,dwords(0x7EB6D4,self.alloc(64),16,1,0,10));self.invoke(0x7510D0,INI)
  assert self.invoke(0x7514D0,self.cstring('GenVehicleDie'))==0
  self.extra_layers=[]
  map_path=self.scene['map_path']
  for name,p in [('RULESMD.INI',ASSETS/'RULESMD.INI'),('MPBattleMD.ini',ASSETS/'MPBattleMD.ini'),(map_path.name,map_path)]:
   raw=p.read_bytes();sec,_=lexical(raw,{'MTNK','General','AudioVisual'});self.make_ini(sec)
   if 'AudioVisual' in sec:self.block(0x66A6C0,0x66A6E9,{UC_X86_REG_ESI:RULES,UC_X86_REG_EDI:INI})
   if 'General' in sec:self.block(0x66DA90,0x66DB93,{UC_X86_REG_ESI:RULES,UC_X86_REG_EDI:INI})
   if 'MTNK' in sec:
    regs={UC_X86_REG_EBP:self.typ,UC_X86_REG_EBX:self.typ+0x24,UC_X86_REG_ESI:INI,UC_X86_REG_EDI:INI}
    for a,b in ((0x712587,0x7125DF),(0x71398B,0x713B85),(0x7134D9,0x713553),(0x7151E5,0x715227),(0x714CC8,0x714CE9)):
     self.block(a,b,regs)
   self.block(0x7476D3,0x747711,{UC_X86_REG_EDI:self.typ,UC_X86_REG_EBX:INI,UC_X86_REG_EBP:self.typ+0x24})
   self.extra_layers.append(dict(file=name,sha256=sha(raw),type=self.extra_state()))
  self.sound_input=dict(sha256=sha((ASSETS/'SOUNDMD.INI').read_bytes()),selected=wanted,samples=self.samples.copy(),registry_index=0)
 def extra_state(self):
  def vector(off,names=True):
   p=self.read32(self.typ+off+4);n=self.read32(self.typ+off+16)
   return [self.string(self.read32(p+i*4)+0x24) if names else signed(self.read32(p+i*4)) for i in range(n)]
  return dict(speed_type=signed(self.read32(self.typ+0x67C)),crusher=self.u.mem_read(self.typ+0xD28,1)[0],jumpjet=self.u.mem_read(self.typ+0xD94,1)[0],crashable=self.u.mem_read(self.typ+0xD95,1)[0],max_debris=signed(self.read32(self.typ+0x5BC)),min_debris=signed(self.read32(self.typ+0x5C0)),explosion=vector(0x72C),destroy_anim=vector(0x748),die_sound=vector(0x510,False))
 def who(self,p):return self.symbols.get(p,hex(p))
 def add(self,kind,**v):self.events2.append(dict(kind=kind,visit=self.visit,**v))
 def hook(self,u,pc,n,d):
  self.visit+=1;mark=len(self.calls)
  sp=u.reg_read(UC_X86_REG_ESP);this=u.reg_read(UC_X86_REG_ECX)
  if pc==0x4015C0:
   sample=self.string(u.reg_read(UC_X86_REG_EDX));self.samples.append(sample);self.ret(len(self.samples)-1);return
  if pc in (0x7C978A,0x6C8C40):assert self.phase=='setup';self.ret(0);return
  if self.phase=='measure':
   if self.return_events and pc==self.return_events[-1]['return_pc']:
    row=self.return_events.pop();row.pop('return_pc');row.update(visit=self.visit,result=signed(u.reg_read(UC_X86_REG_EAX)),result_al=u.reg_read(UC_X86_REG_EAX)&255);self.events2.append(row)
   if pc==0x575EE0:self.add('span_endpoint_notify',a=list(struct.unpack('<hh',u.mem_read(sp+4,4))),b=list(struct.unpack('<hh',u.mem_read(sp+8,4))))
   if pc==0x487A10:self.add('occupants',coord=self.cell_coord(this),mode_raw=self.read32(sp+4),mode_byte=u.mem_read(sp+4,1)[0],land=signed(self.read32(this+0xEC)))
   if pc in (0x73F0A0,0x51BF90):
    p=self.read32(sp+4);row=dict(kind='can_enter_return',actor=self.who(this),coord=self.cell_coord(p),land=signed(self.read32(p+0xEC)),args=[signed(self.read32(sp+8+i*4)) for i in range(4)]);self.add('can_enter',**{k:v for k,v in row.items() if k!='kind'});self.return_events.append(dict(row,return_pc=self.read32(sp)))
   if pc==0x65C84B:self.add('ranged_raw_draw',rng=next((k for k,p in self.rngs.items() if p==u.reg_read(UC_X86_REG_EDX)),hex(u.reg_read(UC_X86_REG_EDX))),raw_u32=u.reg_read(UC_X86_REG_ESI))
   if pc in (0x65C780,0x65C7E0):self.return_events.append(dict(kind='rng_return',rng=next((k for k,p in self.rngs.items() if p==this),hex(this)),operation='next' if pc==0x65C780 else 'range',return_pc=self.read32(sp)))
   if pc in (0x6FCDB0,0x741970):self.add('assign_target' if pc==0x6FCDB0 else 'assign_destination',this=self.who(this),target=self.who(self.read32(sp+4)),mission=signed(self.read32(this+0xAC)),timer=list(struct.unpack('<3i',u.mem_read(this+0x640,12))))
   if pc==0x4B4920:
    xyz=list(struct.unpack('<3i',u.mem_read(sp+8,12)));self.add('drive_at_coord',probe=xyz);self.return_events.append(dict(kind='drive_at_coord_return',probe=xyz,return_pc=self.read32(sp)))
   if pc in (0x4D3780,0x47EA90,0x47D2B0,0x70D4A0,0x4D8F80,0x7013E0,0x5B36B0,0x4DE5D0,0x5F65F0,0x7440B0,0x4DB260,0x6F6AC0,0x5F4D30,0x55BAE0,0x4A9770,0x406060):
    names={0x4D3780:'mark',0x47EA90:'remove_content',0x47D2B0:'recalc',0x70D4A0:'detach_target',0x4D8F80:'foot_restore',0x7013E0:'techno_restore',0x5B36B0:'mission_restore',0x4DE5D0:'foot_uninit',0x5F65F0:'object_uninit',0x7440B0:'unit_limbo',0x4DB260:'foot_limbo',0x6F6AC0:'techno_limbo',0x5F4D30:'object_limbo',0x55BAE0:'logic_remove',0x4A9770:'display_remove',0x406060:'release_sound'}
    self.add(names[pc],this=self.who(this),arg0=self.who(self.read32(sp+4)),health=signed(self.read32(self.actor+0x6C)))
   if pc==0x7509E0:
    self.add('sound_boundary',index=signed(this),xyz=list(struct.unpack('<3i',u.mem_read(u.reg_read(UC_X86_REG_EDX),12))),handle=self.who(self.read32(sp+4)),return_pc=hex(self.read32(sp)));self.ret(0,4);return
   if pc==0x421EA0:
    args=[self.read32(sp+4+i*4) for i in range(7)];self.add('anim_constructor_boundary',anim_type=self.string(args[0]+0x24),xyz=list(struct.unpack('<3i',u.mem_read(args[1],12))),remaining=args[2:],caller=hex(self.read32(sp)));self.ret(this,28);return
   if pc==0x7C8E17:
    p=self.alloc(self.read32(sp+4));self.add('allocation',size=self.read32(sp+4));self.ret(p);return
   if pc==0x7C8B3D:self.add('free',pointer=self.who(self.read32(sp+4)));self.ret(0);return
   if pc in (0x47FDE0,0x47FB90):p=self.read32(sp+4);u.mem_write(p,dwords(0,0,0,0));self.ret(p,4);return
   if pc==0x56C510:self.add('connectivity_boundary');self.ret(0);return
   if pc==0x586990:self.add('hierarchy_boundary');self.ret(0,4);return
   if pc==0x47DD70:self.add('redraw');self.ret(0);return
   if pc==0x6551C0:self.add('radar');self.ret(0,4);return
   if pc==0x6D2140:
    p=self.read32(sp+8);u.mem_write(p,dwords(0,0));self.ret(p,8);return
   if pc==0x6D2790:self.add('dirty_rect');self.ret(0,20);return
  super().hook(u,pc,n,d)
  for row in self.calls[mark:]:
   row['visit']=self.visit
   if 'health' in row:row['primary_health']=row.pop('health')
   if 'health_after' in row:row['primary_health_after']=row.pop('health_after')
 def cell_coord(self,p):return list(struct.unpack('<hh',self.u.mem_read(p+0x24,4)))
 def snapshot(self):
  u=self.u
  second=None
  if hasattr(self,'second'):
   p=self.second;second=dict(health=signed(self.read32(p+0x6C)),alive=u.mem_read(p+0x90,1)[0],limbo=u.mem_read(p+0x81,1)[0],marked=u.mem_read(p+0x74,1)[0],object_next=self.who(self.read32(p+0x30)),target=self.who(self.read32(p+0x2B4)))
  return dict(second=second,health=signed(self.read32(self.actor+0x6C)),alive=u.mem_read(self.actor+0x90,1)[0],marked=u.mem_read(self.actor+0x74,1)[0],limbo=u.mem_read(self.actor+0x81,1)[0],on_bridge=u.mem_read(self.actor+0x8C,1)[0],sink=u.mem_read(self.actor+0x3CD,1)[0],ground_head=self.who(self.read32(self.current_cell+0xE4)),object_next=self.who(self.read32(self.actor+0x30)),logic_count=self.read32(0x87F788),display_count=self.read32(0x8A03A0),deferred_count=self.read32(0xB0F6A8),owner_losses=self.read32(self.house+0x5434),owner_type_count=signed(self.read32(self.counter)),current_land=signed(self.read32(self.current_cell+0xEC)),impact_land=signed(self.read32(self.selected+0xEC)),target=self.who(self.read32(self.actor+0x2B4)),mission=signed(self.read32(self.actor+0xAC)),suspended_target=self.who(self.read32(self.actor+0x2B8)),nav=self.who(self.read32(self.actor+0x5A4)),suspended=signed(self.read32(self.actor+0xB0)),head=list(struct.unpack('<3i',u.mem_read(self.loco+0x40,12))),xyz=list(struct.unpack('<3i',u.mem_read(self.actor+0x9C,12))),fraction=struct.unpack('<d',u.mem_read(self.loco+0x50,8))[0],timer=list(struct.unpack('<3i',u.mem_read(self.actor+0x640,12))))
 def run(self):
  rows=[]
  for stage in ('first_damage','final_collapse'):
   self.active_stage=stage;self.visit=0;self.events2=[];self.calls=[];self.writes=[];self.accesses={};before=self.snapshot();rng0={k:bytes(self.u.mem_read(p,1012)).hex() for k,p in self.rngs.items()}
   self.u.mem_write(SP,dwords(self.selected+0x24));self.u.reg_write(UC_X86_REG_ESP,SP);self.u.reg_write(UC_X86_REG_EDI,self.selected)
   self.u.reg_write(UC_X86_REG_ECX,MAP)
   end=run_checked(self.u,self.scene['area_entry'],self.scene['area_stop'],count=5000000)
   assert not self.return_events,self.return_events
   rows.append(dict(stage=stage,before=before,after=self.snapshot(),events=self.events2,calls=self.calls,writes=self.writes,reads=[dict(region=k[0],offset=hex(k[1]),size=k[2],initial_hex=v)for k,v in sorted(self.accesses.items())],rng_before=rng0,rng_after={k:bytes(self.u.mem_read(p,1012)).hex() for k,p in self.rngs.items()},span=[[x,y,signed(self.read32(self.cells[x,y]+0x44)),signed(self.read32(self.cells[x,y]+0xEC))] for x,y in self.scene['span']]))
   assert sha(bytes(self.u.mem_read(0x401000,0x3E0000)))==self.code_hash
  return rows

def jumpjet_case(donor,t,mode):
 m=Occupied(donor,t,dict(name='jumpjet_mode',current_cell=[87,54]));u=m.u;m.phase='setup'
 # Full InfantryType constructor and admitted original selected field blocks.
 m.typ=m.alloc(0xF00);m.invoke(0x5236A0,m.typ,(m.cstring('JUMPJET'),));ctor=m.type_state();layers=[]
 for name,p in [('RULESMD.INI',ASSETS/'RULESMD.INI'),('MPBattleMD.ini',ASSETS/'MPBattleMD.ini'),('XMP03T4.MAP',identity.ASSETS/'XMP03T4.MAP')]:
  raw=p.read_bytes();sec,lines=lexical(raw,{'JUMPJET'});m.make_ini(sec)
  if 'JUMPJET' in sec:
   m.block(0x5F94B3,0x5F9516,{UC_X86_REG_EBX:m.typ,UC_X86_REG_ESI:INI,UC_X86_REG_EBP:m.typ+0x24,UC_X86_REG_EAX:u.mem_read(m.typ+0x231,1)[0]})
   for a,b in ((0x7121D1,0x7121EB),(0x712270,0x71228A),(0x7151E5,0x715227)):
    m.block(a,b,{UC_X86_REG_EBP:m.typ,UC_X86_REG_EBX:m.typ+0x24,UC_X86_REG_ESI:INI,UC_X86_REG_EDI:INI})
  layers.append(dict(file=name,sha256=sha(raw),sections=sec,source_lines=lines,type=m.type_state(),jumpjet=u.mem_read(m.typ+0xD94,1)[0]))
 # Full original base/Infantry initialization through InitFromType; stop before COM.
 actor=m.alloc(0x800);u.mem_write(0xA83DE8,dwords(0x7EB6D4,m.alloc(64),16,1,0,10))
 u.mem_write(SP,dwords(RET_MAGIC,m.typ,0));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,actor)
 run_checked(u,0x517A50,0x517B44,count=500000)
 m.actor=actor;m.symbols[actor]='jumpjet';m.symbols[m.typ]='JUMPJET'
 u.mem_write(actor+0x21C,dwords(m.house));u.mem_write(actor+0x6C,dwords(m.read32(m.typ+0xA0)));u.mem_write(actor+0x90,b'\1');u.mem_write(actor+0x81,b'\0');u.mem_write(actor+0x8C,b'\0')
 u.mem_write(actor+0x9C,dwords(22400,13952,416));u.mem_write(actor+0x55C,packed(87,54));u.mem_write(actor+0xAC,dwords(5));u.mem_write(actor+0xB0,dwords(-1))
 for cp in m.cells.values():u.mem_write(cp+0xE4,dwords(0))
 u.mem_write(m.selected+0xE4,dwords(actor));u.mem_write(m.selected+0x124,dwords(4))
 m.read_ranges=[('jumpjet',actor,0x800),('jumpjet_type',m.typ,0xF00),('selected_cell',m.selected,0x144)];m.calls=[];m.events2=[];m.writes=[];m.pending=[];m.return_events=[];m.visit=0;m.phase='measure'
 before={k:bytes(u.mem_read(p,1012)).hex()for k,p in m.rngs.items()}
 u.mem_write(SP,dwords(RET_MAGIC,mode));u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,m.selected)
 endpoint=run_checked(u,0x487A10,(RET_MAGIC,0x517FA0),count=500000)
 packet=None
 if endpoint==0x517FA0:
  sp=u.reg_read(UC_X86_REG_ESP);args=[m.read32(sp+4+i*4)for i in range(7)];packet=dict(damage=signed(m.read32(args[0])),health_alias=args[0]==actor+0x6C,distance=signed(args[1]),warhead=m.string(args[2]+0x24),attacker=args[3],ignore_defenses=args[4],arg6=args[5],source_house=args[6])
 assert sha(bytes(u.mem_read(0x401000,0x3E0000)))==m.code_hash
 return dict(mode_raw=mode,mode_byte=mode&255,constructor=ctor,layers=layers,actor_vtable=hex(m.read32(actor)),actor_flags=m.read32(actor+0x14),health=signed(m.read32(actor+0x6C)),endpoint=hex(endpoint),damage_packet=packet,events=m.events2,calls=m.calls,writes=m.writes,rng_before=before,rng_after={k:bytes(u.mem_read(p,1012)).hex()for k,p in m.rngs.items()})

def generate():
 t=identity.theater();r,fields=resident_rules();case=inputs(t);donor=Resident(case,r,t);rows=[]
 for c in [dict(name='resident_center',current_cell=[87,54]),dict(name='north_head_center',current_cell=[87,53],head_cell=[87,54]),dict(name='north_stationary',current_cell=[87,53],target_impact=True),dict(name='north_restore_replacement',current_cell=[87,53],target_impact=True,restore_target=[87,52]),dict(name='north_restore_same',current_cell=[87,53],target_impact=True,restore_target=[87,54]),dict(name='resident_two_members',current_cell=[87,54],second_member=True),dict(name='north_two_heads',current_cell=[87,53],head_cell=[87,54],second_member=True)]:
  m=Occupied(donor,t,c)
  try:result=m.run()
  except Exception:
   print(json.dumps(dict(case=c,events=m.events2,calls=m.calls[-30:],snapshot=m.snapshot()),indent=2));raise
  rows.append(dict(input=c,initializers=m.initializers,native_inputs=m.inputs,extra_layers=m.extra_layers,result=result))
 jumpjet=[jumpjet_case(donor,t,mode) for mode in (0,1,256,257)]
 return dict(schema=1,code_sha256=m.code_hash,fpcw_supplied=hex(m.u.reg_read(UC_X86_REG_FPCW)),base_reader_layers=m.layers,jumpjet=jumpjet,input=case,assets=donor.assets,overlay_rules=fields,sound=m.sound_input,cases=rows)

def metadata():
 from unicorn import Uc,UC_ARCH_X86,UC_MODE_32
 from tools.native_oracle import load_image
 u=Uc(UC_ARCH_X86,UC_MODE_32);load_image(u)
 p=provenance(scope=__doc__,assumptions=[
  'Physical XMP03T4 projection and original all23 selected Recalc from frozen anytown_resident. Measured original admitted area continuation48A2B4..48A2C4 calls real57CCF0 and conditionally real70D4A0; no Scenario damage roll, firing or projectile is composed here.',
  'Actual Cell487A10, Unit73F0A0, Drive4B4920, Unit/Foot/Techno/Object damage and marked removal execute. Heap objects, House, mission, cell/Techno membership, frame1000 and retained heads are supplied active-state projections, not native Unlimbo/order/head production.',
  'MTNK original UnitType7470D0 constructor, selected shared readers and actual UnitType7476D3..747711 postpass execute. +D28 is exact Crusher, not Wheeled; physical Crusher=yes derives SpeedType1 Track. Native inputs include full Super reader, land rows, physical DieSound, Explosion, DestroyAnim, Max/MinDebris, MetallicDebris, JumpJet, Crashable and AudioVisual ShakeScreen. The inherited base-reader map layer is physical XShrapnel with no relevant MTNK/land override; Anytown has no selected override. Actual three layer sources/hashes remain explicit; no fresh full native Rules or scenario loader claim.',
  'GenVehicleDie is loaded by original7510D0/7514D0 from physical SOUNDMD Defaults/SoundList/section strings into a one-entry relative registry0. Sample resolution is an explicit IO boundary; its native class/audio flags still execute. Index0 identifies GenVehicleDie in this reduced registry, not its full retail ordinal.',
  'Full original InfantryType5236A0 and Infantry/base ctor prefix517A50..517B44 establish the separate JUMPJET control. Selected actual Strength/Armor/Immune/SpeedType/JumpJet readers execute. Resident-only query needs no active locomotor. mode0/256 return through Cell; mode1/257 stop BEFORE actual517FA0 and record its packet. JumpJet lifetime and physical flight/landing reachability excluded.',
  'Original two-stage MTNK rows include no suspended mission, replacement restoration, same-target restoration and two-member resident versus neighboring-list controls. Actual original constructors initialize both tank objects; world list insertion and retained heads are supplied. The second tank has no suspended mission or target.',
  'All three complete0x3F4-byte RNG states come from original65C6D0 seed0 after setup and bracket every measured visit. Next requests65C780, ranged requests65C7E0, raw ranged draws65C84B and results are separately recorded. Ambient x87 FPCW0E7F is supplied, not established as an active-game invariant. Raw padding writes such as Timer+178 are recorded mechanically; meaningful RadarCombatFlash timer fields are+174/+17C.',
  'calls and events have common instruction-visit numbers, allowing exact interleaving. Inherited call health fields are explicitly primary_health for the main fixture tank; receiver identity and packet remain separate. reads are first-access raw bytes, writes preserve original PCs. No .text byte is patched; full mapped text has the expected original checksum before and after each case.'
 ],substitutions=[
  'Setup INI signed-CRC caches replace physical file parsing; allocation/free/CRT/OS Interlocked use inherited bounded seams. No CanEnter, AtCoord, damage, Mark, list removal, Restore, AssignTarget or Detach callback returns are supplied.',
  'Animation421EA0 and positional sound7509E0 stop at observed request boundaries, with constructor storage returned to the original caller. Their playback/lifetime and additional RNG are excluded; this is not a complete spawned-effect world trace.',
  'Connectivity56C510 and hierarchy586990 remain recorded return boundaries. Renderer/radar/dirty-rectangle extents are presentation sinks. Untagged span575EE0 executes; no live trigger/Team/Aircraft-patrol/airstrike exception is supplied.'
 ],entry_points={'area_continuation':0x48A2B4,'driver':0x57CCF0,'span_notify':0x575EE0,'occupants':0x487A10,'can_enter':0x73F0A0,'drive_at_coord':0x4B4920,'unit_damage':0x737C90,'detach':0x70D4A0,'foot_restore':0x4D8F80,'unit_speed_postpass':0x7476D3,'jumpjet_reader':0x7151E5,'infantry_ctor_prefix':0x517A50,'infantry_can_enter':0x51BF90,'infantry_damage_boundary':0x517FA0})
 spans=[(0x48A2B4,0x48A2C4),(0x487A10,0x487C15),(0x70D4A0,0x70D581),(0x4D8F80,0x4D8FA7),(0x7013E0,0x701405),(0x7476D3,0x747711),(0x714CC8,0x714CE9),(0x7151E5,0x715227),(0x517A50,0x517B44)]
 p['original_slices']=[dict(start=hex(a),end_exclusive=hex(b),sha256=sha(bytes(u.mem_read(a,b-a))),hex=bytes(u.mem_read(a,b-a)).hex())for a,b in spans]
 return p

from .publication import finish_vectors

if __name__=='__main__':finish_vectors(generate,HERE/'anytown_occupants.json.gz',provenance=metadata)
