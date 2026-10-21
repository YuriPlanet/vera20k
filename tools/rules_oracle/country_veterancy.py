"""Bounded original country list readers and their original rank consumers.

Reuses BulletReader cached-INI/CRT/TLS/allocator authority. Synthetic names and
object priors only; no game assets. Original .text remains byte-identical.
"""
import struct
from pathlib import Path

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import *
from tools.native_oracle import run_checked, finish_vectors, provenance
from tools.projectile_oracle.bridge_render_inputs import BulletReader
from tools.rmg_oracle.gen_rng_vectors import STRUCT_LEN, seeded_struct
from tools.spatial_oracle.building_body_rules import RULES, SP, dwords

OUT = Path(__file__).resolve().parent
EMPTY = OUT / '__country_veterancy_no_assets__'  # No physical assets are used.
KEYS = [('VeteranInfantry',0x14C,0x524CB0),('VeteranUnits',0x168,0x7480D0),('VeteranAircraft',0x184,0x41CEF0)]
TIMER=0xB0EA80

def fresh():
    m=BulletReader({}, EMPTY)
    for p in (0xA83C98,0xA83CE0,0xA8E348,0xA8B218,0xA8EB00):
        m.u.mem_write(p,dwords(0x7EB6D4,m.alloc(4096),1024,1,0,10))
    m.u.mem_write(0x889F64,b'\0')
    m.rngs={'main':0x886B88,'scenario':m.read32(0xA8B230)+0x218,'mapgen':0xABE890}
    for p in m.rngs.values():m.u.mem_write(p,seeded_struct(1))
    m.u.mem_write(TIMER,struct.pack('<3i',995,0,25))
    m.country=m.alloc(0x2000)
    m.u.mem_write(m.country,b'\xa5'*0x2000)
    m.invoke(0x5113F0,m.country,(m.cstring('SYNTHCOUNTRY'),))
    m.code=bytes(m.u.mem_read(0x401000,0x3E0000))
    m.timer_writes=[]
    def timer(u,access,address,size,value,data):
        m.timer_writes.append(dict(pc=hex(u.reg_read(UC_X86_REG_EIP)),offset=address-TIMER,size=size,value=value))
    m.u.hook_add(UC_HOOK_MEM_WRITE,timer,begin=TIMER,end=TIMER+11)
    return m

def rng(m):return {k:bytes(m.u.mem_read(p,STRUCT_LEN)) for k,p in m.rngs.items()}
def bounds(m,before):
    # Every selected reader and rank region leaves original text, all three
    # RNG streams and the action-timer sentinel untouched.
    assert bytes(m.u.mem_read(0x401000,0x3E0000))==m.code
    assert rng(m)==before
    assert not m.timer_writes and list(struct.unpack('<3i',m.u.mem_read(TIMER,12)))==[995,0,25]
def state(m):
    out={}
    for key,offset,_ in KEYS:
        p=m.country+offset;data=m.read32(p+4);count=m.read32(p+16)
        out[key]=dict(names=[m.string(m.read32(data+i*4)+0x24) for i in range(count)])
    return out

def read(m,sections):
    events=[];pending=[]
    def observer(u,pc,_n,_d):
        sp=u.reg_read(UC_X86_REG_ESP)
        if pc==0x528A10:
            args=[m.read32(sp+4+i*4) for i in range(5)]
            if m.string(args[1]) not in [key for key,_,_ in KEYS]:return
            row=dict(kind='ReadString',section=m.string(args[0]),key=m.string(args[1]),default=m.string(args[2]),capacity=args[4]);events.append(row);pending.append((m.read32(sp),sp+24,row,args[3]))
        elif pc in (0x524CB0,0x7480D0,0x41CEF0):
            row=dict(kind='FindOrAllocate',entry=hex(pc),token=m.string(u.reg_read(UC_X86_REG_ECX)));events.append(row);pending.append((m.read32(sp),sp+4,row,None))
        for item in list(pending):
            ret,endsp,row,buffer=item
            if pc==ret and sp==endsp:
                row['returned']=u.reg_read(UC_X86_REG_EAX)
                if buffer:row['buffer']=m.string(buffer)
                pending.remove(item)
    hook=m.u.hook_add(UC_HOOK_CODE,observer)
    m.rules_cache(sections)
    before=rng(m);m.timer_writes.clear()
    m.invoke(0x511850,m.country,(RULES,))
    assert not pending
    m.u.hook_del(hook)
    assert bytes(m.u.mem_read(0x401000,0x3E0000))==m.code
    bounds(m,before)
    return dict(sections=sections,lists=state(m))

def reader_controls():
    m=fresh();rows=[]
    cases=[('synthetic_names','ONE,TWO'),('missing',None),('stored_empty',''),('whitespace','   '),('delimiters',',,,'),('duplicates_none_case','ONE,none,NONE,<none>, TWO,one'),('mixed_delimiters','ONE TWO\tTHREE,FOUR'),('long_buffer','A'*126+',Z'),('long_token','B'*140)]
    for name,raw in cases:
        sections={'SYNTHCOUNTRY':{k:raw for k,_,_ in KEYS}} if raw is not None else {'SYNTHCOUNTRY':{}}
        rows.append(dict(id=name,**read(m,sections)))
    rows.append(dict(id='wrong_key_case',**read(m,{'SYNTHCOUNTRY':{'veteraninfantry':'OTHER','veteranunits':'OTHER','veteranaircraft':'OTHER'}})))
    rows.append(dict(id='wrong_section_case',**read(m,{'synthcountry':{k:'OTHER' for k,_,_ in KEYS}})))
    rows.append(dict(id='absent_section',**read(m,{})))
    return rows

def gate_controls():
    rows=[]
    for kind,offset,entry,stop in [('infantry',0x14C,0x517CE7,0x517D51),('unit_ctor',0x168,0x735604,0x735678),('unit_init',0x168,0x746827,0x74689B),('aircraft_init',0x184,0x413F98,0x413FD2)]:
        for label,match,trainable,naval,spy,prior in [('empty',False,True,False,False,0),('country_untrainable_naval',True,False,True,False,0),('country_replaces_elite',True,False,False,False,0x40000000),('country_and_spy',True,True,False,True,0),('spy_only',False,True,False,True,0),('spy_naval',False,True,True,True,0),('spy_untrainable',False,False,False,True,0)]:
            m=fresh();read(m,{'SYNTHCOUNTRY':{key:'ONE' for key,_,_ in KEYS}})
            p=m.country+offset;typ=m.read32(m.read32(p+4));actor=m.alloc(0x1000);house=m.alloc(0x1000)
            m.u.mem_write(actor+(0x6C0 if kind=='infantry' else 0x6C4),dwords(typ));m.u.mem_write(actor+0x21C,dwords(house));m.u.mem_write(house+0x34,dwords(m.country))
            m.u.mem_write(house+0x2BF,bytes([spy,spy]));m.u.mem_write(typ+0xC8E,bytes([trainable]));m.u.mem_write(typ+0xCCE,bytes([naval]));m.u.mem_write(actor+0x150,dwords(prior))
            if not match:m.u.mem_write(p+16,dwords(0))
            m.u.reg_write(UC_X86_REG_ESI,actor);m.u.reg_write(UC_X86_REG_EDI,typ)
            before=rng(m);m.timer_writes.clear();m.u.reg_write(UC_X86_REG_ESP,SP)
            run_checked(m.u,entry,stop,count=10000,required_addresses=(entry,))
            # Aircraft's selected region includes PUSH EBX at413F9E. Its
            # matching POP is later in unrelated type setup at414039.
            assert m.u.reg_read(UC_X86_REG_ESP)==SP-(4 if kind=='aircraft_init' else 0),(kind,label,hex(m.u.reg_read(UC_X86_REG_ESP)))
            bounds(m,before)
            rows.append(dict(id=kind+'_'+label,kind=kind,country_match=match,trainable=trainable,naval=naval,infiltrated=spy,raw_before=prior,raw_after=m.read32(actor+0x150)))
    return rows

def generate():
    return dict(schema=1,reader_controls=reader_controls(),rank_controls=gate_controls())


def metadata():
    return provenance(
        scope='Full original Country constructor and twelve retained full ReadINI controls, and twenty-eight selected original rank-initialization regions; synthetic identities and object priors only',
        assumptions=[
            'BulletReader owns supplied exact-case cached INI entries, CRT TLS and allocation. Physical INI/MIX loading is excluded. Full Country511850 executes its admission and list logic; no parser or list result is supplied.',
            'Fresh type-family registries have spare capacity. Original FindOrAllocate524CB0/7480D0/41CEF0 and complete type constructors bind synthetic names, preserving duplicates, case-insensitive identity and the24-byte stored ID. Color uses the existing unrelated initialized scheme prior; graphics and asset loading are excluded.',
            'Each selected rank region receives a supplied actor, ownerHouse, native-constructed Country/type, prior rank and Trainable/Naval/infiltration flags. These are exact region controls, not whole Unit/Infantry/Aircraft construction, COM/Unlimbo or spy lifecycle. Aircraft region413F98..413FD2 retains the actual PUSH EBX, whose POP lies later in unrelated type setup.',
            'Every reader and rank control is asserted to leave all three complete RNG states byte-identical, write no action-timer byte and keep the sentinel[995,0,25]. No reached selected rank code makes an RNG or detach call. Constructor and reader allocation effects are separate from object-creation RNG chronology.',
            'FPCW0E7F is the declared fixture environment. Rank SetVeteran750090 writes integer bits3F800000 directly and makes no floating arithmetic. Existing selection_navigation map-percent goldens own map overrides and their numeric rounding; this fixture does not replace them.'
        ],
        substitutions=[
            'Existing BulletReader: operator_new7C8E17 returns bump storage; operator_delete7C8B3D is inert; CRT TLS7D140B returns thread storage. Cached exact-case INI indexes replace physical file/archive loading, while original ReadString528A10, strtok7C9CC2, factories, constructors, vector copies and rank setter execute unchanged.'
        ],
        entry_points=dict(country_constructor=0x5113F0,country_read=0x511850,
                          infantry_list=0x511D16,unit_list=0x511E20,aircraft_list=0x511F56,
                          infantry_init_rank=0x517CE7,unit_ctor_rank=0x735604,
                          unit_init_rank=0x746827,aircraft_init_rank=0x413F98,
                          set_veteran=0x750090))


if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
