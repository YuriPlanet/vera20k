"""Original FootApproach candidates with supplied raw occupation and geometry inputs.

Reuse the frozen full pursuit VM, readers, native hierarchy and MissionDispatch.
No rejection, candidate, range, cost, destination or RNG result is substituted.
"""
from pathlib import Path
import hashlib,json,struct,sys,tempfile
HERE=Path(__file__).resolve().parent
from . import pursuit as base
from .publication import finish_vectors
from tools.native_oracle import run_checked
from tools.spatial_oracle.building_body_rules import SP,dwords
from unicorn.x86_const import UC_X86_REG_ESP,UC_X86_REG_EAX,UC_X86_REG_EBX,UC_X86_REG_EDI,UC_X86_REG_FPCW
# An unexpected fixture failure is diagnostic only, and belongs here.
base.HERE=Path(tempfile.gettempdir())/'vera20k-fv-cell-attack';base.HERE.mkdir(exist_ok=True)

class CandidateControls(base.Continuation):
 def reset(self):
  super().reset()
  for x,y,bits in getattr(self,'active_row',{}).get('ground_occupation',[]):
   assert self.m.read32(self.cells[x,y]+0x140)&0x100==0
   self.u.mem_write(self.cells[x,y]+0x124,bytes((bits,)))
 def observe_flow(self,pc,sp):
  super().observe_flow(pc,sp)
  if pc==0x4834A0:
   event=self.flow[-1];cell=int(event['this'],16)
   event.update(ground_occupation=self.u.mem_read(cell+0x124,1)[0],deck_occupation=self.u.mem_read(cell+0x128,1)[0])
 def control(self,row):
  self.active_row=row
  result=self.execute_case(row)
  return result

def geometry(q, poses=None, radius_indices=None):
 """Two original interior numeric blocks; no search/admission claims."""
 q.active_row={};q.reset();q.phase='geometry'
 u=q.u;m=q.m;sp=SP-0x400;source=m.alloc(12)
 context=u.context_save()
 outputs=[]
 # Full quadrants, axes and fractional direction-boundary inputs. Source/target
 # coordinates are supplied to this numeric boundary, not authored Cell poses.
 poses=poses or [([22400,13952],[22400,12414]),([22400,13952],[22400,15490]),
        ([22400,13952],[20862,13952]),([22400,13952],[23938,13952]),
        ([22400,13952],[20800,12352]),([22400,13952],[24000,12352]),
        ([22400,13952],[20800,15552]),([22400,13952],[24000,15552]),
        ([22400,13952],[22017,12352]),([22400,13952],[22783,12352]),
        ([22400,13952],[22401,12352]),([22400,13952],[22399,12352]),
        ([128,128],[-1500,-400]),([-128,-128],[1500,400]),
        ([22400,13952],[22400,13952])]
 for target,src in poses:
  u.context_restore(context);u.mem_write(source,dwords(*src,0));u.mem_write(sp+0x4C,dwords(*target,416))
  u.reg_write(UC_X86_REG_ESP,sp);u.reg_write(UC_X86_REG_EAX,source);u.reg_write(UC_X86_REG_EBX,q.src)
  run_checked(u,0x4D5C53,0x4D5CBC,count=100000,required_addresses=(0x4CAE30,0x7C5F00))
  word=struct.unpack('<H',u.mem_read(sp+0x1C,2))[0];direction=m.read32(sp+0xD0)
  rows=[]
  for radius,index in (radius_indices if radius_indices is not None else
                      [(1228,i)for i in range(25)]+[(r,0)for r in (972,716,460,204,256,205)]):
   u.context_restore(context);u.mem_write(sp+0x24,dwords(0x8224DC+index*4));u.mem_write(sp+0x4C,dwords(*target,416));u.mem_write(sp+0xD0,dwords(direction));u.mem_write(sp+0x88,struct.pack('<d',radius))
   u.reg_write(UC_X86_REG_ESP,sp);u.reg_write(UC_X86_REG_EBX,q.src)
   run_checked(u,0x4D5FFE,0x4D6081,count=100000,required_addresses=(0x4CACB0,0x4CAD00,0x7C5F00))
   value=lambda x:struct.unpack('<i',dwords(x))[0]
   rows.append(dict(radius=radius,direction_index=index,offset=struct.unpack('<i',u.mem_read(0x8224DC+4*index,4))[0],unsnapped_xyz=[value(u.reg_read(UC_X86_REG_EAX)),struct.unpack('<i',u.mem_read(sp+0x38,4))[0],value(u.reg_read(UC_X86_REG_EDI))]))
  outputs.append(dict(target_xy=target,source_xy=src,facing_word=word,base_direction=direction,candidates=rows))
 return dict(fpcw=u.reg_read(UC_X86_REG_FPCW),poses=outputs,neighbor_offsets=[list(struct.unpack('<hh',u.mem_read(0x89F688+4*i,4)))for i in range(8)],scope='Original4D5C53..5CBC and4D5FFE..6081 only. Coordinates/radii are supplied; no admission, snap or actual map pose claimed for these numeric controls.')

def generate():
 q=CandidateControls(dict(name='prepare_healthy_candidates',stage='healthy'));q.plane();q.construct(True);q.pathfinder();q.graphs();q.checkpoint()
 rows=[dict(name='first_radial_clear'),
       dict(name='first_radial_ground_bit40',ground_occupation=[[87,49,0x40]]),
       dict(name='first_radial_and_sixth_neighbor_ground_bit40',ground_occupation=[[87,49,0x40],[89,50,0x40]]),
       dict(name='first_radial_and_later_neighbors_ground_bit40',ground_occupation=[[87,49,0x40],[89,50,0x40],[88,50,0x40]]),
       dict(name='first_radial_ground_bit01',ground_occupation=[[87,49,1]]),
       dict(name='first_radial_deeper_occupation',ground_occupation=[[x,y,0x40]for y in (49,50)for x in range(85,91)])]
 for row in rows:row.update(stage='healthy',source_y_delta=-2)
 try:
  results=[q.control(row)for row in rows]
  numeric=geometry(q)
  result=dict(schema=1,native_sha256=base.NATIVE_SHA256,original_constants=base.original_constants(),cases=results,numeric=numeric)
  assert base.proof.sha(bytes(q.u.mem_read(base.proof.TEXT_START,base.proof.TEXT_SIZE)))==q.prefix['text_sha256']
  return result
 finally:q.close()

def metadata():
 result=base.metadata();result['scope']=__doc__;result['pursuit_harness_sha256']=result['harness_sha256'];result['harness_sha256']=base.proof.sha(Path(__file__).read_bytes());result['assumptions'] += [
  'Additional complete MissionDispatch controls begin with supplied retained Cell+124 raw ground-occupation bytes. Native4834A0 reads them and determines rejection. No owner/actor/mark producer of these inputs is claimed; underlying frozen Terrain/class/zone data is unchanged. These input controls are not manufactured callback returns.',
  'Numeric-only controls enter original4D5C53 and4D5FFE with declared stack/source inputs in the same native-initialized VM, ambientFPCW0E3F. Native facing conversion and Sin/Cos/ftol execute; no numeric expected result is hand-computed. Negative/zero-delta poses and shorter radii are boundary inputs, not stock map states.'
 ];return result

def cleg_candidate_heights():
 """Original snapped candidate height; observe the changing ESP aliases.

 The existing bridge-query fixture owns the sparse map/Cell inputs. Native
 Map578080, Cell47B3A0 and the later independent Map565730 all execute.
 """
 from unicorn import UC_HOOK_CODE,UC_HOOK_MEM_WRITE
 from unicorn.x86_const import UC_X86_REG_EIP
 from types import SimpleNamespace
 from tools.spatial_oracle.foot_navigation_coordinate import OriginalBridgeQuery,bridge_case
 from tools.spatial_oracle.mapgen_range import Machine
 rows=[]
 for level,slope in ((0,0),(2,0),(4,0),(2,1)):
  for flags in (0,0x100,0x400,0x500):
   case=bridge_case('walk','foot_candidate_height',nav_com_coord=None,
                   cells=[dict(xy=[7,5],level=level,slope=slope,flags=flags)])
   fixture=OriginalBridgeQuery(case);u=fixture.uc;fixture.stage='approach_candidate'
   Machine.startup(SimpleNamespace(u=u))
   incoming=[1833,1485,987]
   u.mem_write(SP+0x2C,dwords(*incoming));u.mem_write(SP+0x23,b'\0')
   # The deck scalar is a declared initialized input, not a queried answer.
   u.mem_write(0x8B3DF4,dwords(416));u.reg_write(UC_X86_REG_ESP,SP)
   stack=[];writes=[]
   def observe_code(_u,pc,_size,_data):
    if pc in (0x4D6172,0x4D6177,0x4D6180,0x4D6181,0x4D6185,0x4D618A,0x4D61A0):
     stack.append(dict(pc=f'{pc:08x}',esp_delta=u.reg_read(UC_X86_REG_ESP)-SP,
                       candidate_z=struct.unpack('<i',u.mem_read(SP+0x34,4))[0]))
   def observe_write(_u,_access,address,size,value,_data):
    if address==SP+0x34:
     writes.append(dict(pc=f'{u.reg_read(UC_X86_REG_EIP):08x}',
                        effective_offset=address-SP,size=size,value=value))
   u.hook_add(UC_HOOK_CODE,observe_code);u.hook_add(UC_HOOK_MEM_WRITE,observe_write)
   run_checked(u,0x4D60FB,0x4D61A4,count=10000,
               required_addresses=(0x4D6134,0x578080,0x47B3A0,0x565730,0x4D6181))
   assert u.reg_read(UC_X86_REG_ESP)==SP and not fixture.pending
   candidate_xyz=list(struct.unpack('<iii',u.mem_read(SP+0x2C,12)))
   rows.append(dict(level=level,slope=slope,flags=flags,
                    ground_z=fixture.queries[0]['ground'],candidate_z=candidate_xyz[2],
                    point_xy=candidate_xyz[:2],candidate_xyz=candidate_xyz,
                    input=dict(incoming_xyz=incoming,cell_xy=[7,5],level_step=104,deck_offset=416),
                    queries=fixture.queries,stack=stack,writes=writes))
 return rows

def cleg_approach_vectors():
 """Original class-specific radius and radial geometry at supplied boundaries.

 Reuse this module's numeric geometry owner; no complete Approach, native
 target getter, map admission, placement or teleport lifetime is claimed.
 """
 from types import SimpleNamespace
 from tools.spatial_oracle.mapgen_range import Machine
 from tools.spatial_oracle.locomotor_head_coordinates import INITIALIZER_CONTROLS
 from unicorn import Uc,UC_ARCH_X86,UC_MODE_32
 from unicorn.x86_const import UC_X86_REG_EDI
 from tools.native_oracle import (load_image,image_bytes,file_span,image_sha256,
                                STACK_BASE,STACK_SIZE,SCRATCH,SCRATCH_SIZE,RET_MAGIC,call)
 u=Uc(UC_ARCH_X86,UC_MODE_32);load_image(u)
 u.mem_map(STACK_BASE,STACK_SIZE);u.mem_map(SCRATCH,SCRATCH_SIZE);u.mem_map(RET_MAGIC,0x1000)
 startup=Machine.startup(SimpleNamespace(u=u))
 # The existing numeric packet also observes the runtime Cell compass table.
 cell_init=INITIALIZER_CONTROLS[0]
 u.mem_write(SP,dwords(RET_MAGIC));u.reg_write(UC_X86_REG_ESP,SP)
 run_checked(u,cell_init[1],RET_MAGIC,count=100,
             required_addresses=(cell_init[1],cell_init[2]))
 sp=SP
 read=lambda address:struct.unpack('<i',u.mem_read(address,4))[0]
 write=lambda address,*values:u.mem_write(address,dwords(*values))
 # This is the original Infantry object's physical vtable, not an annotation.
 rtti_address=struct.unpack('<I',file_span(image_bytes(),0x7EB058+0x2C,4)[1])[0]
 rtti=call(rtti_address,ecx=SCRATCH)['eax']
 assert rtti==15
 rows=[]
 ranges=(-2147483648,-1,0,128,179,180,281,282,383,384,511,512,
         640,768,1024,1280,1536,2147483000,2147483647)
 foundations=((0,0),(1,1),(2,2),(3,2),(0x02000000,1))
 for weapon_range in ranges:
  for foundation in foundations:
   for actor_rtti in (1,rtti):
    for fpcw in (0x0E3F,0x0E7F):
     u.reg_write(UC_X86_REG_ESP,sp);u.reg_write(UC_X86_REG_FPCW,fpcw)
     # EAX is the return from GetWeaponRange, before its original EDI copy.
     u.reg_write(UC_X86_REG_EAX,weapon_range&0xFFFFFFFF)
     run_checked(u,0x4D56E4,0x4D56FC,count=1000)
     adjusted=read(sp+0x38)
     if foundation!=(0,0):
      write(sp+0x1C,foundation[0]);u.reg_write(UC_X86_REG_EAX,foundation[1])
      run_checked(u,0x4D5B97,0x4D5BAA,count=1000)
     expanded=read(sp+0x38)
     u.reg_write(UC_X86_REG_EAX,actor_rtti);u.reg_write(UC_X86_REG_EDI,0)
     run_checked(u,0x4D5BB1,0x4D5BEE,count=10000,required_addresses=(0x7C5F00,))
     rows.append(dict(weapon_range=weapon_range,foundation=foundation,
                      rtti=actor_rtti,fpcw=fpcw,adjusted=adjusted,
                      expanded=expanded,radius=read(sp+0x38)))
 # The same geometry owner used by the historical complete FV controls.
 q=SimpleNamespace(u=u,src=SCRATCH+0x100,phase=None,reset=lambda:None,
                   m=SimpleNamespace(alloc=lambda _size:SCRATCH,
                                     read32=lambda address:struct.unpack('<I',u.mem_read(address,4))[0]))
 u.reg_write(UC_X86_REG_FPCW,0x0E3F)
 poses=[([6528,2688],[2688,2688]),([6528,6784],[2688,2688]),
        ([2688,6528],[2688,2688]),([2688,2688],[6528,2688])]
 numeric=geometry(q,poses=poses,
                  radius_indices=[(radius,index)for radius in (358,486,614,870,1126)
                                  for index in (0,1,2,23,24)])
 return dict(schema=1,native_sha256=image_sha256(),
             infantry_identity=dict(vtable='007eb058',rtti_entry=f'{rtti_address:08x}',rtti=rtti),
             startup=startup,cases=rows,numeric=numeric,
             candidate_heights=cleg_candidate_heights())

def cleg_metadata():
 from tools.native_oracle import provenance
 result=provenance(
  scope='Original FootApproach class-specific radius and radial numeric fragments, supplied boundary inputs only.',
  assumptions=[
   'Native image mapping is the shared native_oracle owner. Infantry vtable7EB058+2C selects actual original523340, which executes and returns15. Unit RTTI1 is a supplied comparison input.',
   'Ranges and foundation dimensions are supplied stack/register inputs. The0,0 row omits the building-only addition; huge dimensions and extreme ranges are arithmetic boundary controls, not valid retail foundations. Original signed wrapping, binary64 subtraction, CRT ftol and radius clamping execute unchanged.',
   'The existing mapgen_range.Machine.startup owner executes original CRT/WinMain floating-control fragments and cached-control-word writes. Windows/CPU startup probes are excluded. Radius controls additionally declare FPCW0E3F and0E7F inputs; radial geometry retains the established0E3F/cache state. Original RTTI leaf uses the shared call default environment and returns a constant.',
   'Four supplied XY poses, targetZ416/sourceZ0 and five supplied radii execute the existing geometry owner through original facing/Sin/Cos/ftol. These100 rays do not execute target getters, Cell snapping, map heights, Cell admission, zone costs, destination setters or whole Approach.',
   'Original Cell compass initializer49F2F0 executes through its realRET49F39B before the shared geometry owner reads its eight packed direction pairs. No compass output is supplied.',
   'Sixteen candidate-height controls reuse OriginalBridgeQuery sparse Cell setup, supplied level/slope/flags, level step104 and deck416. Original CRT/WinMain floating setup executes. The non-CloseRange snap/height block4D60FB..61A4 executes Map578080/Cell47B3A0 then independent Map565730 without substituted queries. Returned snapped XY, ground and final Z are observed, not calculated. At6181 ESP is four bytes below its entry value: [esp+38] aliases the candidate at entryESP+34; the saved ground therefore contributes to final Z. No target/range/admission or whole approach is executed.',
  ],substitutions=[],entry_points=dict(infantry_rtti=0x523340,range_adjustment=0x4D56E4,
   building_radius_addition=0x4D5B97,class_radius=0x4D5BB1,
   approach_facing=0x4D5C53,approach_radial=0x4D5FFE,
   candidate_height=0x4D60FB,map_ground=0x578080,cell_ground=0x47B3A0,map_flags=0x565730))
 return result

if __name__=='__main__':
 if '--cleg-radius' in sys.argv:
  from tools.native_oracle import finish_vectors as native_finish_vectors
  args=[argument for argument in sys.argv[1:]if argument!='--cleg-radius']
  native_finish_vectors(cleg_approach_vectors,HERE/'cleg_approach_vectors.json',
                        provenance=cleg_metadata,argv=args,
                        source_paths={
                         'tools/spatial_oracle/fv_cell_attack/candidates.py':Path(__file__),
                         'tools/native_oracle.py':HERE.parents[1]/'native_oracle.py',
                         'tools/oracle_sources.py':HERE.parents[1]/'oracle_sources.py',
                         'tools/spatial_oracle/building_body_rules.py':HERE.parent/'building_body_rules.py',
                         'tools/spatial_oracle/mapgen_range.py':HERE.parent/'mapgen_range.py',
                         'tools/spatial_oracle/locomotor_head_coordinates.py':HERE.parent/'locomotor_head_coordinates.py',
                         'tools/spatial_oracle/foot_navigation_coordinate.py':HERE.parent/'foot_navigation_coordinate.py',
                         'tools/spatial_oracle/locomotor_at_coord.py':HERE.parent/'locomotor_at_coord.py',
                         'tools/spatial_oracle/map_queries.py':HERE.parent/'map_queries.py',
                        })
 else:
  finish_vectors(generate,HERE/'candidates.json.gz',provenance=metadata)
