"""Original animation Image25 reader and image-loader fallback selection.

The ObjectType caller at 0x005F933B supplies its exact type section and current
Image as default. Cached INI indexes are supplied (no physical ART parsing), then
ReadString executes. The loader's original 0x00427B9F..0x00427BBC selects Image
or the literal type ID. This does not execute filename formatting or asset IO.
"""
import struct
from pathlib import Path
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP
from tools.native_oracle import run_checked, RET_MAGIC, finish_vectors, provenance
from tools.spatial_oracle.building_body_rules import Fixture, TYPE, INI, SP, SCRATCH, dwords


def generate():
    rows = []
    for name, raw in [
        (' FX', None), (' FX', 'REAL'), ('FX', 'WRONG'),
        ('TRAIL ', None), (' ', None), (' FX', ''), (' FX', '   '),
        ('TYPE', 'abcdefghijklmnopqrstuvwxyz1234'),
        ('TYPE', '  abcdefghijklmnopqrstuvwxyz1234'),
        ('TYPE', '\tMixed Image\t'), ('TYPE', 'none'),
    ]:
        f = Fixture()
        u = f.u
        f.ini(0x819420, raw)
        identity = name.encode('ascii') + b'\0'
        u.mem_write(TYPE + 0x24, identity)
        u.mem_write(SCRATCH + 0x7000, identity)
        f.write(INI + 4, TYPE + 0x24)
        u.mem_write(SP, dwords(RET_MAGIC, TYPE + 0x24, 0x819420,
                              SCRATCH + 0x7000, TYPE + 0x1F8, 25))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, INI)
        run_checked(u, 0x528A10, RET_MAGIC)
        length = u.reg_read(UC_X86_REG_EAX)
        image = bytes(u.mem_read(TYPE + 0x1F8, 25)).split(b'\0')[0].decode('ascii')
        assert u.reg_read(UC_X86_REG_ESP) == SP + 24
        u.reg_write(UC_X86_REG_ESI, TYPE)
        run_checked(u, 0x427B9F, 0x427BBC)
        pointer = u.reg_read(UC_X86_REG_EAX)
        selected = bytes(u.mem_read(pointer, 25)).split(b'\0')[0].decode('ascii')
        rows.append({'type': name, 'raw': raw, 'read_length': length,
                     'image': image, 'selected': selected})
    return rows


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='ObjectType Image25 native ReadString and AnimType image-loader fallback',
        assumptions=[
            'Fresh type ID is supplied as current Image default. Cached INI lookup indexes are supplied for the exact type section; no physical ART parse.',
            'Original caller 0x005F933B establishes capacity25, exact type section and current Image default; caller setup is supplied to ReadString.',
            'No image filename formatting, theater substitution, asset IO, ART read admission or rendering execution.'],
        substitutions=[], entry_points={'read_string': 0x528A10, 'loader_selection': 0x427B9F}))
