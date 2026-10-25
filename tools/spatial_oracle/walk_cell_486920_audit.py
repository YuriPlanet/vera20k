"""Bounded original Cell486920 retail gates and Infantry +37C defaults.

Run from the VERA20k repo with PYTHONPATH=. and VERA20K_GAMEMD_EXE;
VERA20K_PROJECTILE_RENDER_ASSETS selects extracted RULESMD/MPBattleMD/Hills.
Default --check reproduces the saved evidence; --write records it.
No original instructions are patched. See the adjacent Markdown for limits.
"""
from pathlib import Path
import hashlib
import struct
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *
from tools.native_oracle import RET_MAGIC, run_checked, finish_vectors, provenance
from tools.rules_oracle.bridge_anim_inputs import Reader
from tools.rules_oracle.bridge_child_sound import Sound
from tools.rules_oracle.infantry_speed_type import SpeedReader
from tools.projectile_oracle.bridge_render_inputs import assets_root, lexical
from tools.spatial_oracle.building_body_rules import INI, SP, dwords

ROOT = assets_root()
LAYERS = ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'Hills.map')


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def physical(section):
    result = []
    for name in LAYERS:
        path = ROOT / name
        if not path.exists():
            assert name == 'LANGRULE.INI'
            result.append(dict(file=name, absent=True))
            continue
        raw = path.read_bytes()
        sections, lines = lexical(raw, {section})
        result.append(dict(file=name, bytes=len(raw), sha256=sha(raw),
                           sections=sections, source_lines=lines))
    return result


def frame(m, **registers):
    m.u.mem_write(SP, dwords(RET_MAGIC))
    m.u.reg_write(UC_X86_REG_ESP, SP)
    for register, value in registers.items():
        m.u.reg_write(globals()['UC_X86_REG_' + register.upper()], value)


class Registry(Sound):
    """Only borrow the original-order INI-link fixture, not sound seams."""
    def __init__(self, sections):
        Reader.__init__(self, ROOT, sections)

    hook = Reader.hook


def overlay_registry():
    raw = (ROOT / 'RULESMD.INI').read_bytes()
    sections, lines = lexical(raw, {'OverlayTypes'})
    m = Registry(sections)
    m.u.mem_write(0xA83D80, dwords(0x7EB6D4, m.alloc(256 * 4), 256, 1, 0, 10))
    frame(m, esi=INI)
    run_checked(m.u, 0x668CE3, 0x668D34, count=6000000,
                required_addresses=(0x526CC0, 0x5FEC70, 0x5FE250))
    table = m.read32(0xA83D84)
    rows = []
    for index in range(m.read32(0xA83D90)):
        typ = m.read32(table + 4 * index)
        rows.append(dict(index=index, name=m.string(typ + 0x24),
                         stored_index=m.read32(typ + 0x294)))
    assert len(rows) == 250
    assert [rows[i]['name'] for i in (24, 25, 122, 123, 124, 125, 126)] == [
        'BRIDGE1', 'BRIDGE2', 'LOBRDGE1', 'LOBRDGE2', 'LOBRDGE3', 'LOBRDGE4', 'DUMMYOLD']
    return dict(rules_sha256=sha(raw), physical_sections=sections,
                source_lines=lines, registry=rows)


def engineer_immunity():
    # This shared fixture runs full original InfantryType5236A0 and base
    # constructors. Its SpeedType observations do not supply the C91 result.
    m = SpeedReader()
    constructor = m.u.mem_read(m.typ + 0xC91, 1)[0]
    assert constructor == 0
    layers = physical('ENGINEER')
    for row in layers:
        if row.get('absent'):
            continue
        m.make_ini(row['sections'])
        before = m.u.mem_read(m.typ + 0xC91, 1)[0]
        frame(m, ebp=m.typ, ebx=m.typ + 0x24, edi=INI)
        run_checked(m.u, 0x714C23, 0x714C44, count=200000,
                    required_addresses=(0x5295F0, 0x714C3E))
        row.update(before=before, after=m.u.mem_read(m.typ + 0xC91, 1)[0])
    assert [r['after'] for r in layers if not r.get('absent')] == [1, 1, 1]
    return dict(type_name=m.string(m.typ + 0x24), key=m.string(0x8438CC),
                constructor=constructor, layers=layers)


def vein_rule():
    m = Reader(ROOT, {})
    rules = m.alloc(0x2000)
    m.u.mem_write(rules, bytes([0xA5]) * 0x2000)
    frame(m, ecx=rules)
    run_checked(m.u, 0x665650, 0x6657C3, count=200000,
                required_addresses=(0x665663, 0x6657BD))
    constructor = m.read32(rules + 0xE8)
    assert constructor == 0
    layers = physical('AudioVisual')
    for row in layers:
        if row.get('absent'):
            continue
        m.make_ini(row['sections'])
        before = m.read32(rules + 0xE8)
        frame(m, esi=rules, edi=INI)
        run_checked(m.u, 0x6692E4, 0x66932C, count=200000,
                    required_addresses=(0x528A10, 0x669326))
        row.update(before=before, after=m.read32(rules + 0xE8))
    assert [r['after'] for r in layers if not r.get('absent')] == [0, 0, 0]
    return dict(key=m.string(0x83AB7C), constructor=constructor, layers=layers)


def emp_predicate():
    m = Reader(ROOT, {})
    obj = m.alloc(0x800)
    m.u.mem_write(obj, bytes([0xA5]) * 0x800)
    # Start after Radio base construction, retaining the actual original XOR
    # and all subsequent Techno initialization through its +504 write.
    frame(m, esi=obj)
    run_checked(m.u, 0x6F2B4B, 0x6F3118, count=200000,
                required_addresses=(0x6F3112,))
    constructor = m.read32(obj + 0x504)
    assert constructor == 0
    infantry_receiver = m.read32(0x7EB058 + 0x37C)
    assert infantry_receiver == 0x70EFD0
    rows = []
    for value in (0, 1, 255, -1, 2147483647, -2147483648):
        m.u.mem_write(obj + 0x504, struct.pack('<i', value))
        result = m.invoke(infantry_receiver, obj)
        rows.append(dict(supplied_signed_emp_timer=value, result=result))
    assert [r['result'] for r in rows] == [0, 1, 1, 0, 1, 0]
    return dict(infantry_receiver=hex(infantry_receiver), constructor=constructor,
                controls=rows)


def cell_early_gates():
    m = Reader(ROOT, {})
    cell = m.alloc(0x200)
    calls = []
    watched = (0x48695C, 0x48697A, 0x4869BD, 0x4869CF, 0x421EA0)
    def trace(u, pc, _size, _data):
        if pc in watched:
            calls.append(hex(pc))
    m.u.hook_add(UC_HOOK_CODE, trace)
    rows = []
    cases = [(f'overlay_{index}', index, 255, 0, 0, 0xDEADC000)
             for index in (-1, 24, 25, 122, 123, 124, 125)]
    cases += [('density_47', 126, 47, 0, 0, 0xDEADC000),
              ('slope_1', 126, 48, 1, 0, 0xDEADC000),
              ('already_latched', 126, 48, 0, 0x20000, 0xDEADC000),
              ('empty_ground_list', 126, 48, 0, 0, 0)]
    for name, overlay, density, slope, flags, ground in cases:
        m.u.mem_write(cell, bytes(0x200))
        m.u.mem_write(cell + 0x44, struct.pack('<i', overlay))
        m.u.mem_write(cell + 0x11E, bytes([density]))
        m.u.mem_write(cell + 0x11C, bytes([slope]))
        m.u.mem_write(cell + 0x140, dwords(flags))
        m.u.mem_write(cell + 0xE4, dwords(ground))
        before = bytes(m.u.mem_read(cell, 0x200))
        calls.clear()
        m.invoke(0x486920, cell)
        unchanged = before == bytes(m.u.mem_read(cell, 0x200))
        assert unchanged
        assert calls == (['0x48695c'] if name == 'empty_ground_list' else [])
        rows.append(dict(case=name, supplied_overlay=overlay,
                         supplied_density=density, supplied_slope=slope,
                         supplied_flags=flags, ground_list_present=bool(ground),
                         cell_unchanged=unchanged, reached=list(calls)))
    return rows


def generate():
    return dict(schema_version=1,
                overlay_registry=overlay_registry(), engineer_immunity=engineer_immunity(),
                vein_attack_rule=vein_rule(), emp=emp_predicate(),
                cell_early_gates=cell_early_gates())


def metadata():
    return provenance(scope=__doc__,
        entry_points={'overlay_rules_loop': 0x668CE3, 'overlay_find_or_create': 0x5FEC70,
                      'overlay_constructor': 0x5FE250, 'infantry_type_constructor': 0x5236A0,
                      'immune_to_veins_reader': 0x714C23, 'rules_ctor_prefix': 0x665650,
                      'vein_attack_reader': 0x6692E4, 'techno_ctor_prefix': 0x6F2B4B,
                      'infantry_emp_predicate': 0x70EFD0, 'cell_effect': 0x486920},
        assumptions=[
            'Physical unique lexical section/key strings prepare native CRC INI caches; the full archive/file loader is outside this boundary. OverlayTypes additionally preserves original entry links and runs all250 registry allocations in physical declaration order. Numeric INI keys are not registry indices.',
            'Full original InfantryType constructor executes; C91 field reader blocks run with supplied original reader registers. Other type reads and the full scenario load are excluded. Rules ctor and Techno ctor execute only the declared original prefixes, starting Techno after its Radio base constructor.',
            'Cell controls execute the actual early gates only. Object height/category/type/ability tests and successful animation construction are established by original instruction reading, not emulated by these rows. No gameplay effect or audio/render output is certified.',
            'The EMP controls supply signed +504 values directly. Zero is natively initialized; positive controls do not establish active-retail EMP arming or serialized-state reachability.'],
        substitutions=[
            'Inherited bounded allocator, inert free and CRT TLS fixture seams. No original code patches or replacement native readers. Prepared INI caches substitute archive/file loading.',
            'Nonmatching Cell gate cases deliberately supply an invalid ground-list pointer; successful early return proves it is not dereferenced. The empty-list case supplies null.'])


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
