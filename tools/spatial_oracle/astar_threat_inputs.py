"""Original finishing coefficients, Team override and spatial-threat arithmetic.

Companion to astar_structural_height --threat-inputs. Scalar slices execute
original readers; supplied lexical caches are not full native Scenario loading.
"""
from pathlib import Path
import struct
from tools.native_oracle import SCRATCH, call, provenance, configured_gamemd, image_bytes, file_span
from tools.sidebar_oracle.stock import mix, mix_hash
from tools.spatial_oracle.astar_mtnk_inputs import RecordedInputs, sha
from tools.spatial_oracle.anytown_damage import navigation_inputs, next_family_native
from tools.projectile_oracle.bridge_render_inputs import assets_root, lexical
from tools.spatial_oracle.shrapnel_repair import retail_inputs as ri
from unicorn.x86_const import UC_X86_REG_ESI, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_EDI


def spatial_controls():
    """Execute complete original index and nine-slot adjustment bodies.

    Supplied House storage is an explicit scalar prestate, not a constructor
    or an object census. All nine targets and surrounding canaries are read
    back; signed overflow and negative clamping come from original execution.
    """
    indices=[]
    coord=SCRATCH+0x20
    for xy in ((0,0),(3,3),(4,4),(75,75),(76,75),(80,75),(511,511),
               (-1,-1),(-3,-3),(-4,-4),(-5,5),(-32768,32767),(32767,-32768)):
        result=call(0x56BC50,stack_args=[coord],writes={coord:struct.pack('<hh',*xy)},
                    required_addresses=(0x56BC50,0x56BC75))
        signed=struct.unpack('<i',struct.pack('<I',result['eax']))[0]
        indices.append(dict(coord=list(xy),index=signed))
    house=SCRATCH+0x100
    center=1000
    # Native immutable table8243C8; rows carry indices rather than reproducing
    # the arithmetic in Python. Original4FA2E0 still reads its own table.
    offsets=(-131,-130,-129,-1,0,1,129,130,131)
    cases=[]
    for amount in (0,1,2,3,4,5,7,100,-1,-3,-4,-100,2147483647,-2147483648):
        for initial in (0,2,2147483646):
            address=house+0x57E4+(center-132)*4
            before=struct.pack('<'+'i'*265,*([initial]*265))
            result=call(0x4FA2E0,ecx=house,stack_args=[center,amount & 0xffffffff],
                        writes={address:before},dumps={'region':(address,len(before)),
                                                       'text':(0x4FA2E0,0x68)},
                        required_addresses=(0x4FA2E0,0x4FA31B,0x4FA32A,0x4FA347))
            region=bytes.fromhex(result['dumps']['region'])
            assert bytes.fromhex(result['dumps']['text'])==file_span(image_bytes(),0x4FA2E0,0x68)[1]
            values=struct.unpack('<'+'i'*265,region)
            touched={offset+132 for offset in offsets}
            assert all(value==initial for i,value in enumerate(values) if i not in touched)
            cases.append(dict(amount=amount,initial=initial,index=center,
                              offsets=list(offsets),after=[values[i+132] for i in offsets],
                              canaries_intact=True,text_sha256=sha(bytes.fromhex(result['dumps']['text']))))
    assert len({r['text_sha256'] for r in cases})==1
    return dict(indices=indices,adjustments=cases)


def generate():
    m=RecordedInputs(next_family_native.theater(),assets_root()/'Hills.map')
    u=m.u
    text_before=sha(bytes(u.mem_read(0x401000,0x3E0000)))
    scenario=m.read32(0xA8B230)
    rngs=(0x886B88,scenario+0x218,0xABE890)
    for p in rngs:m.invoke(0x65C6D0,p,(0,))
    rng_before=[bytes(u.mem_read(p,1012)).hex() for p in rngs]
    readers=[]
    for value in ('0','0.5','5%','-0.01',None,'-0'):
        section={} if value is None else {'ThreatAvoidanceCoefficient':value}
        m.make_ini({'MTNK':section})
        before=bytes(u.mem_read(m.mtnk+0x2F0,8)).hex()
        m.block(0x712452,0x712473,{UC_X86_REG_EBP:m.mtnk,UC_X86_REG_ESI:ri.INI,UC_X86_REG_EBX:m.mtnk+0x24})
        after=bytes(u.mem_read(m.mtnk+0x2F0,8)).hex()
        readers.append(dict(value=value,before=before,after=after))
    # Execute the original TeamType constructor to establish +F2's default,
    # retaining native spare-capacity registry bookkeeping, not full loading.
    u.mem_write(0xA8ECA0,ri.dwords(0x7EB6D4,m.alloc(4096),1024,1,0,10))
    p=m.alloc(0x200)
    m.invoke(0x6F06E0,p,(m.cstring('FINISH_TEAM'),))
    ctor=bytes(u.mem_read(p+0xF2,1))[0]
    team_readers=[]
    # Keep the original eight controls unchanged, then exercise retained true
    # and false defaults on malformed/empty caches and explicit overrides.
    # A supplied empty lexical cache is not the physical INI parser: the Rust
    # production regression separately covers its omitted empty-value input.
    for value in (None,'yes',None,'no','1','0','true','false',
                  'yes','junk','2','on','',None,'no','junk','yes','no'):
        fields={} if value is None else {'AvoidThreats':value}
        m.make_ini({'FINISH_TEAM':fields})
        before=bytes(u.mem_read(p+0xF2,1))[0]
        # EAX is the preceding Group reader's return; only its unrelated
        # +9C store is reached alongside the exact AvoidThreats body.
        m.block(0x6F1371,0x6F1391,{UC_X86_REG_ESI:p,UC_X86_REG_EBX:ri.INI,UC_X86_REG_EDI:p+0x24})
        after=bytes(u.mem_read(p+0xF2,1))[0]
        team_readers.append(dict(value=value,before=before,after=after))
    retail=[]
    archive=(configured_gamemd().parent/'ra2md.mix').read_bytes()
    local=mix(archive)[mix_hash('localmd.mix')]
    raw=mix(local)[mix_hash('AIMD.INI')]
    listing,_=lexical(raw,{'TeamTypes'})
    names=list(listing['TeamTypes'].values())
    sections,_=lexical(raw,set(names))
    m.make_ini(sections)
    for name in names:
        # Reader current-value default is constructor false for each type.
        u.mem_write(p+0xF2,bytes([ctor]))
        m.block(0x6F1371,0x6F1391,{UC_X86_REG_ESI:p,UC_X86_REG_EBX:ri.INI,UC_X86_REG_EDI:m.cstring(name)})
        retail.append(dict(name=name,authored=sections.get(name,{}).get('AvoidThreats'),native=bytes(u.mem_read(p+0xF2,1))[0]))
    getter=[]
    actor,team,typ=SCRATCH+0x1000,SCRATCH+0x2000,SCRATCH+0x3000
    for bits in (0,0x8000000000000000,0x3fe0000000000000,0xbf847ae147ae147b):
        for flag in (None,0,1):
            result=call(0x4DC760,ecx=actor,writes={actor+0x530:struct.pack('<Q',bits),actor+0x5D4:ri.dwords(0 if flag is None else team),team+0x24:ri.dwords(typ),typ+0xF2:bytes([flag or 0])},capture_st0=True,required_addresses=(0x4DC760,))
            getter.append(dict(coefficient_bits=f'{bits:016x}',team_flag=flag,returned_bits=f"{result['st0_bits']:016x}"))
    rng_after=[bytes(u.mem_read(p,1012)).hex() for p in rngs]
    assert rng_before==rng_after and text_before==sha(bytes(u.mem_read(0x401000,0x3E0000)))
    return dict(schema_version=1,scope=__doc__,type_readers=readers,team_constructor_default=ctor,team_readers=team_readers,retail_aimd=dict(source='ra2md.mix/localmd.mix/AIMD.INI',outer_sha256=sha(archive),inner_sha256=sha(local),sha256=sha(raw),teams=retail),getter=getter,spatial=spatial_controls(),rng_before=rng_before,rng_after=rng_after,code_unchanged=True)


def metadata():
    return provenance(scope=__doc__,entry_points={'type_coefficient_reader':0x712452,'team_constructor':0x6F06E0,'team_avoid_reader':0x6F1371,'foot_coefficient_getter':0x4DC760,'threat_bucket_index':0x56BC50,'adjust_spatial_threat':0x4FA2E0},assumptions=['The existing actual Hills MTNK type/input owner supplies the original type constructor and scalar layer states. Custom type controls then execute the exact ReadDouble block with its retained default.','Original TeamType constructor uses supplied spare-capacity registries. AvoidThreats controls and every physical AIMD TeamTypes row execute its reached ReadBool slice, not full TeamType ReadINI or Scenario team creation. Malformed and supplied-empty cache controls retain the original caller default; physical empty INI omission is separately checked through the production Rust parser.','Foot getter controls supply raw Foot+530, nullable Team pointer and TeamType+F2; shared native_oracle.call observes original ST0 through its declared out-of-image FSTP capture. No contribution lifecycle or path output claim.','Original56BC50 executes13 signed packed-coordinate controls. Original4FA2E0 executes42 nine-slot adjustments with supplied initial House values, including signed overflow, INT_MIN negation and zero clamping. Untouched storage canaries and original executable bytes are asserted; no object census, constructor or incremental lifecycle is substituted by these arithmetic controls.'],substitutions=['Existing input owner allocator/TLS/file/lexical cache boundaries. No coefficient reader, constructor, RNG or getter return substitution.'])
