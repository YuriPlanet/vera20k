"""Original selected retail sound registry, Anim Report binding and release/stop.

Use VERA20K_BRIDGE_CHILD_SOUND_ASSETS for extracted SOUNDMD.INI and
VERA20K_BRIDGE_ANIM_ASSETS for the independent ART/SHP input directory.
The selected SoundList entries retain their original keys and source order;
the other sound types and physical INI/archive loading are outside this fixture.
"""
import json,struct,hashlib,os
from pathlib import Path
from unicorn.x86_const import *
from tools.rules_oracle.bridge_anim_inputs import Reader,crc,physical_sections
from tools.spatial_oracle.building_body_rules import INI,SP,dwords
from tools.native_oracle import run_checked,RET_MAGIC,finish_vectors,provenance
ROOT=Path(os.environ.get('VERA20K_BRIDGE_CHILD_SOUND_ASSETS',str(Path(os.environ.get('CARGO_TARGET_DIR','target'))/'asset/bridge-child-sound/extract')))
def sections(raw):
 out={};cur=None
 for line in raw.decode('latin1').splitlines():
  line=line.split(';',1)[0].strip()
  if line.startswith('[') and ']' in line:cur={};out[line[1:line.index(']')]]=cur;continue
  if cur is not None and '=' in line:
   k,v=map(str.strip,line.split('=',1))
   if k and v:cur[k]=v
 return out

def playing_handle(m, entry, *, handle=None, serial=41, event=None):
 """Supply this fixture's tagged playing-event boundary in an existing VM.

 No device channel or sample buffer is attached. Original Release/Stop bodies
 own every later mutation; callers may attach the handle to their real object.
 """
 if event is None:event=m.alloc(0x300)
 if handle is None:handle=m.alloc(16)
 m.u.mem_write(event+0x24,dwords(entry));m.u.mem_write(event+0x138,dwords(serial))
 m.u.mem_write(event+0x1c,dwords(3));m.u.mem_write(event+0x18,dwords(8))
 m.u.mem_write(handle,dwords(event,serial,entry,0x87e294))
 return event,handle

class Sound(Reader):
 def __init__(self):
  self.samples=[];self.calls=[]
  raw=(ROOT/'SOUNDMD.INI').read_bytes();self.raw=raw;self.physical=sections(raw)
  selected={k:v for k,v in self.physical.items() if k in ('Defaults','Explosion06','ExplosionShard')}
  selected['SoundList']={k:v for k,v in self.physical['SoundList'].items() if v in ('Explosion06','ExplosionShard')}
  super().__init__(ROOT,selected)
  self.selected=selected
  self.u.mem_write(0x87e2a0,dwords(1));self.u.mem_write(0x87e294,dwords(self.alloc(0x100)))
  self.invoke(0x4072c0,0x87e250)
  self.u.mem_write(0xb1d378,dwords(0x7eb6d4,self.alloc(64),16,1,0,10))
  self.samples=[];self.invoke(0x7510d0,INI)
  self.registry_samples=self.samples.copy()
 def make_ini(self,sections):
  super().make_ini(sections)
  # Supply the source-order entry links as well as Reader's CRC index.
  index=self.read32(INI+0x28)
  for i in range(self.read32(INI+0x2c)):
   sec=self.read32(index+i*8+4);name=self.string(self.read32(sec+0xc));keys=sections[name]
   entryindex=self.read32(sec+0x2c);entries={self.read32(entryindex+j*8):self.read32(entryindex+j*8+4) for j in range(len(keys))}
   ordered=[entries[crc(key)] for key in keys];sentinel=self.alloc(0x20)
   if ordered:self.u.mem_write(sec+0x18,dwords(ordered[0]))
   for j,p in enumerate(ordered):self.u.mem_write(p+4,dwords(ordered[j+1] if j+1<len(ordered) else sentinel,ordered[j-1] if j else sentinel))
 def hook(self,u,p,n,d):
  if p==0x4015c0:
   name=self.string(u.reg_read(UC_X86_REG_EDX));self.samples.append(name)
   self.ret({'gexp06a':0,'gexpshaa':1}[name.lower()]);return
  if p in (0x406060,0x405d40,0x4052f0,0x65c7e0,0x65c780):self.calls.append(hex(p))
  super().hook(u,p,n,d)
 def read(self,name):
  index=self.invoke(0x7514d0,self.cstring(name));assert index<self.read32(0xb1d388)
  voc=self.read32(self.read32(0xb1d37c)+index*4);ptr=self.read32(voc);ok=1
  sample_ids=[self.read32(ptr+0xb4+i*4) for i in range(self.read32(ptr+0x134))]
  self.samples=[{0:'gexp06a',1:'gexpshaa'}[i] for i in sample_ids]
  keys={'control':0x10,'type_flags':0x14,'volume_fixed16_raw':0x1c,'priority':0x40,'limit':0x48,'loop_count':0x4c,'range':0x50,'delay_min':0x58,'delay_max':0x5c,'fshift_min':0x60,'fshift_max':0x64,'vshift':0x68,'sample_count':0x134}
  return ptr,dict(name=name,registry_index=index,admitted=ok,volume_linear=self.read32(ptr+0x1c)>>16,samples=self.samples.copy(),**{k:struct.unpack('<i',self.u.mem_read(ptr+v,4))[0] for k,v in keys.items()})
 def release(self,entry,fn):
  e,h=playing_handle(self,entry)
  before=bytes(self.u.mem_read(e,0x300));self.calls=[];self.invoke(fn,h)
  after=bytes(self.u.mem_read(e,0x300))
  return dict(function=hex(fn),event_unchanged=before==after,event_state=self.read32(e+0x1c),event_flags=self.read32(e+0x18),event_serial=self.read32(e+0x138),handle_event_is_present=bool(self.read32(h)),handle_sound_is_present=bool(self.read32(h+8)),calls=self.calls.copy())
def generate():
 m=Sound();out=[]
 for n in ['Explosion06','ExplosionShard']:
  p,r=m.read(n);r['release']=m.release(p,0x406060);r['hard_stop']=m.release(p,0x405d40);out.append(r)
 art_root=Path(os.environ.get('VERA20K_BRIDGE_ANIM_ASSETS',str(Path(os.environ.get('CARGO_TARGET_DIR','target'))/'asset/bridge-anim-inputs/extract')));art=(art_root/'ARTMD.INI').read_bytes()
 m.assets.update({p.name.upper():p.read_bytes() for p in art_root.glob('*') if p.is_file()})
 m.make_ini(physical_sections(art));anims=[]
 for name in ['SMOKEY2','TWLT026','TWLT036']:
  ptr=m.alloc(0x400);m.invoke(0x427530,ptr,(m.cstring(name),));assert m.invoke(0x427d00,ptr,(INI,))
  report=struct.unpack('<i',m.u.mem_read(ptr+0x2f8,4))[0]
  report_name=None
  if report>=0:report_name=m.string(m.read32(m.read32(m.read32(0xb1d37c)+report*4))+0x6c)
  anims.append(dict(name=name,report_index=report,report_name=report_name,stop_sound_index=struct.unpack('<i',m.u.mem_read(ptr+0x2fc,4))[0]))
 return dict(sound_sha256=hashlib.sha256(m.raw).hexdigest(),sound_raw=m.selected,rows=out,anim_reports=anims,art_sha256=hashlib.sha256(art).hexdigest())
def metadata():
 return provenance(scope='Selected retail Explosion06/ExplosionShard original sound registry/readers, SMOKEY2/TWLT026/TWLT036 original ART Report binding, original handle release versus hard-stop',
  assumptions=[
   'Original7510D0 executes with supplied source-order linked/CRC INI indexes from physical SOUNDMD Defaults and actual SoundList keys280/291; other registry entries are excluded. Numeric defaults/parsers/control tokenization/factory and full750440 bodies execute unchanged.',
   'Original427530/427D00 ART readers resolve the three selected Anim reports against the native-created sound registry. Registry indices0/1 are fixture-relative; output identities are the original native retained names, not claims of full retail absolute sound indices.',
   'Release and hard-stop each receive a prepared valid tagged handle and a playing event with serial41, flags8, no attached device channel/sample buffers. Full406060/405D40 bodies execute; outputs record lifecycle state, not rendered audio.',
   'Physical winning bag metadata independently confirms gexp06a and gexpshaa exist; the archive lookup callback maps these to fixture-local indices0/1. Sound sample decoding, pool admission, device mixing and audio RNG execution are outside this run.',
   'Original4055C0 and4047B0 instructions route pitch/volume/playlist draws to MainRng886B88, not Scenario; this corpus does not execute those playback draws.'],
  substitutions=[
   'Original allocator uses inherited bump allocation; deletion is inert; CRT TLS is supplied. No original instructions are patched.',
   'AudioIndex::FindSample4015C0 accepts the exact selected physical bag names and returns fixture-local indices. Native AddSample stores/duplicates those indices itself.',
   'Anim image archive IO returns unchanged physical SHP bytes; physical INI loading and MIX traversal are outside the prepared-cache boundary.'],
  entry_points={'sound_list':0x7510D0,'sound_factory':0x4063B0,'voc_read':0x750440,
   'anim_read':0x427D00,'release':0x406060,'hard_stop':0x405D40})
if __name__=='__main__':
 finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
