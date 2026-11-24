"""Original power output/drain bodies and House ADD32 accumulator slices.

No instruction patches or callback replacements. Power/ExtraPower inputs enter
at original post-INI-read split blocks; this is not CRT/INI parser coverage.
"""
from pathlib import Path
from itertools import product
import hashlib
import struct
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EBP, UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.native_oracle import load_image, run_checked, finish_vectors, provenance, SCRATCH, STACK_BASE, STACK_SIZE, RET_MAGIC
from tools.spatial_oracle.map_queries import dwords

OBJECT, TYPE, HOUSE = SCRATCH+0x1000, SCRATCH+0x3000, SCRATCH+0x8000
SP = STACK_BASE+STACK_SIZE-0x1000
RANGES = ((0x44E7B0,0x44E879),(0x44E880,0x44E8E2),(0x70C5B0,0x70C5B7),
          (0x459EE0,0x459EE7),(0x5F5C60,0x5F5C80),(0x7C5F00,0x7C5F2C),
          (0x461080,0x4610A0),(0x4610C0,0x4610D8),
          (0x508CF9,0x508D07),(0x508D0E,0x508D1E))

def signed(value): return struct.unpack('<i',struct.pack('<I',value & 0xffffffff))[0]

def execute(actual, strength, power, extra=0, occupants=0, unit_absorb=False,
            infantry_absorb=False, online=True, warped=False, overpowered=False,
            prior_output=0, prior_drain=0):
    inputs=dict(actual=actual,strength=strength,power=power,extra=extra,
        occupants=occupants,unit_absorb=unit_absorb,infantry_absorb=infantry_absorb,
        online=online,warped=warped,overpowered=overpowered,
        prior_output=prior_output,prior_drain=prior_drain)
    u=Uc(UC_ARCH_X86,UC_MODE_32);load_image(u)
    u.mem_map(SCRATCH,0x10000);u.mem_map(STACK_BASE,STACK_SIZE);u.mem_map(RET_MAGIC,0x1000)
    original=[bytes(u.mem_read(a,b-a)) for a,b in RANGES]
    u.mem_write(OBJECT,dwords(0x7E3EBC));u.mem_write(OBJECT+0x520,dwords(TYPE))
    u.mem_write(OBJECT+0x6C,dwords(actual));u.mem_write(TYPE+0xA0,dwords(strength))
    u.mem_write(OBJECT+0x114,dwords(occupants))
    for address,value in ((OBJECT+0x660,online),(OBJECT+0x270,warped),
                          (OBJECT+0x668,overpowered),(TYPE+0x16AE,unit_absorb),
                          (TYPE+0x16AF,infantry_absorb)):
        u.mem_write(address,bytes([int(value)]))
    # Execute original post-read signed field splits, including NEG INT_MIN.
    u.reg_write(UC_X86_REG_EBP,TYPE);u.reg_write(UC_X86_REG_EDI,0)
    u.reg_write(UC_X86_REG_EAX,power & 0xffffffff);run_checked(u,0x461080,0x4610A0)
    u.reg_write(UC_X86_REG_EAX,extra & 0xffffffff);run_checked(u,0x4610C0,0x4610D8)
    results={}
    for label,address in (('output',0x44E7B0),('drain',0x44E880)):
        u.reg_write(UC_X86_REG_ECX,OBJECT);u.reg_write(UC_X86_REG_ESP,SP)
        u.reg_write(UC_X86_REG_FPCW,0x0E7F);u.mem_write(SP,dwords(RET_MAGIC))
        run_checked(u,address,RET_MAGIC)
        assert u.reg_read(UC_X86_REG_ESP)==SP+4
        results[label]=signed(u.reg_read(UC_X86_REG_EAX))
    u.mem_write(HOUSE+0x53A4,dwords(prior_output));u.mem_write(HOUSE+0x53A8,dwords(prior_drain))
    u.reg_write(UC_X86_REG_ESI,HOUSE);u.reg_write(UC_X86_REG_EDI,OBJECT)
    u.reg_write(UC_X86_REG_EAX,results['output'] & 0xffffffff);run_checked(u,0x508CF9,0x508D07)
    u.reg_write(UC_X86_REG_EAX,results['drain'] & 0xffffffff);run_checked(u,0x508D0E,0x508D1E)
    results['total_output'],results['total_drain']=struct.unpack('<ii',u.mem_read(HOUSE+0x53A4,8))
    assert original==[bytes(u.mem_read(a,b-a)) for a,b in RANGES]
    return dict(input=inputs,output=results)

def admission_rows():
    u=Uc(UC_ARCH_X86,UC_MODE_32);load_image(u)
    u.mem_map(SCRATCH,0x10000)
    start,end=0x508CA2,0x508CF2
    original=bytes(u.mem_read(start,end-start))
    rows=[]
    for marked,alive,limbo in product((False,True),repeat=3):
        u.mem_write(HOUSE+0x6C,dwords(SCRATCH+0xE000))
        u.mem_write(SCRATCH+0xE000,dwords(OBJECT))
        u.mem_write(OBJECT+0x74,bytes([marked]))
        u.mem_write(OBJECT+0x90,bytes([alive]))
        u.mem_write(OBJECT+0x81,bytes([limbo]))
        # Noncampaign mode and non-local House avoid unrelated registration gates.
        u.mem_write(0xA8B238,dwords(1));u.mem_write(0xA83D4C,dwords(0))
        u.reg_write(UC_X86_REG_ESI,HOUSE);u.reg_write(UC_X86_REG_EBP,0);u.reg_write(UC_X86_REG_EBX,0)
        stop=run_checked(u,start,(end,0x508D37),count=32)
        rows.append(dict(input=dict(marked=marked,alive=alive,limbo=limbo),admitted=stop==end))
    assert bytes(u.mem_read(start,end-start))==original
    return rows

def empty_contribution_rows():
    """Authentic House entry/reset/loop with no qualifying contributor."""
    u=Uc(UC_ARCH_X86,UC_MODE_32);load_image(u)
    u.mem_map(SCRATCH,0x10000);u.mem_map(STACK_BASE,STACK_SIZE)
    start,end=0x508C30,0x508D45
    original=bytes(u.mem_read(start,end-start))
    rows=[]
    for prior_output,prior_drain in ((200,0),(0,20),(200,100),(-2147483648,2147483647)):
        for member in ('empty','null','unmarked','limbo'):
            u.mem_write(HOUSE+0x53A4,dwords(prior_output,prior_drain))
            u.mem_write(HOUSE+0x78,dwords(0 if member=='empty' else 1))
            u.mem_write(HOUSE+0x6C,dwords(SCRATCH+0xE000))
            u.mem_write(SCRATCH+0xE000,dwords(0 if member=='null' else OBJECT))
            u.mem_write(OBJECT+0x74,bytes([member=='limbo']))
            u.mem_write(OBJECT+0x81,bytes([member=='limbo']))
            u.reg_write(UC_X86_REG_ECX,HOUSE);u.reg_write(UC_X86_REG_ESP,SP)
            u.reg_write(UC_X86_REG_FPCW,0x0E7F)
            run_checked(u,start,end)
            output,drain=struct.unpack('<ii',u.mem_read(HOUSE+0x53A4,8))
            rows.append(dict(input=dict(prior_output=prior_output,prior_drain=prior_drain,member=member),
                             output=dict(total_output=output,total_drain=drain)))
    assert bytes(u.mem_read(start,end-start))==original
    return rows

def generate():
    rows=[execute(h,s,p) for h,s,p in product(
        (-2147483648,-1,0,1,49,65536,2147483647),
        (-2147483648,-1,0,1,3,100,65536,2147483647),
        (-2147483648,-75,0,1,200,2147483647))]
    for extra,occupants,ua,ia in product((-1,0,100,2147483647),(0,1,3,65536),(False,True),(False,True)):
        rows.append(execute(65536,100000,150,extra,occupants,ua,ia))
    for flags in product((False,True),repeat=3):
        rows.append(execute(50,100,150,100,3,False,True,*flags))
    for prior in (-2147483648,-1,0,2147483647):
        for power in (-2147483648,-75,200,2147483647):
            rows.append(execute(100,100,power,prior_output=prior,prior_drain=prior))
    for values in ((300,600,200),(600,600,200),(40,400,150),
                   (375,750,150,100,5,False,True),(500,500,100,80,2,True,False)):
        rows.append(execute(*values))
    return dict(schema_version=3,rows=rows,admission_rows=admission_rows(),empty_contribution_rows=empty_contribution_rows())

def metadata():
    u=Uc(UC_ARCH_X86,UC_MODE_32);load_image(u)
    result=provenance(scope='Original Building GetPowerOutput/GetPowerDrain with signed-health ratio and House ADD32 totals',
        assumptions=['PC53/chop FPCW0E7F. Actual and type Strength are raw signed32. Original retail vtable getters execute unchanged.',
            'Original post-INI signed field splits execute with supplied parsed EAX; no native text parser coverage.',
            'No upgrade slots supplied. Warped/online/output-overpowered(+668) bytes are fixture inputs, not proof of lifecycle writers. ExtraDrain(+669) remains false and is not covered.',
            'House accumulator and8 admission combinations execute; admission uses noncampaign mode1/non-local House. Original House entry and16 empty/rejected-contribution iterations also execute through508D45. Qualifying iteration callbacks, campaign registration, blackout and scheduler are excluded.',
            'Corpus does not establish whole power lifecycle or sidebar theoretical-total parity.'],
        substitutions=['Data-shaped Building/type/House, integer inputs, stack and ambient FPCW only; no callback replacement or original code patches.'],
        entry_points=dict(output=0x44E7B0,drain=0x44E880,ratio=0x5F5C60,output_add=0x508CF9,drain_add=0x508D0E))
    result['original_code']={f'{a:08X}..{b:08X}':dict(hex=bytes(u.mem_read(a,b-a)).hex(),sha256=hashlib.sha256(bytes(u.mem_read(a,b-a))).hexdigest()) for a,b in RANGES}
    a,b=0x508CA2,0x508CF2
    raw=bytes(u.mem_read(a,b-a))
    result['caller_admission_executed']={
        f'{a:08X}..{b:08X}':dict(hex=raw.hex(),sha256=hashlib.sha256(raw).hexdigest())}
    a,b=0x508C30,0x508D45
    raw=bytes(u.mem_read(a,b-a))
    result['empty_iteration_executed']={
        f'{a:08X}..{b:08X}':dict(hex=raw.hex(),sha256=hashlib.sha256(raw).hexdigest())}
    result['vtable_slots']={f'{a:08X}':bytes(u.mem_read(a,4)).hex() for a in (0x7E3EBC+0x88,0x7E3EBC+0x1D4)}
    return result

if __name__=='__main__':finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
