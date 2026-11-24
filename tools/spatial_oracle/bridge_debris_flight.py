"""Original bridge producer followed by bounded primary AnimClass AI flights.

See bridge_debris_flight.md and metadata for inputs, substitutions and coverage.
All child constructors execute; only the primary animation is AI-scheduled.
"""
import hashlib
import json
import struct
from pathlib import Path
from unicorn.x86_const import *
from tools.native_oracle import run_checked,finish_vectors,provenance
from tools.spatial_oracle.bounce_height import matrices
from tools.spatial_oracle import bridge_debris_producer as p
from tools.rules_oracle.bridge_landing_inputs import recorded_retail_inputs
LANDING=recorded_retail_inputs()
l=p.launch
TABLE=l.MEM+0x100000
CELLS=l.MEM+0x60000
DUMMY=0xABDC50
OUT=l.MEM+0x1F8000

class Flight(p.Machine):
 def __init__(self,seed):
  self.phase='init';self.trace=[];self.tick=0;self.primary=None;self.bounce_result=None
  super().__init__(seed)
 def hook(self,u,a,n,d):
  sp=u.reg_read(UC_X86_REG_ESP)
  if a in (0x565730,0x5657A0):return
  if self.phase=='drain' and a==0x7CAA5E:
   pointer=l.read32(u,sp);length=l.read32(u,sp+4);u.mem_read(pointer,length)
   self.valid_read_checks.append(dict(pointer=pointer,length=length,supplied_is_bad_read_ptr=0))
   u.reg_write(UC_X86_REG_EAX,0);u.reg_write(UC_X86_REG_ESP,sp+8);u.reg_write(UC_X86_REG_EIP,0x7CAA64);return
  if self.phase=='drain' and a==0x7C8B3D:self.ret(0,0);return
  if self.phase=='drain' and a==0x426590:
   self.scalar_deletions.append(dict(this=u.reg_read(UC_X86_REG_ECX),flags=l.read32(u,sp+4),caller=f'{l.read32(u,sp):08X}'));self.ret(0,4);return
  if self.phase=='flight':
   if a==0x423946:self.bounce_result=u.reg_read(UC_X86_REG_EAX)
   if a in (0x423930,0x4255B0,0x5F65F0,0x5F4D30,0x7258D0,0x489280,0x48A620,0x439150,0x4A9770,l.CTOR,l.START,l.RANGED):
    ev=dict(a=f'{a:08X}',caller=f'{l.read32(u,sp):08X}',this=u.reg_read(UC_X86_REG_ECX),tick=self.tick);self.trace.append(ev)
    if a==l.CTOR:
     ev.update(type=self.types[l.read32(u,sp+4)],coord=list(struct.unpack('<3i',u.mem_read(l.read32(u,sp+8),12))),delay=l.read32(u,sp+12))
    if a==l.RANGED:ev.update(min=p.signed(l.read32(u,sp+4)),max=p.signed(l.read32(u,sp+8)))
    if a==0x489280:ev.update(coord=list(struct.unpack('<3i',u.mem_read(u.reg_read(UC_X86_REG_ECX),12))),damage=p.signed(u.reg_read(UC_X86_REG_EDX)),args=[l.read32(u,sp+4*i) for i in range(1,5)])
   if a in (0x439150,0x4A9770):self.ret(0,4);return
   if a in (0x587180,0x57BAA0,0x57CCF0):
    self.trace.append(dict(a=f'{a:08X}',caller=f'{l.read32(u,sp):08X}',this=u.reg_read(UC_X86_REG_ECX),tick=self.tick,supplied_return=False));self.ret(0,4);return
   if a==0x7C8B3D:self.ret(0,0);return
  before=len(self.events)
  super().hook(u,a,n,d)
  if self.phase=='flight':
   for event in self.events[before:]:event['tick']=self.tick
 def setup(self,case):
  self.input_path=Path(__file__).with_suffix('.inputs.json')
  self.input_data=json.loads(self.input_path.read_text())
  self.input_rows={row['name']:row for row in self.input_data['rows']}
  prior_input=p.INPUT_PATH
  p.INPUT_PATH=self.input_path;p.retail_inputs.cache_clear()
  original=p.EXPLOSIONS
  p.EXPLOSIONS=original+['WAKE1','H2O_EXP1','H2O_EXP2','H2O_EXP3','SMOKEY2']
  try:types=super().setup(dict(case,explosion_count=4))
  finally:
   p.EXPLOSIONS=original;p.INPUT_PATH=prior_input;p.retail_inputs.cache_clear()
  u=self.uc
  expanded=LANDING['rules']
  assert self.input_data['wake']==expanded['wake']
  assert self.input_data['splash_list']==expanded['splash_list']
  u.mem_write(l.RULES+0x94,l.dwords(self.type_by_name[expanded['wake']]))
  splashes=l.MEM+0x1A800
  u.mem_write(splashes,l.dwords(*(self.type_by_name[n] for n in expanded['splash_list'])))
  u.mem_write(l.RULES+0xBC4,l.dwords(splashes));u.mem_write(l.RULES+0xBD0,l.dwords(3))
  for name,ptr in self.type_by_name.items():
   row=self.input_rows[name]
   if row.get('trailer_anim'):
    u.mem_write(ptr+0x308,l.dwords(self.type_by_name[row['trailer_anim']],row['trailer_seperation']))
  return types
 def setup_flight(self,terrain):
  u=self.uc
  u.mem_write(0x87F924,l.dwords(TABLE,0x40000));u.mem_write(0x87F914,l.dwords(64,64));u.mem_write(0x89C778,l.dwords(104))
  u.mem_write(l.SP,l.dwords(l.STOP));u.reg_write(UC_X86_REG_ESP,l.SP);run_checked(u,0x439610,l.STOP)
  u.mem_write(0x89A1C0,l.dwords(104));u.mem_write(l.SP,l.dwords(l.STOP));u.reg_write(UC_X86_REG_ESP,l.SP);run_checked(u,0x421E20,l.STOP);assert l.read32(u,0x89A1B4)==416
  u.mem_write(0x89E870,l.dwords(104));u.mem_write(l.SP,l.dwords(l.STOP));u.reg_write(UC_X86_REG_ESP,l.SP);run_checked(u,0x489100,l.STOP)
  for i,m in enumerate(matrices()):u.mem_write(0xB45188+48*i,struct.pack('<12I',*m))
  u.mem_write(DUMMY,bytes(0x200));u.mem_write(DUMMY,l.dwords(0x7E4EEC));u.mem_write(DUMMY+0x44,l.dwords(-1))
  index=0
  for y in range(0,30):
   for x in range(0,32):
    cell=l.CELL if (x,y)==(12,9) else CELLS+index*0x200;index+=1
    level=4
    if terrain=='mesa':level=4 if max(abs(x-12),abs(y-9))<=1 else 0
    if terrain=='pit':level=0 if max(abs(x-12),abs(y-9))<=2 else 4
    if terrain=='cliff':level=4 if max(abs(x-12),abs(y-9))<=0 else 12
    if terrain=='ground0':level=0
    u.mem_write(cell,l.dwords(0x7e4eec));u.mem_write(cell+0x24,struct.pack('<hh',x,y));u.mem_write(cell+0x44,l.dwords(-1));u.mem_write(cell+0x11b,bytes([level,0]));u.mem_write(cell+0xEC,l.dwords(2 if terrain in ('water','waterbridge') else 0));u.mem_write(cell+0x140,l.dwords(0x100 if terrain in ('bridge','waterbridge') else 0));u.mem_write(TABLE+(y*512+x)*4,l.dwords(cell));u.mem_write(cell+0x38,l.dwords(1020 if terrain in ('bridge','waterbridge') else -1));u.mem_write(cell+0x2c,l.dwords(l.MEM+0x3C000))
  u.mem_write(l.SP,l.dwords(l.STOP));u.reg_write(UC_X86_REG_ESP,l.SP);run_checked(u,0x725850,0x725886)
  u.mem_write(0xB0F69C,l.dwords(l.MEM+0x30000,256));u.mem_write(0xB0F6A8,l.dwords(0))
  u.mem_write(0xAC1398,l.dwords(1,0x7fff7fff))
  # Independently executed original readers establish the exact injected bits.
  he=LANDING['warheads']['HE'];rules=LANDING['rules']
  u.mem_write(p.WARHEAD+0x147,bytes([he['wood']]))
  u.mem_write(p.WARHEAD+0xa0,struct.pack('<11Q',*(int(x,16) for x in he['verses_bits'])))
  u.mem_write(p.WARHEAD+0x124,struct.pack('<I',int(he['cell_spread_bits'],16)))
  u.mem_write(p.WARHEAD+0x12c,struct.pack('<I',int(he['percent_at_max_bits'],16)))
  assert self.input_data['bridge_strength']==rules['bridge_strength']
  assert self.input_data['he_wall']==he['wall']
  u.mem_write(l.RULES+0x16c8,l.dwords(rules['max_damage']));u.mem_write(l.RULES+0x1740,l.dwords(rules['bridge_strength']));u.mem_write(l.SCENARIO,l.dwords(0x8000));u.mem_write(p.WARHEAD+0x144,bytes([he['wall']]))
  u.mem_write(l.MEM+0x3C000+0x44,l.dwords(0x18))
  for address,value in ((0xAA0E28,1000),(0xABAD1C,2000),(0xABAD30,20),(0xAA1028,40)):u.mem_write(address,l.dwords(value))
  for name,ptr in self.type_by_name.items():
   if self.input_rows[name].get('warhead')=='HE':u.mem_write(ptr+0x330,l.dwords(p.WARHEAD))

 def execute_flight(self,case):
  self.setup(case);self.setup_flight(case['terrain']);u=self.uc
  self.events.clear();self.advances=0;before=self.rng()
  u.mem_write(l.SP,l.dwords(l.STOP));u.reg_write(UC_X86_REG_ESP,l.SP);u.reg_write(UC_X86_REG_ECX,l.CELL);run_checked(u,p.ENTRY,l.STOP,count=2000000)
  producer_events=list(self.events);anims=list(self.constructed);producer_after=self.rng();producer_raw=self.advances
  primary=next((a for a in anims if self.constructed_types[a] in p.METALLIC),None)
  assert primary is not None
  self.primary=primary;self.phase='flight';self.events.clear();ticks=[];birth_state=l.anim_state(u,primary)
  for tick in range(1,301):
   self.tick=tick;self.bounce_result=None;u.mem_write(l.FRAME,l.dwords(1000+tick))
   u.mem_write(l.SP,l.dwords(l.STOP));u.reg_write(UC_X86_REG_ESP,l.SP);u.reg_write(UC_X86_REG_ECX,primary)
   try:run_checked(u,0x423AC0,l.STOP,count=2000000)
   except Exception:
    print('events',json.dumps(self.events[-12:],indent=2));print('trace',json.dumps(self.trace[-12:],indent=2));raise
   ticks.append(dict(tick=tick,rng_state=self.rng(),current_frame=p.signed(l.read32(u,primary+0xAC)),delay_remaining=p.signed(l.read32(u,primary+0x184)),loop_remaining=u.mem_read(primary+0x195,1)[0],first_ai_guard=u.mem_read(primary+0x19C,1)[0],inactive=u.mem_read(primary+0x19B,1)[0],frame_step=p.signed(l.read32(u,primary+0xC4)),timer_start=p.signed(l.read32(u,primary+0xB4)),timer_duration=p.signed(l.read32(u,primary+0xBC)),rate_reload=p.signed(l.read32(u,primary+0xC0)),outcome=self.bounce_result,alive=u.mem_read(primary+0x90,1)[0],location=list(struct.unpack('<3i',u.mem_read(primary+0x9c,12))),body=bytes(u.mem_read(primary+0x128,0x50)).hex()))
   if not u.mem_read(primary+0x90,1)[0]:break
  result=dict(input=case,type=self.constructed_types[primary],birth_state=birth_state,producer_events=producer_events,producer_rng_before=before,producer_rng_after=producer_after,ticks=ticks,flight_events=list(self.events),trace=list(self.trace),rng_after=self.rng(),producer_raw_draw_count=producer_raw,flight_raw_draw_count=self.advances-producer_raw,delete_count=l.read32(u,0xB0F6A8),limbo=u.mem_read(primary+0x81,1)[0],in_logic=u.mem_read(primary+0x98,1)[0])
  self.phase='drain';self.scalar_deletions=[];self.valid_read_checks=[]
  # Original CRT RTTI helper uses the Windows SEH chain at FS:[0].
  u.mem_map(0,0x1000);u.mem_write(0,l.dwords(0xffffffff))
  u.mem_write(l.SP,l.dwords(l.STOP));u.reg_write(UC_X86_REG_ESP,l.SP);run_checked(u,0x725C70,l.STOP,count=100000)
  result['drain']=dict(queue_before=result['delete_count'],queue_after=l.read32(u,0xB0F6A8),scalar_deletions=self.scalar_deletions,valid_read_checks=self.valid_read_checks)
  assert result['drain']['queue_after']==0 and len(self.scalar_deletions)==1,result['drain']
  self.phase='continuation';result['next_rng']=[]
  for _ in range(4):
   u.mem_write(l.SP,l.dwords(l.STOP));u.reg_write(UC_X86_REG_ESP,l.SP);u.reg_write(UC_X86_REG_ECX,l.SCENARIO+0x218);run_checked(u,l.NEXT,l.STOP);result['next_rng'].append(u.reg_read(UC_X86_REG_EAX))
  return result
def generate():
 rows=[]
 for terrain in ('ground4','ground0','water','bridge','waterbridge','mesa','pit','cliff'):
  for seed in (1,10,25,65):
   c=dict(seed=seed,cell=[12,9],level=0 if terrain in ('ground0','pit') else 4,terrain=terrain)
   rows.append(Flight(seed).execute_flight(c))
 return rows
def metadata():
 result=provenance(scope='32 joined original47DD70 bridge producers -> original423AC0 primaryAnimAI perframe -> original439B00 physics -> original423930 contact/result -> native dry/water/deck landing constructors, actual489280 area/bridge admission and48A620 combat-light call, native primaryDestroy/UnInit and original725C70 duplicatequeue drain toonescalardestructorboundary. Emptycontactreceiver lists. D normalexpiry and DBRIS1LG trailer construction included. Primary-only scheduler boundary: otheranims are constructed but not AI-scheduled.',assumptions=[
  'Imports companion bridge_debris_producer.py; original producer inputs/assumptions inherit its metadata. Companion bridge_debris_flight.inputs.json production PR552 release Hills export supplies24 resolvedtypes, actualWAKE1/orderedSplashList[H2O_EXP3,H2O_EXP2,H2O_EXP1], HEWalltrue and BridgeStrength1500.',
  'All24 AnimType constructors427530 execute; ART numericfields and bindings supplied from export. Originalimage suffix derivesMiddle fromactualframecounts. D remainsnativeconstructor-only, absentsection/image. Framebuffers/pixels absent.',
  'OriginalMapGetCell565730/5657A0 run on supplied32x30 realCelltable. Slopematricesretail, allcellsflat slope0; ordinarylevels4/0, 3x3level4mesa surrounded0,5x5level0pit surrounded4, or singlelevel4source surrounded12(cliff). Structuralbridge cases allcellsraw0x100; watercases CellLand2. Dummycelllevel0/overlay-1. This is boundedgeometry, notloadedmap/topologyproof.',
  'Original47B2C0/439610/421E20/489100 initialize bridge/Bounce/Anim/Area416 offsets fromsupplied104 scalars. Runtimeframe1000 then incrementsonceperprimaryAI. No otherAnimAI runs, including siblingbridgeexplosion, trailer or expiry/wake/splash; their later sound/smudge/frame RNG is outside scope.',
  'Bridge and waterbridge cases supplyconcretefamilytile1020, base1000,middle20, sharedanchoroverlay18; originalareaA/B/C/D execute on admittedimpact. ScenarioDestroyabletrue, MaxDamage/HE exact stored bits independently established by rules_oracle/bridge_landing_inputs original readers; active retail MaxDamage10000, wood0.75, spread0.5/PAM0.5. Driver callbacks, ifreached, returnfalse toexclude collapsephysics; no geometrymutation claim.',
  'PrimaryfullAnimAI uses nativecreated state fromproducer. DBRIS1LG trailerSMOKEY2 binding andseparation2 supplied andoriginalconstructor runs on cadence. WaterWake/Splash bindings followorderedproductionexport. Object/tag/audio/house/Techno observers empty. GlobalAnimvector containsconstructedobjects; commonLogicvector empty, pendingdeletebuffer hascapacity256.',
  'Report/sound indices remain nativector-1 in alltypes, despiteproduction names ininputfile: soundStart side effects intentionally notexecuted. This is a declared input substitution, not silent actualARTsound parity. Scorch/crater flags are copied but delayedchildMiddle bodies notscheduled. No cross-animation scheduler/RNGparity claim.',
 ],substitutions=[
  'Inherited ObjectMark5F5850 returns1; DisplaySubmit4A9720 returns1; operatornew7C8E17 is bumpallocator. Actual Maplookups override inheritedcontrolledlookup, withno map substitution.',
  '439150 empty BombListPointerGotInvalid and4A9770 DisplayRemove return0; operator_delete7C8B3D no-op. NativeAnimDestroy4255B0/ObjectUnInit5F65F0/AnimLimbo/ObjectLimbo/pointerexpiry execute withdeclaredemptyobservers.',
  'Bridge driver587180/57BAA0/57CCF0, ifreached, returnfalse andlogactualcaller. No contactreceiver substitutes becausecontactlistsareempty. Dryarea489280 andcombatlight48A620 originalbodiesexecute; lighttypefieldszero/noactualdynamiclight.',
  'Original725850..725886 initializespendingdeletevector beforeprovidedcapacity/buffer; terminal725C70 drainexecuteswholebody. FS:[0] suppliesvalidemptySEHchain. OriginalRTTI/classcomparisonsrun, withWindowsIsBadReadPtr importedcallat7CAA5E supplied0 aftercheckingmappedfixturememory. ScalarAnimDtor426590 returns0, pops4, andrecordscall/flags. Queuecompactionisnotstubbed.',
  'Originalexecutableinstructionsunchanged. RuntimeART/report/reference/image/observer/bufferstate suppliedasdeclaredabove. No graphics output orwholegame/nativefullschedulerclaim.',
 ],entry_points={'producer':p.ENTRY,'anim_type_ctor':0x427530,'anim_ctor':l.CTOR,'anim_ai':0x423AC0,'bounce_update':0x439B00,'bounce_result':0x423930,'anim_destroy':0x4255B0,'area_damage':0x489280,'combat_light':0x48A620,'anim_deck_init':0x421E20,'area_deck_init':0x489100,'pending_vector_init':0x725850,'pending_drain':0x725C70,'scalar_anim_dtor_boundary':0x426590})
 result['native_landing_inputs_sha256']=hashlib.sha256((Path(__file__).parent.parent/'rules_oracle/bridge_landing_inputs.json').read_bytes()).hexdigest()
 result['fixture_input_sha256']=hashlib.sha256(Path(__file__).with_suffix('.inputs.json').read_bytes()).hexdigest()
 return result
if __name__=='__main__':finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
