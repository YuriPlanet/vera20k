"""Read-only research: original retained Bullet render call arguments.

Actual independent native Cannon reader output establishes default type fields;
prepared Bullet lifetime/coordinates, map, camera and dirty rectangles remain
explicit supplied boundaries. Whole scene and native shape pixels are separate.
"""
import hashlib, json, struct
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.bridge_damage_admission import base, call, CELL, MEM, SCENARIO, TABLE, read32, words
from tools.native_slope import slope_matrices

from tools.projectile_oracle.bridge_render_inputs import generate as native_inputs, assets_root
OFFSETS={'shadow':0x29a,'inviso':0x29e,'inverse_rotates':0x2a1,
         'anim_palette':0x2a8,'firers_palette':0x2a9,'flat':0x2f7,'voxel':0x236}

def ints(u,p,n):
    return list(struct.unpack('<'+'i'*n,u.mem_read(p,n*4)))

def setup(case, native_type, raw_shp):
    u=base(case)
    for p,v in [(0x89E7C0,104),(0x89DE64,416),(0xAC13BC,416)]:
        u.mem_write(p,words(v))
    u.mem_write(0xB0CD48,struct.pack('<Q',0x3FC25E5374344960))
    for n,m in enumerate(slope_matrices()):
        u.mem_write(0xB45188+48*n,struct.pack('<12I',*m))
    obj,typ,shp,tactical,clip,point=[MEM+x for x in (0x18000,0x19000,0x19400,0x1A000,0x1C000,0x1D000)]
    u.mem_write(0xA8B230,words(SCENARIO));u.mem_write(SCENARIO,words(0x1000))
    u.mem_write(obj,words(0x7E46E4));u.mem_write(obj+0xAC,words(typ))
    u.mem_write(typ,words(0x7E4948));u.mem_write(typ+0xA4,words(0 if case.get('missing_image') else shp))
    u.mem_write(shp,raw_shp)
    if 'synthetic_frames' in case:
        u.mem_write(shp+6,struct.pack('<h',case['synthetic_frames']))
    coord=case.get('xyz',[2688,5248,case.get('z',1041)])
    if 'mapped_cell' in case:
        cell_x,cell_y=case['mapped_cell']
        assert 0 <= cell_x < 512 and 0 <= cell_y < 512
        u.mem_write(TABLE+(cell_y*512+cell_x)*4,words(CELL))
        u.mem_write(CELL+0x24,struct.pack('<hh',cell_x,cell_y))
    u.mem_write(obj+0x9C,words(*coord))
    u.mem_write(obj+0x8C,bytes((case.get('on_bridge',0),)))
    u.mem_write(obj+0x158,bytes((case.get('hidden',0),)))
    for name,offset in OFFSETS.items():
        u.mem_write(typ+offset,bytes((int(case.get(name,native_type[name])),)))
    u.mem_write(typ+0x2f4,bytes((case.get('anim_low',native_type['anim_low']),case.get('anim_high',native_type['anim_high']),native_type['anim_rate'])))
    u.mem_write(obj+0x12c,bytes((case.get('runtime_frame',0),)))
    u.mem_write(obj+0xE8,struct.pack('<3d',*case.get('velocity',[10.,0.,0.])))
    # Distinct prepared Convert identities establish producer selection only.
    u.mem_write(0x87f6c4,words(MEM+0x1f000));u.mem_write(0x87f6c0,words(MEM+0x1f100))
    # Original Draw fastcall receives surface ECX and Convert EDX.
    u.mem_write(0x887314,words(MEM+0x1f200))
    u.mem_write(0x887324,words(tactical))
    u.mem_write(tactical+0xB0,words(*case.get('camera',[-600,0])))
    viewport=case.get('viewport',[0,0,800,600]);dirty=case.get('dirty',viewport)
    u.mem_write(0xB0CE30,words(viewport[2],viewport[3]))
    u.mem_write(0x886FA0,words(*viewport));u.mem_write(clip,words(*dirty));u.mem_write(0xB73550,words(1))
    return u,obj,typ,shp,tactical,clip,point

def render(u,obj,typ,shp,tactical,clip,point):
    visible=call(u,0x6D2140,tactical,(obj+0x9C,point))&255
    projected=ints(u,point,2);draws=[]
    def sink(_u,a,_size,_data):
        if a!=0x4AED70:return
        sp=u.reg_read(UC_X86_REG_ESP);args=ints(u,sp+4,14)
        draws.append(dict(caller=f'{read32(u,sp):08X}',frame=args[1],point=ints(u,args[2],2),
            flags=args[4],z_adjust=args[6],gradient=args[7],brightness=args[8],
            args=args,convert=u.reg_read(UC_X86_REG_EDX),surface=u.reg_read(UC_X86_REG_ECX)))
        u.reg_write(UC_X86_REG_EAX,0);u.reg_write(UC_X86_REG_EIP,read32(u,sp));u.reg_write(UC_X86_REG_ESP,sp+60)
    hook=u.hook_add(UC_HOOK_CODE,sink)
    result=call(u,0x5F4B10,obj,(clip,1,0))&255
    u.hook_del(hook)
    return dict(xyz=ints(u,obj+0x9c,3),projection=projected,projection_visible=visible,render_result=result,draws=draws)

def execute(case,native_type,raw_shp):
    return dict(input=case,**render(*setup(case,native_type,raw_shp)))

def generate():
    source=native_inputs()
    native_type=next(r['state'] for r in reversed(source['layers']) if 'state' in r)
    raw_shp=(assets_root()/'120mm.shp').read_bytes()
    cases=[dict(name=f'height_l{level}_z{z}_flags{flags}',level=level,flags=flags,z=z)
           for level in (-1,0,6) for flags in(0,256)
           for z in(-105,-104,-1,0,52,104,127,128,255,416,624,1039,1040,1041,1500)]
    cases += [dict(name=f'slope{s}_flags{flags}',level=2,slope=s,flags=flags,z=1000)
              for s in (0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15) for flags in(0,256)]
    cases += [dict(name=f'gate_{key}_{val}',level=6,flags=256,z=1041,**{key:val})
              for key,val in [('on_bridge',1),('shadow',0),('inviso',1),('hidden',1),('missing_image',True),('anim_palette',1)]]
    cases += [dict(name=f'camera_{i}',level=6,flags=256,z=1041,camera=cam,viewport=vp,dirty=dr)
              for i,(cam,vp,dr) in enumerate([
                  ([0,0],[0,0,800,600],[0,0,800,600]),
                  ([-600,100],[0,37,800,600],[21,100,400,250]),
                  ([61,0],[0,0,800,600],[0,0,800,600]),
                  ([60,0],[0,0,800,600],[0,0,800,600]),
                  ([-1460,0],[0,0,800,600],[0,0,800,600]),
                  ([-1461,0],[0,0,800,600],[0,0,800,600]),
                  ([-600,496],[0,0,800,600],[0,0,800,600]),
                  ([-600,495],[0,0,800,600],[0,0,800,600]),
                  ([-600,-465],[0,0,800,600],[0,0,800,600]),
                  ([-600,-466],[0,0,800,600],[0,0,800,600]),
              ])]
    cases += [dict(name=f'dummy_xy_{x}_{y}',level=6,flags=256,xyz=[x,y,1041],camera=[0,0])
              for x,y in [(-1,0),(-256,0),(-257,0),(0,-1),(0,-256),(0,-257),(131072,0)]]
    # Runtime animation and inverseRotates controls use a declared synthetic
    # frame-count header; they are draw-frame evidence, not physical120MM frames.
    cases += [dict(name=f'frame_{i}',level=6,flags=256,z=1041,
                   synthetic_frames=32,**fields) for i,fields in enumerate([
        dict(inverse_rotates=0,velocity=[10.,0.,0.]),
        dict(inverse_rotates=1,velocity=[0.,10.,0.]),
        dict(inverse_rotates=0,anim_low=3,anim_high=6,runtime_frame=5),
        dict(inverse_rotates=1,anim_low=3,anim_high=6,runtime_frame=5),
    ])]
    sequences=[]
    for on_bridge in (0,1):
        case=dict(name=f'live_structural_{on_bridge}',level=6,flags=256,z=1041,on_bridge=on_bridge)
        state=setup(case,native_type,raw_shp);u=state[0];rows=[]
        for phase,flags in [('intact',256),('collapsed',0),('repaired',256)]:
            # Supply already-completed bridge lifecycle state transitions. This
            # proves Draw re-reads live cell state, not collapse/repair itself.
            u.mem_write(CELL+0x140,words(flags))
            rows.append(dict(phase=phase,flags=flags,**render(*state)))
        sequences.append(dict(input=case,rows=rows))
    return dict(type_source_sha256=hashlib.sha256(json.dumps(source,sort_keys=True,separators=(',',':')).encode()).hexdigest(),
                selected_native_type=native_type,shp_sha256=hashlib.sha256(raw_shp).hexdigest(),
                rows=[execute(case,native_type,raw_shp) for case in cases],live_structural_sequences=sequences)


def metadata():
    return provenance(
        scope='149 original retained Bullet render captures plus six same-object live structural-state captures. Original Object Render5F4B10, Bullet Draw468090, map-ground, projection and frame calls execute; draw endpoints record arguments. No complete Display admission, shape pixels, GPU or full-game parity.',
        assumptions=[
            'Cannon type fields and physical120MM image independently rerun original BulletType construction/full layered readers through bridge_render_inputs. Prepared Bullet/Map/Tactical storage, retained signedXYZ, OnBridge and hidden byte are supplied boundaries; no Bullet constructor or flight integration in this corpus.',
            'Map flat/sloped height uses original578080 and slope matrices from original initializer. Startup LevelHeight104/deck416 and AdjustForZ multiplier0x3FC25E5374344960 are supplied. Ambient x87 control0x0E7F. VERA common world-row bias15 is absent from native pixels.',
            'Prepared distinct palette.pal/anim.pal Convert identities test selection only. Original46836D and46841B supply Convert in EDX; ECX is destination surface. Real Convert construction and final colors are independent evidence.',
            'Camera and dirty pairs cover inclusive padded projection edges, shifted viewport/rebasing, signed/off-mapXY, levels-1/0/6, signedZ and sixteen slopes. Four frame controls intentionally replace only SHP frame count with32; physical120MM remains one frame.',
            'Same retained Bullet is rendered with supplied structural flag intact/collapsed/repaired; lifecycle calls are excluded. This isolates live structural-query ownership and independent OnBridge height terms.',
        ],
        substitutions=['CC_Draw_Shape4AED70 records all14 stack arguments, pointed draw position, EDX Convert and ECX surface, then returns0 with56-byte callee cleanup. All other reached native calls execute unchanged.'],
        entry_points={'object_render':0x5F4B10,'bullet_draw':0x468090,
                      'projection':0x6D2140,'ground':0x578080,'height':0x5F5F40,
                      'raw_z':0x5F5F30,'frame':0x468000,'adjust_for_z':0x6D20E0,
                      'shape_draw':0x4AED70})


if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
