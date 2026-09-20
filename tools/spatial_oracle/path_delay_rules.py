"""Original [AI] path-delay readers and each original delay conversion site.

INI lookup indexes are supplied. Original readers, decimal CRT conversion and
all FLD/FMUL/_ftol slices execute unchanged. No locomotor body is certified.
"""
import hashlib
import struct
from pathlib import Path
from unicorn.x86_const import *
from tools.native_oracle import run_checked, finish_vectors, provenance, RET_MAGIC
from tools.spatial_oracle.building_body_rules import Fixture, INI, RULES, SP

AI = 0x839DA4
PATH_KEY = 0x83D328
BLOCK_KEY = 0x83D314
PATH_DEFAULT = '3f90624dd2f1a9fc'
CONVERSIONS = (0x4B2850,0x4B3A65,0x4D4022,0x516696,0x5168DC,0x5B0286,
    0x5B0CAE,0x6A1EA0,0x6A30B4,0x75AF6F,0x75B98C)
SPANS = [(0x6673CD,0x6673D2),(0x66760E,0x667628),
    (0x6739E5,0x673A37),(0x5283D0,0x52859B),(0x7C8F5E,0x7C8F96)] + [(a,a+17) for a in CONVERSIONS]


class DelayFixture(Fixture):
    def __init__(self):
        super().__init__()
        self.write(SP,RET_MAGIC)
        self.u.reg_write(UC_X86_REG_ESP,SP)
        run_checked(self.u,0x7C8F5E,RET_MAGIC)

    def parse_path(self, raw, initial):
        self.ini(PATH_KEY,raw)
        self.write(INI+4,AI)
        u=self.u
        u.mem_write(RULES+0x1760,struct.pack('<Q',int(initial,16)))
        u.reg_write(UC_X86_REG_ESI,RULES)
        u.reg_write(UC_X86_REG_EDI,INI)
        u.reg_write(UC_X86_REG_ESP,SP)
        run_checked(u,0x6739E5,0x673A0C)
        return f"{struct.unpack('<Q',u.mem_read(RULES+0x1760,8))[0]:016x}"

    def defaults(self):
        self.u.reg_write(UC_X86_REG_ESI,RULES)
        run_checked(self.u,0x6673CD,0x6673D2)
        run_checked(self.u,0x66760E,0x667628)
        return dict(path_bits=f"{struct.unpack('<Q',self.u.mem_read(RULES+0x1760,8))[0]:016x}",
            blockage=struct.unpack('<i',self.u.mem_read(RULES+0x1768,4))[0])

    def parse_block(self, raw, initial):
        self.ini(BLOCK_KEY,raw)
        self.write(INI+4,AI)
        u=self.u
        self.write(RULES+0x1768,initial)
        u.reg_write(UC_X86_REG_ESI,RULES)
        u.reg_write(UC_X86_REG_EDI,INI)
        u.reg_write(UC_X86_REG_ESP,SP)
        run_checked(u,0x673A0C,0x673A37)
        return struct.unpack('<i',u.mem_read(RULES+0x1768,4))[0]

    def convert(self, bits, entry):
        u=self.u
        u.mem_write(RULES+0x1760,struct.pack('<Q',int(bits,16)))
        u.reg_write(UC_X86_REG_EAX,RULES)
        u.reg_write(UC_X86_REG_ECX,RULES)
        u.reg_write(UC_X86_REG_EDX,RULES)
        u.reg_write(UC_X86_REG_ESP,SP)
        run_checked(u,entry,entry+17)
        return struct.unpack('<i',struct.pack('<I',u.reg_read(UC_X86_REG_EAX)))[0]


def generate():
    f=DelayFixture()
    paths=[]
    for initial in (PATH_DEFAULT,'3fb999999999999a'):
        for raw in (None,'','0','-0','0.01','0.016','0.1','0.0011111111111111',
                '-0.01','1%','-1%','0.25junk','1e-3','2147483648','-2147483649',
                '1e30','nan','inf','junk'):
            bits=f.parse_path(raw,initial)
            converted=[f.convert(bits,entry) for entry in CONVERSIONS]
            assert len(set(converted))==1
            paths.append(dict(raw=raw,initial_bits=initial,output_bits=bits,ticks=converted[0]))
    blocks=[]
    for initial in (60,-7):
        for raw in (None,'','0','-1','65535','65536','2147483648','4294967295',
                '4294967296','$FFFFFFFF','FFFFFFFFh','junk'):
            blocks.append(dict(raw=raw,initial=initial,output=f.parse_block(raw,initial)))
    conversions=[]
    for bits in ('0000000000000000','8000000000000000',PATH_DEFAULT,
            '3f847ae147ae147b','bf847ae147ae147b','7ff8000000000001',
            '7ff0000000000000','fff0000000000000','41323456789abcde',
            'c1323456789abcde','7fefffffffffffff'):
        outputs=[f.convert(bits,entry) for entry in CONVERSIONS]
        assert len(set(outputs))==1
        conversions.append(dict(bits=bits,ticks=outputs[0]))
    return dict(schema_version=1,defaults=f.defaults(),path_delay=paths,blockage_path_delay=blocks,
        conversion_sites=[f'{a:08X}' for a in CONVERSIONS],conversions=conversions)


def metadata():
    f=DelayFixture()
    result=provenance(scope='Rules [AI] PathDelay and BlockagePathDelay retained readers; eleven original PathDelay*900 conversion slices',
        assumptions=['Supplied INI indexes; original CRT floating-scanner initializer7C8F5E executes first; original ReadDouble5283D0/ReadInt5276D0 and parser call/store instructions execute',
            'PC53/chop, masked exceptions. Binary64 retained field becomes signedlow32 of original _ftol signed64 conversion',
            'Missing defaults remain native double0.016 and signed60; supplied prior values exercise retained reads',
            'ReadDouble52854D passes its section argument slot to sscanf as float output; failed conversion exposes section-pointer bits00839DA4 widened from float. Malformed/empty rows describe this exact AI caller ABI; Rust shared reader deliberately returns zero for malformed non-retail inputs. No portable arbitrary-section malformed-input claim',
            'Conversion slices stop immediately after _ftol return, before timer store. No whole locomotor or timer aging claim',
            'Raw NaN/infinity conversion rows are supplied retained data, not claims of authored parser reachability'],
        substitutions=[],entry_points={'read_path':0x6739E5,'read_blockage':0x673A0C})
    result['original_slices']=[dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',
        hex=bytes(f.u.mem_read(a,b-a)).hex(),sha256=hashlib.sha256(bytes(f.u.mem_read(a,b-a))).hexdigest()) for a,b in SPANS]
    return result


if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
