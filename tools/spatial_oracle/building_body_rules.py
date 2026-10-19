"""Original BuildingType body range readers and asset-bound construction range.

Supplied INI lookup indexes stand in for file loading, not the native readers.
Original sprintf/ReadString/sscanf and integer/bool readers execute unchanged.
The construction slice starts after the asset lookup and receives a supplied SHP
pointer/header; it does not claim filename resolution, theater selection or IO.
"""
import hashlib
import struct
import sys
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import *
from tools.native_oracle import (load_image, run_checked, call, SCRATCH,
    STACK_BASE, STACK_SIZE, RET_MAGIC, finish_vectors, provenance)

TYPE = SCRATCH
SECTION = SCRATCH + 0x3000
INDEX = SCRATCH + 0x3100
ENTRY = SCRATCH + 0x3200
RAW = SCRATCH + 0x4000
SHP = SCRATCH + 0x5000
RULES = SCRATCH + 0x6000
SP = STACK_BASE + 0x80000
INI = 0x887180
TRIPLES = [(0x4615CA, 0x46164A, 0x81A600, 0xF10),
           (0x46164A, 0x4616C4, 0x81A5F4, 0xF1C),
           (0x4616C4, 0x46173E, 0x81A5E8, 0xF34),
           (0x46173E, 0x4617B8, 0x81A5DC, 0xF40)]
SPANS = [(0x45DD90,0x45DDA6),(0x45E27F,0x45E284),
         (0x45E13A,0x45E158),(0x45E1F0,0x45E1FA),
         (0x45E362,0x45E3BC),(0x460AC0,0x460AE0),
         (0x4612D1,0x4612EE),(0x4615CA,0x4617B8),
         (0x45F2AA,0x45F310),(0x45EAA5,0x45EADA)]


def dwords(*values):
    return b''.join(struct.pack('<I', v & 0xFFFFFFFF) for v in values)


class Fixture:
    def __init__(self):
        self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(self.u)
        self.u.mem_map(SCRATCH, 0x10000)
        self.u.mem_map(STACK_BASE, STACK_SIZE)
        self.u.mem_map(RET_MAGIC, 0x1000)
        self.u.reg_write(UC_X86_REG_FPCW, 0x0E7F)
        self.u.mem_write(TYPE + 0x1F8, b'TEST\0')

    def write(self, address, value):
        self.u.mem_write(address, dwords(value))

    def ini(self, key, raw):
        u = self.u
        name = bytes(u.mem_read(key, 64)).split(b'\0')[0]
        crc = call(0x4A1DE0, ecx=SCRATCH, stack_args=[SCRATCH+0x100,len(name)],
            writes={SCRATCH:bytes(16), SCRATCH+0x100:name})['eax']
        u.mem_write(INI, bytes(0x40))
        self.write(INI+4, TYPE+0x1F8)
        self.write(INI+8, SECTION)
        self.write(SECTION+0x2C, INDEX)
        self.write(SECTION+0x30, int(raw is not None))
        self.write(SECTION+0x38, 1)
        self.write(SECTION+0x3C, INDEX)
        self.write(INDEX, crc)
        self.write(INDEX+4, ENTRY)
        self.write(ENTRY+0x10, RAW)
        u.mem_write(RAW, (raw or '').encode('ascii')+b'\0')

    def triple(self, raw, initial, which):
        u=self.u
        begin,end,key,offset=TRIPLES[which]
        self.ini(key,raw)
        u.mem_write(TYPE+offset, dwords(*initial))
        u.reg_write(UC_X86_REG_ESP,SP)
        u.reg_write(UC_X86_REG_EBP,TYPE)
        u.reg_write(UC_X86_REG_EDI,TYPE+0x1F8)
        run_checked(u,begin,end)
        return list(struct.unpack('<iii',u.mem_read(TYPE+offset,12)))

    def scalar(self, raw, key, default, fn):
        self.ini(key,raw)
        u=self.u
        u.mem_write(SP,dwords(RET_MAGIC,TYPE+0x1F8,key,default))
        u.reg_write(UC_X86_REG_ESP,SP)
        u.reg_write(UC_X86_REG_ECX,INI)
        run_checked(u,fn,RET_MAGIC)
        value=u.reg_read(UC_X86_REG_EAX)
        return struct.unpack('<i',dwords(value))[0] if fn==0x5276D0 else bool(value & 255)

    def buildup(self, count, gate, stages, speed_bits):
        u=self.u
        u.mem_write(TYPE+0xF04,dwords(0,1,0))
        u.mem_write(TYPE+0x16B7,bytes([gate]))
        self.write(TYPE+0x16F8,stages)
        u.mem_write(SHP+6,struct.pack('<H',(count or 0)&65535))
        self.write(0x8871E0,RULES)
        u.mem_write(RULES+0x1518,struct.pack('<Q',int(speed_bits,16)))
        u.reg_write(UC_X86_REG_ESP,SP)
        u.reg_write(UC_X86_REG_EBP,TYPE)
        u.reg_write(UC_X86_REG_EBX,0)
        u.reg_write(UC_X86_REG_EAX,0 if count is None else SHP)
        run_checked(u,0x45F2AA,0x45F310)
        return list(struct.unpack('<iii',u.mem_read(TYPE+0xF04,12)))

    def light_constructor(self):
        """Original literal stores; the wider Type constructor is excluded."""
        u = self.u
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ESI, TYPE)
        u.reg_write(UC_X86_REG_EBX, 0)
        run_checked(u, 0x45DE12, 0x45DE39)
        return list(struct.unpack('<5i', u.mem_read(TYPE + 0xE30, 20)))

    def light_pass(self, values, *, admitted=True):
        """Real readers on cached lexical keys; original per-field defaults.

        Each of the five contiguous read/store regions uses the existing
        single-key cache transport. No parsed value or expected integer is
        supplied. A missing section instead executes the complete45FE50
        rejection path, including AbstractType's actual HasSection.
        """
        u = self.u
        before = list(struct.unpack('<5i', u.mem_read(TYPE + 0xE30, 20)))
        if not admitted:
            u.mem_write(TYPE + 0x24, b'TEST\0')
            u.mem_write(INI, bytes(0x40))
            u.mem_write(SP, dwords(RET_MAGIC, INI))
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_ECX, TYPE)
            run_checked(u, 0x45FE50, RET_MAGIC, required_addresses=(0x410A60, 0x526810))
            assert not (u.reg_read(UC_X86_REG_EAX) & 255)
        else:
            for key, key_address, begin, end in LIGHT_READS:
                # The parser drops empty/whitespace authored values. Its
                # physical input handling is checked by the Rust regression.
                token = values.get(key)
                if token is not None and not token.strip():
                    token = None
                self.ini(key_address, token)
                for register, value in (
                    (UC_X86_REG_ESP, SP), (UC_X86_REG_EBP, TYPE),
                    (UC_X86_REG_ESI, INI), (UC_X86_REG_EBX, TYPE + 0x1F8),
                ):
                    u.reg_write(register, value)
                run_checked(u, begin, end,
                    required_addresses=(0x5276D0,) if key == 'LightVisibility'
                    else (0x5283D0, 0x7C5F00))
                assert u.reg_read(UC_X86_REG_ESP) == SP
        return dict(authored_keys=values, section_present=admitted, before=before,
                    after=list(struct.unpack('<5i', u.mem_read(TYPE + 0xE30, 20))))


LIGHT_READS = (
    ('LightVisibility', 0x81A92C, 0x460C8C, 0x460CA6),
    ('LightIntensity', 0x81A91C, 0x460CA6, 0x460CEF),
    ('LightRedTint', 0x81A90C, 0x460CEF, 0x460D38),
    ('LightGreenTint', 0x81A8FC, 0x460D38, 0x460D81),
    ('LightBlueTint', 0x81A8EC, 0x460D81, 0x460DCA),
)


def generate_lights():
    f = Fixture()
    u = f.u
    # Native startup installs the floating scanner before ReadDouble.
    u.mem_write(SP, dwords(RET_MAGIC))
    u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(u, 0x7C8F5E, RET_MAGIC)
    constructor = f.light_constructor()
    rows = []
    cases = [
        ('stock_green', dict(LightVisibility='4000', LightIntensity='.01',
                            LightRedTint='.01', LightGreenTint='1.5', LightBlueTint='.01'), True),
        ('missing_section_retains_green', {}, False),
        ('storm_sound_only', dict(WorkingSound='_Amb_BirdsTemperate2'), True),
        ('explicit_light_override', dict(LightVisibility='2048', LightIntensity='.75',
                                        LightRedTint='-.25', LightGreenTint='0,01', LightBlueTint='1.5'), True),
        ('empty_authored_keys_are_omitted', dict(WorkingSound='keeps_section_present', LightIntensity='', LightRedTint=' \t ',
                                              LightGreenTint='', LightBlueTint=''), True),
        ('negative_fractional_intensity', dict(LightIntensity='-.5'), True),
        ('missing_negative_fraction_truncates_to_zero', dict(WorkingSound='ignored'), True),
        ('percent_and_negative_visibility', dict(LightVisibility='-25', LightIntensity='12.5%',
                                               LightRedTint='250%', LightGreenTint='50%', LightBlueTint='-50%'), True),
        ('wrong_key_case', dict(lightvisibility='999', lightintensity='999',
                               lightredtint='999', lightgreentint='999', lightbluetint='999'), True),
        ('signed_intensity_low_dword', dict(LightIntensity='2147483.75'), True),
        ('small_biased_fraction', dict(LightIntensity='.000989'), True),
        ('explicit_zero', dict(LightIntensity='0', LightVisibility='0'), True),
    ]
    for name, values, admitted in cases:
        rows.append(dict(name=name, **f.light_pass(values, admitted=admitted)))
    # The literal constructor tint must also survive an admitted section
    # whose nonzero intensity is explicit but all three tint keys are absent.
    assert f.light_constructor() == constructor
    rows.append(dict(name='constructor_tints_with_explicit_intensity',
                     reset_constructor=True, **f.light_pass(dict(LightIntensity='.2'))))
    assert f.light_constructor() == constructor
    rows.append(dict(name='stock_snow_day', reset_constructor=True,
                     **f.light_pass(dict(LightVisibility='3000', LightIntensity='0.005',
                                         LightRedTint='0.1', LightGreenTint='0.1', LightBlueTint='0.00'))))
    rows.append(dict(name='snow_day_sound_only',
                     **f.light_pass(dict(WorkingSound='_Amb_BirdsHarbor'))))
    return dict(schema_version=1, constructor=constructor, rows=rows)


def light_metadata():
    return provenance(
        scope='Original BuildingType radius-light constructor stores, all five reached read/store blocks and complete missing-section reader rejection',
        assumptions=[
            'Constructor literal region45DE12..45DE39 executes with supplied Type storage and the constructor established EBX0; whole Type construction, allocation and numeric identity are excluded.',
            'Reached sections execute five original contiguous read/store regions in order using existing Fixture single-key cached INI lookup transport; no whole BuildingType reader or native physical map/rules loader is claimed for these admitted sections.',
            'Native ReadInt5276D0 and ReadDouble5283D0, signed default division, original floating scanner and Math::ftol execute. FPCW0E7F is supplied. Authored empty/whitespace cache entries are omitted as declared inputs, separately checked through the production Rust parser.',
            'Missing-section control executes whole45FE50, Techno712170, Object5F92D0 and Abstract410A60 rejection with an empty INI cache and supplied TEST identity. It establishes retention independently of the selected scalar cuts.',
            'stock_green and stock_snow_day lexical values match the installed RULESMD INGRNLMP and SNODAYLAMP fields; the latter has0.005 intensity, rather than the other selected lamps0.01. The sound-only controls use the map key shape. Retail-layer integration is checked separately by the Rust production reader. No source intensity/tint arithmetic is replaced by host results.',
            'LightSource allocation/activation, distance sampling, palette conversion, rendering, complete Scenario startup and whole-map parity are excluded. No gameplay RNG, timer or detach body is passed by these selected reads.',
        ],
        substitutions=[
            'Existing cached INI section/entry storage and native CRC lookup transport only; no reader, default, quantization or stored-field result is substituted.',
        ],
        entry_points=dict(constructor=0x45DE12, read_begin=0x460C8C, read_end=0x460DCA,
                          building_reader=0x45FE50, has_section=0x526810,
                          read_int=0x5276D0, read_double=0x5283D0, ftol=0x7C5F00))


def generate():
    f=Fixture()
    triples=[]
    values=[None,'','1,2,3','-4,-5,-6','2147483648,4294967295,4294967296',
        '99999999999999999999999999999999999999999999999999999999999999999999,2,3',
        '1','1,','1,,3','1,2x,3','1 ,2,3','1, 2, 3','+,-1,2','x,2,3',
        '1,0x10,3','1,\t-2,3','1,\x1f2,3','1,2,3junk','1,2,3,4',
        '"1,2,3"',' 1,2,3 ',' '*62+'1,2,3','\v1,2,3','1,\v2,3','1,\f2,3']
    for initial in ([0,1,0],[7,-9,11]):
        for raw in values:
            outputs=[f.triple(raw,initial,n) for n in range(4)]
            assert all(x==outputs[0] for x in outputs)
            triples.append(dict(raw=raw,initial=initial,output=outputs[0]))
    scalars=[]
    for raw in (None,'','9','-1','2147483648','4294967295','$FFFFFFFF','65535','junk'):
        scalars.append(dict(raw=raw,output=f.scalar(raw,0x81A6F4,9,0x5276D0)))
    flags=[]
    for raw in (None,'','yes','no','true','false','1','0','junk'):
        flags.append(dict(raw=raw,output=f.scalar(raw,0x81AA20,0,0x5295F0)))
    buildup=[]
    for count in (None,0,1,2,23,24,32767,32768,65535):
        for gate,stages in ((False,9),(True,9),(True,-1),(True,2147483647)):
            bits='3fd3333333333333'
            buildup.append(dict(count=count,gate=gate,stages=stages,speed_bits=bits,
                output=f.buildup(count,gate,stages,bits)))
    return dict(schema_version=1,triples=triples,gate_stages=scalars,firestorm_wall=flags,buildup=buildup)


def metadata():
    f=Fixture()
    result=provenance(scope='BuildingType art body triples, GateStages, FirestormWall and ordinary buildup asset-count slice',
        assumptions=['Supplied INI section/entry indexes with native CRC; cached section pointer matches supplied TEST type name',
          'Original ReadString64, sprintf and sscanf execute for all four animation triplets',
          'Buildup count is supplied raw SHP header16, pointer presence supplied after asset lookup',
          'Constructor triple defaults0,1,0 from EBX0 at45DD9A and EBP1 at45E27F; GateStages9 at45E1F0; FirestormWall0 at45E14B',
          'Buildup ordinary45F230 path only; alternate theater45EAA5 full-frame-count path is recorded but not executed or unified'],
        substitutions=[],entry_points={'triples':0x4615CA,'integer_reader':0x5276D0,'bool_reader':0x5295F0,'buildup':0x45F2AA})
    result['original_slices']=[dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',hex=bytes(f.u.mem_read(a,b-a)).hex(),
        sha256=hashlib.sha256(bytes(f.u.mem_read(a,b-a))).hexdigest()) for a,b in SPANS]
    return result


if __name__=='__main__':
    if '--lights' in sys.argv:
        finish_vectors(generate_lights, Path(__file__).with_name('building_light_fields.json'),
                       provenance=light_metadata,
                       argv=[arg for arg in sys.argv[1:] if arg != '--lights'])
    else:
        finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
