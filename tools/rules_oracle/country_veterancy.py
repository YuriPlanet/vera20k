"""Bounded original country list readers and their original rank/cameo consumers.

Reuses BulletReader cached-INI/CRT/TLS/allocator authority. Synthetic names and
object priors only; no game assets. Original .text remains byte-identical.
"""
import hashlib, struct
from pathlib import Path

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import *
from tools.native_oracle import run_checked, NativeCallTrace, finish_vectors, provenance, image_bytes
from tools.projectile_oracle.bridge_render_inputs import BulletReader
from tools.rmg_oracle.gen_rng_vectors import STRUCT_LEN, seeded_struct, draws
from tools.spatial_oracle.building_body_rules import RULES, SP, dwords
from tools.spatial_oracle.cost_of import VTABLE

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
    after=rng(m);n,_=draws(after['main'],4)
    assert bytes(m.u.mem_read(0x401000,0x3E0000))==m.code
    return dict(rng_before_hex={k:v.hex() for k,v in before.items()},rng_after_hex={k:v.hex() for k,v in after.items()},main_next_four=n,timer_after=list(struct.unpack('<3i',m.u.mem_read(TIMER,12))),timer_writes=list(m.timer_writes))
def state(m):
    out={}
    for key,offset,_ in KEYS:
        p=m.country+offset;data=m.read32(p+4);count=m.read32(p+16)
        out[key]=dict(count=count,capacity=m.read32(p+8),capacity_increment=m.read32(p+20),names=[m.string(m.read32(data+i*4)+0x24) for i in range(count)],pointers=[m.read32(data+i*4) for i in range(count)])
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
    admitted=m.invoke(0x511850,m.country,(RULES,))&255
    assert not pending
    m.u.hook_del(hook)
    assert bytes(m.u.mem_read(0x401000,0x3E0000))==m.code
    return dict(sections=sections,admitted=bool(admitted),events=events,lists=state(m),**bounds(m,before))

def reader_controls():
    m=fresh();rows=[]
    initial=state(m)
    cases=[('synthetic_names','ONE,TWO'),('missing',None),('stored_empty',''),('whitespace','   '),('delimiters',',,,'),('duplicates_none_case','ONE,none,NONE,<none>, TWO,one'),('mixed_delimiters','ONE TWO\tTHREE,FOUR'),('long_buffer','A'*126+',Z'),('long_token','B'*140)]
    for name,raw in cases:
        sections={'SYNTHCOUNTRY':{k:raw for k,_,_ in KEYS}} if raw is not None else {'SYNTHCOUNTRY':{}}
        rows.append(dict(id=name,**read(m,sections)))
    rows.append(dict(id='wrong_key_case',**read(m,{'SYNTHCOUNTRY':{'veteraninfantry':'OTHER','veteranunits':'OTHER','veteranaircraft':'OTHER'}})))
    rows.append(dict(id='wrong_section_case',**read(m,{'synthcountry':{k:'OTHER' for k,_,_ in KEYS}})))
    rows.append(dict(id='absent_section',**read(m,{})))
    return initial,rows

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
            stores=[];calls=[]
            trace=NativeCallTrace(m.u,m.read32,calls)
            def observe(u,pc,n,d):
                trace.returned(pc,u.reg_read(UC_X86_REG_ESP))
                if pc==0x750090:trace.entered(pc,u.reg_read(UC_X86_REG_ESP),('SetVeteran',1,4))
            def store(u,access,address,size,value,d):stores.append(dict(pc=hex(u.reg_read(UC_X86_REG_EIP)),address=hex(address-actor),size=size,value=value))
            h=m.u.hook_add(UC_HOOK_CODE,observe);h2=m.u.hook_add(UC_HOOK_MEM_WRITE,store,begin=actor+0x150,end=actor+0x153)
            before=rng(m);m.timer_writes.clear();m.u.reg_write(UC_X86_REG_ESP,SP)
            run_checked(m.u,entry,stop,count=10000,required_addresses=(entry,));trace.returned(stop,m.u.reg_read(UC_X86_REG_ESP));assert not trace.pending
            # Aircraft's selected region includes PUSH EBX at413F9E. Its
            # matching POP is later in unrelated type setup at414039.
            assert m.u.reg_read(UC_X86_REG_ESP)==SP-(4 if kind=='aircraft_init' else 0),(kind,label,hex(m.u.reg_read(UC_X86_REG_ESP)))
            m.u.hook_del(h);m.u.hook_del(h2)
            rows.append(dict(id=kind+'_'+label,kind=kind,entry=hex(entry),stop=hex(stop),country_match=match,trainable=trainable,naval=naval,infiltrated=spy,raw_before=prior,raw_after=m.read32(actor+0x150),calls=calls,stores=stores,**bounds(m,before)))
    return rows

def cameo_controls():
    rows=[]
    for kind in ('infantry','unit','aircraft','building'):
        for label,match,trainable,naval,spy,undeploy in [('country_only',True,True,False,False,False),('spy_trainable',False,True,False,True,True),('spy_untrainable',True,False,False,True,True),('spy_naval',False,True,True,True,True),('building_no_undeploy',True,True,False,True,False)]:
            m=fresh();read(m,{'SYNTHCOUNTRY':{k:'ONE' for k,_,_ in KEYS}})
            key,offset,_=KEYS[0 if kind=='infantry' else 2 if kind=='aircraft' else 1]
            typ=m.read32(m.read32(m.country+offset+4));house=m.alloc(0x1000)
            m.u.mem_write(typ,dwords(VTABLE[kind]));m.u.mem_write(typ+0x6F0,dwords(0x11111111));m.u.mem_write(typ+0x710,dwords(0x22222222));m.u.mem_write(typ+0x408,dwords(1 if undeploy else 0));m.u.mem_write(typ+0xC8E,bytes([trainable]));m.u.mem_write(typ+0xCCE,bytes([naval]));m.u.mem_write(house+0x34,dwords(m.country));m.u.mem_write(house+0x2BF,bytes([spy,spy]));m.u.mem_write(0xA83D4C,dwords(house))
            if not match:m.u.mem_write(m.country+offset+16,dwords(0))
            before=rng(m);m.timer_writes.clear();result=m.invoke(0x712040,typ)
            rows.append(dict(id=kind+'_'+label,kind=kind,country_match=match,trainable=trainable,naval=naval,infiltrated=spy,undeploy=undeploy,result=result,**bounds(m,before)))
    return rows

def stored_id_admission_controls():
    rows=[]
    for key,offset,_ in KEYS:
        for section in ('TWO',' TWO'):
            m=fresh();read(m,{'SYNTHCOUNTRY':{key:'ONE, TWO'}})
            typ=m.read32(m.read32(m.country+offset+4)+4)
            m.rules_cache({section:{'Name':'SYNTHETIC'}})
            before=rng(m);m.timer_writes.clear()
            result=m.invoke(0x410A60,typ,(RULES,))&255
            rows.append(dict(family_key=key,stored_id=m.string(typ+0x24),provided_section=section,
                             abstract_type_read_admitted=bool(result),**bounds(m,before)))
    return rows


def generate():
    initial,readers=reader_controls()
    payload=dict(schema=1,binary_sha256=hashlib.sha256(image_bytes()).hexdigest(),
                 constructor_lists=initial,reader_controls=readers,
                 rank_controls=gate_controls(),cameo_controls=cameo_controls(),
                 stored_id_admission_controls=stored_id_admission_controls())
    # Every observed stream is byte-identical to the same fresh native seed.
    # Retain those complete raw bytes once instead of duplicating them per row.
    baseline=readers[0]['rng_before_hex']
    payload['rng_state_hex']=baseline
    for key in ('reader_controls','rank_controls','cameo_controls','stored_id_admission_controls'):
        for row in payload[key]:
            before=row.pop('rng_before_hex');after=row.pop('rng_after_hex')
            assert before==baseline and after==baseline,(key,row.get('id',row.get('family_key')))
            row['rng_unchanged']=True
    return payload


def metadata():
    return provenance(
        scope='Full original Country constructor and twelve retained full ReadINI controls, twenty-eight selected original rank-initialization regions, twenty whole original GetCameo controls and six stored-ID base-reader admissions; synthetic identities and object priors only',
        assumptions=[
            'BulletReader owns supplied exact-case cached INI entries, CRT TLS and allocation. Physical INI/MIX loading is excluded. Full Country511850 executes its admission and list logic; no parser or list result is supplied.',
            'Fresh type-family registries have spare capacity. Original FindOrAllocate524CB0/7480D0/41CEF0 and complete type constructors bind synthetic names, preserving duplicates, case-insensitive identity and the24-byte stored ID. Color uses the existing unrelated initialized scheme prior; graphics and asset loading are excluded.',
            'Each selected rank region receives a supplied actor, ownerHouse, native-constructed Country/type, prior rank and Trainable/Naval/infiltration flags. These are exact region controls, not whole Unit/Infantry/Aircraft construction, COM/Unlimbo or spy lifecycle. Aircraft region413F98..413FD2 retains the actual PUSH EBX, whose POP lies later in unrelated type setup.',
            'Whole712040 receives actual original type vtables and synthetic Cameo11111111/AltCameo22222222 pointer results; no category predicate result is substituted. Building consumer storage uses supplied BuildingType fields. Complete sidebar drawing is excluded.',
            'All three complete RNG states are recorded once as rng_state_hex; every measured before/after is asserted identical to those exact bytes. Main next4, action-timer sentinel[995,0,25] and every timer write are retained per row. No reached selected rank/cameo code makes an RNG or detach call. Constructor and reader allocation effects are separate from object-creation RNG chronology.',
            'FPCW0E7F is the declared fixture environment. Rank SetVeteran750090 writes integer bits3F800000 directly and makes no floating arithmetic. Existing selection_navigation map-percent goldens own map overrides and their numeric rounding; this fixture does not replace them.'
        ],
        substitutions=[
            'Existing BulletReader: operator_new7C8E17 returns bump storage; operator_delete7C8B3D is inert; CRT TLS7D140B returns thread storage. Cached exact-case INI indexes replace physical file/archive loading, while original ReadString528A10, strtok7C9CC2, factories, constructors, vector copies, rank setter and GetCameo execute unchanged.'
        ],
        entry_points=dict(country_constructor=0x5113F0,country_read=0x511850,
                          infantry_list=0x511D16,unit_list=0x511E20,aircraft_list=0x511F56,
                          infantry_init_rank=0x517CE7,unit_ctor_rank=0x735604,
                          unit_init_rank=0x746827,aircraft_init_rank=0x413F98,
                          set_veteran=0x750090,get_cameo=0x712040,
                          stored_id_admission=0x410A60))


if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
