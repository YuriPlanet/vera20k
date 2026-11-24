"""Native Terrain Strength constructor, ObjectType integer read and -1 fallback."""
import struct
from pathlib import Path
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ESI, UC_X86_REG_ESP
from tools.native_oracle import finish_vectors, provenance, run_checked
from tools.spatial_oracle.building_body_rules import Fixture, TYPE, INI, RULES, SP, dwords


def execute(raw, tree_strength, read_success=True, prior_strength=None):
    f = Fixture()
    u = f.u
    u.reg_write(UC_X86_REG_ESI, TYPE)
    run_checked(u, 0x71DBAC, 0x71DBB6, count=10)
    initial = struct.unpack('<i', u.mem_read(TYPE + 0xA0, 4))[0]
    if prior_strength is not None:
        u.mem_write(TYPE + 0xA0, dwords(prior_strength))
    # Physical INIClass525A60 input omits empty values after strtrim <=0x20.
    # Keep the authored value in the corpus, but supply only loader-reachable
    # cached values to this reader slice. The section has another nonempty key.
    cached_raw = None if raw is None else raw.strip(''.join(map(chr, range(0x21)))) or None
    if read_success:
        f.ini(0x832B78, cached_raw)
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_EBX, TYPE)
        u.reg_write(UC_X86_REG_EBP, TYPE + 0x1F8)
        u.reg_write(UC_X86_REG_ESI, INI)
        run_checked(u, 0x5F94D3, 0x5F94F3, count=10000,
                    required_addresses=(0x5276D0, 0x5F94ED))
        assert u.reg_read(UC_X86_REG_ESP) == SP
    read_value = struct.unpack('<i', u.mem_read(TYPE + 0xA0, 4))[0]
    u.mem_write(0x8871E0, dwords(RULES))
    u.mem_write(RULES + 0x1144, dwords(tree_strength))
    u.reg_write(UC_X86_REG_ESI, TYPE)
    u.reg_write(UC_X86_REG_EAX, int(read_success))
    run_checked(u, 0x71DEC0, (0x71DEE2, 0x71E0B4), count=100)
    return dict(raw=raw, cached_raw=cached_raw, tree_strength=tree_strength, read_success=read_success,
                prior_strength=prior_strength, constructor_strength=initial, after_integer_read=read_value,
                strength=struct.unpack('<i', u.mem_read(TYPE + 0xA0, 4))[0])


def generate():
    values = [None, '', ' ', '-1', '0', '1', '200', '-27', '$ff', 'ffh',
              '12junk', '-1junk', 'junk', '2147483647', '-2147483648',
              '2147483648', '4294967295']
    sequences = []
    for raws in ((None, None), ('-1', None), (None, '-1')):
        first = execute(raws[0], 200)
        second = execute(raws[1], 375, prior_strength=first['strength'])
        sequences.append(dict(passes=[first, second]))
    return dict(cases=[execute(raw, general) for raw in values for general in (200, 0, -375, 375)]
                + [execute(None, 200, False)], sequences=sequences)


def metadata():
    return provenance(
        scope='69 bounded native Terrain Strength initialization rows: ctor store71DBAC, ObjectType Strength read/store5F94D3..5F94F3 and successful-read sentinel fallback71DEC0..71DEE2; one failed-base-reader gate, three two-pass retention characterizations. Not complete Terrain/Rules parsing or complete scenario processing.',
        assumptions=[
            'Supplied cached INI section/key indexes built with original CRC, standing in for file loading. Physical raw values are trimmed at ASCII<=0x20 and empty values omitted before populating the cache, following existing INIClass525A60 loader evidence. Original5276D0 ReadInt executes. The supplied section retains another nonempty key. Fresh TerrainType Strength initialized by original store to-1.',
            'Rules TreeStrength+1144 supplied as explicit resolved200,0,-375,375. This does not establish Rules constructor default or its General reader. ObjectType reader-success AL is supplied at the Terrain suffix boundary.',
        ],
        substitutions=['No code hooks or patches. The complete base ObjectType reader is replaced by its Strength read/store slice plus declared success gate.'],
        entry_points={'terrain_ctor_strength':0x71DBAC,'object_strength_read':0x5F94D3,
                      'read_int':0x5276D0,'terrain_strength_fallback':0x71DEC0},
    )


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
