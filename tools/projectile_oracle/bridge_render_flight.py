"""Bounded native GetSpeed -> FireAt launch -> ordinary motion -> Bullet Render.

Source/target coordinates and admitted collision commits are supplied boundaries.
The selected native weapon/type/Rules readers produce all trajectory scalars.
No module with a legacy import-time corpus writer is imported.
"""
from pathlib import Path
import hashlib
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI,
    UC_X86_REG_ESP, UC_X86_REG_FPCW,
)
from tools.native_oracle import NATIVE_FPCW, run_checked, finish_vectors, provenance
from tools.spatial_oracle.building_body_rules import SP, dwords
from tools.projectile_oracle.bridge_render_inputs import BulletReader, assets_root, lexical
from tools.projectile_oracle.bridge_render_inputs_selection import initialize_weapon
from tools.projectile_oracle.bridge_render import execute as render
from tools.projectile_oracle.ordinary_motion import run as motion


def launch(origin, delta):
    art,_ = lexical((assets_root()/'ARTMD.INI').read_bytes(),{'Cannon','120MM'})
    m=BulletReader(art)
    weapon,ptype,rules=initialize_weapon(m)
    u=m.u
    source,stype,bullet,target,vt,ref,hook=[m.alloc(n) for n in (0x1000,0x1000,0x180,0x1000,0x800,16,64)]
    # Original Bullet constructor requires its preexisting global registry.
    u.mem_write(0xA8ED40,dwords(0x7EB6D4,m.alloc(4096),1024,1,0,10))
    m.invoke(0x466380,bullet)
    u.mem_write(0x8871E0,dwords(rules))
    target_coord=[origin[i]+delta[i] for i in range(3)]
    for p,values in ((source,[vt]),(source+0x2b4,[target]),(vt+0x84,[hook]),
                     (vt+0x3f8,[hook+16]),(vt+0x48,[0x5F65A0]),
                     (vt+0x58,[0x5F65A0]),(vt+0x2c,[hook+32]),
                     (target,[vt]),(ref,[weapon]),(bullet+0xac,[ptype]),
                     (bullet+0x10c,[target]),(source+0x9c,origin),
                     (target+0x9c,target_coord)):
        u.mem_write(p,dwords(*values))

    # Actual FireAt horizontal-distance producer followed by GetSpeed773070.
    for p,values in ((SP+0x40,[weapon]),(SP+0x44,origin),(SP+0x88,target_coord)):
        u.mem_write(p,dwords(*values))
    u.reg_write(UC_X86_REG_ESP,SP)
    run_checked(u,0x6FE4F2,0x6FE53F,required_addresses=(0x4CAC40,0x773070,0x48AB90))
    speed=u.reg_read(UC_X86_REG_EAX)
    assert u.reg_read(UC_X86_REG_ESP)==SP
    for p,values in ((SP+0x28,[speed]),(SP+0x3c,[bullet]),(SP+0x40,[weapon]),
                     (SP+0x44,origin),(SP+0x68,[ptype]),(SP+0x94,delta),
                     (SP+0x1000+12,[0])):
        u.mem_write(p,dwords(*values))
    for reg,value in ((UC_X86_REG_ESP,SP),(UC_X86_REG_EBP,SP+0x1000),
                      (UC_X86_REG_EBX,weapon),(UC_X86_REG_ECX,ptype),
                      (UC_X86_REG_ESI,source),(UC_X86_REG_EDI,delta[2]&0xffffffff),
                      (UC_X86_REG_EAX,delta[1]&0xffffffff),(UC_X86_REG_FPCW,NATIVE_FPCW)):
        u.reg_write(reg,value)
    events=[]

    def observe(_u,address,_size,_data):
        if address in (hook,hook+16,hook+32):
            m.ret({hook:stype,hook+16:ref,hook+32:1}[address],4 if address==hook+16 else 0)
        elif address in (0x5F4EC0,0x4A9770,0x4A9720):
            sp=u.reg_read(UC_X86_REG_ESP)
            if address==0x5F4EC0:
                u.mem_write(bullet+0x9c,bytes(u.mem_read(m.read32(sp+4),12)))
                u.mem_write(bullet+0x90,b'\x01')
            events.append(f'{address:08X}')
            m.ret(1,8 if address==0x5F4EC0 else 4)

    h=u.hook_add(UC_HOOK_CODE,observe)
    run_checked(u,0x6FE8EE,(0x6FF01A,0x6FF93C),count=200000,
                required_addresses=(0x70D590,0x48A8D0,0x48A9D0,0x468670))
    u.hook_del(h)
    assert u.reg_read(UC_X86_REG_EIP)==0x6FF01A
    raw=bytes(u.mem_read(bullet+0xe8,24))
    assert raw==bytes(u.mem_read(SP+0x50,24))
    return dict(origin=origin,target=target_coord,delta=delta,
                stored_weapon_speed=m.read32(weapon+0xa8),launch_speed=speed,
                gravity=m.read32(rules+0x16b8),floater=bool(u.mem_read(ptype+0x295,1)[0]),
                native_type=m.bullet_state(ptype),
                velocity=list(struct.unpack('<3d',raw)),velocity_bits=[f'{v:016x}' for v in struct.unpack('<3Q',raw)],
                retained_xyz=list(struct.unpack('<3i',u.mem_read(bullet+0x9c,12))),
                pitch=m.read32(SP+0x80)&65535,success=u.mem_read(SP+0x27,1)[0],
                supplied_world_calls=events)


def generate():
    shot=launch([2688,5248,1120],[500,0,0])
    # Ordinary admitted flight carries exact native velocity/candidate bytes.
    flight=motion(shot['velocity'],[shot['gravity']]*8,shot['floater'],shot['retained_xyz'])
    raw=(assets_root()/'120mm.shp').read_bytes()
    rows=[]
    for tick,frame in enumerate(flight['frames'],1):
        xyz=frame['candidate']
        velocity=[struct.unpack('<d',struct.pack('<Q',int(v,16)))[0] for v in frame['bits']]
        flags=256 if tick<=3 or tick>=7 else 0
        case=dict(name=f'flight_tick_{tick}',xyz=xyz,velocity=velocity,level=6,flags=flags,
                  mapped_cell=[xyz[0]//256,xyz[1]//256],camera=[-600,100])
        rows.append(dict(tick=tick,velocity_bits=frame['bits'],**render(case,shot['native_type'],raw)))
    return dict(launch=shot,motion=flight,rows=rows,
                shp_sha256=hashlib.sha256(raw).hexdigest())


def metadata():
    return provenance(
        scope='One selected native105mm/Cannon launch followed by eight original ordinary-motion blocks and original Bullet Render/Draw argument captures. Reader-owned speed/gravity/type inputs, launch and motion numeric outputs are recomputed. This is a bounded composed transcript, not a complete FireAt/AI/collision/Display/scene execution.',
        assumptions=[
            'Independent original Weapon772080 and Cannon46BEE0 readers plus RulesGravity66B3C4..66B3E4 initialize physical selected inputs. Original FireAt6FE4F2..6FE53F computes horizontal range and GetSpeed773070; storedSpeed102 is not substituted for the produced launch speed.',
            'Upstream source/target/FLH admission is supplied: origin2688,5248,1120, target500leptons east at sameZ. Original Bullet constructor466380 and launch6FE8EE..6FF01A execute; no complete Techno FireAt prefix, aim resolver or launch scatter claim.',
            'Original ordinary-motion46718F..467494 runs eight frames through the shared ordinary_motion harness; collision commits between frames are supplied as admitted. The current positive-coordinate cell is supplied as level6/flat, with structuralflag256 for frames1..3,0 for4..6,and256 for7..8.',
            'Draw consumes each native candidate and native velocity; selected native Cannon flags and physical SHP header are supplied unchanged. Camera-600,100 keeps draw points admitted. No expected coordinate/frame/velocity/Z-adjust output is calculated by Python.',
        ],
        substitutions=[
            'Launch virtual GetType/GetWeapon/selected-positive gate return prepared source/type references. World SetLocation5F4EC0 stores supplied native source coordinates; Display Remove4A9770/Submit4A9720 return success. These match the preexisting fireat_launch bounded fixture; registration and map admission are not proved.',
            'Original motion collision/commit boundary is supplied between visits. Each subsequent Render fixture supplies current mapped cell state; collapse/repair calls are not executed. Final CC_Draw_Shape is a draw-argument sink, as documented by bridge_render.',
            'Upstream native reader archive/allocation boundaries are documented by bridge_render_inputs. Ambient x87 control0x0E7F.',
        ],
        entry_points={'speed_producer':0x6FE4F2,'weapon_speed':0x773070,
                      'launch_begin':0x6FE8EE,'launch_end':0x6FF01A,
                      'bullet_fire':0x468670,'motion_begin':0x46718F,
                      'motion_end':0x467494,'object_render':0x5F4B10,
                      'bullet_draw':0x468090,'shape_sink':0x4AED70})


if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
