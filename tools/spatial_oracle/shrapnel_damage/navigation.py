"""Original full Shrapnel navigation with native-derived physical Cell inputs.

The shared Anytown Navigation owner executes original constructors, Recalc,
Terrain placement, connectivity and hierarchy. This wrapper changes only the
physical input selection and retained low-wood controller sequence.
"""
from pathlib import Path
import copy,gzip,json,struct,sys
from collections import Counter
from unicorn.x86_const import UC_X86_REG_ESP,UC_X86_REG_FPCW
from tools.native_oracle import provenance,_canonical,first_difference
from tools.spatial_oracle.anytown_damage.navigation_inputs import Inputs,extract_tiles,map_inputs,ri,sha
from tools.spatial_oracle.anytown_damage.navigation import Navigation as SharedNavigation
from tools.spatial_oracle.anytown_damage.next_family_native import seed_state
from tools.spatial_oracle.shrapnel_repair import shrapnel_repair as sr,packet_io
from tools.spatial_oracle.shrapnel_repair.zone_composition import BASE,PLANE
from tools.spatial_oracle.bridge_rim import GLOBALS

HERE=Path(__file__).resolve().parent
LOW={0x57BAA0:'damage_selector',0x57BCF0:'ns_root',0x57C2B0:'ew_root',
     0x57DD50:'ns_leaf',0x57E2A0:'ew_leaf',0x57B870:'ns_classifier',
     0x57B990:'ew_classifier',0x57C990:'ns_endpoints',0x57C870:'ew_endpoints',0x575EE0:'notification'}
STAGES=[('first_repair',0x570050,[117,56]),('damaged',0x57BAA0,[115,59]),
        ('collapsed',0x57BAA0,[115,59]),('repaired_again',0x570050,[117,56]),
        ('repeat_repair',0x570050,[117,56])]


def input_case(t):
 raw,sections,cells=map_inputs(ri.ASSETS/'XShrapnel.MAP')
 fields={k:v for k,v,line in sections['Map']}
 assert fields['Theater']=='SNOW' and not sections.get('Tubes') and not sections.get('CellTags')
 seed=seed_state()
 return dict(name='shrapnel_wooden_lifecycle',start=[117,56],impact=[115,59],cells=[],supplied_cells=[],
  size=[int(v)for v in fields['Size'].split(',')[2:]],local_size=[int(v)for v in fields['LocalSize'].split(',')],
  bridge_base=t['globals'][0xAA0E28],wood_base=t['globals'][0xABAD1C],
  rim_keys={k:t['globals'][a]for k,a in GLOBALS.items()},
  supplied_rng={k:copy.deepcopy(seed)for k in ('main','scenario','mapgen')},
  map_sha256=sha(raw),rng_boundary='Original seed0 complete states supplied separately to all three RNG receivers. No whole-Scenario startup order claimed.')


class Navigation(SharedNavigation):
 def observe(self,u,a,n,d):
  if self.phase=='measure' and a in LOW:
   self.counters[f'{self.activity}:{a:08X}']+=1
   if a==0x57BAA0:
    sp=u.reg_read(UC_X86_REG_ESP);p=sr.u32(u,sp+4)
    self.event('damage_entry',coord=list(struct.unpack('<hh',u.mem_read(p,4))))
  if self.phase=='measure' and a==0x47DD70:
   raise AssertionError('ordinary wooden transition reached structural fallout')
  super().observe(u,a,n,d)
 def state(self):
  state=super().state();u=self.uc
  state.update(base_plane_hex=bytes(u.mem_read(BASE,self.side*self.side*4)).hex(),
   hierarchy_plane_hex=bytes(u.mem_read(PLANE,self.side*self.side*10)).hex(),
   strip=[self.snapshot(self.ptrs[x,y])for y in range(54,65)for x in range(114,117)],
   rng_raw={k:bytes(u.mem_read(p,1012)).hex()for k,p in self.rngs.items()})
  return state


def prepare_inputs(*,create_actor=True):
 print('reading physical Shrapnel/SNOW inputs',flush=True)
 t=ri.theater();r=Inputs(t,map_file=ri.ASSETS/'XShrapnel.MAP',theater_file='SNOWMD.INI',damage_overlays=range(74,102))
 scenario_theater=r.read_map_theater();assert scenario_theater['value']==1
 print('native reader inputs established',scenario_theater,flush=True)
 tiles,assets=extract_tiles(t,theater_archive='isosnow.mix',theater_override='isosnomd.mix',tile_suffix='sno',disjoint_theater_additions=True)
 print('physical SNOW primary TMP inputs established',flush=True)
 inputs=dict(case=input_case(t),create_actor=create_actor,actor_coord=(115,58),actor_height=2,
  scenario_theater=scenario_theater,admission_cells=[(114,58),(115,58),(116,58),(114,59),(115,59),(116,59),(114,60),(115,60),(116,60)],stages=STAGES)
 return r,t,tiles,assets,inputs


def setup(*,create_actor=True):
 r,t,tiles,assets,inputs=prepare_inputs(create_actor=create_actor)
 m=Navigation(r,t,tiles,**inputs)
 m.scenario_theater=inputs['scenario_theater']
 return r,t,assets,m


def repaired_donor(*,create_actor=False):
 """Shared full native VM after actual first repair, for joined consumers."""
 r,t,assets,m=setup(create_actor=create_actor)
 m.stages=STAGES[:1]
 first_repair=m.run()[0]
 m.stages=STAGES[1:]
 return r,t,assets,m,first_repair


def generate():
 r,t,assets,m=setup()
 result=dict(schema=1,scenario_theater=m.scenario_theater,readers=r.snapshot(),theater=t,assets=assets,text_sha256=m.code_hash,
  case=m.case,width=m.width,stride=m.side,sweeps=m.sweeps,terrain_placement=m.terrain_placement,
  final_tiberium_value=m.final_tiberium_value,fpcw=m.uc.reg_read(UC_X86_REG_FPCW),
  bootstrap=m.bootstrap,cell_startup=m.cell_startup,actor_constructor=m.actor_constructor,initial=m.initial,
  stages=m.run(),native_reached=dict(m.counters),reached_primary_tmp_heads=sorted(m.used_tile_heads))
 assert not m.range_pending
 assert sha(bytes(m.uc.mem_read(0x401000,0x3E0000)))==m.code_hash
 return result


def metadata():
 return provenance(scope=__doc__,assumptions=[
  'Original Full_Init687631..68764F reads Map/Theater through475870/528A10 and48DBE0, storing Scenario+1258. The derived1 is transplanted into this VM before Terrain71C110 and Cell483DDF consume it.',
  'Physical XShrapnel.MAP diamond fields, original scalar/type readers and physical SNOWMD/primary TMP bytes; original Cell constructors and Recalc derive all class/slope/height inputs. No VERA planes, graph IDs or crop state supplied.',
  'Original Terrain constructors and direct Place_Down follow physical source order; complete ordinary building class gates must independently read false. Initial/final full Cell sweeps and all13 movement rows/all3 graphs are retained.',
  'Actual568BB0 final initialization,56C510 and581F90(2,1,0), then original570050 repair,57BAA0 damage/collapse,570050 repair and repeat execute. Shared builder owns all navigation initialization/publication.',
  'Main, Scenario and MapGen begin with original seeded0 complete states, supplied separately. Original Terrain/Unit constructor draws are observed; no whole native scenario-load RNG or command timing claim.',
  'MTNK constructor prefix and active73F0A0 execute on a supplied alive receiver at115,58,height2; queries use direction4/null previousCell/arg5=1. No spawn, route, occupancy or command admission claim.',
 ],substitutions=[
  'Successful bounded malloc/free, CRT atexit/TLS, host MIX decoding and lexical INI cache preparation inherited from shared owners.',
  'Waterfall Anim421EA0 returns allocated receiver; animation registration/playback/lifetime and its hidden RNG excluded. Shroud returns hidden; full Terrain Unlimbo/Logic registration excluded.',
  'Screen/radar/dirty rectangles are shared sinks. Ordinary mobiles/buildings omitted from class-neutral memberships; bridge occupants are empty and no CellTags exist.',
  'No outer area-damage/strength RNG, projectile/firing, Engineer prefix/suffix, save/load or native raster output claimed.',
 ],entry_points={'cell_ctor':0x47BBF0,'terrain_ctor':0x71BB90,'terrain_place':0x5683C0,'recalc':0x47D2B0,'zone':0x483C80,'final_init':0x568BB0,'connectivity':0x56C510,'hierarchy':0x581F90,'batch':0x586990,'damage':0x57BAA0,'repair':0x570050,'unit_can_enter':0x73F0A0})


if __name__=='__main__':
 result=generate();output=HERE/'navigation.json.gz';promotion=HERE/'navigation_promotion.json'
 if '--write' in sys.argv and not promotion.exists():
  projected=packet_io.publication_projection(result)
  payload_hash=sha(_canonical(json.loads(_canonical(projected))))
  promotion.write_text(json.dumps(dict(results={'navigation.json':dict(published_payload_sha256=payload_hash)}),indent=2)+'\n')
 packet_io.finish_vectors(result,output,provenance=metadata,promotion_path=promotion)
