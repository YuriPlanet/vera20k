"""Original high-body hits, real perpendicular helpers, and live rim cleanup.

The stock witness deliberately covers cells whose perpendicular calls never
write tiles. Fail if a sequence reaches a tile writer; do not substitute one.
Object fallout, radar, screen and connectivity outputs remain declared sinks.
"""
import hashlib
import json
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.bridge_rim import OriginalRim, COORD
from tools.spatial_oracle.bridge_body_publication import PERPENDICULAR
from tools.spatial_oracle.map_queries import packed


class OriginalRimBody(OriginalRim):
    def observe(self, u, address, size, data):
        if address in (0x56EB80, 0x56E990):
            raise AssertionError(f'Unexpected tile/pavement writer {address:08X}')
        if address in PERPENDICULAR or address in (0x576770, 0x56DAE0):
            sp = u.reg_read(UC_X86_REG_ESP)
            args = struct.unpack('<2I', u.mem_read(sp + 4, 8))
            coord = list(struct.unpack('<hh', u.mem_read(args[0], 4)))
            if address in PERPENDICULAR:
                self.events.append(dict(kind='perpendicular', function=PERPENDICULAR[address],
                                        coord=coord, direction=args[1]))
                return  # Execute every original helper instruction.
            if address == 0x576770:
                self.events.append(dict(kind='rim', coord=coord))
                return  # Execute the selector, edge loop and notifications.
            self.events.append(dict(kind='zone_sink', coord=coord))
            destination = struct.unpack('<I', u.mem_read(sp, 4))[0]
            u.reg_write(UC_X86_REG_EAX, 0)
            u.reg_write(UC_X86_REG_ESP, sp + 8)
            u.reg_write(UC_X86_REG_EIP, destination)
            return
        super().observe(u, address, size, data)


def cases():
    source = Path(__file__).with_name('bridge_rim_stock_inputs.json')
    case = json.loads(source.read_text(encoding='utf-8'))
    results = []
    for name, hits in (
        ('single_section', [(112, 140)] * 2),
        ('two_breaks_remove_middle', [(112, 140)] * 2 + [(112, 144)] * 2),
        ('reverse_breaks_remove_middle', [(112, 144)] * 2 + [(112, 140)] * 2),
        ('nonanchor_two_breaks', [(111, 140)] * 2 + [(111, 144)] * 2),
    ):
        native = OriginalRimBody(case)
        steps = []
        for coord in hits:
            before = {c: native.snapshot(p) for c, p in native.ptrs.items()}
            native.events.clear()
            native.writes.clear()
            native.uc.mem_write(COORD, packed(*coord))
            returned = native.call(0x576BA0, args=(COORD,))
            # These fields stay live and readable for the actual rim selector.
            for x, y, tile, sub, *_ in case['cells']:
                p = native.ptrs[x, y]
                assert struct.unpack('<i', native.uc.mem_read(p + 0x38, 4))[0] == tile
                assert bytes(native.uc.mem_read(p + 0x11A, 1))[0] == sub
            steps.append(dict(input=coord, returned=returned, calls=list(native.events),
                              changes=[dict(before=before[c], after=native.snapshot(p))
                                       for c, p in native.ptrs.items()
                                       if before[c] != native.snapshot(p)]))
        results.append(dict(name=name, hits=steps))
    return dict(stock_input_sha256=hashlib.sha256(source.read_bytes()).hexdigest(), cases=results)


if __name__ == '__main__':
    finish_vectors(cases, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original576BA0 body, actual EW perpendicular helpers,576770/576200 rim and47E040 across stock consecutive hits',
        assumptions=[
            '225 stock xbayopigs scalar cells supplied by the current Rust loader; native loader and outer damage/RNG admission excluded',
            'All hits begin from the real stock state9; no intermediate state bytes are supplied',
            'Both input orders and nonanchor inputs covered; only the named vertical stock span is certified',
            'Every actual perpendicular instruction executes; reaching56EB80 or56E990 fails the corpus instead of substituting tile writes',
            'All225 tile/subtile identities are asserted unchanged after every hit; terminal middle-ramp tile collapse remains outside this witness',
            'No objects or CellTags supplied; untagged575EE0 executes; live tag dispatch is excluded',
        ], substitutions=[
            '47DD70 fallout receiver,6551C0 radar,6D2140 screen projection and6D2790 screen sinks inherited from bridge_rim',
            '56DAE0 connectivity rebuild records coordinate and returns0; connectivity internals excluded',
            'No body, perpendicular, rim, setter, or notification instructions replaced',
        ], entry_points={'body': 0x576BA0, 'selector': 0x576770, 'edge': 0x576200,
                         'setter': 0x47E040, 'notification': 0x575EE0,
                         'ew_damage_a': 0x572B80, 'ew_damage_b': 0x572C90,
                         'ew_collapse_a': 0x572DA0, 'ew_collapse_b': 0x573170}))
