"""Original caller-specific health ratio predicates, unpatched native slices.

These boundaries establish comparison/threshold/extra-HP semantics, not the
full callbacks, visual allocation, garrison lifecycle or locomotor processing.
"""
from pathlib import Path
from itertools import product
import hashlib
import struct
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_FPCW, UC_X86_REG_FPSW, UC_X86_REG_FPTAG, UC_X86_REG_EAX
from tools.native_oracle import load_image, run_checked, finish_vectors, provenance, SCRATCH, STACK_BASE, STACK_SIZE, RET_MAGIC
from tools.spatial_oracle.map_queries import dwords
UNIT, BUILDING, TYPE, RULES = [SCRATCH+n for n in (0x1000,0x2000,0x4000,0x6000)]
SP=STACK_BASE+STACK_SIZE-0x1000
RANGES=((0x5F5C60,0x5F5C80),(0x5F5CD0,0x5F5D1A),(0x459EE0,0x459EE7),(0x741490,0x741497),
        (0x43FC39,0x43FC84),(0x459254,0x45928A),(0x44E958,0x44E98F),
        (0x4B3DD4,0x4B3DFA),(0x6A3423,0x6A3449),
        (0x73E390,0x73E3B1),(0x73E4EE,0x73E50E),
        (0x6FB88D,0x6FB8A4),(0x702825,0x70283C),
        (0x5F5DD0,0x5F5E79),(0x43EF90,0x43F174),(0x4581F0,0x4581F7))
EVIDENCE=((0x43FB20,0x43FC39),(0x43FC84,0x43FCC7),(0x456E00,0x456F80),
          (0x6F9DD0,0x6F9E04),(0x457CE0,0x457DDD),(0x458200,0x45822E),
          (0x4594CF,0x45956E),(0x459614,0x4596B1))

def bits(value): return struct.unpack('<Q',struct.pack('<d',value))[0]

def execute(current,strength,yellow_bits=0x3fe0000000000000,red_bits=0x3fd0000000000000):
    u=Uc(UC_ARCH_X86,UC_MODE_32);load_image(u)
    u.mem_map(SCRATCH,0x10000);u.mem_map(STACK_BASE,STACK_SIZE);u.mem_map(RET_MAGIC,0x1000)
    original=[bytes(u.mem_read(a,b-a)) for a,b in RANGES]
    for obj,vt,offset in ((UNIT,0x7F5C70,0x6C4),(BUILDING,0x7E3EBC,0x520)):
        u.mem_write(obj,dwords(vt));u.mem_write(obj+offset,dwords(TYPE));u.mem_write(obj+0x6C,dwords(current))
    u.mem_write(TYPE+0xA0,dwords(strength));u.mem_write(0x8871E0,dwords(RULES))
    u.mem_write(RULES+0x1700,struct.pack('<QQ',yellow_bits,red_bits))
    def start(obj):
        u.reg_write(UC_X86_REG_ECX,obj);u.reg_write(UC_X86_REG_ESI,obj)
        u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_EBX,0)
        u.reg_write(UC_X86_REG_FPCW,0x0E7F);u.reg_write(UC_X86_REG_FPSW,0);u.reg_write(UC_X86_REG_FPTAG,0xffff)
        u.mem_write(SP,dwords(RET_MAGIC))
    out={}
    start(UNIT);run_checked(u,0x5F5DD0,RET_MAGIC);out['navigation_category']=u.reg_read(UC_X86_REG_EAX)
    out['occupied_body_frames']=[]
    for tech_level,occupants in product((-1,0,5),(0,8)):
        start(BUILDING)
        u.mem_write(BUILDING+0x534,dwords(1));u.mem_write(BUILDING+0x694,dwords(occupants))
        u.mem_write(TYPE+0x157B,b'\x01');u.mem_write(TYPE+0x634,dwords(tech_level))
        run_checked(u,0x43EF90,RET_MAGIC)
        out['occupied_body_frames'].append(dict(tech_level=tech_level,occupants=occupants,frame=u.reg_read(UC_X86_REG_EAX)))
    start(BUILDING);run_checked(u,0x5F5CD0,RET_MAGIC);out['garrison_red']=bool(u.reg_read(UC_X86_REG_EAX))
    for occupied in (False,True):
        start(BUILDING);u.mem_write(TYPE+0x157B,bytes([int(occupied)]))
        run_checked(u,0x43FC39,0x43FC84)
        out['damage_fire_occupied' if occupied else 'damage_fire_ordinary']=bool(u.reg_read(UC_X86_REG_EBX)&0xff)
    for label,begin,end in (('bunker_wall_damaged',0x459254,0x45928A),('generic_art_damaged',0x44E958,0x44E98F)):
        start(BUILDING);run_checked(u,begin,end);out[label]=bool(u.reg_read(UC_X86_REG_ECX))
    for label,begin,end in (('drive_speed_bits',0x4B3DD4,0x4B3DFA),('ship_speed_bits',0x6A3423,0x6A3449)):
        start(UNIT);u.mem_write(SP+0x18,struct.pack('<d',1.0));run_checked(u,begin,end)
        out[label]=f'{struct.unpack("<Q",u.mem_read(SP+0x18,8))[0]:016x}'
    for label,begin,end in (('refinery_special_damaged',0x73E390,0x73E3B1),('refinery_active_damaged',0x73E4EE,0x73E50E)):
        start(BUILDING);run_checked(u,begin,end);out[label]=bool(u.reg_read(UC_X86_REG_EAX))
    for label,begin,above,other in (('cloak_above_red',0x6FB88D,0x6FB8A4,0x6FB8B9),('smoke_above_yellow',0x702825,0x70283C,0x702857)):
        start(UNIT);stop=run_checked(u,begin,(above,other));out[label]=stop==above
    assert original==[bytes(u.mem_read(a,b-a)) for a,b in RANGES]
    return dict(input=dict(current=current,strength=strength,yellow_bits=f'{yellow_bits:016x}',red_bits=f'{red_bits:016x}'),output=out)

def generate():
    rows=[execute(h,s) for h,s in product((-2147483648,-1,0,1,25,50,65536,2147483647),
        (-2147483648,-1,0,1,100,65536,2147483647))]
    for threshold in (0xbfe0000000000000,0x3fd5555555555555,0x3fe0000000000001,
                      0x3fdfffffffffffff,0x7ff0000000000000,0xfff0000000000000,0x7ff8000000000001):
        for h,s in ((0,0),(1,0),(-1,0),(50,100),(1,3),(0,100)):
            rows.append(execute(h,s,threshold,threshold))
    rows.extend((
        execute(1,10,0x3fe0000000000000,0x3fb9999999999999),
        execute(1,10,0x3fb9999999999999,0x3f9999999999999a),
        execute(1,100,0x3fe0000000000000,0x7ff8000000000001),
        execute(50,100,0x7ff8000000000001,0x3fd0000000000000),
        execute(16777217,33554432),
    ))
    return dict(schema_version=2,rows=rows)

def metadata():
    u=Uc(UC_ARCH_X86,UC_MODE_32);load_image(u)
    def packet(ranges):
        result={}
        for a,b in ranges:
            raw=bytes(u.mem_read(a,b-a));result[f'{a:08X}..{b:08X}']=dict(hex=raw.hex(),sha256=hashlib.sha256(raw).hexdigest())
        return result
    result=provenance(scope='Caller-specific signed health ratio branches, whole navigation category and completed occupied body frame',
        assumptions=['PC53/chop with masked exceptions. Original class getters and ratio instructions execute unchanged.',
            'Thresholds are supplied raw binary64 Rules+1700/+1708; NaN/infinity fixtures do not claim retail parser admission.',
            'Damage-fire predicate ends before cache comparison/callback; art/bunker predicates end before animation creation.',
            'Drive/Ship slice begins after terrain/slope/zero substitution with stored speed1.0 and stops after health penalty.',
            'Garrison red executes whole Object5F5CD0, but CanDock/ejection callers are instruction evidence, not executed whole bodies.',
            'Refinery ends before animation call; cloak/smoke end at branch boundary before cloak action/RNG or smoke removal/creation.',
            'Navigation executes whole Object5F5DD0. Body executes whole43EF90 with state534=1, CanBeOccupied=1, occupant0/8, TechLevel-1/0/5; gate/laser/fence flags unset.',
            'Changed callback and Building Update prefix are saved instruction evidence only; no whole lifecycle parity is claimed.'],
        substitutions=['Only data-shaped Unit/Building/type/Rules, stack/register state and ambient FPCW; no original instruction patches or callbacks.'],
        entry_points=dict(navigation_category=0x5F5DD0,occupied_body=0x43EF90,garrison=0x5F5CD0,damage_fire=0x43FC39,bunker_wall=0x459254,generic_art=0x44E958,drive=0x4B3DD4,ship=0x6A3423,refinery_special=0x73E390,refinery_active=0x73E4EE,cloak=0x6FB88D,smoke=0x702825))
    result['original_code']=packet(RANGES);result['instruction_evidence_not_executed']=packet(EVIDENCE)
    result['native_data']={f'{a:08X}':bytes(u.mem_read(a,n)).hex() for a,n in
        ((0x7E3EBC+0x148,4),(0x7E3EBC+0x88,4),(0x7F5C70+0x88,4),(0x7E7FC0,8))}
    return result

if __name__=='__main__':finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
