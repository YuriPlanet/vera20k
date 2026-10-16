"""Original Cell pickup prefix and speed pickup, including original slot removal.

Gameplay callees execute original bytes. Only screen-rectangle queries and the
screen-dirty sink are supplied presentation seams. Regeneration is disabled;
other effect arms stop at the dispatch boundary so they are never simulated.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import SCRATCH, RET_MAGIC, run_checked, finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.crate_speed_effect import fixture, CELL, RULES, HOUSES

MAP, TABLE = 0x87F7E8, 0xC00000
OVERLAY, OVERLAYS, SCENARIO = SCRATCH + 0x18000, SCRATCH + 0x18500, SCRATCH + 0x19000
STOCK_WEIGHTS = [20, 20, 10, 0, 0, 0, 0, 0, 10, 10, 10, 10, 0, 0, 20, 0, 0, 0, 0]


def execute(case, *, native_fixture=None):
    state = dict(case, actors=case.get('actors', [{}]))
    u, sp, pointers, _ = fixture(state, uc=None if native_fixture is None else native_fixture.u)
    actor = pointers[0]
    u.mem_write(actor + 0x14, dwords(5))
    u.mem_write(actor + 0x158, struct.pack('<d', 1.0))
    u.mem_write(actor + 0x160, struct.pack('<d', 1.0))
    u.mem_write(actor + 0x90, b'\1')
    u.mem_write(HOUSES + 0x2F0, dwords(1))  # owns a building: no free-MCV preemption
    u.mem_write(HOUSES + 0x200 + 0x1A6, bytes([case.get('passive', 0)]))
    u.mem_write(CELL, dwords(0x7E4EEC))
    cell = case.get('cell', [10, 10])
    u.mem_write(CELL + 0x24, packed(*cell))
    u.mem_write(CELL + 0x44, dwords(case.get('overlay', 0)))
    u.mem_write(CELL + 0x11E, bytes([case.get('selection', 10)]))
    u.mem_write(CELL + 0xEC, dwords(case.get('land', 0)))
    u.mem_write(OVERLAY + 0x2AA, bytes([case.get('crate', 1)]))
    u.mem_write(OVERLAYS, dwords(OVERLAY))
    u.mem_write(0xA83D84, dwords(OVERLAYS))
    u.mem_write(RULES + 0xF8, dwords(OVERLAY + (0x400 if case.get('different_image') else 0)))
    if case.get('same_images'):
        u.mem_write(RULES + 0xFC, dwords(OVERLAY, OVERLAY))
    u.mem_write(RULES + 0x1464, dwords(*case.get('solo_choices', [2, 10, 0])))
    u.mem_write(RULES + 0x1140, dwords(case.get('solo_money', 2000)))
    u.mem_write(0xA8B238, dwords(case.get('mode', 3)))
    u.mem_write(0xA8B261, b'\0')
    u.mem_write(0xA8ED84, dwords(100))
    u.mem_write(0xA8B230, dwords(SCENARIO))
    u.mem_write(0x81DA8C, dwords(*case.get('weights', STOCK_WEIGHTS)))
    u.mem_write(0x81DAD8, dwords(*([-1] * 19)))
    u.mem_write(0x89ECC0, bytes([1] * 19))
    u.mem_write(0x89EC28 + 10 * 8, struct.pack('<d', case.get('multiplier', 1.2)))
    table = bytearray(0x100000)
    struct.pack_into('<I', table, (cell[1] * 512 + cell[0]) * 4, CELL)
    u.mem_write(TABLE, bytes(table))
    u.mem_write(MAP + 0x13C, dwords(TABLE, 0x40000))
    u.mem_write(MAP + 0xF4, dwords(10, 10))
    u.mem_write(MAP + 0x158, bytes(16 * 256))
    if case.get('registered', True):
        u.mem_write(MAP + 0x158, dwords(50, 777, 100) + packed(*cell))
    if case.get('duplicate_slot'):
        u.mem_write(MAP + 0x168, dwords(60, 888, 200) + packed(*cell))
    u.mem_write(sp, dwords(RET_MAGIC, case.get('seed', 31)))
    u.reg_write(UC_X86_REG_ECX, SCENARIO + 0x218)
    u.reg_write(UC_X86_REG_ESP, sp)
    run_checked(u, 0x65C6D0, RET_MAGIC, count=100000, required_addresses=[0x65C6D0])
    rng_before = bytes(u.mem_read(SCENARIO + 0x218, 0x3F4)).hex()
    events, selected, removal_returns, prepared = [], [], [], []
    joined = native_fixture is not None
    tag_calls, credit_calls, random_calls = [], [], []
    if joined:
        from types import SimpleNamespace
        from tools.input_oracle.campaign_start import CampaignReader
        # Explicit empty-storage, neutral-income House priors. No full House
        # construction, FillSilos or storage conversion is represented.
        for index, balance in enumerate(case.get('balances', [100, 200])):
            house = HOUSES + index * 0x400
            u.mem_write(house, dwords(0x7EA8A0))
            u.mem_write(house + 0x24, dwords(0x7EA834))
            u.mem_write(house + 0x34, dwords(house + 0x200))
            u.mem_write(house + 0x200 + 0x148, struct.pack('<f', 1.0))
            u.mem_write(house + 0x2FC, bytes(16))
            u.mem_write(house + 0x30C, dwords(balance))
        u.mem_write(0xA83D4C, dwords(HOUSES))
        u.mem_write(OVERLAY + 0x2AB, bytes([case.get('crate_trigger', True)]))
        u.mem_write(actor + 0x34, dwords(0x123456 if case.get('tag', True) else 0))
        u.mem_write(actor + 0x90, bytes([case.get('alive', True)]))
        u.mem_write(0x89EC28, struct.pack('<d', case.get('money_magnitude', 500.0)))
        # Original startup establishes the global EmptyCell used by Event49.
        native_fixture.call(0x47B2F0, [])
        for stream in (SCENARIO + 0x218, 0x886B88, 0xABE890):
            native_fixture.call(0x65C6D0, [case.get('seed', 31)], ecx=stream)
        rng_full_before = CampaignReader.rng_state(SimpleNamespace(u=u, scenario=SCENARIO))

    def read32(p):
        return struct.unpack('<I', u.mem_read(p, 4))[0]

    def observe(_u, address, _size, _data):
        current_sp = u.reg_read(UC_X86_REG_ESP)
        if address in (0x47FDE0, 0x47FB90):
            # Screen-space rectangles only. No collision/selection result supplied.
            output = read32(current_sp + 4)
            u.mem_write(output, bytes(16))
            u.reg_write(UC_X86_REG_EAX, output)
            u.reg_write(UC_X86_REG_EIP, read32(current_sp))
            u.reg_write(UC_X86_REG_ESP, current_sp + 8)
        elif address == 0x6D2790:
            events.append('dirty_screen')
            u.reg_write(UC_X86_REG_EIP, read32(current_sp))
            u.reg_write(UC_X86_REG_ESP, current_sp + 24)
        elif joined and address == 0x4068E0:
            # Diagnostic printf only, cdecl caller cleanup.
            u.reg_write(UC_X86_REG_EIP, read32(current_sp))
            u.reg_write(UC_X86_REG_ESP, current_sp + 4)
        elif joined and address == 0x6E53A0:
            args = list(struct.unpack('<5I', u.mem_read(current_sp + 4, 20)))
            tag_calls.append(dict(tag=u.reg_read(UC_X86_REG_ECX), kind=args[0],
                actor=args[1], cell=list(struct.unpack('<2h', dwords(args[2]))),
                force=args[3], source=args[4], mutation=case.get('mutation', 'none')))
            mutation = case.get('mutation', 'none')
            if mutation == 'kill':
                u.mem_write(actor + 0x90, b'\0')
            elif mutation == 'owner1':
                u.mem_write(actor + 0x21C, dwords(HOUSES + 0x400))
            elif mutation == 'selection10':
                u.mem_write(CELL + 0x11E, b'\x0a')
            elif mutation == 'selection0':
                u.mem_write(CELL + 0x11E, b'\0')
            u.reg_write(UC_X86_REG_EAX, 1)
            u.reg_write(UC_X86_REG_EIP, read32(current_sp))
            u.reg_write(UC_X86_REG_ESP, current_sp + 24)
        elif joined and address == 0x4F9950:
            credit_calls.append(dict(house=(u.reg_read(UC_X86_REG_ECX) - HOUSES) // 0x400,
                amount=struct.unpack('<i', u.mem_read(current_sp + 4, 4))[0]))
        if joined and address in (0x65C780, 0x65C7E0):
            random_calls.append(dict(address=f'{address:08X}',
                receiver=f'{u.reg_read(UC_X86_REG_ECX):08X}',
                args=list(struct.unpack('<2i', u.mem_read(current_sp + 4, 8)))
                    if address == 0x65C7E0 else []))
        elif address == 0x481B99 or (address == 0x481D86 and case.get('mode', 3) == 0):
            prepared.append([u.reg_read(UC_X86_REG_EBX), read32(current_sp + 0x14)])
        if address == 0x481D86:
            selected.append(u.reg_read(UC_X86_REG_EBX))
        elif address in (0x56C087, 0x56C1DC, 0x56C1EB):
            removal_returns.append(u.reg_read(UC_X86_REG_EAX) & 255)
        elif address in (0x56C020, 0x4A1750, 0x4A1AA0, 0x482F4A, 0x48306C):
            events.append(f'{address:08x}')

    u.hook_add(UC_HOOK_CODE, observe)
    u.reg_write(UC_X86_REG_ESP, sp)
    u.reg_write(UC_X86_REG_ECX, CELL)
    u.mem_write(sp, dwords(RET_MAGIC, 0 if case.get('null_actor') else actor))
    stops = (RET_MAGIC, 0x481DE0) if case.get('prefix_only') else RET_MAGIC
    end = run_checked(u, 0x481A00, stops, count=50000, required_addresses=[0x481A00])
    if end == RET_MAGIC:
        assert u.reg_read(UC_X86_REG_ESP) == sp + 8
    result = dict(input=case, selected=selected, prepared=prepared,
                dispatched=u.reg_read(UC_X86_REG_EBX) if end == 0x481DE0 else None,
                returned=(u.reg_read(UC_X86_REG_EAX) & 255) if end == RET_MAGIC else None,
                events=events, removal_returns=removal_returns,
                overlay=struct.unpack('<i', u.mem_read(CELL + 0x44, 4))[0],
                selection=u.mem_read(CELL + 0x11E, 1)[0],
                slot=list(struct.unpack('<iiiHH', u.mem_read(MAP + 0x158, 16))),
                second_slot=list(struct.unpack('<iiiHH', u.mem_read(MAP + 0x168, 16))),
                factors=[f'{struct.unpack("<Q", u.mem_read(p + 0x580, 8))[0]:016x}' for p in pointers if p],
                rng_before=rng_before, rng_after=bytes(u.mem_read(SCENARIO + 0x218, 0x3F4)).hex())
    if joined:
        result.update(tag_calls=tag_calls, credit_calls=credit_calls, random_calls=random_calls,
            rng_full_before=rng_full_before,
            rng_full_after=CampaignReader.rng_state(SimpleNamespace(u=u, scenario=SCENARIO)),
            alive=bool(u.mem_read(actor + 0x90, 1)[0]),
            owner=(read32(actor + 0x21C) - HOUSES) // 0x400,
            dirty34be=u.mem_read(SCENARIO + 0x34BE, 1)[0],
            balances=[struct.unpack('<i', u.mem_read(HOUSES + i*0x400 + 0x30C, 4))[0]
                for i in range(2)])
    return result


def generate():
    return [execute(case) for case in [
        dict(name='registered_speed'), dict(name='unregistered_speed', registered=False),
        dict(name='solo_speed', mode=0), dict(name='solo_wood_override', mode=0, selection=0),
        dict(name='null_actor', null_actor=True), dict(name='no_overlay', overlay=-1),
        dict(name='noncrate', crate=0), dict(name='passive_multiplayer', passive=1),
        dict(name='passive_solo', passive=1, mode=0),
        dict(name='different_image_multiplayer', different_image=True),
        dict(name='different_image_solo', different_image=True, mode=0),
        dict(name='duplicate_slot', duplicate_slot=True),
        dict(name='outside_diamond', cell=[1, 1]),
        dict(name='outside_diamond_solo', cell=[1, 1], mode=0),
        dict(name='solo_same_image_priority', mode=0, selection=0, same_images=True, prefix_only=True),
        dict(name='solo_fixed_choice_bypasses_images', mode=0, selection=10, same_images=True),
        dict(name='random_solo_bypasses_images', mode=0, selection=255, same_images=True, prefix_only=True),
        dict(name='single_weight_no_draw', selection=255, weights=[0] * 10 + [1] + [0] * 8),
        dict(name='existing_speed_falls_back', actors=[dict(factor=1.2)], prefix_only=True),
        dict(name='aircraft_falls_back', actors=[dict(kind='aircraft')], prefix_only=True),
        *[dict(name=f'random_{seed}', selection=255, seed=seed, prefix_only=True) for seed in range(8)],
    ]]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'pickup': 0x481A00, 'dispatch': 0x481DE0, 'remove_by_cell': 0x56C020,
                      'slot_clear': 0x4A1750, 'remove_overlay': 0x4A1AA0},
        assumptions=['Original vtables/callees; flat crate cell, stored runtime slot and supplied actor/house/type state.',
                     'House owns one building, so free-MCV preemption is excluded. No attached crate trigger; regeneration disabled and no pickup animation.',
                     'PC53/chop; stock weight table unless the input supplies weights. Speed effect executes to original return; other effects stop at the dispatch jump.'],
        substitutions=['47FDE0/47FB90 return empty screen rectangles;6D2790 screen-dirty sink returns without rendering. No gameplay receiver substituted.'],
        scope='28 original full pickup/prefix witnesses for speed, guards, weighted selection including equal bounds, fallback, ordered solo image overrides, first-match slots, image identity and registered/unregistered removal. Other effects, free-MCV, trigger, replacement generation and presentation remain outside coverage.',
    ))
