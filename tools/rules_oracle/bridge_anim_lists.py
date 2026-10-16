"""Original RulesClass bridge animation vectors: constructors and ReadGeneral.

Run from the repository root with PYTHONPATH=. and VERA20K_GAMEMD_EXE set:
  python tools/rules_oracle/bridge_anim_lists.py --check

INI lookup indexes are supplied; the original ReadString, strtok, AnimType
factory/constructors and vector copy execute. This does not execute physical INI
loading, ART loading or animation playback. Rust consumers compare the retained
lists through RulesLayerStack and RuleSet in native_processing_tests.rs.
"""
import json, struct
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *
from tools.native_oracle import run_checked, finish_vectors, provenance
from tools.spatial_oracle.building_body_rules import Fixture, TYPE, INI, SP, dwords
HEAP=0x24000000
KEYS={'MetallicDebris':(0x83CEF0,0x66DA90,0x66DB93,0x13C), 'BridgeExplosions':(0x83CEDC,0x66DB93,0x66DC96,0x158)}
class Lists:
 def __init__(self,*,arena_size=0x400000):
  if arena_size<0x400000 or arena_size%0x1000:raise ValueError('Input arena must be at least4MiB and page aligned')
  self.arena_size=arena_size
  self.f=Fixture();self.u=self.f.u;self.u.mem_map(HEAP,arena_size);self.cursor=HEAP+0x10000
  self.events=[];self.read_result=None;self.u.hook_add(UC_HOOK_CODE,self.hook)
  for address,items in ((0x8B4150,HEAP+0x1000),(0xB0F670,HEAP+0x4000)):
   self.u.mem_write(address,dwords(0x7EB6D4,items,1024,1,0,10))
  self.initialize_vectors()
  self.initial={k:self.state(k) for k in KEYS}
 def reserve_arena(self,size):
  """Grow the same input allocation owner without changing prior pointers.

  Reader's physical SHPs and native operator-new share this cursor/capacity.
  Runtime JoinedFixture copies the declared mapped capacity. Existing callers
  retain the historical4MiB mapping; growth is explicit and page aligned.
  """
  if size<self.arena_size or size%0x1000:raise ValueError('Input arena must grow in whole pages')
  if size>self.arena_size:self.u.mem_map(HEAP+self.arena_size,size-self.arena_size)
  self.arena_size=size
 def initialize_vectors(self,rules_ptr=TYPE):
  """Run the same native vector constructors on a composed Rules object."""
  self.u.reg_write(UC_X86_REG_ESP,SP);self.u.reg_write(UC_X86_REG_ESI,rules_ptr)
  self.u.reg_write(UC_X86_REG_EBX,0);self.u.reg_write(UC_X86_REG_EDI,10)
  run_checked(self.u,0x665827,0x66585F)
 def read32(self,p):return struct.unpack('<I',self.u.mem_read(p,4))[0]
 def string(self,p):return bytes(self.u.mem_read(p,512)).split(b'\0')[0].decode('latin1')
 def ret(self,eax,cleanup=0):
  sp=self.u.reg_read(UC_X86_REG_ESP);ret=self.read32(sp)
  self.u.reg_write(UC_X86_REG_EAX,eax);self.u.reg_write(UC_X86_REG_ESP,sp+4+cleanup);self.u.reg_write(UC_X86_REG_EIP,ret)
 def hook(self,u,p,n,user):
  if p==0x7C8E17:
   size=self.read32(u.reg_read(UC_X86_REG_ESP)+4);result=self.cursor;self.cursor+=(size+15)&~15
   assert self.cursor<HEAP+self.arena_size
   self.ret(result)
  elif p==0x7C8B3D:self.ret(0)
  elif p==0x7D140B:self.ret(HEAP+0x8000)
  elif p==0x428B80:self.events.append(self.string(u.reg_read(UC_X86_REG_ECX)))
  elif p in (0x66DAB2,0x66DBB5):self.read_result={'length':u.reg_read(UC_X86_REG_EAX),'value':self.string(SP+0x50)}
 def state(self,key,rules_ptr=TYPE):
  off=KEYS[key][3];p=self.read32(rules_ptr+off+4);n=self.read32(rules_ptr+off+16)
  return {'data_is_null':p==0,'count':n,'names':[self.string(self.read32(p+i*4)+0x24) for i in range(n)]}
 def read(self,key,raw,rules_ptr=TYPE):
  p,begin,end,off=KEYS[key];self.f.ini(p,raw);self.f.write(INI+4,0x826278)
  self.u.reg_write(UC_X86_REG_ESP,SP);self.u.reg_write(UC_X86_REG_ESI,rules_ptr);self.u.reg_write(UC_X86_REG_EDI,INI)
  self.events=[];self.read_result=None
  run_checked(self.u,begin,end,count=2000000,required_addresses=(0x528A10,))
  assert self.u.reg_read(UC_X86_REG_ESP)==SP
  return {'key':key,'raw':raw,'native_read_string':self.read_result,'find_or_allocate_inputs':list(self.events),'result':self.state(key,rules_ptr)}
def generate():
 s=Lists();rows=[]
 for key in KEYS:
  for raw in ['ONE,TWO',None,'','   ', ',,,',
              'ONE, none,NONE,<none>,TWO,ONE',
              'abcdefghijklmnopqrstuvwxyz1234,abcdefghijklmnopqrstuvwxyz1234',
              'MixedCase,mIXEDcASE, MIXEDCASE , ,ONE',
              'none,<none>']:
   rows.append(s.read(key,raw))
 inputs=json.loads(Path(__file__).with_name('bridge_anim_list_inputs.json').read_text())
 for layer in inputs:
  if layer.get('absent'):continue
  for key in KEYS:rows.append({'file':layer['file'],**s.read(key,layer[key])})
 return {'constructor':s.initial,'rows':rows}


if __name__=='__main__':
 finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
  scope='Original Rules two vector constructors and complete MetallicDebris/BridgeExplosions ReadGeneral blocks',
  assumptions=[
   'Supplied cached INI indexes. Retail input strings exported by production AssetManager and IniFile from RULESMD.INI, optional LANGRULE.INI, MPBattleMD.ini and Hills.mmx; no native physical INI load.',
   'Fresh AnimType registry with spare capacity, full original FindOrAllocate and constructors, original vector storage/copy routines. No AnimType ART read or game production claim.',
   'Rows execute sequentially in one RulesClass and AnimType registry; missing/empty/whitespace reads retain vectors while comma-only input replaces with empty.'],
  substitutions=[
   'operator_new returns bump storage; operator_delete is a no-op; CRT TLS accessor returns supplied per-thread storage for original strtok'],
  entry_points={'constructor':0x665827,'metallic_list':0x66DA90,'explosion_list':0x66DB93,
                'read_string':0x528A10,'strtok':0x7C9CC2,'find_or_allocate':0x428B80,'anim_type_ctor':0x427530}))
