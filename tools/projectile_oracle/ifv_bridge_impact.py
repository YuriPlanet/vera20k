"""Research only. Add retained stock bridge topology to original guided impact.
Imports preserved impact harness, adapting ONLY fixture initialization statements
in memory; every launch/AI/damage/bridge/lifetime native instruction is unchanged.
"""
import importlib.util,inspect,json,struct,sys,os
from pathlib import Path
from unicorn.x86_const import *
from unicorn import UC_HOOK_CODE
from tools.native_oracle import run_checked,finish_vectors,provenance
from tools.spatial_oracle.bridge_rim import GLOBALS
from tools.spatial_oracle.building_body_rules import SP,INI,dwords
spec=importlib.util.spec_from_file_location('_bridge_impact',Path(__file__).with_name('ifv_impact.py'));impact=importlib.util.module_from_spec(spec);spec.loader.exec_module(impact)
old_prepare=impact.prepare
CASE=Path('tools/spatial_oracle/bridge_rim_stock_inputs.json')
case=json.loads(CASE.read_text());PRIME=False

def bridge_snapshot(m,cells):
 u=m.u
 return [dict(coord=list(xy),flags=m.read32(p+0x140),overlay=impact.i32(u,p+0x44),state=u.mem_read(p+0x11e,1)[0],anchor=(list(struct.unpack('<2h',u.mem_read(m.read32(p+0x2c)+0x24,4))) if m.read32(p+0x2c) else None)) for xy,p in sorted(cells.items())]

def prepare():
 out=old_prepare();m,source,st,w,oldcells,initial=out;u=m.u
 # Initialize original registry, construct the physical dense prefix, then read
 # reached bridge overlays through full native ObjectType+OverlayType readers.
 u.reg_write(UC_X86_REG_ESP,SP);run_checked(u,0x4e71e0,0x4e7216)
 listed,_=impact.lexical((impact.assets_root()/'RULESMD.INI').read_bytes(),{'OverlayTypes'})
 names=list(listed['OverlayTypes'].values())[:28];types={}
 for name in names:
  t=m.alloc(0x300);m.invoke(0x5fe250,t,(m.cstring(name),));types[name]=t
 selected={names[row[5]] for row in case['cells'] if row[5] is not None};rows=[]
 art,_=impact.lexical((impact.assets_root()/'ARTMD.INI').read_bytes(),selected);m.make_ini(art)
 for file in ('RULESMD.INI','MPBattleMD.ini','Hills.map'):
  sections,_=impact.lexical((impact.assets_root()/file).read_bytes(),selected);m.rules_cache(sections)
  for name in sorted(selected):
   t=types[name];admitted=m.invoke(0x5fe770,t,(impact.launch.RULES,))
   rows.append(dict(name=name,file=file,admitted=admitted&255,index=impact.i32(u,t+0x294),wall=bool(u.mem_read(t+0x2a8,1)[0]),explodes=bool(u.mem_read(t+0x2b0,1)[0]),land=impact.i32(u,t+0x298)))
 initial['overlay_types']=dict(dense_prefix=names,read_layers=rows)
 cells={};base=0x40000000;table=0x26000000
 u.mem_map(base,0x20000);u.mem_write(table,bytes(0x100000))
 for i,row in enumerate(case['cells']):
  x,y=row[:2];cells[x-102,y-120]=base+i*0x200
 for row in case['cells']:
  x,y,tile,sub,flags,overlay,state,anchor,level,land=row;p=cells[x-102,y-120]
  ap=cells[anchor[0]-102,anchor[1]-120] if anchor and not flags&0x80 else 0
  u.mem_write(p,dwords(0x7e4eec));u.mem_write(p+0x24,struct.pack('<2h',x-102,y-120));u.mem_write(p+0x2c,dwords(ap));u.mem_write(p+0x38,dwords(tile));u.mem_write(p+0x44,dwords(-1 if overlay is None else overlay));u.mem_write(p+0xec,dwords(land));u.mem_write(p+0x11a,bytes((sub,level)));u.mem_write(p+0x11e,bytes((state,)));u.mem_write(p+0x140,dwords(flags));u.mem_write(table+((y-120)*512+x-102)*4,dwords(p))
 u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,0x87f7e8);u.reg_write(UC_X86_REG_EBX,0);run_checked(u,0x56509b,0x565105)
 u.reg_write(UC_X86_REG_ESP,SP);run_checked(u,0x6d1a60,0x6d1a96)
 u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,0x87f7e8);run_checked(u,0x65296e,0x6529f5)
 u.mem_write(0x87f7e8+0xf4,dwords(*case['size']));u.mem_write(0x87f914,dwords(*case['size']));u.mem_write(0xaa0e28,dwords(case['bridge_base']))
 for k,a in GLOBALS.items():u.mem_write(a,dwords(case['rim_keys'][k]))
 def supplied_clock(uc,a,n,d):
  if a==0x6c8c40:u.reg_write(UC_X86_REG_EAX,0);u.reg_write(UC_X86_REG_EIP,0x6c8c46)
 clock_hook=u.hook_add(UC_HOOK_CODE,supplied_clock)
 tactical=m.alloc(0xe20);m.invoke(0x6d1c20,tactical);u.hook_del(clock_hook);u.mem_write(0xb0ce30,dwords(800,600));u.mem_write(0xb0cd48,struct.pack('<Q',0x3fc25e5374344960))
 u.mem_write(source+0x2b4,dwords(cells[10,20]));u.mem_write(m.read32(0xa8b230),dwords(0x8000))
 u.mem_write(0x89e870,dwords(104));m.invoke(0x489100,0);m.invoke(0x49f2f0,0)
 r=m.read32(0x8871e0);u.reg_write(UC_X86_REG_ESI,r);run_checked(u,0x6675da,0x6675e4)
 for file in ('RULESMD.INI','MPBattleMD.ini','Hills.map'):
  sections,_=impact.lexical((impact.assets_root()/file).read_bytes(),{'CombatDamage'});m.make_ini(sections)
  if m.invoke(0x526810,INI,(m.cstring('CombatDamage'),)):
   u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,r);u.reg_write(UC_X86_REG_EDI,INI);run_checked(u,0x66cd66,0x66cd8c)
 initial['bridge_strength']=impact.i32(u,r+0x1740)
 # Original bridge effect vector constructors/retail reads and full ART reads.
 from tools.rules_oracle.bridge_anim_inputs import NAMES
 from tools.rules_oracle.bridge_anim_inputs import ROOT as bridge_assets_root
 extra=bridge_assets_root
 for path in extra.glob('*.shp'):m.assets[path.name.upper()]=path.read_bytes()
 art,_=impact.lexical((impact.assets_root()/'ARTMD.INI').read_bytes(),set(NAMES));m.make_ini(art)
 u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,r);u.reg_write(UC_X86_REG_EBX,0);u.reg_write(UC_X86_REG_EDI,10);run_checked(u,0x665827,0x66585f);run_checked(u,0x6665d5,0x666604)
 for file in ('RULESMD.INI','MPBattleMD.ini','Hills.map'):
  sections,_=impact.lexical((impact.assets_root()/file).read_bytes(),{'General','CombatDamage'});m.rules_cache(sections)
  for start,end in ((0x66da90,0x66db93),(0x66db93,0x66dc96),(0x66c184,0x66c287),(0x66d847,0x66d894)):
   u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ESI,r);u.reg_write(UC_X86_REG_EDI,impact.launch.RULES);run_checked(u,start,end)
 for name in NAMES:
  t=m.invoke(0x428b80,m.cstring(name));m.invoke(0x427d00,t,(INI,))
 u.mem_write(0xa8eb60,dwords(4))
 initial['bridge_effect_types']=[dict(name=n,physical_asset_sha256=__import__('hashlib').sha256(m.assets[(n+'.shp').upper()]).hexdigest() if (n+'.shp').upper() in m.assets else None) for n in NAMES]
 # Supplied retained hierarchy classes are explicit (not stock loader output).
 # Original vector construction and bridge edge publication run before impact.
 side=sum(case['size'])+1;plane=m.alloc(side*side*10);raw=bytearray(side*side*10)
 for y in range(side):
  for x in range(side):struct.pack_into('<3H',raw,(y*side+x)*10,*([1 if y<=20 else 2]*3))
 u.mem_write(plane,bytes(raw));u.mem_write(0x87f7e8+0x6c,dwords(side*side,plane))
 for level in range(3):
  header=0x87f7e8+0x8c+24*level;m.invoke(0x58ae60,header,(3,0));u.mem_write(header,dwords(0x7ed4a0));u.mem_write(header+0x10,dwords(3,10));u.mem_write(0x87f7e8+0x74+4*level,dwords(3))
 base_nodes=m.alloc(side*side*4);nodes=bytearray(b'\x07\x00\x00\x00'*(side*side))
 for (x,y),p in cells.items():nodes[(y*side+x)*4:(y*side+x)*4+2]=bytes((0,u.mem_read(p+0x11b,1)[0]))
 u.mem_write(base_nodes,bytes(nodes));u.mem_write(0x87f7e8+0x68,dwords(base_nodes))
 buckets=m.alloc(256*24);root=m.alloc(12);u.mem_write(root,dwords(buckets,256,0));u.mem_write(0x87f7e8+0x14,dwords(root))
 for index in range(256):
  q=buckets+index*24;m.invoke(0x58aff0,q,(32,0));u.mem_write(q,dwords(0x7ed5a0));u.mem_write(q+16,dwords(0,16))
 record=m.alloc(16);u.mem_write(record,struct.pack('<4h2I',10,14,10,32,1,0));u.mem_write(0x87f7e8+0x54,dwords(record,1,1,1,10));m.invoke(0x5851b0,0x87f7e8,(record,))
 initial['supplied_hierarchy']=dict(side=side,all_three_levels='zone1 for y<=20; zone2 otherwise',native_graph_construction='58AE60',native_edge_publication='5851B0',record=dict(a=[10,14],b=[10,32],active=True,kind=0))
 if PRIME:
  q=m.alloc(4);u.mem_write(q,struct.pack('<2h',10,20));m.invoke(0x576ba0,0x87f7e8,(q,))
  initial['native_priming_call']=dict(entry='0x576ba0',coord=[10,20],state_after=bridge_snapshot(m,cells))

 u.mem_write(0xa8e9a0,b'\x01') # Supplied live-game flag admits original Anim Unlimbo.
 global last_prepared
 last_prepared=(m,cells)
 initial['bridge_before']=bridge_snapshot(m,cells)
 initial['stock_fixture']={'source':str(CASE),'coordinate_translation':[-102,-120],'cells':len(cells),'no_objects':True}
 return m,source,st,w,cells,initial
impact.launch.prepare=prepare
# The preserved launch fixture clears flags before calling native FireAt. This
# companion replaces that fixture write with the already supplied stock flags.
src=inspect.getsource(impact.launch.launch)
line=' for xy,c in cells.items():u.mem_write(c+0x140,dwords(0x100 if bridge and xy==(10,20) else 0))'
assert src.count(line)==1
exec(src.replace(line,' # Stock bridge flags remain as loaded above.').replace('target=cells[16,20]','target=cells[10,20]'),impact.launch.__dict__)
# Preserve stock tile/overlay identities and map extents at impact setup.
src=inspect.getsource(impact.execute)
line=' initialize_effect_world(m,cells)'
assert src.count(line)==1
src=src.replace(line,' initialize_effect_world(m,cells,seed=BRIDGE_SEED,map_size=(136,140),clear_terrain=False)')
impact.BRIDGE_SEED=31
src=src.replace('for _ in range(100):','for _ in range(0):').replace('count=1000000','count=6000000').replace("0x587180:'BridgeDriver'","0x587180:'BridgeDriver',0x576ba0:'HighBridgeBody',0x576770:'Rim',0x47dd70:'Fallout',0x56dae0:'Connectivity',0x6551c0:'RadarDirty',0x6d2790:'ScreenDirty',0x575ee0:'Notify',0x487720:'DirtyCell'").replace('0x65c780,0x65c7e0,0x5f4ec0):pending','0x65c780,0x65c7e0,0x5f4ec0,0x587180,0x576ba0):pending')
exec(src,impact.__dict__)
def retained_snapshot(m):
 u=m.u;record=m.read32(0x87f7e8+0x54)
 graphs=[]
 for level in range(3):
  h=0x87f7e8+0x8c+level*24;nodes=m.read32(h+4);graph=[]
  for i in range(m.read32(h+16)):
   q=nodes+i*36;items=m.read32(q+4);graph.append([list(struct.unpack('<2I',u.mem_read(items+j*8,8))) for j in range(m.read32(q+16))])
  graphs.append(graph)
 count=m.read32(0x87f7e8+0x4c)
 from tools.spatial_oracle.anim_bouncer_launch import anim_state
 anims=[]
 for i in range(m.read32(0xa8e9b8)):
  a=m.read32(m.read32(0xa8e9ac)+4*i);anims.append(dict(native_id=impact.i32(u,a+0x10),type=m.string(m.read32(a+0xc8)+0x24),**anim_state(u,a)))
 return dict(record_active=bool(u.mem_read(record+8,1)[0]),graphs=graphs,raw_zone_count=count,
  raw_rows=[list(struct.unpack('<'+'H'*count,u.mem_read(m.read32(0x87f7e8+0x18+i*4),count*2))) for i in range(13)] if count else [],
  dirty_rectangles=m.read32(0xb0ce88),radar_dirty=[list(struct.unpack('<2h',u.mem_read(m.read32(0x87f7e8+0x1228)+i*4,4))) for i in range(m.read32(0x87f7e8+0x1234))],retained_animations=anims)

def generate():
 global PRIME
 rows=[]
 for name,seed,prime in (('rejected',31,False),('damaged',39,False),('collapsed',39,True)):
  PRIME=prime;impact.BRIDGE_SEED=seed
  r=impact.execute(False,origin=(2176,5248,600));m,cells=last_prepared
  assert 'failure' not in r,(name,r.get('failure'))
  r['name']=name;r['bridge_after']=bridge_snapshot(m,cells);r['retained_after']=retained_snapshot(m)
  r['supplied']['target_cell']=[10,20];r['supplied']['scenario_rng_seed']=seed
  assert r['after_drain']['queue_count']==0 and r['after_drain']['bullet_count']==0
  rows.append(r)
 return dict(scope='Original physical selected FV guided launch/flight into stock scalar bridge topology; rejected/damaged/primed-collapse controls through full AreaDamage, bridge body/perpendicular/rim/fallout, original retained graph disconnection/raw connectivity rebuild, physical effect creation and actual Bullet queue drain. Retained graph/base classifiers, map crop, source lifecycle, launch admission and independent hit seed are supplied. No native map loader, full Rules Process, wholematch native IDs, rendering or post-impact Anim scheduler proof.',rows=rows)

def metadata():
 return provenance(scope='Three original guided hits: rejected, damaged and primed-collapse through bridge effects, retained connectivity and Bullet retirement',assumptions=[
  'All native launch/flight/impact/bridge/lifetime bodies execute. 225 stock scalar cells from bridge_rim_stock_inputs.json are translated[-102,-120]; map size136x140 remains. Direct cell queries execute; native map loading and translated map enumeration do not.',
  'Supplied retained hierarchy: three levels use zone1 throughy20 thenzone2, a single active high bridge connects[10,14] to[10,32]; baseclass0inside crop and7outside. Native container constructors/initial edge publication execute, then original collapse graph disconnection584E50 and raw rebuilding56C510 execute.',
  'Native OverlayType dense prefix/full BRIDGE2 and GEM01 reads, BridgeStrength constructor/read, bridge effect/SplashList/Wake constructor/read blocks, and full physical Anim ART readers execute. Overlay SHPs are missing; no overlay renderer claim.',
  'Independent seeds31/39 bound rejection/admission; collapse is primed by native576BA0, not a prior full weapon hit. Bullet94 and subsequent runtime effect IDs are actual partial fixture constructor order, not wholematch identity.',
  'Original Tactical constructor and Map/Radar/dirty vector initializers execute. Viewport800x600, camera0, known projection multiplier, GameSpeed4 and timeGetTime0 are supplied. Live-game flagA8E9A0=1 admits complete original Anim Unlimbo including native coordinate commit and Display submission. Source lifecycle/Bullet launch admission boundaries are inherited.',
  'The shared Bullet admission boundary now executes original InLimbo clearing and coordinate fixup. Its later Conceal therefore includes DetachAll PointerExpired7258D0 and DisplayRemove4A9770 before drain; all other retained bridge, impact, ID, RNG and state outputs are unchanged.',
  'Stops after full Bullet queue drain with impact/bridge animations retained. No post-impact Anim scheduler/retirement or whole Rules Process chronology claim. Shared ifv_impact separately executes single XGRYSML2 lifetime.',
 ],substitutions=[
  'Shared ifv_impact transport boundaries plus verified WINMM timeGetTime import at6C8C40 supplied0. Only Python fixture setup statements are adapted in memory; no original executable instruction is patched or replaced.',
 ],entry_points={'area_damage':0x489280,'bridge_driver':0x587180,'high_body':0x576ba0,'rim':0x576770,'fallout':0x47dd70,'disconnect':0x584e50,'raw_connectivity':0x56c510,'select_anim':0x48a4f0,'splash_reader':0x66c184,'wake_reader':0x66d847,'radar_dirty':0x6551c0,'screen_dirty':0x6d2790,'drain':0x725c70})

if __name__=='__main__':
 finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
