"""Native references for Prerequisite_INI_Parser (0x4770E0).

Run python -m tools.rules_oracle.prerequisite_lists --check (or explicit --write).
Rust consumer: src/rules/prerequisite.rs (tests).

The original parser reads `Prerequisite=`, `PrerequisiteOverride=` and the six
[General] Prerequisite* lists. Each case executes it on one supplied INI
section against a supplied BuildingType array (the names below, in index
order) and records the int list it returns:
-1..-6 for POWER, FACTORY, BARRACKS, RADAR, TECH and PROC, else the
BuildingTypeClass::FindIndexByName (0x45E7B0) index.
"""
from pathlib import Path

from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESP

from tools.native_oracle import RET_MAGIC, finish_vectors, provenance, run_checked
from tools.rules_oracle.bridge_anim_inputs import Reader
from tools.spatial_oracle.building_body_rules import INI, SP, dwords

PARSER = 0x4770E0
BUILDING_ARRAY = 0xA83C6C
BUILDING_COUNT = 0xA83C78
TYPE_LIST_INT = 0x7E4DD8

BUILDINGS = ['GAPOWR', 'NAPOWR', 'GAWEAP', 'Power', 'GaPile']

CASES = (
    ('groups in any case', 'power,FACTORY,Barracks,rAdAr,tech,PROC', []),
    ('a building is its array index, in any case', 'gaweap,NAPOWR,gapile', []),
    ('a group name wins over a building of that name', 'POWER', []),
    ('unknown names are dropped', 'GAPOWR,NOPE,GAWEAP', []),
    ('a group name must match whole', 'POWERS,PRO,TECHS', []),
    ('spaces stay in a token', 'GAPOWR, GAWEAP ,NAPOWR', []),
    ('empty fields are skipped', ',,GAPOWR,,NAPOWR,', []),
    ('repeats are kept', 'GAPOWR,GAPOWR,POWER,POWER', []),
    ('a value of commas replaces the default with nothing', ',,,', [2]),
    ('an absent key keeps the default', None, [2, -3]),
    ('the value is cut at 127 bytes', 'GAPOWR,' * 18 + 'NAPOWR', []),
)


class Parser(Reader):
    def __init__(self):
        super().__init__(Path(__file__).with_name('no-assets'), {})
        types = []
        for name in BUILDINGS:
            ptr = self.alloc(0x40)
            self.u.mem_write(ptr + 0x24, name.encode('latin1') + b'\0')
            types.append(ptr)
        array = self.alloc(4 * len(types))
        self.u.mem_write(array, dwords(*types))
        self.u.mem_write(BUILDING_ARRAY, dwords(array))
        self.u.mem_write(BUILDING_COUNT, dwords(len(types)))

    def parse(self, value, default):
        self.make_ini({'T': {} if value is None else {'Prerequisite': value}})
        items = self.alloc(4 * max(len(default), 1))
        self.u.mem_write(items, dwords(*[v & 0xFFFFFFFF for v in default]) if default else bytes(4))
        out = self.alloc(0x1C)
        # The default list is passed by value: vtable, items, capacity,
        # flags, count, growth step and TypeList's own dword.
        by_value = (TYPE_LIST_INT, items, len(default), 0x0101, len(default), 10, 0)
        self.u.mem_write(SP, dwords(RET_MAGIC, self.cstring('T'), self.cstring('Prerequisite'), *by_value))
        self.u.reg_write(UC_X86_REG_ESP, SP)
        self.u.reg_write(UC_X86_REG_ECX, out)
        self.u.reg_write(UC_X86_REG_EDX, INI)
        run_checked(self.u, PARSER, RET_MAGIC, count=2_000_000, required_addresses=(0x528A10,))
        assert self.u.reg_read(UC_X86_REG_ESP) == SP + 4 + 0x24
        data, count = self.read32(out + 4), self.read32(out + 0x10)
        return [int.from_bytes(self.u.mem_read(data + 4 * i, 4), 'little', signed=True)
                for i in range(count)]


def generate():
    rows = []
    for name, value, default in CASES:
        rows.append({'name': name, 'value': value, 'default': default,
                     'expected': Parser().parse(value, default)})
    return {'buildings': BUILDINGS, 'rows': rows}


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Prerequisite_INI_Parser over one supplied section per case and a supplied '
              'BuildingType array; the original ReadString, strtok, strcmpi, '
              'FindIndexByName and vector routines execute.',
        assumptions=['Supplied cached INI indexes stand in for a loaded INI: values are '
                     'given as the loader stores them, trimmed, and an absent key has no '
                     'entry. Each case runs in a fresh emulator.',
                     'The BuildingType array holds only the supplied names at +0x24; '
                     'nothing else of a BuildingType is read.'],
        substitutions=['operator_new returns bump storage; operator_delete is a no-op; '
                       'CRT TLS accessor returns supplied per-thread storage for strtok'],
        entry_points={'prerequisite_ini_parser': PARSER, 'read_string': 0x528A10,
                      'strtok': 0x7C9CC2, 'find_index_by_name': 0x45E7B0},
    ))
