"""Original Cell pickup prefix and speed pickup, including original slot removal.

Gameplay callees execute original bytes. Only screen-rectangle queries and the
screen-dirty sink are supplied presentation seams. Regeneration is disabled;
other effect arms stop at the dispatch boundary so they are never simulated.
Additive --cash composes the existing Anim/map/sound owners for stock Money,
including original weighted selection, replacement and presentation producers.
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import SCRATCH, RET_MAGIC, run_checked, finish_vectors, provenance
from tools.native_oracle import require
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


def cash_case(case):
    """Joined original stock Money pickup and replacement in the Anim owner's VM.

    Cells and neutral House/Unit state are declared priors. Readers, weighted
    selection, removal, nearby search, replacement, amount, credits and Anim
    lifecycle execute original instructions. No gameplay return is supplied.
    """
    from types import SimpleNamespace
    from unicorn.x86_const import UC_X86_REG_EDX, UC_X86_REG_ESI, UC_X86_REG_EDI
    from tools.input_oracle.campaign_start import CampaignReader
    from tools.input_oracle.fast_scroll import return_from_sink
    from tools.spatial_oracle.trigger_action_values import (
        AnimationFixture, initialize_animation_sounds, INI, SCENARIO as anim_scenario)
    from tools.spatial_oracle.unit_source_scatter import make_source_fixture
    from tools.spatial_oracle.bridge_constructor import OriginalBridgeConstructor, D
    f = AnimationFixture(original_object_mark=True); u = f.u
    # Existing immutable-code check also covers every newly joined leaf.
    ranges=[(0x481A00,0x483393),(0x56BD40,0x56C1F1),
        (0x4A1750,0x4A1C02),(0x65C6D0,0x65C88C),(0x4F9950,0x4F9965),
        (0x66B900,0x66BB9D),(0x673E80,0x673FF5),(0x5FE250,0x5FE400),
        (0x5FE7FF,0x5FE839),(0x5662AC,0x56633E),(0x567230,0x56736B),
        (0x5F5850,0x5F5924),(0x5F4D10,0x5F4D2E),(0x5F3900,0x5F5224),
        (0x40DD70,0x40DDB3),(0x4238B0,0x4238C2)]
    f.ranges.extend(ranges)
    f.original.extend(bytes(u.mem_read(a,b-a)) for a,b in ranges)
    u.mem_map(SCRATCH+0x10000,0x10000)
    rules = f.read32(0x8871E0)
    cells = 0x45000000
    make_source_fixture(dict(seed=31, live_entry=True), uc=u, cells_base=cells)
    pickup_xy=(11,11)  # independent interior cell after native LocalSize normalization
    cell = cells + (pickup_xy[1]*32+pickup_xy[0])*0x200
    u.mem_map(D, 0x200000)
    overlay = f.allocate(0x400)
    native = OriginalBridgeConstructor.__new__(OriginalBridgeConstructor)
    native.uc=u; native.typ=overlay; native.overlay_id=0; native.required=[]; native.cell=cell
    native.prepare_success('success')
    _, sp, actors, _ = fixture(dict(actors=[dict(kind='unit',player_controlled=1)]), uc=u)
    actor=actors[0]
    u.mem_write(0x8871E0,dwords(rules))
    u.mem_write(0xA8B230,dwords(anim_scenario))
    u.mem_write(anim_scenario+0x214,dwords(1000))
    u.mem_write(0xA8ED84,dwords(100))
    u.mem_write(MAP+0xFC,dwords(0,0,16,16))
    # Read_Map4ACF0D dispatches Resize through vtable+70 after Map Size.
    # Execute Resize's original size/MapRect stores, not a second formula;
    # allocated cells and unrelated Resize lifetime remain with the map owner.
    size_input=f.allocate(16);u.mem_write(size_input,dwords(0,0,16,16))
    u.mem_write(sp+0x64,dwords(size_input))
    u.reg_write(UC_X86_REG_ESP,sp);u.reg_write(UC_X86_REG_EBX,MAP)
    run_checked(u,0x5662AC,0x56633E,count=100000)
    local_input=f.allocate(16);u.mem_write(local_input,dwords(0,0,16,16))
    # Normal map load reaches Radar654490 -> Map567230 after LocalSize read.
    # Full original normalization executes, including its empty-world updates.
    f.call(0x567230,[local_input],ecx=MAP)
    geometry=dict(raw_local_size=list(struct.unpack('<4i',u.mem_read(local_input,16))),
        size=list(struct.unpack('<4i',u.mem_read(MAP+0xEC,16))),
        local_size=list(struct.unpack('<4i',u.mem_read(MAP+0xFC,16))),
        map_rect=list(struct.unpack('<4i',u.mem_read(MAP+0x124,16))),
        allocated_cell_extent=[0,0,32,32])
    u.mem_write(MAP+0x158,bytes(16*256))
    u.mem_write(0xA83D80,dwords(0x7EB6D4,f.allocate(256),64,1,0,10))
    # Actual OverlayType construction registers each physical CrateRules name.
    overlay_types={}
    for name in dict.fromkeys(f.rules_input['CrateRules'][key]
            for key in ('WoodCrateImg','CrateImg','WaterCrateImg')):
        pointer=f.allocate(0x400); text=f.allocate(512)
        u.mem_write(text,name.encode()+b'\0')
        f.call(0x5FE250,[text],ecx=pointer)
        overlay_types[name]=pointer
    # Only the actual Crate/CrateTrigger native field block is needed here;
    # no unrelated whole ObjectType/ART image loading is claimed.
    for name,pointer in overlay_types.items():
        f.ordered_ini({name:f.rules_input[name]})
        assert f.string(pointer+0x24)==name, (name,f.string(pointer+0x24))
        require(f.call(0x5295F0,[pointer+0x24,0x833578,0],ecx=INI)&255, f'{name}: native Crate read refused')
        u.reg_write(UC_X86_REG_ESI,pointer);u.reg_write(UC_X86_REG_EDI,pointer+0x24)
        u.reg_write(UC_X86_REG_EBX,INI)
        u.reg_write(UC_X86_REG_ESP,sp)
        run_checked(u,0x5FE7FF,0x5FE839,count=100000)
        assert u.reg_read(UC_X86_REG_ESP)==sp
        assert bytes(u.mem_read(pointer+0x2AA,2))==b'\1\1', (name,bytes(u.mem_read(pointer+0x2AA,2)))
    f.ordered_ini({'CrateRules':f.rules_input['CrateRules']})
    f.call(0x66B900,[INI],ecx=rules)
    f.ordered_ini({'Powerups':f.rules_input['Powerups']})
    f.call(0x673E80,[INI],ecx=rules)
    # Fixture-local sample slots are the existing sound IO transport boundary;
    # native SoundList/CrateMoney lookup owns the Voc identity, not this number.
    sound=initialize_animation_sounds(f,('CrateMoney',),sample_ids={'ubonus':0})
    f.ordered_ini({'AudioVisual':f.rules_input['AudioVisual']})
    # Native ReadString + Voc lookup for this exact AudioVisual key. Whole AV
    # catalog loading already belongs to initialize_animation_catalog.
    key=f.allocate(512); value=f.allocate(512); section=f.allocate(512)
    u.mem_write(key,b'CrateMoneySound\0');u.mem_write(section,b'AudioVisual\0')
    f.call(0x528A10,[section,key,0x889F64,value,128],ecx=INI)
    money_sound=f.call(0x7514D0,[],ecx=value)
    u.mem_write(rules+0x1E4,dwords(money_sound))
    index=f.catalog['names'].index('MONEY')
    _,art=f.read_anim_art(index,f.art_input['MONEY'])
    # Active House priors: local control, nonpassive, one building, no storage.
    u.mem_write(HOUSES,dwords(0x7EA8A0));u.mem_write(HOUSES+0x24,dwords(0x7EA834))
    u.mem_write(HOUSES+0x2F0,dwords(1));u.mem_write(HOUSES+0x30C,dwords(case.get('balance',100)))
    u.mem_write(HOUSES+0x2FC,bytes(16));u.mem_write(HOUSES+0x200+0x148,struct.pack('<f',1.0))
    u.mem_write(actor+0x14,dwords(5));u.mem_write(actor+0x90,b'\1')
    u.mem_write(actor+0x158,struct.pack('<2d',1.0,1.0))
    u.mem_write(0xA83D4C,dwords(HOUSES));u.mem_write(0xA8B238,dwords(3))
    u.mem_write(0xA8B261,bytes([case.get('regenerate',True)]))
    f.call(0x47B2F0,[]);f.call(0x5FC310,[])
    proxy=SimpleNamespace(u=u,scenario=anim_scenario)
    def rng():return CampaignReader.rng_state(proxy)
    def slots():return [list(struct.unpack('<3i2h',u.mem_read(MAP+0x158+i*16,16))) for i in range(2)]
    def cell_state():return dict(xy=list(struct.unpack('<2h',u.mem_read(cell+0x24,4))),
        overlay=f.integer(cell+0x44),data=u.mem_read(cell+0x11E,1)[0])
    events=[];ranged=[];raw=[];centers=[];pending={};checkpoints=[]
    object_marks=[];mark_pending={};overlay_objects=[];phase='initial_placement'
    def mark_state(pointer):
        return dict(native_id=f.read32(pointer+0x10),alive=bool(u.mem_read(pointer+0x90,1)[0]),
            limbo=bool(u.mem_read(pointer+0x81,1)[0]),marked=bool(u.mem_read(pointer+0x74,1)[0]),
            redraw=bool(u.mem_read(pointer+0x80,1)[0]))
    def observe(vm,pc,size,data):
        stack=vm.reg_read(UC_X86_REG_ESP);receiver=vm.reg_read(UC_X86_REG_ECX)
        if pc in mark_pending:
            row,pointer=mark_pending.pop(pc)
            row.update(returned=vm.reg_read(UC_X86_REG_EAX)&255,after=mark_state(pointer))
        if pc==0x5F5850:
            vtable=f.read32(receiver)
            assert vtable in (0x7EF3D4,0x7E3354), f'unsupported Mark receiver {vtable:08X}'
            row=dict(phase=phase,site=None if f.read32(stack)==RET_MAGIC else f'{f.read32(stack)-5:08X}',
                kind='overlay' if vtable==0x7EF3D4 else 'anim',mode=f.integer(stack+4),
                before=mark_state(receiver))
            object_marks.append(row);mark_pending[f.read32(stack)]=(row,receiver)
        if pc==0x5FC380:overlay_objects.append(receiver)
        if pc in pending:
            pending.pop(pc)['result']=f.integer_reg(UC_X86_REG_EAX)
        if pc==0x65C7E0:
            row=dict(site=f'{f.read32(stack)-5:08X}',stream=f'{receiver:08X}',
                bounds=list(struct.unpack('<2i',vm.mem_read(stack+4,8))))
            ranged.append(row);pending[f.read32(stack)]=row
        if pc in (0x65C788,0x65C837):
            stream=receiver if pc==0x65C788 else vm.reg_read(UC_X86_REG_EDX)
            raw.append(f'{stream:08X}')
        if pc in (0x56C020,0x56BD40,0x4A17C0,0x5FC380,0x5FC570,0x4F9950,0x421EA0):
            row=dict(pc=f'{pc:08X}')
            if pc==0x4F9950:row.update(amount=f.integer(stack+4),balance_before=f.integer(receiver+0x30C))
            if pc==0x421EA0:row.update(coord=list(struct.unpack('<3i',vm.mem_read(f.read32(stack+8),12))))
            events.append(row)
        if pc in (0x481D9C,0x481DB8,0x4824D9):
            checkpoints.append(dict(pc=f'{pc:08X}',cell=cell_state(),slots=slots(),
                balance=f.integer(HOUSES+0x30C),ranged_calls=len(ranged),raw_draws=len(raw),rng=rng()))
        if pc==0x47B3A0 and f.read32(stack) in (0x48252E,0x483339):
            centers.append(dict(site=f'{f.read32(stack)-5:08X}',receiver_matches=receiver==cell,
                xy=list(struct.unpack('<2h',vm.mem_read(receiver+0x24,4))),
                fractional=list(struct.unpack('<2i',vm.mem_read(f.read32(stack+4),8)))))
        if pc in (0x47FDE0,0x47FB90):
            output=f.read32(stack+4);vm.mem_write(output,bytes(16));return_from_sink(vm,4,output)
        elif pc==0x6D2790:return_from_sink(vm,20,0)
        elif pc==0x4068E0:return_from_sink(vm,0,0)
    hook=u.hook_add(UC_HOOK_CODE,observe)
    for stream in (anim_scenario+0x218,0x886B88,0xABE890):f.call(0x65C6D0,[31],ecx=stream)
    f.call(0x56BEC0,[struct.unpack('<I',packed(*pickup_xy))[0],case.get('scalar',20)],ecx=MAP)
    initial_cell=cell_state();initial_slots=slots();phase='pickup'
    assert initial_cell['overlay']>=0
    # Seed is an independent input after initial original placement; output is
    # not derived from VERA or from a replayed hand-written RNG implementation.
    for stream in (anim_scenario+0x218,0x886B88,0xABE890):f.call(0x65C6D0,[case['seed']],ecx=stream)
    events.clear();ranged.clear();raw.clear();centers.clear();pending.clear();f.events.clear()
    before=rng();f.draw_count=0;id_cursor_before=f.read32(anim_scenario+0x214)
    initial_type=f.read32(f.read32(0xA83D84)+initial_cell['overlay']*4)
    assert u.mem_read(initial_type+0x2AA,1)[0]
    u.mem_write(sp,dwords(RET_MAGIC,actor));u.reg_write(UC_X86_REG_ESP,sp);u.reg_write(UC_X86_REG_ECX,cell)
    run_checked(u,0x481A00,0x481D86,count=300000,required_addresses=[0x481A00])
    selected=u.reg_read(UC_X86_REG_EBX)
    assert selected==0, ('native selection is not Money',case,selected)
    run_checked(u,0x481D86,RET_MAGIC,count=1000000)
    assert u.reg_read(UC_X86_REG_ESP)==sp+8
    pickup_return=u.reg_read(UC_X86_REG_EAX)&255
    id_cursor_after=f.read32(anim_scenario+0x214)
    pickup_events=f.events.copy();after=rng();anim=f.read32(f.read32(0xA8E9AC))
    initial_anim=f.anim_snapshot(anim);history=[]
    # Negative controls use actual retained objects, with no supplied flags or
    # returns: the transient Overlay is now limbo; the live Anim is marked.
    phase='controls';mark_controls=[]
    for label,pointer in (('limbo_overlay',overlay_objects[0]),('already_marked_anim',anim)):
        state=mark_state(pointer)
        returned=f.call(0x5F5850,[1],ecx=pointer)&255
        # run_checked stops before RET_MAGIC; native callers are observed
        # at their return PC, while this external control is finalized here.
        row,observed_pointer=mark_pending.pop(RET_MAGIC)
        assert observed_pointer==pointer
        row.update(returned=returned,after=mark_state(pointer))
        mark_controls.append(dict(name=label,before=state,returned=returned,after=mark_state(pointer)))
    phase='animation'
    for frame in range(100,1100):
        u.mem_write(0xA8ED84,dwords(frame));f.events.clear()
        f.call(0x423AC0,[],ecx=anim)
        if frame==100 or not u.mem_read(anim+0x90,1)[0]:
            history.append(dict(frame=frame,state=f.anim_snapshot(anim),calls=f.events.copy()))
        if not u.mem_read(anim+0x90,1)[0]:break
    else:raise AssertionError('MONEY animation failed to expire within1000 frames')
    phase='cleanup';f.events.clear();f.call(0x725C70,[])
    cleanup=dict(pending_delete_count=f.integer(0xB0F6A8),anim_count=f.integer(0xA8E9B8),calls=f.events.copy())
    assert not mark_pending
    u.hook_del(hook);f.check_code()
    # Consecutive identical frame observations carry their actual call count;
    # retain state transitions without a repeated per-frame trace.
    compact_marks=[]
    for row in object_marks:
        if compact_marks and row=={k:v for k,v in compact_marks[-1].items() if k!='count'}:
            compact_marks[-1]['count']+=1
        else:
            compact_marks.append(dict(row,count=1))
    return dict(input=dict(case,geometry=geometry,frame=100,pickup_cell=list(pickup_xy),
        balance=case.get('balance',100),placement_seed=31,mode=3,
        game_speed=f.integer(0xA8EB60)),
        id_cursor_before=id_cursor_before,id_cursor_after=id_cursor_after,
        id_cursor_after_cleanup=f.read32(anim_scenario+0x214),retail=dict(files=f.identity,powerups=f.rules_input['Powerups'],
        crate_rules=f.rules_input['CrateRules'],overlay_flags={name:dict(crate=u.mem_read(ptr+0x2AA,1)[0],
            trigger=u.mem_read(ptr+0x2AB,1)[0]) for name,ptr in overlay_types.items()},
        weights=list(struct.unpack('<19i',u.mem_read(0x81DA8C,76))),
        money_magnitude=struct.unpack('<d',u.mem_read(0x89EC28,8))[0],
        regen_minutes=struct.unpack('<d',u.mem_read(rules+0x1678,8))[0],sound=sound,art=art),
        initial_cell=initial_cell,initial_slots=initial_slots,selected=selected,
        returned=pickup_return,balance=f.integer(HOUSES+0x30C),
        final_cell=cell_state(),final_slots=slots(),events=events,checkpoints=checkpoints,
        ranged=ranged,raw_streams=raw,centers=centers,pickup_events=pickup_events,
        object_marks=compact_marks,mark_controls=mark_controls,
        rng_before=before,rng_after=after,rng_after_cleanup=rng(),
        initial_anim=initial_anim,history=history,cleanup=cleanup)


def cash_generate():
    return [cash_case(case) for case in (
        dict(name='weighted_money_replacement',seed=2),
        dict(name='fixed_money_replacement',seed=31,scalar=0),
        dict(name='weighted_money_no_replacement',seed=2,regenerate=False))]


def cash_metadata():
    return provenance(entry_points={'pickup':0x481A00,'remove':0x56C020,'replacement':0x56BD40,
        'place':0x56BEC0,'credits':0x4F9950,'powerups_reader':0x673E80,'crate_rules_reader':0x66B900,
        'anim_ctor':0x421EA0,'anim_ai':0x423AC0,'deferred_drain':0x725C70,
        'resize_field_stores':0x5662AC,'normalize_local_size':0x567230,
        'object_mark':0x5F5850,'mark_redraw':0x5F4D10},
        assumptions=['Prepared local neutral-income Unit/House, House owns a building; no attached Tag. ExistingAnimationFixture supplies storedGameOptions speed4; eachrowreadsbacktheactualvalue.',
            'Shared32x32 flat allocated-cell map; Size(0,0,16,16) and rawLocalSize(0,0,16,16) priors; originalResize5662AC..56633E establishes MapRect and full567230 normalizesLocalSize. Synthetic1x1 resident TMP metadata and empty registries. Slot storage starts zeroed; emptiness uses only coordinates. Native lookup/height/placement remain original.',
            'Original initial crate placement; seed reset is an explicit independent pickup input. Physical stock RULESMD/ARTMD/SOUNDMD bindings; no mode/map overrides are claimed.'],
        substitutions=['Cached INI/retail asset IO, allocator/TLS and Windows pointer validation use the existing AnimationFixture transport.',
            'ObjectMark5F5850 admission and marked/redraw mutations execute original code for Overlay and Anim; historical AnimationFixture Mark substitution is disabled for cash. Screen rectangles/dirty sinks remain supplied. DisplaySubmit4A9720 returns1; the reachedObjectUnlimbo caller ignores this return. Sound sample lookup uses declared fixture-local ubonus slot0; native Voc lookup bindsCrateMoney. Playback stops at7509E0; no audio scheduler RNG parity.'],
        scope='Bounded original weighted/fixed Money pickup through removal, optional replacement, amount/AddCredits, separate sound/tail center sampling and MONEY Anim lifecycle. No free-MCV, attached trigger, other reward, full skirmish bootstrap, audio device or rendering parity.')


if __name__ == '__main__':
    import sys
    if '--cash' in sys.argv[1:]:
        from tools.spatial_oracle.trigger_action_values import SOURCE_PATHS
        sources=dict(SOURCE_PATHS,pickup=Path(__file__),source_map=Path(__file__).with_name('unit_source_scatter.py'),
            source_state=Path(__file__).with_name('unit_scatter_state.py'),speed_setup=Path(__file__).with_name('crate_speed_effect.py'))
        finish_vectors(cash_generate,Path(__file__).with_name('cash_crate.json'),provenance=cash_metadata,
            argv=[arg for arg in sys.argv[1:] if arg!='--cash'],source_paths=sources)
        sys.exit(0)
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'pickup': 0x481A00, 'dispatch': 0x481DE0, 'remove_by_cell': 0x56C020,
                      'slot_clear': 0x4A1750, 'remove_overlay': 0x4A1AA0},
        assumptions=['Original vtables/callees; flat crate cell, stored runtime slot and supplied actor/house/type state.',
                     'House owns one building, so free-MCV preemption is excluded. No attached crate trigger; regeneration disabled and no pickup animation.',
                     'PC53/chop; stock weight table unless the input supplies weights. Speed effect executes to original return; other effects stop at the dispatch jump.'],
        substitutions=['47FDE0/47FB90 return empty screen rectangles;6D2790 screen-dirty sink returns without rendering. No gameplay receiver substituted.'],
        scope='28 original full pickup/prefix witnesses for speed, guards, weighted selection including equal bounds, fallback, ordered solo image overrides, first-match slots, image identity and registered/unregistered removal. Other effects, free-MCV, trigger, replacement generation and presentation remain outside coverage.',
    ))
