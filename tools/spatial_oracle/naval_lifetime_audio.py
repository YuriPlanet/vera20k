"""Bounded original sinking sound readers and FootAI edge/selection controls.

Extends, and does not modify, frozen naval_occupants evidence. Prepared INI
caches and selected SoundList are explicit boundaries; 7509E0 is an observed
void playback boundary. No audible output or whole audio RNG claim is made.
"""
from tools.spatial_oracle.naval_occupants import *
from tools.rules_oracle.bridge_anim_inputs import crc
from tools.rules_oracle.bridge_child_sound import sections

MODE=ASSETS/'MPBattleMD.ini'
SOUND=ASSETS/'SOUNDMD.INI'

class Audio(Native):
 def make_ini(self,sections):
  super().make_ini(sections)
  index=self.read32(INI+0x28)
  for i in range(self.read32(INI+0x2C)):
   sec=self.read32(index+i*8+4);keys=sections[self.string(self.read32(sec+0xC))]
   table=self.read32(sec+0x2C);entries={self.read32(table+j*8):self.read32(table+j*8+4) for j in range(len(keys))}
   ordered=[entries[crc(k)] for k in keys];end=self.alloc(0x20)
   if ordered:self.u.mem_write(sec+0x18,dwords(ordered[0]))
   for j,p in enumerate(ordered):self.u.mem_write(p+4,dwords(ordered[j+1] if j+1<len(ordered) else end,ordered[j-1] if j else end))
 def __init__(self):
  self.samples=[];self.audio_events=[]
  super().__init__(next(c for c in inputs() if c['name']=='west_water_head_road_dz0'))
  self.phase='setup';u=self.u
  assert sha(MODE.read_bytes())=='50406e81d7523f6be1954daab6b25bd85a8347c455f3d53dcf515d7f719b4963'
  self.constructor=dict(type_sinking=signed(self.read32(self.typ+0x548)),voice_sinking=signed(self.read32(self.typ+0x554)))
  self.block(0x665940,0x665946,{UC_X86_REG_ESI:RULES,UC_X86_REG_EBP:0xFFFFFFFF})
  self.constructor['rules_sinking']=signed(self.read32(RULES+0x208))
  physical=sections(SOUND.read_bytes());self.sound_sections={k:v for k,v in physical.items() if k in ('Defaults','GenLargeWaterDie')}
  self.sound_sections['SoundList']={k:v for k,v in physical['SoundList'].items() if v=='GenLargeWaterDie'}
  self.make_ini(self.sound_sections)
  u.mem_write(0x87E2A0,dwords(1));u.mem_write(0x87E294,dwords(self.alloc(0x100)))
  self.invoke(0x4072C0,0x87E250)
  u.mem_write(0xB1D378,dwords(0x7EB6D4,self.alloc(64),16,1,0,10))
  self.invoke(0x7510D0,INI)
  idx=self.invoke(0x7514D0,self.cstring('GenLargeWaterDie'));assert idx==0
  self.sound_type=self.read32(self.read32(self.read32(0xB1D37C)))
  self.sound=dict(index=idx,name=self.string(self.sound_type+0x6C),sample_names=self.samples.copy(),fields={hex(k):signed(self.read32(self.sound_type+k)) for k in (0x10,0x14,0x1C,0x40,0x48,0x4C,0x50,0x58,0x5C,0x60,0x64,0x68,0x134)})
  self.sound_layers=[]
  for name,path in [('RULESMD.INI',ASSETS/'RULESMD.INI'),('MPBattleMD.ini',MODE),('XShrapnel.MAP',ASSETS/'XShrapnel.MAP')]:
   raw=path.read_bytes();sec,lines=lexical(raw,{'AudioVisual','AEGIS'});self.make_ini(sec)
   before=self.sound_state();self.read_sound_blocks(sec);self.sound_layers.append(dict(name=name,sha256=sha(raw),sections=sec,before=before,after=self.sound_state()))
  self.phase='measure'
 def sound_state(self):return dict(type_sinking=signed(self.read32(self.typ+0x548)),voice_sinking=signed(self.read32(self.typ+0x554)),rules_sinking=signed(self.read32(RULES+0x208)))
 def read_sound_blocks(self,sec):
  if 'AudioVisual' in sec:self.block(0x669986,0x6699CE,{UC_X86_REG_ESI:RULES,UC_X86_REG_EDI:INI,UC_X86_REG_EAX:self.read32(RULES+0x204)})
  if 'AEGIS' in sec:
   self.block(0x712FA1,0x712FF7,{UC_X86_REG_EBP:self.typ,UC_X86_REG_ESI:INI,UC_X86_REG_EBX:self.typ+0x24,UC_X86_REG_EAX:self.read32(self.typ+0x544)})
   self.block(0x713055,0x7130AB,{UC_X86_REG_EBP:self.typ,UC_X86_REG_ESI:INI,UC_X86_REG_EBX:self.typ+0x24,UC_X86_REG_EAX:self.read32(self.typ+0x550)})
 def hook(self,u,pc,n,d):
  if pc==0x4015C0:
   name=self.string(u.reg_read(UC_X86_REG_EDX));assert name.lower()=='gnavsina',name
   self.samples.append(name);self.ret(0);return
  if self.phase=='measure' and pc==0x7509E0:
   sp=u.reg_read(UC_X86_REG_ESP);handle=self.read32(sp+4)
   self.audio_events.append(dict(kind='sound_boundary',pc=hex(pc),index=signed(u.reg_read(UC_X86_REG_ECX)),xyz=list(struct.unpack('<3i',u.mem_read(u.reg_read(UC_X86_REG_EDX),12))),handle='Foot+544' if handle==self.actor+0x544 else hex(handle),return_pc=hex(self.read32(sp))))
   self.ret(0,4);return
  if self.phase=='measure' and pc==0x406060:self.audio_events.append(dict(kind='original_release',handle='Foot+544' if u.reg_read(UC_X86_REG_ECX)==self.actor+0x544 else hex(u.reg_read(UC_X86_REG_ECX))))
  super().hook(u,pc,n,d)
 def edge(self,case):
  u=self.u;self.audio_events=[];self.writes=[];self.calls=[]
  u.mem_write(self.actor+0x3CD,bytes([case['sinking'],case['seen']]))
  u.mem_write(self.actor+0x53C,bytes([case.get('move_sound',0)]))
  if case.get('fallback'):u.mem_write(self.typ+0x548,dwords(-1))
  if case.get('voice'):u.mem_write(self.typ+0x554,dwords(0))
  if case.get('quiet'):u.mem_write(self.typ+0x548,dwords(-1));u.mem_write(RULES+0x208,dwords(-1))
  before={k:bytes(u.mem_read(p,0x3F4)).hex() for k,p in self.rngs.items()}
  self.block(0x4DABC7,0x4DACDD,{UC_X86_REG_ESI:self.actor,UC_X86_REG_EBX:0,UC_X86_REG_EDI:0xFFFFFFFF})
  return dict(seen=u.mem_read(self.actor+0x3CE,1)[0],events=self.audio_events,writes=self.writes,rng_before=before,rng_after={k:bytes(u.mem_read(p,0x3F4)).hex() for k,p in self.rngs.items()})

def generate():
 cases=[];m=None
 for case in [dict(name='type_rising',sinking=1,seen=0),dict(name='type_stable',sinking=1,seen=1),dict(name='type_raw255_rising',sinking=255,seen=0),dict(name='type_raw255_after1',sinking=255,seen=1),dict(name='rules_fallback',sinking=1,seen=0,fallback=True),dict(name='voice_then_type',sinking=1,seen=0,voice=True),dict(name='both_silent',sinking=1,seen=0,quiet=True),dict(name='falling_empty_release',sinking=0,seen=1),dict(name='falling_move_active_no_release',sinking=0,seen=1,move_sound=1)]:
  m=Audio();cases.append(dict(input=case,output=m.edge(case)))
 return dict(schema=1,constructor=m.constructor,sound=m.sound,sound_sections=m.sound_sections,sound_sha256=sha(SOUND.read_bytes()),layers=m.sound_layers,cases=cases)

def metadata():
 return provenance(scope=__doc__,assumptions=['Original UnitType constructor7470D0 and Rules constructor store665940 establish -1. Selected actual7510D0/750440/7514D0 registry resolves physical GenLargeWaterDie with its original SoundList key; index0 is fixture relative.', 'Actual AudioVisual/SinkingSound block669986..6699CE and type SinkingSound712FA1..712FF7/VoiceSinking713055..7130AB execute over physical RULESMD, active MPBattleMD and XShrapnel caches. Native full loader not claimed; The physical MPBattleMD fixture hash is asserted.', '4DABC7..4DACDD executes a supplied FootAI interior frame ESIactor,EBX0,EDI-1; preceding FootAI, movement audio and UnitAI cadence remain separate. All three full RNG states bracket this bounded edge.'],substitutions=['Inherited native setup allocator/CRT/Interlocked boundaries.', 'AudioIndex4015C0 maps physical sample name gnavsina to local index0; audio bag and sample decode excluded.', '7509E0 is a declared observed void playback boundary; full playback, device output and its Main RNG draws excluded. Actual406060 release executes on an empty constructor handle.'],entry_points={'edge':0x4DABC7,'sound':0x7509E0,'release':0x406060,'rules_read':0x669986,'type_sound_read':0x712FA1,'type_voice_read':0x713055})
if __name__=='__main__':finish_vectors(generate,HERE/'naval_lifetime_audio.json',provenance=metadata)
