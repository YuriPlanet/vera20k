"""Session Country history on the existing physical Rules oracle owner."""
from pathlib import Path
import json,struct
from tools.native_oracle import configured_gamemd,image_sha256
from tools.rules_oracle.weapon_speed_order import RetailRulesInputs,retail_history
physical=None
fixtures={}
class Inputs:
 def lookup(self,name): return fixtures.get(name.upper(),physical.lookup(name))
def signed(x):return struct.unpack('<i',struct.pack('<I',x))[0]
def snapshot(m):
 result=[]
 for i in range(m.read32(0xA83CA8)):
  p=m.read32(m.read32(0xA83C9C)+i*4)
  result.append(dict(index=i,id=m.string(p+0x24),name=m.string(p+0x64),multiplay=m.u.mem_read(p+0x1A5,1)[0]))
 return result
def ini(m,body):
 name=f'CTRL{len(fixtures)}.INI';fixtures[name]=body.encode('ascii');return m.load_physical_ini(name)[0]
def fixture(root):
 m=retail_history(Inputs());r=m.alloc(0x2000);m.invoke(0x665650,r)
 return m,r,ini(m,root)
def bodies(m,root):
 # Original PrepareSession52E123 invokes each live Country vtable+64.
 index=0
 while index<m.read32(0xA83CA8):
  p=m.read32(m.read32(0xA83C9C)+index*4)
  m.invoke(m.read32(m.read32(p)+0x64),p,(root,));index+=1
def generate():
 global physical
 physical=RetailRulesInputs(configured_gamemd().parent)
 fixtures.clear()
 rows=[]
 root='[Countries]\n0=Alpha\n1=Beta\n[Alpha]\nName=Alias\nMultiplay=yes\n[Orphan]\nName=LaterAlias\nMultiplay=yes\n'
 for token in [None,'','Alpha','Alias','Orphan','<random>','<none>','none','<Player @ A>','<Player @ H>','<player @ a>','012345678901234567890123Long']:
  m,r,rootini=fixture(root)
  phases=[]
  for phase in range(2):
   m.invoke(0x6722F0,r,(rootini,))
   settings=ini(m,'[MultiPlayer]\n'+('' if token is None else 'Side='+token+'\n'))
   rng=m.rng_state()
   result=signed(m.invoke(0x475540,settings,(m.cstring('MultiPlayer'),m.cstring('Side'),0xfffffffd)))
   after=snapshot(m)
   bodies(m,rootini);m.check_code()
   assert rng==m.rng_state()
   phases.append(dict(result=result,allocated=after,refreshed=snapshot(m)))
  rows.append(dict(token=token,phases=phases));m.close()
 m=retail_history(Inputs());r=m.alloc(0x2000);m.invoke(0x665650,r)
 rootini,_=m.load_physical_ini('RULESMD.INI');m.invoke(0x6722F0,r,(rootini,))
 retail=snapshot(m);m.check_code();m.close()
 return dict(native_sha256=image_sha256(),root=root,current=-3,rows=rows,retail_registered=retail)


def generate_parent():
 """Original retained parent reader and the mixed-identity AI list gate.

 This mode uses only synthetic physical INI bytes on the same Rules VM.
 Country/type/lookup/eligibility bodies execute unchanged.
 """
 from unicorn import UC_HOOK_CODE,UC_HOOK_MEM_WRITE
 from unicorn.x86_const import UC_X86_REG_EAX,UC_X86_REG_EIP,UC_X86_REG_ESP
 from tools.spatial_oracle.building_body_rules import dwords

 class NoAssets:
  def lookup(self,_name):return None

 global physical
 physical=NoAssets();fixtures.clear()
 m,_rules,_root=fixture('[Probe]\nName=Synthetic\n')
 m.seed_streams(1)
 events=[];pending=[];timer_writes=[]
 timer=0xB0EA80

 def observe(u,pc,_size,_data):
  sp=u.reg_read(UC_X86_REG_ESP)
  if pc==0x528A10 and m.string(m.read32(sp+8))=='ParentCountry':
   args=[m.read32(sp+4+i*4) for i in range(5)]
   row=dict(caller=f'{m.read32(sp):08X}',section=m.string(args[0]),key=m.string(args[1]),
    default=m.string(args[2]),capacity=args[4])
   events.append(row);pending.append((m.read32(sp),sp+24,row,args[3]))
  for item in pending[:]:
   ret,endsp,row,buffer=item
   if pc==ret and sp==endsp:
    row.update(returned=m.u.reg_read(UC_X86_REG_EAX),buffer=m.string(buffer));pending.remove(item)

 def timer_write(u,_access,address,size,value,_data):
  timer_writes.append(dict(pc=f'{u.reg_read(UC_X86_REG_EIP):08X}',offset=address-timer,size=size,value=value))

 def call(entry,obj,args=()):
  before=m.rng_state();timer_writes.clear()
  result=m.invoke(entry,obj,args);m.check_code()
  assert before==m.rng_state()
  assert not pending
  return result,dict(rng_unchanged=True,timer_after=list(struct.unpack('<3i',m.u.mem_read(timer,12))),timer_writes=timer_writes.copy())

 def country(name):
  pointer=m.alloc(0x2000);m.u.mem_write(pointer,b'\xA5'*0x2000)
  m.invoke(0x5113F0,pointer,(m.cstring(name),))
  return pointer

 def read(pointer,body):
  source=ini(m,body);events.clear();prior=m.string(pointer+0x98)
  result,receipt=call(0x511850,pointer,(source,))
  return dict(body=body,physical_sections=m.read32(source+0x2C),prior=prior,
   admitted=bool(result&255),parent=m.string(pointer+0x98),
   raw25_hex=bytes(m.u.mem_read(pointer+0x98,25)).hex(),events=events.copy(),**receipt)

 try:
  country('BASE');parent=country('PARENT');child=country('CHILD')
  ctor=dict(id=m.string(child+0x24),index=m.read32(child+0xB8),parent=m.string(child+0x98),
   raw25_hex=bytes(m.u.mem_read(child+0x98,25)).hex())
  m.u.mem_write(timer,struct.pack('<3i',995,0,25))
  hooks=[m.u.hook_add(UC_HOOK_CODE,observe),m.u.hook_add(UC_HOOK_MEM_WRITE,timer_write,begin=timer,end=timer+11)]
  initial_rng=m.rng_state()
  cases=[
   ('constructor_missing_section','[OTHER]\nName=OTHER\n'),
   ('explicit_parent','[CHILD]\nParentCountry=PARENT\n'),
   ('absent_section_retains','[OTHER]\nParentCountry=OTHER\n'),
   ('wrong_section_case_retains','[child]\nParentCountry=OTHER\n'),
   ('missing_key_defaults_self','[CHILD]\nName=CHILD\n'),
   ('parent_before_empty','[CHILD]\nParentCountry=PARENT\n'),
   ('empty_only_elides_section_retains','[CHILD]\nParentCountry=\n'),
   ('empty_with_name_defaults_self','[CHILD]\nName=CHILD\nParentCountry=\n'),
   ('parent_before_whitespace','[CHILD]\nParentCountry=PARENT\n'),
   ('whitespace_only_elides_section_retains','[CHILD]\nParentCountry= \t \n'),
   ('whitespace_with_name_defaults_self','[CHILD]\nName=CHILD\nParentCountry= \t \n'),
   ('trim_preserve_value_case','[CHILD]\nParentCountry= pArEnT \t\n'),
   ('wrong_key_case_defaults_self','[CHILD]\nparentcountry=OTHER\n'),
   ('truncate_to_24_bytes','[CHILD]\nParentCountry=ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789\n'),
   ('unknown_name_stored','[CHILD]\nParentCountry=UNKNOWN\n'),
   ('failed_read_retains_unknown','[OTHER]\nName=OTHER\n'),
  ]
  rows=[dict(id=name,**read(child,body)) for name,body in cases]
  alias_read=read(parent,'[PARENT]\nName=ParentAlias\n')
  lookup=[]
  for token in ('PARENT','parent','ParentAlias','UNKNOWN','','CHILD','<none>','<random>'):
   raw,receipt=call(0x5117D0,m.cstring(token))
   lookup.append(dict(token=token,index=signed(raw),**receipt))
  masks=[]
  for text in ['<random>','<none>','none','UNKNOWN','PARENT','ParentAlias','PARENT,CHILD,<random>','PARENT,PARENT']:
   source=ini(m,f'[Mask]\nOwner={text}\n')
   result,receipt=call(0x4750D0,source,(m.cstring('Mask'),m.cstring('Owner'),0))
   masks.append(dict(text=text,mask=result,**receipt))
  house=m.alloc(0x1000);m.u.mem_write(house+0x34,dwords(child))
  typ=m.alloc(0x1800);vector=m.alloc(24);data=m.alloc(4)
  m.u.mem_write(data,dwords(typ));m.u.mem_write(vector,dwords(0,data,1,1,1,10))
  m.u.mem_write(0xA8B263,b'\x01')
  gates=[]
  for name,owner,required,forbidden,parent_name in (
   ('parent_owner_actual_required',2,4,-1,'PARENT'),
   ('actual_owner_rejected',4,-1,-1,'PARENT'),
   ('parent_required_rejected',2,2,-1,'PARENT'),
   ('parent_forbidden_ignored',2,4,2,'PARENT'),
   ('actual_forbidden_rejected',2,4,4,'PARENT'),
   ('unknown_parent_bit31',0x80000000,4,-1,'UNKNOWN'),
   ('random_parent_bit30',0x40000000,4,-1,'<random>'),
  ):
   prior=read(child,f'[CHILD]\nParentCountry={parent_name}\n')
   m.u.mem_write(typ+0x6CC,dwords(owner,-1));m.u.mem_write(typ+0xDA0,dwords(required,forbidden))
   result,receipt=call(0x5051E0,house,(vector,))
   assert result in (0,typ)
   gates.append(dict(id=name,country='CHILD',actual_index=m.read32(child+0xB8),
    parent=prior['parent'],owner_mask=owner,required_mask=required,forbidden_mask=forbidden,
    selected=bool(result),**receipt))
  for name,parent_name,text in [
   ('authored_random_parent_and_owner','<random>','<random>'),
   ('authored_none_does_not_own_country_zero','BASE','<none>'),
   ('authored_owner_alias','PARENT','ParentAlias'),
  ]:
   prior=read(child,f'[CHILD]\nParentCountry={parent_name}\n')
   source=ini(m,f'[Mask]\nOwner={text}\n')
   owner,_=call(0x4750D0,source,(m.cstring('Mask'),m.cstring('Owner'),0))
   m.u.mem_write(typ+0x6CC,dwords(owner,-1));m.u.mem_write(typ+0xDA0,dwords(-1,-1))
   result,receipt=call(0x5051E0,house,(vector,))
   assert result in (0,typ)
   gates.append(dict(id=name,country='CHILD',actual_index=m.read32(child+0xB8),
    parent=prior['parent'],owner_text=text,owner_mask=owner,required_mask=-1,forbidden_mask=-1,
    selected=bool(result),**receipt))
  return dict(schema_version=1,
   constructor=ctor,rng_state_hex=initial_rng,reader_controls=rows,parent_alias_read=alias_read,
   lookup_controls=lookup,house_mask_controls=masks,first_buildable_controls=gates)
 finally:
  if 'hooks' in locals():
   for hook in hooks:m.u.hook_del(hook)
  m.close()


def parent_metadata():
 from tools.native_oracle import provenance
 return provenance(
  scope='Original Country constructor and sixteen retained physical INI reads; eight whole country-name lookups, eight physical Owner mask reads and ten whole FirstBuildableFromArray controls',
  assumptions=[
   'Synthetic ASCII files only. Existing RetailRulesHistory executes complete INI constructor535B30, file constructor4739F0, physical loader4741F0 and Country reader511850. Physical empty keys are elided: an otherwise empty section disappears and its read retains prior state; a surviving section with another key defaults omitted ParentCountry to currentID. No synthetic cached-empty entry is injected.',
   'Original Country5113F0 registers BASE/PARENT/CHILD in that order. The sampled CHILD storage starts A5-filled; constructor511404 clears only ParentCountry first byte. Original Country+98 is retained across actual511850 calls; currentID section and ParentCountry key are case-sensitive; ReadString528A10 capacity25/defaultcurrentID and the original copy execute.',
   'Whole5117D0 resolves original Name+64 or ID+24 case-insensitively and returns stored index+B8, unknown/empty/<none> -1 and special<random> -2. ParentAlias comes from an original retained Country body read, not supplied comparison output.',
   'Whole INI4750D0 reads physical Owner strings with capacity128 and default0, then48DEB0 rejects only lookup result-1. Random-2 contributesbit30; unknown and unregistered none contribute0. Three joined controls feed this original reader result directly into whole5051E0, rather than synthesizing the candidate mask.',
   'Whole5051E0 receives one synthetic candidate type and an explicit House pointing to CHILD. Owner+6CC uses resolved ParentCountry bit; Required+DA0/Forbidden+DA4 use actualCountry+B8. Candidate AI side+6D0 is -1 and superweapons are enabled; world, factories, AI scheduling, complete object construction and player sidebar behavior are outside coverage.',
   'Three full seeded RNG states are retained once and compared byte-for-byte before/after each selected original call. Action-timer sentinel[995,0,25] and writes to its12bytes are recorded. No claim covers unrelated timers, complete RNG chronology or whole-world detach/lifecycle behavior. Original mapped code bytes are checked unchanged.',
  ],
  substitutions=['Existing RetailRulesHistory/BulletReader Windows/file/heap/CRT/TLS platform transport; supplied synthetic source bytes and candidate/House priors only. No INI parser, Country constructor/read, country-name lookup, mask test or FirstBuildable body is replaced.'],
  entry_points={'country_ctor':0x5113F0,'country_read':0x511850,'parent_key_read':0x5118E2,
   'read_string':0x528A10,'country_name_lookup':0x5117D0,'house_mask_read':0x4750D0,
   'house_mask_token':0x48DEB0,'first_buildable':0x5051E0})

if __name__=='__main__':
 import sys
 from tools.native_oracle import finish_vectors,provenance
 if '--parent' in sys.argv[1:]:
  finish_vectors(generate_parent,Path(__file__).with_name('session_country_parent.json'),
   provenance=parent_metadata,argv=[arg for arg in sys.argv[1:] if arg!='--parent'],
   source_paths={'producer':Path(__file__)})
 else:
  finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=provenance(
   scope='Session Country master/ReadHouseType and subsequent live body history',
   assumptions=['Declared physical root and MultiPlayer sections; original Rules665650, Countries6722F0, whole475540 and live Country virtual+64 reader. Two retained phases per token. Retail root master enumerated through original physical INI loader. Full PrepareSession GUI and destructive reset are not executed.'],
   substitutions=['Existing RetailRulesHistory Windows/file/heap platform transport only; no Country/parser/constructor body replacement.'],
   entry_points={'countries':0x6722F0,'read_house_type':0x475540,'house_ctor':0x5113F0,'house_body':0x511850}))
