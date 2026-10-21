"""Session Country history on the existing physical Rules oracle owner."""
from pathlib import Path
import json,struct
from tools.native_oracle import configured_gamemd
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
 return dict(root=root,current=-3,rows=rows,retail_registered=retail)


class StartingInputs:
 """Only declared synthetic physical INI bytes, with no asset fallback."""
 def lookup(self,name):
  key=name.upper()
  if key not in fixtures:raise ValueError(('Undeclared starting-ownership input',name))
  return fixtures[key]


def starting_ownership_root():
 root='[Countries]\n0=Americans\n1=Alliance\n'
 for name,alias,parent in [('Americans','AmericaAlias','Alliance'),('Alliance','AllianceAlias','Americans')]:
  root+=f'[{name}]\nName={alias}\nParentCountry={parent}\nMultiplay=yes\nMultiplayPassive=no\n'
 types=[('V_CASE','americans',101),('V_PARENT','Alliance',901),('V_UNKNOWN','Unknown',10000),
  ('V_BASE','Americans',10000),('I_ALIAS','AMERICAALIAS',200),('I_EXACT','Americans',302),('I_PARENT','Alliance',402),
  ('I_NONE','<none>',10000),('I_RANDOM','<random>',10000)]
 for name,owner,cost in types:
  required,forbidden=('Americans','Alliance') if owner=='Alliance' else ('Alliance','Americans')
  root+=f'[{name}]\nOwner={owner}\nRequiredHouses={required}\nForbiddenHouses={forbidden}\nTechLevel=1\nCost={cost}\nAllowedToStartInMultiplayer=yes\n'
 return root,types


def starting_ownership_type_block(m,pointer,rootini,begin,end):
 from tools.native_oracle import run_checked
 from tools.spatial_oracle.building_body_rules import SP
 from unicorn.x86_const import UC_X86_REG_ESP,UC_X86_REG_EBP,UC_X86_REG_EBX,UC_X86_REG_ESI,UC_X86_REG_EDI
 m.u.mem_write(SP,bytes(0x200));m.u.reg_write(UC_X86_REG_ESP,SP)
 for reg,value in [(UC_X86_REG_EBP,pointer),(UC_X86_REG_EBX,pointer+0x24),
   (UC_X86_REG_ESI,rootini),(UC_X86_REG_EDI,rootini)]:m.u.reg_write(reg,value)
 run_checked(m.u,begin,end,context=dict(case='starting-ownership-type',type=m.string(pointer+0x24),block=f'{begin:08X}'))


def starting_ownership_types(m,rootini,types):
 # Original translation-unit initializers establish the actual vector vtables,
 # increment and empty state; constructors then enroll their own native slots.
 m.invoke(0x4E7460,0);m.invoke(0x4E7560,0)
 pointers={};rows=[]
 for name,_owner,_cost in types:
  pointer=m.alloc(0x1100);pointers[name]=pointer
  m.invoke(0x7470D0 if name.startswith('V_') else 0x5236A0,pointer,(m.cstring(name),))
  # Exact retained-default/store blocks from the common TechnoType reader.
  # Full weapon/image/production readers are outside this admission corpus.
  for begin,end in [(0x7149E1,0x7149FB),(0x714522,0x71458A),(0x71469F,0x7146B9),(0x714C44,0x714C65)]:
   starting_ownership_type_block(m,pointer,rootini,begin,end)
  rows.append(dict(id=name,owner_mask=m.read32(pointer+0x6CC),required_mask=m.read32(pointer+0xDA0),
   forbidden_mask=m.read32(pointer+0xDA4),tech_level=signed(m.read32(pointer+0x634)),
   cost=signed(m.read32(pointer+0x610)),allowed_to_start=bool(m.u.mem_read(pointer+0x6D5,1)[0])))
 return pointers,rows


def starting_ownership_qualifier_gates(m,house):
 from tools.native_oracle import run_checked
 from tools.spatial_oracle.building_body_rules import SP,dwords
 from unicorn.x86_const import UC_X86_REG_ESP,UC_X86_REG_EBP,UC_X86_REG_EDI,UC_X86_REG_EIP
 pointer=m.alloc(0x1100);m.invoke(0x7470D0,pointer,(m.cstring('REQ_CONTROL'),))
 owner_layer='[REQ_CONTROL]\nOwner=Americans,Alliance\n'
 owner_ini=ini(m,owner_layer)
 starting_ownership_type_block(m,pointer,owner_ini,0x7149E1,0x7149FB)
 items=m.alloc(4);m.u.mem_write(items,dwords(pointer));vector=m.alloc(24)
 m.u.mem_write(vector,dwords(0x7EB6D4,items,1,1,1,10))
 # Keep the documented shell superweapon gate open, so this generic Type
 # witness never reads Building-only fields after the qualifier/side gates.
 m.u.mem_write(0xA8B263,b'\x01')
 # Append thirty short existing-country IDs through the actual physical
 # master reader. The complete32-name list fits the native128-byte reader.
 extra_country_layer='[Countries]\n'+''.join(f'{index}={name}\n' for index,name in enumerate('ABCDEFGHIJKLMNOPQRSTUVWXYZ0123'))
 m.invoke(0x6722F0,m.read32(0x8871E0),(ini(m,extra_country_layer),))
 sentinel_registry_ids=[m.string(m.read32(m.read32(0xA83C9C)+index*4)+0x24) for index in range(m.read32(0xA83CA8))]
 all_countries=','.join(sentinel_registry_ids)
 assert len(sentinel_registry_ids)==32 and len(all_countries)<128
 rows=[]
 for name,required,forbidden in [('default_omitted',None,None),('explicit_none','<none>','<none>'),
   ('explicit_unknown','Unknown','Unknown'),('actual_required','Alliance','<none>'),
   ('actual_forbidden','Alliance','Alliance'),('nonempty_all_forbidden','Alliance',all_countries)]:
  body='[REQ_CONTROL]\nName=KeepSection\n'
  if required is not None:body+='RequiredHouses='+required+'\n'
  if forbidden is not None:body+='ForbiddenHouses='+forbidden+'\n'
  layer=ini(m,body)
  starting_ownership_type_block(m,pointer,layer,0x714522,0x71458A)
  result=m.invoke(0x5051E0,house,(vector,))
  m.u.mem_write(SP,bytes(0x200));m.u.reg_write(UC_X86_REG_ESP,SP)
  m.u.reg_write(UC_X86_REG_EBP,house);m.u.reg_write(UC_X86_REG_EDI,pointer)
  run_checked(m.u,0x4F79E4,(0x4F7994,0x4F7A9A),required_addresses=(0x4F79EA,),
   context=dict(case='can-build-country-qualifier-gate',name=name))
  stop=m.u.reg_read(UC_X86_REG_EIP)
  rows.append(dict(name=name,physical_layer=body,required_mask=m.read32(pointer+0xDA0),
   forbidden_mask=m.read32(pointer+0xDA4),first_buildable_result=m.string(result+0x24) if result else None,
   can_build_qualifier_gate=stop==0x4F7A9A,can_build_stop=f'{stop:08X}'))
 country=m.read32(house+0x34)
 return dict(type_id='REQ_CONTROL',owner='Americans,Alliance',owner_layer=owner_layer,owner_mask=m.read32(pointer+0x6CC),
  actual_country=m.string(country+0x24),actual_index=signed(m.read32(country+0xB8)),owner_parent=m.string(country+0x98),
  extra_country_layer=extra_country_layer,sentinel_registry_ids=sentinel_registry_ids,
  planned_side=signed(m.read32(pointer+0x6D0)),alternate_owner_masks=[m.read32(house+off) for off in (0x2C4,0x2C8,0x2CC,0x2D0)],
  rows=rows)


def generate_starting_ownership():
 import hashlib
 from unicorn import UC_HOOK_CODE
 from unicorn.x86_const import UC_X86_REG_ESP,UC_X86_REG_EBP
 from tools.native_oracle import RET_MAGIC,run_checked
 from tools.spatial_oracle.building_body_rules import SP,dwords
 global physical
 physical=StartingInputs();fixtures.clear()
 root,types=starting_ownership_root();m,r,rootini=fixture(root)
 try:
  m.invoke(0x6722F0,r,(rootini,));bodies(m,rootini)
  countries={m.string(p+0x24):p for p in
   (m.read32(m.read32(0xA83C9C)+index*4) for index in range(m.read32(0xA83CA8)))}
  country_inputs=[dict(id=name,name=m.string(pointer+0x64),parent=m.string(pointer+0x98),
   actual_index=signed(m.read32(pointer+0xB4)),passive=bool(m.u.mem_read(pointer+0x1A6,1)[0]))
   for name,pointer in countries.items()]
  m.u.mem_write(0x8871E0,dwords(r));pointers,type_rows=starting_ownership_types(m,rootini,types)
  base_items=m.alloc(4);m.u.mem_write(base_items,dwords(pointers['V_BASE']))
  m.u.mem_write(r+0xB24,dwords(base_items));m.u.mem_write(r+0xB30,dwords(1))
  houses={}
  for name,country in countries.items():
   house=m.alloc(0x17000);houses[name]=house
   m.u.mem_write(house+0x34,dwords(country));m.u.mem_write(house+0x1D4,dwords(10))
  m.u.mem_write(0x822CF4,dwords(10))
  m.seed_streams(31)
  rng=m.rng_state();rng_entries=[]
  def observe(u,pc,_size,_data):
   if pc in (0x65C780,0x65C7E0):rng_entries.append(f'{pc:08X}')
  hook=m.u.hook_add(UC_HOOK_CODE,observe)
  candidate_lists={};candidate_house_tech_controls=[]
  for name,house_tech_level in [('Americans',10),('Alliance',10),('Americans',0)]:
   house=houses[name];m.u.mem_write(house+0x1D4,dwords(house_tech_level))
   budget=m.alloc(4);m.u.mem_write(budget,dwords(1000))
   m.u.mem_write(SP,dwords(RET_MAGIC,house,budget));m.u.reg_write(UC_X86_REG_ESP,SP)
   run_checked(m.u,0x5D70F0,0x5D72AB,required_addresses=(0x5D7156,0x5D7186,0x5D7246),
    context=dict(case='starting-ownership-candidates',country=name,house_tech_level=house_tech_level))
   sp=m.u.reg_read(UC_X86_REG_ESP);lists={}
   for key,offset in [('vehicles',0x30),('infantry',0x18)]:
    count=m.read32(sp+offset+16);items=m.read32(sp+offset+4)
    lists[key]=[m.string(m.read32(items+index*4)+0x24) for index in range(count)]
   if house_tech_level==10:candidate_lists[name]=lists
   else:candidate_house_tech_controls.append(dict(country=name,house_tech_level=house_tech_level,**lists))
   # Resume the original vector cleanup/return tail after observing the lists.
   # No spawn, random choice or placement body is replaced or executed here.
   run_checked(m.u,0x5D743E,RET_MAGIC,context=dict(case='starting-ownership-cleanup',country=name))
   assert rng==m.rng_state()
  for house in houses.values():m.u.mem_write(house+0x1D4,dwords(10))
  rows=[];passive_layer='[Alliance]\nName=AllianceAlias\nParentCountry=Americans\nMultiplay=yes\nMultiplayPassive=yes\n'
  for name,participants,passive in [
    ('american_only',['Americans'],False),('alliance_only',['Alliance'],False),
    ('both_countries',['Americans','Alliance'],False),
    ('passive_alliance',['Americans','Alliance'],True),('passive_only',['Alliance'],True)]:
   if passive and not rows[-1].get('alliance_passive',False):
    # A real retained physical read changes the Country flag; never patch the
    # mask, constructor output or an observed ownership decision.
    layer=ini(m,passive_layer)
    bodies(m,layer)
   items=m.alloc(4*len(participants));m.u.mem_write(items,dwords(*(houses[n] for n in participants)))
   m.u.mem_write(0xA8022C,dwords(items));m.u.mem_write(0xA80238,dwords(len(participants)))
   m.u.mem_write(0xA8B270,dwords(4));m.u.mem_write(0x822CF4,dwords(10))
   m.u.mem_write(SP,dwords(RET_MAGIC));m.u.reg_write(UC_X86_REG_ESP,SP)
   required=(0x5D6DB5,0x5D6E08,0x5D6E86,0x5D6ECB)+(() if name=='passive_only' else (0x711EB0,))
   run_checked(m.u,0x5D6D80,0x5D6ED6,required_addresses=required,
    context=dict(case='starting-ownership-budget',name=name,participants=participants))
   sp=m.u.reg_read(UC_X86_REG_ESP)
   rows.append(dict(name=name,participants=participants,alliance_passive=passive,
    actual_country_mask=m.read32(sp+0x10),budget=signed(m.u.reg_read(UC_X86_REG_EBP))))
   assert rng==m.rng_state()
  qualifier_gates=starting_ownership_qualifier_gates(m,houses['Alliance'])
  assert rng==m.rng_state()
  m.u.hook_del(hook);assert not rng_entries;m.check_code()
  return dict(schema_version=1,
   root=root,passive_layer=passive_layer,country_inputs=country_inputs,
   base_units=['V_BASE'],tech_level=10,unit_count=4,type_inputs=type_rows,
   candidate_lists=candidate_lists,candidate_house_tech_controls=candidate_house_tech_controls,
   budget_rows=rows,qualifier_gates=qualifier_gates,rng_entries=rng_entries,rng_unchanged=True,
   rng_state_sha256={name:hashlib.sha256(bytes.fromhex(raw)).hexdigest() for name,raw in rng.items()})
 finally:m.close()


def starting_ownership_metadata():
 from tools.native_oracle import provenance
 return provenance(scope='Original starting-extra admission/shared budget and adjacent FirstBuildable/CanBuild country qualifier gates',
  assumptions=[
   'Synthetic physical INI bytes; whole original Rules665650, Countries6722F0 and live Country virtual+64 readers. Both Countries deliberately have opposite ParentCountry. Original Unit/Infantry vector initializers and full type constructors enroll native registry order. Selected common Owner/RequiredHouses/ForbiddenHouses/TechLevel/Cost/AllowedToStart reader blocks execute; other type readers are excluded.',
   'Two additional types read exact Owner=<none> and Owner=<random> through the same original Owner reader. Their stored mask values are observed, not supplied. Both have AllowedToStart=yes, TechLevel1 and Cost10000; the ordinary candidate and budget results exclude them.',
   'Consumer House storage supplies Country pointer and TechLevel10, plus a separate American HouseTechLevel0 candidate control while global TechLevel remains10; selected House registry membership, BaseUnit pointer list, global TechLevel10 and unit_count4 are explicit continuation inputs. House/Scenario construction, full PrepareSession and full Rules Process are excluded. A retained physical Country reader sets Alliance MultiplayPassive for two budget controls.',
   'Six independent qualifier controls follow the completed starting-unit observations. A fresh full UnitType constructor plus actual retained physical RequiredHouses/ForbiddenHouses reader produces omitted, none, unknown, actual-country and nonempty-all32-country masks. Original6722F0 appends thirty short Country IDs; the saved complete32-name list fits128 bytes. Full FirstBuildable5051E0 executes with a singleton generic-type vector, PlannedForSide=-1 from its constructor and supplied shell SuperWeapons=true. The original CanBuild qualifier block4F79E4 executes through pass4F7A9A or rejection4F7994 with supplied House/type registers and zero alternate-owner masks; preceding/following CanBuild gates and whole CanBuild return are excluded.',
   'Candidate5D70F0 executes through5D72AB, then resumes original vector cleanup5D743E to return. Budget5D6D80 executes through5D6ED6 before any per-House callbacks. Both are bounded instruction regions, not whole starting-force creation, RNG selection, placement, world lifecycle or renderer parity. Full three-stream RNG states are compared unchanged; their hashes and raw/ranged draw-entry visits are retained. No timer write or detach call occurs in these executed admission/arithmetic paths; downstream spawn/mission effects are excluded.'],
  substitutions=['Existing RetailRulesHistory physical-file/Windows/heap platform transport only. No original code, INI parser output, mask decision, cost arithmetic or RNG result is patched. The synthetic input catalog rejects undeclared resource requests.'],
  entry_points={'countries':0x6722F0,'country_body':0x511850,'unit_constructor':0x7470D0,
   'infantry_constructor':0x5236A0,'owner_reader':0x7149E1,'qualifiers_and_tech_level_reader':0x714522,
   'cost_reader':0x71469F,'allowed_to_start_reader':0x714C44,
   'candidate_prefix':0x5D70F0,'candidate_stop':0x5D72AB,'candidate_cleanup':0x5D743E,
   'budget_prefix':0x5D6D80,'budget_stop':0x5D6ED6,'first_buildable':0x5051E0,
   'can_build_qualifier_start':0x4F79E4,'can_build_qualifier_pass':0x4F7A9A,'can_build_qualifier_reject':0x4F7994})


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
 if '--starting-ownership' in sys.argv[1:]:
  args=sys.argv[1:];args.remove('--starting-ownership')
  finish_vectors(generate_starting_ownership,Path(__file__).with_name('session_country_starting_ownership.json'),
   argv=args,provenance=starting_ownership_metadata,source_paths={'producer':Path(__file__),
    'physical_rules_owner':Path(__file__).with_name('weapon_speed_order.py')})
 elif '--parent' in sys.argv[1:]:
  finish_vectors(generate_parent,Path(__file__).with_name('session_country_parent.json'),
   provenance=parent_metadata,argv=[arg for arg in sys.argv[1:] if arg!='--parent'],
   source_paths={'producer':Path(__file__)})
 else:
  finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=provenance(
   scope='Session Country master/ReadHouseType and subsequent live body history',
   assumptions=['Declared physical root and MultiPlayer sections; original Rules665650, Countries6722F0, whole475540 and live Country virtual+64 reader. Two retained phases per token. Retail root master enumerated through original physical INI loader. Full PrepareSession GUI and destructive reset are not executed.'],
   substitutions=['Existing RetailRulesHistory Windows/file/heap platform transport only; no Country/parser/constructor body replacement.'],
   entry_points={'countries':0x6722F0,'read_house_type':0x475540,'house_ctor':0x5113F0,'house_body':0x511850}))
