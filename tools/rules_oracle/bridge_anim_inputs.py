"""Original AnimType ART reader independently establishes bridge oracle inputs.

Set VERA20K_BRIDGE_ANIM_ASSETS to a directory containing extracted ARTMD.INI
and the 23 physical SHPs named in bridge_anim_inputs.md. Run with PYTHONPATH=.
and VERA20K_GAMEMD_EXE; --write records and --check reproduces native results.
No VERA scalar/parser output initializes native AnimType fields. Both existing
production-export input fixtures are asserted against the native results.
"""
import json,struct,hashlib,os,uuid
from pathlib import Path
from tools.rules_oracle.bridge_anim_lists import Lists,HEAP
from tools.projectile_oracle.flat_art import crc
from tools.spatial_oracle.building_body_rules import INI,SP,dwords
from tools.native_oracle import run_checked,RET_MAGIC,finish_vectors,provenance
from unicorn.x86_const import *
DEFAULT_ASSETS=Path(os.environ.get('CARGO_TARGET_DIR','target'))/'asset/bridge-anim-inputs/extract'
ROOT=Path(os.environ.get('VERA20K_BRIDGE_ANIM_ASSETS',str(DEFAULT_ASSETS)))
HERE=Path(__file__).resolve().parent
NAMES=[f'DBRIS{i}LG' for i in range(1,10)]+['DBRS10LG']+[f'DBRIS{i}SM' for i in range(1,5)]+['D','TWLT026','TWLT036','TWLT050','TWLT070','WAKE1','H2O_EXP1','H2O_EXP2','H2O_EXP3','SMOKEY2']
SCALARS={'start':0x2b4,'loop_start':0x2b8,'loop_end':0x2bc,'end':0x2c0,'loop_count':0x2c4,'rate':0x2b0,'damage_radius':0x334,'trailer_seperation':0x30c}
DOUBLES={'damage_f64_bits':0x2a8,'elasticity_f64_bits':0x310,'min_z_vel_f64_bits':0x318,'max_z_vel_f64_bits':0x320,'max_xy_vel_f64_bits':0x328}
BOOLS={'bouncer':0x35a,'normalized':0x362,'scorch':0x36b,'crater':0x36d,'shadow':0x372}
REFS={'bounce_anim':0x300,'expire_anim':0x304,'trailer_anim':0x308,'warhead':0x330}
def physical_sections(raw):
 # Cache preparation is deliberately bounded to the selected unique sections.
 # It supplies physical lexical strings, never parses scalar/type values.
 # Original INIClass525A60 file loading/archive selection are not executed.
 result={};current=None
 for line in raw.splitlines():
  line=line.strip(bytes(range(33)))
  if line.startswith(b'[') and b']' in line:
   name=line[1:line.index(b']')].decode('latin1');current={}
   if name in NAMES:assert name not in result,name;result[name]=current
   else:current=None
   continue
  if current is None:continue
  line=line.split(b';',1)[0].strip(bytes(range(33)))
  if b'=' not in line:continue
  k,v=(x.strip(bytes(range(33))) for x in line.split(b'=',1))
  if k and v:
   k=k.decode('latin1');assert k not in current,('duplicate-key',k);current[k]=v.decode('latin1')
 return {n:v for n,v in result.items() if v}
class Reader(Lists):
 def __init__(self,root,sections,*,arena_size=0x400000):
  self.assets={p.name.upper():p.read_bytes() for p in root.glob('*')};self.asset_loaded=[];self.asset_ptr={};self.sound_names=[]
  super().__init__(arena_size=arena_size)
  # Retail startup installs the CRT floating scanner before ReadDouble5283D0.
  self.invoke(0x7C8F5E,0)
  self.make_ini(sections)
 def alloc(self,n):
  out=self.cursor;self.cursor+=(n+15)&~15;assert self.cursor<HEAP+self.arena_size;return out
 def cstring(self,s):
  raw=s.encode('latin1')+b'\0';ptr=self.alloc(len(raw));self.u.mem_write(ptr,raw);return ptr
 def invoke(self,addr,obj,args=(),*,timeout_us=10_000_000,context=None,instruction_limit=2_000_000):
  self.u.mem_write(SP,dwords(RET_MAGIC,*args));self.u.reg_write(UC_X86_REG_ESP,SP);self.u.reg_write(UC_X86_REG_ECX,obj)
  run_checked(self.u,addr,RET_MAGIC,count=instruction_limit,timeout_us=timeout_us,context=context)
  return self.u.reg_read(UC_X86_REG_EAX)
 def install_waypoint_character_format_transport(self):
  """Supply only ToLetters763610's two Windows wsprintfA calls.

  Original instructions calculate and push every character. The raw PE IAT
  7E14B0 is unresolved40FFF0, not an operating-system formatter. Keep code
  bytes unchanged and supply the two exact %c formats at the import call.
  wsprintfA is cdecl: leave arguments for the original ADD ESP cleanup.
  Contract: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-wsprintfa
  No native division, letter selection, INI reader or gameplay return is supplied.
  """
  from unicorn import UC_HOOK_CODE
  if hasattr(self,'waypoint_character_formats'):return self.waypoint_character_formats
  self.waypoint_character_formats=[]
  def transport(u,pc,_size,_data):
   if pc not in (0x763635,0x763671):return
   assert bytes(u.mem_read(pc,6))==bytes.fromhex('ff15b0147e00')
   sp=u.reg_read(UC_X86_REG_ESP)
   dest,fmt=struct.unpack('<2I',u.mem_read(sp,8))
   template=self.string(fmt)
   assert template in ('%c','%c%c'),(hex(pc),template)
   count=template.count('%c')
   args=list(struct.unpack('<'+'I'*count,u.mem_read(sp+8,4*count)))
   raw=bytes(value&255 for value in args)
   assert all(0<value<128 for value in raw),(hex(pc),args)
   u.mem_write(dest,raw+b'\0')
   u.reg_write(UC_X86_REG_EAX,len(raw))
   u.reg_write(UC_X86_REG_EIP,pc+6)
   self.waypoint_character_formats.append(dict(call=f'{pc:08X}',iat='007E14B0',
       format=template,arguments=args,output=raw.decode('ascii'),returned=len(raw)))
  self.waypoint_character_format_hook=self.u.hook_add(UC_HOOK_CODE,transport)
  return self.waypoint_character_formats
 def guid_transport(self,u,pc,sp,events):
  """Existing Mission's two Windows GUID-reader transport boundaries.

  Original527920 reads the INI string and supplies its current GUID default.
  Only ASCII-to-UTF16 and CLSIDFromString OS calls are supplied here; no type
  field, caller return or gameplay result is invented. Mission and composed
  type-input fixtures share this exact transport owner.
  """
  if pc==0x527AF9:
   codepage,flags,source,length,dest,capacity=struct.unpack('<6I',u.mem_read(sp,24));assert (codepage,flags,length)==(0,1,0xffffffff)
   value=self.string(source);assert value.isascii();raw=(value+'\0').encode('utf-16-le');assert len(raw)//2<=capacity
   u.mem_write(dest,raw);u.reg_write(UC_X86_REG_EAX,len(raw)//2);u.reg_write(UC_X86_REG_ESP,sp+24);u.reg_write(UC_X86_REG_EIP,0x527AFF);events.append(dict(kind='OS_ascii_to_utf16',value=value));return True
  if pc==0x527B0C:
   source,dest=struct.unpack('<2I',u.mem_read(sp,8));s=[]
   while (v:=struct.unpack('<H',u.mem_read(source+len(s)*2,2))[0]):s.append(chr(v))
   value=''.join(s);raw=uuid.UUID(value).bytes_le;u.mem_write(dest,raw);u.reg_write(UC_X86_REG_EAX,0);u.reg_write(UC_X86_REG_ESP,sp+8);u.reg_write(UC_X86_REG_EIP,0x527B12);events.append(dict(kind='OS_CLSIDFromString',value=value,bytes=raw.hex()));return True
  return False
 def make_ini(self,sections):
  u=self.u;u.mem_write(INI,bytes(0x40));rows=[]
  for name,keys in sections.items():
   sec=self.alloc(0x44);u.mem_write(sec+0xc,dwords(self.cstring(name)));entries=[]
   for key,value in keys.items():
    entry=self.alloc(0x28);u.mem_write(entry+0xc,dwords(self.cstring(key),self.cstring(value)));entries.append((crc(key),entry))
   items=self.alloc(len(entries)*8)
   for i,(key,pointer) in enumerate(sorted(entries,key=lambda x:struct.unpack('<i',dwords(x[0]))[0])):u.mem_write(items+i*8,dwords(key,pointer))
   u.mem_write(sec+0x2c,dwords(items,len(entries),len(entries),1,0));rows.append((crc(name),sec))
  items=self.alloc(len(rows)*8)
  for i,(key,pointer) in enumerate(sorted(rows,key=lambda x:struct.unpack('<i',dwords(x[0]))[0])):u.mem_write(items+i*8,dwords(key,pointer))
  u.mem_write(INI+0x28,dwords(items,len(rows),len(rows),1,0))
 def hook(self,u,p,n,d):
  if p==0x5B40B0:
   name=self.string(u.reg_read(UC_X86_REG_ECX));raw=self.assets.get(name.upper());pointer=0
   if raw:
    if name.upper() not in self.asset_ptr:
     pointer=self.alloc(len(raw));u.mem_write(pointer,raw);self.asset_ptr[name.upper()]=pointer
    pointer=self.asset_ptr[name.upper()]
   self.asset_loaded.append(dict(name=name,bytes=len(raw) if raw else 0,sha256=hashlib.sha256(raw).hexdigest() if raw else None,header8_hex=raw[:8].hex() if raw else None));self.ret(pointer,0);return
  super().hook(u,p,n,d)
 def result(self,name,ptr,admitted):
  u=self.u
  out={'name':name,'art_body_read':bool(admitted&255)}
  out.update({k:struct.unpack('<i',u.mem_read(ptr+v,4))[0] for k,v in SCALARS.items()})
  out.update({k:struct.unpack('<Q',u.mem_read(ptr+v,8))[0] for k,v in DOUBLES.items()})
  out.update({k:bool(u.mem_read(ptr+v,1)[0]) for k,v in BOOLS.items()})
  out.update({k:self.string(self.read32(ptr+v)+0x24) if self.read32(ptr+v) else None for k,v in REFS.items()})
  image=self.read32(ptr+0xa4);out.update(image=self.string(ptr+0x1f8),raw_shp_frame_count=struct.unpack('<h',u.mem_read(image+6,2))[0] if image else 0,middle=struct.unpack('<i',u.mem_read(ptr+0x298,4))[0],random_rate=list(struct.unpack('<2i',u.mem_read(ptr+0x2e4,8))),report_index=struct.unpack('<i',u.mem_read(ptr+0x2f8,4))[0])
  return out

def read_types(root,sections,names):
 m=Reader(root,sections);types={}
 for name in names:
  ptr=m.alloc(0x400);m.invoke(0x427530,ptr,(m.cstring(name),));types[name]=ptr
 rows=[]
 for name,ptr in types.items():
  mark=len(m.asset_loaded);admitted=m.invoke(0x427D00,ptr,(INI,))
  row=m.result(name,ptr,admitted)
  row['asset_loads']=m.asset_loaded[mark:]
  row['physical_art_keys']=sections.get(name)
  rows.append(row)
 return rows

# These are the actual fields supplied by producer/flight harnesses. No expected
# field is read before the independent native constructor/ART call above.
EXPORTED_FIELDS=list(SCALARS)+[x for x in DOUBLES if x!='max_z_vel_f64_bits']+[
 'art_body_read','bouncer','scorch','crater','expire_anim','trailer_anim',
 'bounce_anim','warhead','raw_shp_frame_count']

def assert_production_exports(native_rows):
 native={r['name']:r for r in native_rows};checks=[]
 for filename,count in [('bridge-retail-anim-inputs.json',19),
                         ('bridge_debris_flight.inputs.json',24)]:
  path=HERE.parent/'spatial_oracle'/filename
  rows=json.loads(path.read_text())['rows'];assert len(rows)==count
  compared=[]
  for row in rows:
   name=row['name'];expected=native[name]
   if row.get('missing_runtime_config'):
    # Historical export exposes the production defect fixed by this chain:
    # registered D was missing a runtime config. Native remains constructor-only.
    assert name=='D' and not expected['art_body_read']
    assert expected['raw_shp_frame_count']==0 and expected['asset_loads']==[]
    continue
   for field in EXPORTED_FIELDS:
    assert row[field]==expected[field],(filename,name,field,row[field],expected[field])
   assert (row['random_rate'] or [0,0])==expected['random_rate'],(filename,name,'random_rate')
   normalized='normalized: true' in row['full_config_debug']
   assert normalized==expected['normalized'],(filename,name,'normalized')
   compared.append(name)
  checks.append(dict(fixture=filename,compared=compared,
                    historical_missing_config=['D'],fields=EXPORTED_FIELDS+['random_rate','normalized']))
 return checks

def generate():
 raw=(ROOT/'ARTMD.INI').read_bytes();sections=physical_sections(raw)
 assert set(sections)==set(NAMES)-{'D'},sorted(sections)
 rows=read_types(ROOT,sections,NAMES)
 checks=assert_production_exports(rows)
 # Distinguish +36B Scorch from +36D Crater independently; the selected retail
 # rows set both identically and cannot detect an accidentally swapped injector.
 controls=[]
 for scorch,crater in [('yes','no'),('no','yes')]:
  controls.extend(read_types(ROOT,{'DBRIS1LG':{'Scorch':scorch,'Crater':crater}},['DBRIS1LG']))
 assert [(r['scorch'],r['crater']) for r in controls]==[(True,False),(False,True)]
 return dict(schema_version=1,
             art_sha256=hashlib.sha256(raw).hexdigest(),art_bytes=len(raw),
             rows=rows,asymmetric_flag_controls=controls,production_export_checks=checks)

def metadata():
 return provenance(
  scope='24 independently initialized original AnimType ART readers/image-header consumers plus two asymmetric Scorch/Crater controls; bridge producer/flight input prerequisites',
  assumptions=[
   'Selected exact-case physical ARTMD sections/keys are unique; a bounded lexical extraction prepares original signed-CRC INI caches. Full INIClass525A60 physical loader and MIX precedence are outside this run. All scalar values/defaults/references/rate postprocessing come from original ctor427530 and ReadINI427D00, not VERA exports.',
   'ARTMD and 23 full SHP files are extracted retail bytes. Output pins ART SHA256 and each loaded SHP SHA256, length and first eight bytes. Original filename formation and image metadata427B50 execute; archive IO returns the corresponding unchanged physical bytes.',
   'Fresh original AnimType registry preallocates all24 types in native pool/list order, with empty sound registry. Original missing D section returns false and retains constructor values. HE FindOrAllocate and its original ctor execute; this does not establish Warhead rules fields.',
   'Report/StartSound stay -1 in the empty sound registry, matching the declared silent producer/flight boundary; sound binding/playback is not claimed.',
   'The completed native rows assert every injected ART field against both saved production-export fixtures; historical missing D is explicitly identified instead of certified as correct production behavior. Current production Rust comparisons cover corrected D separately.',
   'PC53/chop FPCW0E7F; original CRT floating scanner initializer7C8F5E executes before native ReadDouble. Native uint64 payloads retain exact binary64 bits.'],
  substitutions=[
   'operator_new7C8E17 returns bump storage; operator_delete7C8B3D is no-op; CRT TLS accessor7D140B returns per-thread storage.',
   'LoadFileFromMIX5B40B0 returns exact full bytes for the original requested retail filename; no SHP header is manufactured.'],
  entry_points={'anim_type_ctor':0x427530,'anim_type_read_art':0x427D00,
   'object_type_read':0x5F92E0,'image_load_and_metadata':0x427B50,
   'read_double':0x5283D0,'read_integer':0x5276D0,'read_bool':0x5295F0,
   'read_minmax':0x529880,'float_scanner_init':0x7C8F5E})

if __name__=='__main__':
 finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
