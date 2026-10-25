"""Physical empty FV passive admission and GreatestThreat wrapper boundary.

Reuses the declared partial physical FV/weapon preparation from ifv_fire_coord.
Full CanAcquireTarget7091D0 and PassiveAcquireGate709290 execute. Original
Unit743190/Foot4D9920 wrappers stop at GreatestThreat6F8DF0; no scan/pick proof.
Source Unit lifecycle, human House, Guard mission and absent target are supplied.
"""
import hashlib
import struct
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP
from tools.native_oracle import RET_MAGIC, finish_vectors, provenance, run_checked
from tools.projectile_oracle.ifv_fire_coord import prepare
from tools.projectile_oracle.bridge_render_inputs import assets_root, lexical
from tools.spatial_oracle.building_body_rules import RULES, SP, dwords


def generate():
    m, source, typ, weapon, cells, initial = prepare()
    u = m.u
    constructor=dict(can_passive_aquire=bool(u.mem_read(typ+0xD99,1)[0]),
                     distributed_fire=bool(u.mem_read(typ+0x6B0,1)[0]))
    layers=[]
    for name in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','Hills.map'):
        path=assets_root()/name
        if not path.exists():
            layers.append(dict(file=name,absent=True))
            continue
        raw=path.read_bytes()
        sections,_=lexical(raw,{'FV'})
        m.rules_cache(sections)
        for start,end in ((0x71446C,0x714486),(0x714850,0x71486A)):
            for reg,value in ((UC_X86_REG_ESP,SP),(UC_X86_REG_EBP,typ),
                              (UC_X86_REG_EBX,typ+0x24),(UC_X86_REG_ESI,RULES),(UC_X86_REG_EDI,RULES)):
                u.reg_write(reg,value)
            run_checked(u,start,end)
        layers.append(dict(file=name,sha256=hashlib.sha256(raw).hexdigest(),
            source_keys={k:sections.get('FV',{}).get(k)for k in ('CanPassiveAquire','DistributedFire')},
            can_passive_aquire=bool(u.mem_read(typ+0xD99,1)[0]),
            distributed_fire=bool(u.mem_read(typ+0x6B0,1)[0])))
    house=m.alloc(0x6000)
    u.mem_write(house+0x1EC,b'\x01')
    u.mem_write(source+0x21C,dwords(house))
    u.mem_write(0xA8B238,dwords(1))
    u.mem_write(source+0x2B4,dwords(0))
    u.mem_write(source+0xAC,dwords(5))
    u.mem_write(source+0x9C,dwords(2688,5248,800))
    observed=[]
    def observe(uc,pc,size,data):
        if pc in (0x65C780,0x65C7E0,0x68BCB0):
            raise AssertionError(('unexpected RNG/identity',hex(pc)))
        if pc in (0x7091D0,0x709290,0x70E140,0x70E1A0,0x6F3270,0x743190,0x4D9920,0x772A90):
            observed.append(f'{pc:08x}')
    hook=u.hook_add(UC_HOOK_CODE,observe)
    can=m.invoke(0x7091D0,source)&255
    passive=m.invoke(0x709290,source)&255
    coords=m.alloc(12)
    u.mem_write(coords,dwords(2688,5248,800))
    u.mem_write(SP,dwords(RET_MAGIC,1,coords,0))
    u.reg_write(UC_X86_REG_ESP,SP)
    u.reg_write(UC_X86_REG_ECX,source)
    run_checked(u,0x743190,0x6F8DF0,required_addresses=(0x4D9920,))
    sp=u.reg_read(UC_X86_REG_ESP)
    ret,mask,coord_arg,third=struct.unpack('<4I',u.mem_read(sp,16))
    assert coord_arg==coords and ret==0x4D9947
    u.hook_del(hook)
    return dict(constructor=constructor,layers=layers,
        source_type=m.string(typ+0x24),selected_weapon=m.string(weapon+0x24),
        can_acquire=can,passive_gate=passive,greatest_threat_entry=dict(mask=mask,
        supplied_initial_mask=1,reference_xyz=list(struct.unpack('<3i',u.mem_read(coords,12))),third_argument=third),
        visited=observed,unit_vtable_slots={f'{off:x}':f'{m.read32(0x7F5C70+off):08x}'for off in (0x39C,0x3C4,0x3C8)},
        rng_calls=0,native_id_calls=0)


def metadata():
    return provenance(scope=__doc__, entry_points={'unit_type_ctor':0x7470D0,
        'can_passive_aquire_reader':0x71446C,'distributed_fire_reader':0x714850,
        'can_acquire':0x7091D0,'passive_gate':0x709290,'unit_wrapper':0x743190,
        'foot_wrapper':0x4D9920,'stop_before_greatest_threat':0x6F8DF0}, assumptions=[
        'Physical FV layers omit both selected keys; original constructor/readers retain CanPassiveAquire true and DistributedFire false.',
        'Source lifecycle/House/mission are supplied, actual Unit vtable/getters/weapon selection execute. No callbacks in the claimed gates/wrappers are replaced.',
        'Inherited physical selected preparation and its archive/CRT/allocator boundaries are those of ifv_fire_coord; not complete Unit construction or native map loading.',
        'Gate and wrapper calls enter separately; no complete AI tick, GreatestThreat traversal, candidate evaluation or retained-target assignment is executed.',
    ], substitutions=['Inherited supplied lexical INI caches, allocation/TLS/archive boundaries; explicit human House and Guard/no-target source state.'])


if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
