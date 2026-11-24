"""Original high-body continuation and full setter callback boundaries.

Stock-loaded scalar cells establish anchor/nonanchor inputs. Perpendicular,
rim and zone routines are explicit synchronous sinks, not executed here.
Synthetic callbacks exercise continuation reads; they are not stock witnesses.
"""
import hashlib
import json
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.bridge_rim import OriginalRim, COORD
from tools.spatial_oracle.map_queries import dwords, packed

PERPENDICULAR = {
    0x572230: 'ns_damage_a', 0x572330: 'ns_damage_b',
    0x572440: 'ns_collapse_a', 0x5727E0: 'ns_collapse_b',
    0x572B80: 'ew_damage_a', 0x572C90: 'ew_damage_b',
    0x572DA0: 'ew_collapse_a', 0x573170: 'ew_collapse_b',
}


class OriginalBody(OriginalRim):
    def __init__(self, case, mutation=None):
        self.mutation = mutation
        self.injected = False
        super().__init__(case)

    def snapshot(self, pointer):
        result = super().snapshot(pointer)
        result['allocation_coord'] = list(self.coords.get(pointer, ('dummy',)))
        return result

    def return_from_sink(self, cleanup):
        u = self.uc
        sp = u.reg_read(UC_X86_REG_ESP)
        destination = struct.unpack('<I', u.mem_read(sp, 4))[0]
        u.reg_write(UC_X86_REG_EAX, 0)
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)
        u.reg_write(UC_X86_REG_EIP, destination)

    def observe(self, u, address, size, data):
        if address in PERPENDICULAR or address in (0x576770, 0x56DAE0):
            sp = u.reg_read(UC_X86_REG_ESP)
            args = struct.unpack('<2I', u.mem_read(sp + 4, 8))
            coord = list(struct.unpack('<hh', u.mem_read(args[0], 4)))
            if address in PERPENDICULAR:
                self.events.append(dict(kind='perpendicular_sink',
                                        function=PERPENDICULAR[address],
                                        coord=coord, direction=args[1]))
                if self.mutation == 'first_perpendicular_changes_anchor_state' and not self.injected:
                    anchor = self.ptrs[112, 140]
                    u.mem_write(anchor + 0x11E, b'\x09')
                    self.events.append(dict(kind='callback_write',
                                            cell=self.snapshot(anchor)))
                    self.injected = True
                self.return_from_sink(8)
            else:
                self.events.append(dict(kind='rim_sink' if address == 0x576770 else 'zone_sink',
                                        coord=coord,
                                        anchor=self.snapshot(self.ptrs[112, 140])))
                self.return_from_sink(4)
            return

        if address == 0x47DD70:
            # Record the actual call boundary before executing the supplied
            # callback. The inherited return changes registers only; emulation
            # cannot resume until this hook completes.
            super().observe(u, address, size, data)
            if self.injected:
                return
            if self.mutation == 'anchor_fallout_changes_next_flags':
                target = self.ptrs[111, 140]
                u.mem_write(target + 0x140, dwords(0xA4010380))
                self.events.append(dict(kind='callback_write', cell=self.snapshot(target)))
                self.injected = True
            elif self.mutation == 'first_forward_fallout_moves_retained_coord':
                receiver = u.reg_read(UC_X86_REG_ECX)
                if receiver == self.ptrs[111, 140]:
                    u.mem_write(receiver + 0x24, packed(110, 140))
                    self.events.append(dict(kind='callback_write', cell=self.snapshot(receiver)))
                    self.injected = True
            return
        super().observe(u, address, size, data)


def cases():
    source = Path(__file__).with_name('bridge_rim_stock_inputs.json')
    case = json.loads(source.read_text(encoding='utf-8'))
    result = []
    for name, coord, state, mutation in (
        ('stock_anchor_first_hit', (112, 140), 9, None),
        ('stock_nonanchor_first_hit', (111, 140), 9, None),
        ('stock_anchor_collapse', (112, 140), 15, None),
        ('stock_nonanchor_collapse', (111, 140), 15, None),
        ('selected_branch_survives_callback_state_change', (112, 140), 15,
         'first_perpendicular_changes_anchor_state'),
        ('setter_reads_callback_changed_next_flags', (112, 140), 15,
         'anchor_fallout_changes_next_flags'),
        ('setter_steps_from_retained_current_coordinate', (112, 140), 15,
         'first_forward_fallout_moves_retained_coord'),
    ):
        native = OriginalBody(case, mutation)
        native.uc.mem_write(native.ptrs[112, 140] + 0x11E, bytes((state,)))
        before = {c: native.snapshot(p) for c, p in native.ptrs.items()}
        native.uc.mem_write(COORD, packed(*coord))
        returned = native.call(0x576BA0, args=(COORD,))
        if mutation:
            assert native.injected, name
        result.append(dict(name=name, input_coord=coord, initial_anchor_state=state,
                           mutation=mutation, returned=returned,
                           calls=native.events, writes=native.writes,
                           changed_cells=[dict(before=before[c], after=native.snapshot(p))
                                          for c, p in native.ptrs.items()
                                          if before[c] != native.snapshot(p)]))
    return dict(stock_input_sha256=hashlib.sha256(source.read_bytes()).hexdigest(), cases=result)


if __name__ == '__main__':
    finish_vectors(cases, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original576BA0 high-body branch and complete47E040 setter with synchronous callback boundaries',
        assumptions=[
            'Executable coverage is body states9/15 and setter direction6,set0; no NS/intact/partial/dummy coverage is claimed',
            'Supplied xbayopigs stock scalar cells; original loader and outer damage admission/RNG are excluded',
            'Initial anchor9 is stock;15 supplies the state following a prior effective body hit',
            'Synthetic callback state/flag/coordinate writes are continuation tests, not retail callback reachability claims',
            'Coordinate mutation covers retained setter F1; body anchor-coordinate reload is separate static instruction evidence',
            'Retained anchor+2C initialized as in bridge_rim; admitted body tests cover stock self and nonself resolution',
        ], substitutions=[
            'All eight perpendicular helpers are synchronous sinks with supplied optional callback writes; their bodies are excluded',
            'Rim576770 and zone56DAE0 are sinks; zone returns0 and no connectivity rebuild executes',
            'BlowUpBridge47DD70/radar6551C0/presentation sinks inherited from bridge_rim; object and RNG bodies are excluded',
            'No high-body or setter instructions are replaced',
        ], entry_points={'high_body': 0x576BA0, 'full_setter': 0x47E040}))
