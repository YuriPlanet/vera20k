"""Cold selected-root Colors prefix, using the existing physical Rules VM."""
from pathlib import Path

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EAX
from tools.rules_oracle.weapon_speed_order import retail_history
from tools.spatial_oracle.building_body_rules import SP, dwords
from tools.native_oracle import (
    run_checked, initialize_empty_windows_seh, finish_vectors, provenance,
)

INPUT = '[Colors]\nPrefixColor=17,128,240\n'


class Inputs:
    def lookup(self, name):
        if name.upper() != 'ROOT.INI':
            raise ValueError(('Unexpected physical INI', name))
        return INPUT.encode('ascii')


def generate():
    m = retail_history(Inputs())
    try:
        initialize_empty_windows_seh(m.u)
        m.invoke(0x68C330, 0)
        root, _ = m.load_physical_ini('ROOT.INI')
        rules = m.alloc(0x2000)
        m.invoke(0x665650, rules)
        m.u.mem_write(0x8871E0, dwords(rules))
        visits = []
        converts = []

        def observe(_u, pc, _size, _data):
            if pc in (0x52D0FF, 0x66D3A0, 0x474C70, 0x68C9C0,
                      0x68C710, 0x68C840, 0x68C84C):
                visits.append(f'{pc:08X}')
            if pc == 0x68C3B0:
                converts.append('Palette/Convert generation excluded; NULL, native ret32 ABI')
                m.ret(0, 32)

        m.u.hook_add(UC_HOOK_CODE, observe)
        m.u.reg_write(UC_X86_REG_ESP, SP)
        m.u.reg_write(UC_X86_REG_EAX, root)
        run_checked(m.u, 0x52D0F3, 0x52D104,
                    required_addresses=(0x52D0FF, 0x66D3A0, 0x68C9C0, 0x68C710),
                    count=300000, timeout_us=3000000)
        m.check_code()
        rows = []
        for index in range(m.read32(0xB054E0)):
            pointer = m.read32(m.read32(0xB054D4) + 4 * index)
            rows.append(dict(index=index, name=m.string(m.read32(pointer + 0x304)),
                             kind=m.read32(pointer + 0x310),
                             hsv=list(bytes(m.u.mem_read(pointer + 0x308, 3))),
                             convert=m.read32(pointer + 0x30C)))
        return dict(input=INPUT, entry='0052D0F3', stop='0052D104',
                    langrule='Absent; no Rules Process invoked',
                    color_scheme_rows=rows, reached=visits, substitutions=converts)
    finally:
        m.close()


def metadata():
    return provenance(
        scope='Cold selected-root Colors admission and ordered scheme identities only',
        assumptions=[
            'Executes original cold caller before the optional LANGRULE branch, '
            'original physical INI, Colors reader and scheme constructors.',
            'Rust tests consume these names/indices to check cold admission and '
            'the later Country reader; no rendered color or whole startup parity.',
        ],
        substitutions=[
            'Existing weapon_speed_order.retail_history VM owns allocator, CRT, '
            'INI and platform boundaries; no second mapper or parser.',
            '68C3B0 palette/Convert generation returns NULL with native ret32 ABI; '
            'no Convert, RGB, loading pixels or palette generation is certified.',
        ],
        entry_points=dict(cold_colors_call=0x52D0FF, colors=0x66D3A0,
                          color_scheme_constructor=0x68C710))


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
