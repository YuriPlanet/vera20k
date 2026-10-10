"""Original HouseClass::SetDifficulty 0x004F6EC0 and its difficulty-row reader.

The existing72 ROF/team-timer controls now record all nine stored scalars and
explicit row, Country and GameSpeedBias priors. The ROF inputs/results and case
count remain unchanged. --readers composes the existing BulletReader INI/CRT
owner to execute ReadDifficulty66D270 over retained physical and control layers.
No gameplay callable, scalar parser or numeric operation is replaced.

Rust consumers: HouseState::set_difficulty and RulesPassProcessor.
"""
import argparse
import hashlib
import struct
from pathlib import Path

from tools.native_oracle import (SCRATCH, call, finish_vectors, image_bytes,
                                 image_sha256, provenance)

SET_DIFFICULTY = 0x4F6EC0
HOUSE, HOUSE_TYPE, RULES, TABLE = SCRATCH, SCRATCH + 0x6000, SCRATCH + 0x7000, SCRATCH + 0xF000
ONE = 0x3FF0000000000000
FIELDS = ('firepower', 'groundspeed', 'airspeed', 'armor', 'rof', 'cost',
          'build_time', 'repair_delay', 'build_delay')
COUNTRY_FIELDS = FIELDS[:7]


def f32d(value):
    """ReadDouble's value: the %f single widened to a double."""
    return struct.unpack('<Q', struct.pack('<d', struct.unpack('<f', struct.pack('<f', value))[0]))[0]


def qword(bits):
    return struct.pack('<Q', bits)


def dword(value):
    return struct.pack('<I', value & 0xFFFFFFFF)


# Retail rows ([Easy], [Normal], [Difficult]) from rulesmd.ini, in the row
# layout ReadDifficulty 0x0066D270 stores: FirePower, Groundspeed, Airspeed,
# Armor, ROF, Cost, BuildTime, RepairDelay, BuildDelay.
RETAIL_ROWS = [
    [ONE, f32d(1.0), f32d(1.0), f32d(1.2), f32d(.8), f32d(1.0), f32d(.8), f32d(.02), f32d(.03)],
    [ONE, f32d(1.0), f32d(1.0), f32d(1.0), f32d(1.0), f32d(1.0), f32d(1), f32d(.02), f32d(.03)],
    [ONE, f32d(1.0), f32d(1.0), f32d(.8), f32d(1.2), f32d(1.0), f32d(1.0), f32d(.05), f32d(.1)],
]


def run(case):
    rows = [[int(value, 16) for value in row] for row in case['row_values']]
    writes = {0x8871E0: dword(RULES), 0xA8B238: dword(case['mode']), 0xA8ED84: dword(100),
              HOUSE + 0x30: dword(case['array_index']), HOUSE + 0x34: dword(HOUSE_TYPE),
              HOUSE + 0x184: dword(7),
              RULES + 0x1418: qword(int(case['game_speed_bias'], 16)), RULES + 0x115C: dword(TABLE),
              TABLE: dword(11) + dword(22) + dword(33)}
    for index, name in enumerate(COUNTRY_FIELDS):
        writes[HOUSE_TYPE + 0xC8 + 8 * index] = qword(int(case['country_scalars'][name], 16))
    for index, row in enumerate(rows):
        writes[RULES + 0x1538 + index * 0x50] = b''.join(qword(value) for value in row)
    result = call(SET_DIFFICULTY, ecx=HOUSE, stack_args=[case['difficulty']], writes=writes,
                  dumps={'house': (HOUSE + 0x184, 0x50), 'team_timer': (HOUSE + 0x5798, 0xC)})
    house = bytes.fromhex(result['dumps']['house'])
    team_timer = bytes.fromhex(result['dumps']['team_timer'])
    return dict(input=case,
                difficulty=struct.unpack_from('<i', house, 0)[0],
                rof_bias=f"{struct.unpack_from('<Q', house, 0x1A8 - 0x184)[0]:016x}",
                biases={name: f'{struct.unpack_from("<Q", house, 4 + 8 * index)[0]:016x}'
                        for index, name in enumerate(FIELDS)},
                team_timer=[struct.unpack_from('<i', team_timer, 0)[0],
                            struct.unpack_from('<i', team_timer, 8)[0]])


# House array indexes, cycled over the cases; the last wraps the 175 product.
ARRAY_INDEXES = (0, 1, 7, 0x01000001)


def cases():
    for number, case in enumerate(rof_cases()):
        rows = [list(row) for row in RETAIL_ROWS]
        for index, value in enumerate(case['row_rof']):
            rows[index][4] = int(value, 16)
        # Other fields exercise the same whole native owner while preserving
        # the existing72 ROF inputs. Every supplied bit pattern is retained.
        if number % 3 == 1:
            for row in rows:
                for index, value in enumerate((.7, 1.3, -.5, 1.1, None, .9, 1.6, .015, .055)):
                    if value is not None:
                        row[index] = f32d(value)
        elif number % 3 == 2:
            for row in rows:
                for index in range(len(FIELDS)):
                    if index != 4:
                        row[index] = 0x3FEFFFFFFFFFFFFF
        country_values = (1.0, 1.1, .9, 1.3, 1.0, .7, 1.6)
        scalars = {name: f'{f32d(value if number % 2 else 1.0):016x}'
                   for name, value in zip(COUNTRY_FIELDS, country_values)}
        scalars['rof'] = case['country_rof']
        speed = (ONE, f32d(1.6), f32d(.7), 0x3FEFFFFFFFFFFFFF)[number % 4]
        yield dict(case, array_index=ARRAY_INDEXES[number % len(ARRAY_INDEXES)],
                   row_values=[[f'{value:016x}' for value in row] for row in rows],
                   country_scalars=scalars, game_speed_bias=f'{speed:016x}')


def rof_cases():
    retail = [f'{f32d(.8):016x}', f'{ONE:016x}', f'{f32d(1.2):016x}']
    odd = [f'{f32d(.7):016x}', f'{f32d(1.3):016x}', '0000000000000000',
           f'{f32d(-.5):016x}', '3fefffffffffffff']
    for mode in (5, 0):
        for country_rof in (f'{ONE:016x}', f'{f32d(1.1):016x}', '3fefffffffffffff',
                            f'{f32d(.9):016x}'):
            for difficulty in (0, 1, 2):
                yield dict(mode=mode, country_rof=country_rof, difficulty=difficulty,
                           row_rof=retail)
                yield dict(mode=mode, country_rof=country_rof, difficulty=difficulty,
                           row_rof=odd[:3])
                yield dict(mode=mode, country_rof=country_rof, difficulty=difficulty,
                           row_rof=odd[2:])


def generate():
    return [run(case) for case in cases()]


def reader_layers():
    from unicorn import UC_HOOK_CODE
    from unicorn.x86_const import UC_X86_REG_EDX, UC_X86_REG_ESP
    from tools.native_inspect import selected_ranges
    from tools.projectile_oracle.bridge_render_inputs import BulletReader
    from tools.rules_oracle.bridge_child_sound import sections
    from tools.spatial_oracle.building_body_rules import INI

    m = BulletReader({}, Path('ini'))
    original_code = selected_ranges(image_bytes(), None, None, code_only=True)
    names = ('Easy', 'Normal', 'Difficult')
    pointers = [m.alloc(0x50) for _ in names]
    for ptr in pointers:
        m.u.mem_write(ptr, b''.join(qword(f32d(2.5 + i)) for i in range(9)) + b'\x01\x00\x01')
    reads = []

    def observe(u, pc, size, data):
        if pc in (0x5283D0, 0x5295F0):
            sp = u.reg_read(UC_X86_REG_ESP)
            read = dict(reader=f'{pc:08x}', section=m.string(m.read32(sp + 4)),
                        key=m.string(m.read32(sp + 8)))
            read['default'] = (f'{struct.unpack("<Q", u.mem_read(sp + 12, 8))[0]:016x}'
                               if pc == 0x5283D0 else m.read32(sp + 12))
            reads.append(read)

    m.u.hook_add(UC_HOOK_CODE, observe)

    def state():
        result = {}
        for name, ptr in zip(names, pointers):
            raw = bytes(m.u.mem_read(ptr, 0x50))
            result[name] = dict(biases={field: f'{struct.unpack_from("<Q", raw, i * 8)[0]:016x}'
                                       for i, field in enumerate(FIELDS)},
                                flags=list(raw[0x48:0x4B]))
        return result

    raw = (Path('ini') / 'rulesmd.ini').read_bytes()
    physical = sections(raw)
    layers = [
        ('absent_initial', {}),
        ('physical_rules', {name: physical[name] for name in names}),
        ('absent_after_retail', {}),
        ('partial_normal', {'Normal': {'ROF': '1.5'}}),
        ('wrong_section_case', {'normal': {'ROF': '2.5'}}),
        ('wrong_key_case', {'Normal': {'rof': '2.5'}}),
        ('numeric_prefix_normal', {'Normal': {'FirePower': '1.5suffix', 'RepairDelay': '0.125suffix',
                                              'BuildDelay': '0.05', 'BuildTime': '-0.5'}}),
        ('stored_empty_section', {'Normal': {}}),
    ]
    rows = []
    for name, values in layers:
        before = state()
        m.make_ini(values)
        reads.clear()
        for label, ptr in zip(names, pointers):
            m.u.reg_write(UC_X86_REG_EDX, ptr)
            m.invoke(0x66D270, INI, (m.cstring(label),))
        rows.append(dict(name=name, sections=values, before=before, after=state(), reads=list(reads)))
        for span in original_code:
            assert bytes(m.u.mem_read(span['address'], len(span['bytes']))) == span['bytes']
    return dict(native_sha256=image_sha256(), rulesmd_sha256=hashlib.sha256(raw).hexdigest(), layers=rows)


def metadata(*, readers=False):
    result = provenance(
        scope=('Whole SetDifficulty body: all nine stored House doubles, literal difficulty '
               'index and wrapped team timer over the existing72 controls. Reader fixture '
               'separately executes full ReadDifficulty over retained rows and8 layers.'),
        assumptions=['x87 control word0x0E7F (PC53, chop), the shared harness default.',
                     'Rules difficulty rows, GameSpeedBias, TeamDelays11/22/33, Country scalar '
                     'priors, frame100, GameMode0/5 and House index are explicit inputs. '
                     'Some odd direct double inputs intentionally do not represent INI-read values.',
                     'Reader rows supply lexical/source cache storage using existing BulletReader '
                     'owner. Original ReadDouble/ReadBool and full66D270 execute; physical file '
                     'admission, complete Rules Process and gameplay consumers are excluded. '
                     'Malformed floating scans leave native stack storage undefined and are '
                     'excluded; defined numeric-prefix scans are retained.',
                     'Arithmetic storage is established here; no downstream speed/armor/cost '
                     'effect is implied merely by these stored scalars.'],
        substitutions=['Reader-only existing bounded allocation/CRT transport and lexical '
                       'INI cache storage; no numeric operation or gameplay callable is replaced.'],
        entry_points={'set_difficulty': SET_DIFFICULTY, 'read_difficulty': 0x66D270})
    result['reproduce_command'] = ("RA2_DIR='<retail-directory-containing-original-gamemd.exe>' "
        'python -m tools.spatial_oracle.house_difficulty' + (' --readers' if readers else '') + ' --check')
    if readers:
        result['rulesmd_sha256'] = hashlib.sha256((Path('ini') / 'rulesmd.ini').read_bytes()).hexdigest()
    return result


def source_paths():
    root = Path(__file__).resolve().parents[1]
    return {name: root / path for name, path in {
        'producer': 'spatial_oracle/house_difficulty.py', 'native_runner': 'native_oracle.py',
        'native_inspect': 'native_inspect.py', 'reader': 'projectile_oracle/bridge_render_inputs.py',
        'reader_base': 'rules_oracle/bridge_anim_inputs.py', 'reader_lists': 'rules_oracle/bridge_anim_lists.py',
        'fixture_base': 'spatial_oracle/building_body_rules.py', 'crc': 'projectile_oracle/flat_art.py',
        'lexical': 'rules_oracle/bridge_child_sound.py'}.items()}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--readers', action='store_true')
    args, remaining = parser.parse_known_args()
    target = Path(__file__).with_name('house_difficulty_readers.json') if args.readers else Path(__file__).with_suffix('.json')
    finish_vectors(reader_layers if args.readers else generate, target,
                   provenance=lambda: metadata(readers=args.readers), argv=remaining,
                   source_paths=source_paths())
