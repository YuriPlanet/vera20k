"""Original wooden hut destruction joined to live residents and Anim constructors.

Both authored Shrapnel huts enter original574C20 directly. The shared Joined
owner supplies original actor/Drive/receiver/cleanup and Anim construction in
one VM. No Bomb/Building prefix, animation AI or whole Logic tick is simulated.
"""
from pathlib import Path
import json
import struct
import sys

from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP

from tools.native_oracle import RET_MAGIC, _canonical, provenance, run_checked
from tools.spatial_oracle.building_body_rules import SP, dwords
from tools.spatial_oracle.map_queries import packed
from tools.spatial_oracle.naval_occupants import MAP
from tools.spatial_oracle.shrapnel_repair import packet_io
from . import occupants, occupants_joined

HERE = Path(__file__).resolve().parent
OUTPUT = HERE / 'hut_joined.json.gz'
PROMOTION = HERE / 'hut_joined_promotion.json'
ENTRY = 0x574C20
ENTRIES = {
    ENTRY: 'wood_hut', 0x574780: 'wood_hut_selector',
    0x575220: 'wood_x_walker', 0x575540: 'wood_y_walker',
    0x57BAA0: 'wood_damage_driver',
}


class HutJoined(occupants_joined.Joined):
    """Add the original hut entry and observations, without replacing its body."""

    def __init__(self, donor, theater, case):
        self.hut_returns = []
        super().__init__(donor, theater, case, bridge_explosions=True)
        self.hut_coord = self.alloc(4)
        self.u.mem_write(self.hut_coord, packed(*case['hut']))

    def hook(self, u, pc, size, data):
        if getattr(self, 'phase', None) == 'measure':
            sp = u.reg_read(UC_X86_REG_ESP)
            if self.hut_returns and pc == self.hut_returns[-1]['return_pc']:
                row = self.hut_returns.pop()
                row.pop('return_pc')
                self.add('hut_call_return', **row,
                         result_eax=u.reg_read(UC_X86_REG_EAX),
                         result_al=u.reg_read(UC_X86_REG_EAX) & 255)
            if pc in ENTRIES:
                coord = list(struct.unpack('<hh', u.mem_read(self.read32(sp + 4), 4)))
                row = dict(name=ENTRIES[pc], pc=hex(pc), coord=coord,
                           caller=hex(self.read32(sp)))
                self.add('hut_call', **row)
                self.hut_returns.append(dict(row, return_pc=self.read32(sp)))
            if pc == 0x5657A0:
                self.add('hut_lookup', coord=list(struct.unpack(
                    '<hh', u.mem_read(self.read32(sp + 4), 4))),
                    caller=hex(self.read32(sp)))
            if pc == 0x47D2B0:
                assert self.cell_coord(u.reg_read(UC_X86_REG_ECX)) in self.physical_resident_cells
        super().hook(u, pc, size, data)

    def span(self):
        return [[x, y, occupants.signed(self.read32(self.cells[x, y] + 0x44)),
                 occupants.signed(self.read32(self.cells[x, y] + 0xEC))]
                for x, y in self.scene['span']]

    def run_hut(self):
        self.active_stage = 'hut_collapse'
        self.visit = 0
        self.events2 = []
        self.calls = []
        self.writes = []
        self.accesses = {}
        before = self.snapshot()
        span_before = self.span()
        rng_before = {k: bytes(self.u.mem_read(p, 1012)).hex()
                      for k, p in self.rngs.items()}
        display = self.read32(0x887324)
        dirty_before = self.u.mem_read(display + 0xD7C, 1)[0]
        self.u.mem_write(SP, dwords(RET_MAGIC, self.hut_coord))
        self.u.reg_write(UC_X86_REG_ESP, SP)
        self.u.reg_write(UC_X86_REG_ECX, MAP)
        run_checked(self.u, ENTRY, RET_MAGIC, count=10000000)
        assert len(self.hut_returns) == 1 and self.hut_returns[0]['return_pc'] == RET_MAGIC
        root = self.hut_returns.pop()
        root.pop('return_pc')
        self.add('hut_call_return', **root,
                 result_eax=self.u.reg_read(UC_X86_REG_EAX),
                 result_al=self.u.reg_read(UC_X86_REG_EAX) & 255,
                 return_boundary=hex(RET_MAGIC))
        assert not self.return_events and not self.pending and not self.anim_pending
        assert occupants.sha(bytes(self.u.mem_read(0x401000, 0x3E0000))) == self.code_hash
        return dict(stage='hut_collapse', before=before, after=self.snapshot(),
                    instruction_visits=self.visit, events=self.events2, calls=self.calls,
                    writes=self.writes,
                    reads=[dict(region=k[0], offset=hex(k[1]), size=k[2], initial_hex=v)
                           for k, v in sorted(self.accesses.items())],
                    rng_before=rng_before,
                    rng_after={k: bytes(self.u.mem_read(p, 1012)).hex()
                               for k, p in self.rngs.items()},
                    span_before=span_before, span=self.span(),
                    display_dirty_before=dirty_before,
                    display_dirty_after=self.u.mem_read(display + 0xD7C, 1)[0])


def generate():
    source = packet_io.read_result(HERE / 'occupants.json.gz')
    source_hash = occupants.sha(_canonical(source))
    expected = json.loads((HERE / 'occupants_promotion.json').read_bytes())
    assert source_hash == expected['results']['occupants.json']['published_payload_sha256']
    rules, theater = occupants.Rules(), occupants.theater()
    resident_cells = occupants.SCENE['span']
    donor = occupants.ResidentRepair(occupants.input_case([117, 56], theater), rules, theater,
                                    resident_cells=resident_cells)
    donor.run()
    cases = []
    for hut in ([117, 56], [113, 62]):
        case = dict(name=f'resident_hut_{hut[0]}_{hut[1]}',
                    current_cell=[115, 59], hut=hut)
        print('native joined wooden hut', case['name'], flush=True)
        machine = HutJoined(donor, theater, case)
        machine.physical_resident_cells = [list(c) for c in resident_cells]
        try:
            result = machine.run_hut()
        except Exception:
            print(json.dumps(dict(input=case, events=machine.events2[-40:],
                                  calls=machine.calls[-20:]), indent=2), flush=True)
            raise
        cases.append(dict(input=case, initializers=machine.initializers,
                          native_inputs=machine.inputs, art_input=machine.art_input,
                          result=[result]))
    slices = [(0x43894C, 0x438987), (0x4402E4, 0x440320),
              (0x574C20, 0x574CB9), (0x575749, 0x5757E6)]
    return dict(schema=1, request_boundary_payload_sha256=source_hash,
                text_section_sha256=machine.code_hash, scene=source['scene'], cases=cases,
                physical_resident_cells=resident_cells, resident_assets=donor.assets,
                initial_recalc=donor.initial_recalc,
                original_slices=[dict(start=hex(a), end_exclusive=hex(b),
                    hex=bytes(machine.u.mem_read(a, b-a)).hex(),
                    sha256=occupants.sha(bytes(machine.u.mem_read(a, b-a))))
                    for a, b in slices])


def metadata():
    return provenance(scope=__doc__, assumptions=[
        'Physical XShrapnel MAP and SNOW Recalc/overlay/land data, original repair570050 and supplied resident MTNK/Drive/House/list state use the unchanged shared Shrapnel repair and Joined occupants owners.',
        'Both authored hut coordinates117,56 and113,62 enter original574C20 after the external hut-death decision. Original574780,575540 and57BAA0 execute. Native active Bomb438720 and Building update call sites43896A/440301 are retained as original instruction bytes; the prefix bodies are not executed.',
        'Native BridgeExplosions constructor/read blocks run over RULESMD, absentLANGRULE, MPBattleMD and XShrapnel before ART reads in this same VM and registry. The shared joined owner executes full original Anim constructor421EA0, Unlimbo, Start and Bouncer initialization, original resident admission/receiver and death cleanup.',
        'Original65C6D0 seed0 initializes all three RNG streams after actor setup. ART/general setup consumes none. Complete Main,Scenario,MapGen states bracket each entire hut call, and instruction visits order constructors, native receivers, list changes and draws.',
        'Supplied game speed4 and frame1000 are fixed. Native type initialization determines the initial Scenario identity cursor; ordered identity deltas are usable, but this is not whole ScenarioLoad identity parity.',
    ], substitutions=[
        'Inherited bounded allocation/free/CRT/OS seams, signed-CRC lexical INI caches and unchanged physical SHP IO. No hut selector/walker, animation constructor, admission, receiver or cleanup algorithm is copied or replaced.',
        'Native map/actor world admission and Bomb/Building destruction prefix are excluded. Connectivity/hierarchy and presentation extents remain declared request boundaries. All original text bytes remain unchanged.',
        'Sound7509E0 stops at its named request. No audio playback, AnimAI, subsequent debris flight/contact/damage/expiry, deferred-delete drain or complete Logic scheduler runs.',
    ], entry_points=dict(wood_hut=ENTRY, selector=0x574780,
        physical_y_walker=0x575540, instruction_only_x_walker=0x575220,
        driver=0x57BAA0, occupants=0x487A10, unit_damage=0x737C90,
        anim_constructor=0x421EA0, bridge_explosion_reader=0x66DB93))


if __name__ == '__main__':
    result = generate()
    if '--write' in sys.argv and not PROMOTION.exists():
        raw = _canonical(json.loads(_canonical(occupants_joined.publication_projection(result))))
        PROMOTION.write_text(json.dumps(dict(results={OUTPUT.name.removesuffix('.gz'):
            dict(published_payload_sha256=occupants.sha(raw))}), indent=2) + '\n')
    packet_io.finish_vectors(result, OUTPUT, provenance=metadata,
        promotion_path=PROMOTION, projection=occupants_joined.publication_projection)
