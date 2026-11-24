"""Original readers for bridge debris's non-ART runtime inputs.

Supply extracted RULESMD.INI, MPBattleMD.ini and Hills.map under
VERA20K_BRIDGE_LANDING_ASSETS, also ARTMD.INI (optional LANGRULE.INI). Native scalar readers,
reference factories, strtok and vector copies execute on cached lexical entries.
This is not a physical INI loader or whole RulesClass load comparison.
"""
import hashlib,json,os,struct
from pathlib import Path
from unicorn.x86_const import *
from tools.native_oracle import run_checked,RET_MAGIC,finish_vectors,provenance,OracleError
from tools.spatial_oracle.building_body_rules import TYPE,INI,SP,RAW,dwords
from tools.rules_oracle.bridge_anim_lists import Lists,HEAP
from tools.projectile_oracle.flat_art import crc
ROOT=Path(os.environ.get('VERA20K_BRIDGE_LANDING_ASSETS',str(Path(os.environ.get('CARGO_TARGET_DIR','target'))/'asset/bridge-landing-inputs/extract')))
WH={
 'CellSpread':(0x847EA0,0x75D3D2,0x75D3F1),
 'PercentAtMax':(0x847E84,0x75D410,0x75D42F),
 'Wall':(0x81AC58,0x75D4F4,0x75D50E),
 'Wood':(0x847E00,0x75D542,0x75D55C),
 'Verses':(0x847C38,0x75DDCC,0x75DE5A)}
RULES={
 'ConditionRed':('AudioVisual',0x66B337,0x66B35E,0),
 'ConditionYellow':('AudioVisual',0x66B35E,0x66B385,0),
 'SplashList':('CombatDamage',0x66C184,0x66C287,0),
 'C4Warhead':('CombatDamage',0x66C304,0x66C34C,-4),
 'BridgeStrength':('CombatDamage',0x66CD66,0x66CD8C,0),
 'MaxDamage':('CombatDamage',0x66CE2C,0x66CE57,0),
 'Wake':('General',0x66D847,0x66D894,-4),
 'TreeStrength':('General',0x671DD2,0x671DF1,0)}
SELECT={'General':{'Wake','TreeStrength'},'AudioVisual':{'ConditionRed','ConditionYellow'},
 'CombatDamage':{'SplashList','C4Warhead','BridgeStrength','MaxDamage'},
 'HE':set(WH),'Super':set(WH),
 'TREE01':{'Strength','Armor','Immune','SpawnsTiberium','TemperateOccupationBits','SnowOccupationBits','Foundation'},
 'TIBTRE01':{'Strength','Armor','Immune','SpawnsTiberium','TemperateOccupationBits','SnowOccupationBits','Foundation'}}
CTOR=[(0x6665D5,0x666604),(0x66572F,0x665735),(0x6674F6,0x667500),
      (0x6675DA,0x6675E4),(0x666DF8,0x666E02),(0x666B52,0x666B58),(0x66755C,0x667574)]

def physical_inputs(raw):
 """Only lexical extraction of unique ordinary selected keys, no scalar parse."""
 sections={};excerpts=[];current=None
 for line_no,line in enumerate(raw.splitlines(),1):
  source=line.decode('latin1');line=line.strip(bytes(range(33)))
  if line.startswith(b'[') and b']' in line:
   current=line[1:line.index(b']')].decode('latin1')
   if current in SELECT:assert current not in sections,current;sections[current]={}
   continue
  if current not in SELECT:continue
  line=line.split(b';',1)[0].strip(bytes(range(33)))
  if b'=' not in line:continue
  k,v=(x.strip(bytes(range(33))) for x in line.split(b'=',1))
  if not k or not v:continue
  k=k.decode('latin1')
  # Include all lexical entries so section admission remains real even if
  # none of the selected keys occur in this layer.
  assert k not in sections[current],(current,k)
  sections[current][k]=v.decode('latin1')
  if k in SELECT[current]:excerpts.append(dict(line=line_no,section=current,key=k,value=v.decode('latin1'),source_line=source))
 return sections,excerpts

class Landing(Lists):
 def __init__(self):
  super().__init__();self.invoke(0x7C8F5E,0);self.invoke(0x71D580,0)
  self.u.mem_write(0x8874C0,dwords(0x7EB6D4,HEAP+0x7000,1024,1,0,10))
  self.u.mem_write(0xA8E318,dwords(0x7EB6D4,HEAP+0x9000,1024,1,0,10))
  self.u.mem_write(0x8871E0,dwords(TYPE))
  for a,b in CTOR:

   self.u.reg_write(UC_X86_REG_ESP,SP);self.u.reg_write(UC_X86_REG_ESI,TYPE)
   self.u.reg_write(UC_X86_REG_EBX,0);self.u.reg_write(UC_X86_REG_EDI,10)
   # The original ECX immediate at667190 dominates both threshold stores.
   run_checked(self.u,0x667190,0x667195)
   run_checked(self.u,a,b)
 def alloc(self,n):
  out=self.cursor;self.cursor+=(n+15)&~15;assert self.cursor<HEAP+0x400000;return out
 def cstring(self,s):
  raw=s.encode('latin1')+b'\0';ptr=self.alloc(len(raw));self.u.mem_write(ptr,raw);return ptr
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
 def invoke(self,fn,this,args=()):
  self.u.mem_write(SP,dwords(RET_MAGIC,*args));self.u.reg_write(UC_X86_REG_ESP,SP);self.u.reg_write(UC_X86_REG_ECX,this)
  run_checked(self.u,fn,RET_MAGIC,count=2000000);return self.u.reg_read(UC_X86_REG_EAX)
 def warhead(self,name):return self.invoke(0x75E3B0,self.cstring(name))
 def whstate(self,p):
  return dict(verses=list(struct.unpack('<11d',self.u.mem_read(p+0xA0,88))),verses_bits=[f'{x:016x}' for x in struct.unpack('<11Q',self.u.mem_read(p+0xA0,88))],cell_spread=struct.unpack('<f',self.u.mem_read(p+0x124,4))[0],cell_spread_bits=f'{self.read32(p+0x124):08x}',percent_at_max=struct.unpack('<f',self.u.mem_read(p+0x12C,4))[0],percent_at_max_bits=f'{self.read32(p+0x12C):08x}',wall=bool(self.u.mem_read(p+0x144,1)[0]),wood=bool(self.u.mem_read(p+0x147,1)[0]))
 def rulesstate(self):
  def name(p):return self.string(p+0x24) if p else None
  def signed(p):return struct.unpack('<i',self.u.mem_read(p,4))[0]
  def real(p):return struct.unpack('<d',self.u.mem_read(p,8))[0]
  spl=self.read32(TYPE+0xBC4);n=self.read32(TYPE+0xBD0)
  return dict(tree_strength=signed(TYPE+0x1144),max_damage=signed(TYPE+0x16C8),bridge_strength=signed(TYPE+0x1740),wake=name(self.read32(TYPE+0x94)),splash_list=[name(self.read32(spl+i*4)) for i in range(n)],c4_warhead=name(self.read32(TYPE+0xFA8)),condition_red=real(TYPE+0x1708),condition_yellow=real(TYPE+0x1700),condition_red_bits=f'{struct.unpack("<Q",self.u.mem_read(TYPE+0x1708,8))[0]:016x}',condition_yellow_bits=f'{struct.unpack("<Q",self.u.mem_read(TYPE+0x1700,8))[0]:016x}')
 def terrain(self,name):
  p=self.alloc(0x400);self.invoke(0x71DA80,p,(self.cstring(name),));return p
 def terrainstate(self,p):
  signed=lambda off:struct.unpack('<i',self.u.mem_read(p+off,4))[0]
  return dict(strength=signed(0xA0),armor=signed(0x9C),immune=bool(self.u.mem_read(p+0x233,1)[0]),spawns_tiberium=bool(self.u.mem_read(p+0x2B1,1)[0]),temperate_occupation_bits=signed(0x2A8),snow_occupation_bits=signed(0x2AC),foundation_index=signed(0x298))
 def terrainread(self,p):
  u=self.u;u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EBX,p);u.reg_write(UC_X86_REG_ESI,INI);u.reg_write(UC_X86_REG_EBP,p+0x24)
  # Preserve the preceding LegalTarget result across this slice's adjacent
  # store, then execute complete Armor, Strength and Immune read/store sequence. The endpoint
  # includes pushes for the following RadarInvisible read (ESP-12).
  u.reg_write(UC_X86_REG_EAX,u.mem_read(p+0x231,1)[0])
  run_checked(u,0x5F94B3,0x5F9516);assert u.reg_read(UC_X86_REG_ESP)==SP-12
  u.reg_write(UC_X86_REG_ESI,p);run_checked(u,0x71DEC8,0x71DEE2)
  u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EBX,INI);u.reg_write(UC_X86_REG_EDI,p+0x24)
  # SpawnsTiberium block, starting before its current default is read;
  # IsFlammable arguments are prepared but its reader is not executed.
  u.reg_write(UC_X86_REG_EAX,u.mem_read(p+0x2B0,1)[0])
  run_checked(u,0x71DF27,0x71DF56);assert u.reg_read(UC_X86_REG_ESP)==SP-12
  # Both signed occupation reads/stores; the trailing pop only restores an
  # unused fixture register before the final native SnowOccupationBits store.
  u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EAX,self.read32(p+0x2A8));u.reg_write(UC_X86_REG_ECX,INI)
  run_checked(u,0x71E079,0x71E0A6);assert u.reg_read(UC_X86_REG_ESP)==SP+4
 def read_layer(self,sections,types,terrains):
  self.make_ini(sections);admitted={}
  for section in SELECT:
   admitted[section]=bool(self.invoke(0x526810,INI,(self.cstring(section),)))
  for key,(section,a,b,delta) in RULES.items():
   if not admitted[section]:continue
   self.u.reg_write(UC_X86_REG_ESP,SP);self.u.reg_write(UC_X86_REG_ESI,TYPE);self.u.reg_write(UC_X86_REG_EDI,INI)
   run_checked(self.u,a,b,count=2000000)
   assert self.u.reg_read(UC_X86_REG_ESP)==SP+delta,(key,hex(self.u.reg_read(UC_X86_REG_ESP)))
  for name,p in types.items():
   if not admitted[name]:continue
   for key,(kp,a,b) in WH.items():
    self.u.reg_write(UC_X86_REG_ESP,SP);self.u.reg_write(UC_X86_REG_ESI,p);self.u.reg_write(UC_X86_REG_EDI,INI);self.u.reg_write(UC_X86_REG_EBP,p+0x24)
    run_checked(self.u,a,b,count=2000000);assert self.u.reg_read(UC_X86_REG_ESP)==SP
  for name,p in terrains.items():
   if admitted[name]:self.terrainread(p)
  return dict(terrain_types={n:self.terrainstate(p) for n,p in terrains.items()},section_admission=admitted,rules=self.rulesstate(),warheads={n:self.whstate(p) for n,p in types.items()})

def damage_sensitivity(retail):
 # Execute the original damage body with the independently read HE bits and
 # adjacent higher binary64 values. The latter is a declared perturbation,
 # not an expected production-reader result. It measures gameplay sensitivity.
 from tools.spatial_oracle.estimated_damage import base,execute
 he=retail['warheads']['HE'];native=he['verses_bits'];higher=list(native)
 for i in (3,4,5,7,8,9):higher[i]=f'{int(native[i],16)+1:016x}'
 rows=[]
 for damage in (10,20):
  for distance in (0,11,49,50,79,80,127,128,129):
   for armor in range(11):
    result=[]
    for bits in (native,higher):
     row=base('HE_input_sensitivity',scope='direct_warhead',target_armor_index=armor,warhead_verses=bits,warhead_cell_spread=he['cell_spread_bits'],warhead_percent_at_max=he['percent_at_max_bits'],rules_max_damage=retail['rules']['max_damage'],kernel_damage=damage,kernel_distance=distance)
     result.append(execute(row)['result_i32'])
    rows.append(dict(damage=damage,distance=distance,armor=armor,native=result[0],higher_binary64_input=result[1]))
 return dict(native_verses_bits=native,higher_verses_bits=higher,rows=rows)

def verses_characterization():
 raws=[None,'',' ',
  '100%,100%,100%,70%,70%,35%,75%,40%,20%,80%,100%',
  '0%,1%,2%,10%,15%,25%,30%,33%,50%,60%,90%',
  '99%,101%,125%,150%,200%,250%,300%,-1%,-33%,-70%,-100%',
  '50.5%,1.5%,0.5%,70.9%,-70.9%,100%,100%,100%,100%,100%,100%',
  '2147483647%,2147483648%,-2147483648%,-2147483649%,4294967295%,4294967296%,1%,1%,1%,1%,1%',
  '%foo,junk%,+%,-%,+70%,-70%,70x%,0x10%,100%tail,100%,100%',
  '70%,,35%,,,,75%,',',,,',' ,70%,  ,35%,',
  '0.505,0.015,0.005,-0.7,0.4,0.2,0.8,1.25e-2,-2.5e1,12junk,1',
  '0,1,0.5,0.75,-0.5,+1.25,.25,1.,junk,+,-',
  '1e,1e+,1.25e2junk,0.00000001,NaN,inf,-inf,1,1,1,1',
  '100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,0%',
  '0'*125+'70%,35%']
 rows=[]
 for raw in raws:
  m=Landing();p=m.warhead('HE')
  cached=None if raw is None else raw.strip(''.join(map(chr,range(33)))) or None
  keys={'FixtureOnly':'1'}
  if cached is not None:keys['Verses']=cached
  m.make_ini({'HE':keys});m.u.reg_write(UC_X86_REG_ESP,SP);m.u.reg_write(UC_X86_REG_ESI,p);m.u.reg_write(UC_X86_REG_EDI,INI);m.u.reg_write(UC_X86_REG_EBP,p+0x24)
  status='completed'
  try:run_checked(m.u,0x75DDCC,0x75DE5A,required_addresses=(0x528A10,))
  except OracleError:
   assert m.u.reg_read(UC_X86_REG_EIP)==0x7CAF66 and m.u.reg_read(UC_X86_REG_EDI)==0
   status='null_strtok_token_fault'
  rows.append(dict(raw=raw,cached_raw=cached,status=status,verses_bits=m.whstate(p)['verses_bits']))
 return rows

def generate():
 m=Landing();types={n:m.warhead(n) for n in ('HE','Super')};terrains={n:m.terrain(n) for n in ('TREE01','TIBTRE01')}
 initial=dict(terrain_types={n:m.terrainstate(p) for n,p in terrains.items()},rules=m.rulesstate(),warheads={n:m.whstate(p) for n,p in types.items()});layers=[]
 for name in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.map'):
  path=ROOT/name
  if not path.is_file():
   assert name=='LANGRULE.INI',path
   layers.append(dict(file=name,absent=True));continue
  raw=path.read_bytes();sections,excerpts=physical_inputs(raw)
  layers.append(dict(file=name,bytes=len(raw),sha256=hashlib.sha256(raw).hexdigest(),physical_inputs=excerpts,**m.read_layer(sections,types,terrains)))
 controls=[]
 for raw in ({'HE':{'Verses':'50%,50%,50%,50%,50%,50%,50%,50%,50%,50%,50%'}},{'HE':{'Wall':'no'}},{'General':{'Wake':'WAKE2'},'CombatDamage':{'SplashList':'ONE,TWO','MaxDamage':'-1','BridgeStrength':'0'}},{'General':{'Unrelated':'yes'},'CombatDamage':{'Unrelated':'yes'}},{}):
  controls.append(dict(input=raw,**m.read_layer(raw,types,terrains)))
 art=(ROOT/'ARTMD.INI').read_bytes();sections,excerpts=physical_inputs(art);m.make_ini(sections);foundations=[]
 for name,p in terrains.items():
  # Native TerrainReadINI71DF61..71DF9C reads ART using Image as section.
  m.u.reg_write(UC_X86_REG_ESP,SP);m.u.reg_write(UC_X86_REG_ESI,p)
  run_checked(m.u,0x71DF61,0x71DF9C)
  assert m.u.reg_read(UC_X86_REG_ESP)==SP
  idx=m.read32(p+0x298);record=0xB0EDC0+idx*40
  foundations.append(dict(name=name,foundation_index=idx,foundation_name=m.string(m.read32(0x81B9D8+idx*8)),occupy_offsets=list(struct.iter_unpack('<hh',m.u.mem_read(record,40)))))
 return dict(schema_version=1,constructor=initial,layers=layers,controls=controls,verses_cases=verses_characterization(),damage_sensitivity=damage_sensitivity(layers[-1]),terrain_art=dict(file='ARTMD.INI',bytes=len(art),sha256=hashlib.sha256(art).hexdigest(),physical_inputs=excerpts,rows=foundations))

def recorded_retail_inputs():
 # Shared native-established fixture inputs; run --check to reproduce them.
 data=json.loads(Path(__file__).with_suffix('.json').read_text())
 return data['layers'][-1]

def metadata():
 return provenance(scope='Original selected RulesClass constructor stores, Warhead factory/constructor, native section admission and complete selected scalar/reference/list reader blocks. Active retail lexical inputs plus five retention controls; full TerrainType ctor and selected inherited/type readers, TreeStrength and original ART Foundation lookup/occupy initialization. No whole reader/physical file load claim.',assumptions=[
  'Seventeen independent fresh-Warhead Verses rows execute the complete original ReadString128/strtok/atoi-or-atof sequence. Empty physical values are omitted from cache; long strings test the original128-byte read boundary. Malformed/decimal/exponent/overflow strings are bounded characterization beyond selected retail HE.',
  'Damage sensitivity executes original489180 on the native-read HE values and an explicit one-ULP-higher perturbation at indices3,4,5,7,8,9. 198 armor/damage/distance pairs use damage10/20 and distances0,11,49,50,79,80,127,128,129. Perturbed inputs are characterization, not a Rust/native parser equivalence claim.',
  'Full TerrainType ctor71DA80 executes; selected Armor/Strength/Immune reader5F94B3..5F9516 and successful-read fallback71DEC8, SpawnsTiberium71DF27..71DF56, occupation71E079..71E0A6 execute. Full base-reader success is supplied after original section admission. Adjacent stores retain pre-existing fields; unused trailing pops are bounded stack locals. Original71D580 initializes occupy lists and71DF61..71DF9C reads lexical ART Foundation using actual type Image. No image pixel or fullTerrain reader claim.',
  'Raw RULESMD from expandmd01.mix entry8218F9F4 (743215 bytes); MPBattleMD from production asset source; Hills.map is the exact decompressed inner Hills.mmx payload. Companion rows pin byte hashes and exact lexical source lines. LANGRULE absent in selected installation.',
  'Lexical unique ordinary keys form supplied INI section/entry indexes using original CRC. Original526810 section admission, ReadString128, ReadInt, ReadBool, ReadDouble and Verses strtok/atoi/atof execute. No VERA scalar value initializes fields.',
  'Selected readers run in their original within-owner order; unrelated fields and original whole scenario load sequence are outside scope. Warhead absent sections skip reads; present sections with missing Verses use native default string. Rules reads retain prior values on missing selected keys.',
  'OriginalCRT floating scanner initialization7C8F5E executes. FPCW0E7F supplies native PC53/chop. Rules constructor register EBX0/EDI10, originalECX3FE00000 instruction667190 supplies both threshold defaults0.5.',
  'OriginalAnimType and Warhead factories/constructors run against spare-capacity registries. ART reads, SHP binding, gameplay, sound and full game scheduling are outside this reader corpus.',
  'Wake/C4 reader endpoints have ESP-4 because the next key push80 precedes the selected pointer store; the next invocation resets the local stack.'],substitutions=[
  'Inherited Lists allocator returns fresh bump storage; operator delete no-op; CRT TLS accessor returns supplied per-thread storage for original strtok. No executable instruction patches or native reader replacements.'],entry_points={'rules_ctor':0x6653C0,'warhead_factory':0x75E3B0,'warhead_ctor':0x75CEC0,'warhead_reader':0x75D3A0,'read_verses':0x75DDCC,'read_splash':0x66C184,'read_c4':0x66C304,'read_bridge_strength':0x66CD66,'read_max_damage':0x66CE2C,'read_wake':0x66D847,'read_condition_red':0x66B337,'read_condition_yellow':0x66B35E,'read_double':0x5283D0,'crt_float_init':0x7C8F5E,'terrain_ctor':0x71DA80,'tree_strength':0x671DD2,'terrain_fields':0x5F94B3,'terrain_occupation':0x71E079,'terrain_foundation':0x71DF61,'terrain_occupy_init':0x71D580,'damage_sensitivity':0x489180})
if __name__=='__main__':finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
