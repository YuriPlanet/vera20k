"""Original zone-3 movement action remaps and pre-admission water-state writes.

Supplied live map/type/sequence state; original callables execute unchanged.
Audio is explicitly disabled at the original global gate; requests are observed.
"""
from pathlib import Path
import hashlib
import os
import struct
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.native_oracle import load_image, run_checked, STACK_BASE, STACK_SIZE, SCRATCH, RET_MAGIC, finish_vectors, provenance

ACTOR, TYPE, SEQUENCES, LOCO, CELL = [SCRATCH + n * 0x4000 for n in range(5)]
TABLE, MAP = 0xC00000, 0x87F7E8


def query(row):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(SCRATCH, 0x20000)
    uc.mem_map(RET_MAGIC, 0x1000)
    uc.reg_write(UC_X86_REG_FPCW, 0x0E7F)

    def write32(address, *values):
        uc.mem_write(address, struct.pack('<' + 'I' * len(values), *(v & 0xffffffff for v in values)))

    def read32(address):
        return struct.unpack('<i', uc.mem_read(address, 4))[0]

    def call(address, owner, args, count=10000):
        sp = STACK_BASE + STACK_SIZE - 0x1000
        write32(sp, RET_MAGIC, *args)
        uc.reg_write(UC_X86_REG_ESP, sp)
        uc.reg_write(UC_X86_REG_ECX, owner)
        run_checked(uc, address, RET_MAGIC, count=count, required_addresses=[address])
        assert uc.reg_read(UC_X86_REG_ESP) == sp + 4 * (len(args) + 1)

    write32(ACTOR, 0x7EB058)
    write32(ACTOR + 0x6C0, TYPE)
    write32(TYPE + 0xE3C, SEQUENCES)
    for action in range(42):
        write32(SEQUENCES + action * 36 + 4, 6)
    if row.get('absent') is not None:
        write32(SEQUENCES + row['absent'] * 36 + 4, 0)
    write32(TYPE + 0x5B4, row.get('zone', 3))
    write32(TYPE + 0xEA4, 101)
    write32(TYPE + 0xEA8, 102)
    write32(ACTOR + 0x6C4, row.get('current', -1))
    write32(ACTOR + 0x6C, row.get('health', 100))
    write32(ACTOR + 0x9C, 2688, 2688, 0)
    uc.mem_write(ACTOR + 0x90, b'\x01')
    uc.mem_write(ACTOR + 0x8C, bytes([row.get('bridge', False)]))
    write32(ACTOR + 0x6E8, row.get('old_water_state', 1))
    write32(ACTOR + 0x6D4, row.get('fear', 0))
    write32(ACTOR + 0xAC, 5)
    write32(ACTOR + 0xB4, -1)
    write32(ACTOR + 0x100, 17, 0, 91, 92)
    write32(ACTOR + 0xF8, 7)
    if row.get('extended'):
        uc.mem_write(TYPE + 0xEBD, bytes([row.get('crawls', True)]))
    write32(0xA8ED84, 100)
    uc.mem_write(0x8464AC, b'\x00')
    table = bytearray(0x100000)
    struct.pack_into('<I', table, (10 * 512 + 10) * 4, CELL)
    uc.mem_write(TABLE, bytes(table))
    write32(MAP + 0x13C, TABLE, 0x40000)
    write32(0x87F924, TABLE)
    write32(CELL, 0x7E4EEC)
    uc.mem_write(CELL + 0x24, struct.pack('<hh', 10, 10))
    write32(CELL + 0xEC, row.get('land', 2))
    call(0x75AA90, LOCO, [])
    write32(LOCO + 0xC, ACTOR)
    write32(ACTOR + 0x674, LOCO + 4)
    events = []
    sounds = []
    writes = []
    rngs = {}
    if row.get('extended'):
        from tools.spatial_oracle.shrapnel_repair.shrapnel_repair import rng_state
        rngs = {'main': 0x886B88, 'scenario': SCRATCH + 0x18218,
                'mapgen': 0xABE890}
        write32(0xA8B230, SCRATCH + 0x18000)
        for address in rngs.values():
            call(0x65C6D0, address, [row.get('seed', 0)], count=100000)
        rng_before = {key: rng_state(uc, ptr) for key, ptr in rngs.items()}
        text_before = hashlib.sha256(bytes(uc.mem_read(0x401000, 0x3E0000))).hexdigest()

    def snapshot():
        return dict(doing=read32(ACTOR + 0x6C4), water_state=read32(ACTOR + 0x6E8),
                    frame=read32(ACTOR + 0xF8), timer_start=read32(ACTOR + 0x100),
                    timer_duration=read32(ACTOR + 0x108), timer_repeat=read32(ACTOR + 0x10C),
                    prone=uc.mem_read(ACTOR + 0x6DB, 1)[0])

    before = snapshot()

    def observe(_uc, address, _size, _data):
        if address in (0x51D6F0, 0x41BEA0, 0x5657A0, 0x51D8B8, 0x51D90B, 0x51D925, 0x51D9D2, 0x51DA34, 0x7509E0):
            events.append(hex(address))
        if address == 0x7509E0:
            sounds.append({'index': uc.reg_read(UC_X86_REG_ECX),
                           'xyz': list(struct.unpack('<iii', uc.mem_read(uc.reg_read(UC_X86_REG_EDX), 12))),
                           'context': read32(uc.reg_read(UC_X86_REG_ESP) + 4),
                           'water_state_at_call': read32(ACTOR + 0x6E8)})

    def observe_write(_uc, _access, address, size, value, _data):
        if address in (ACTOR + 0x6E8, ACTOR + 0x6C4, ACTOR + 0xF8,
                       ACTOR + 0x100, ACTOR + 0x108, ACTOR + 0x10C):
            writes.append(dict(pc=hex(uc.reg_read(UC_X86_REG_EIP)),
                               offset=hex(address - ACTOR), size=size,
                               old=int.from_bytes(uc.mem_read(address, size), 'little', signed=True),
                               new=value, water_state=read32(ACTOR + 0x6E8),
                               doing=read32(ACTOR + 0x6C4)))

    uc.hook_add(UC_HOOK_CODE, observe)
    if row.get('extended'):
        uc.hook_add(UC_HOOK_MEM_WRITE, observe_write)
    call(0x51D6F0, ACTOR, [row.get('request', 3), int(row.get('force', False)), 0])
    output = {'input': row, 'accepted': uc.reg_read(UC_X86_REG_EAX) & 0xff,
            'doing': read32(ACTOR + 0x6C4), 'water_state': read32(ACTOR + 0x6E8),
            'frame': read32(ACTOR + 0xF8), 'timer_start': read32(ACTOR + 0x100),
            'timer_duration': read32(ACTOR + 0x108), 'timer_repeat': read32(ACTOR + 0x10C),
            'events': events, 'sounds': sounds}
    if row.get('extended'):
        rng_after = {key: rng_state(uc, ptr) for key, ptr in rngs.items()}
        assert rng_after == rng_before
        assert text_before == hashlib.sha256(bytes(uc.mem_read(0x401000, 0x3E0000))).hexdigest()
        output.update(kind='action', before=before, after=snapshot(), writes=writes,
                      rng_before=rng_before, rng_after=rng_after,
                      text_sha256=text_before, vtable=hex(read32(ACTOR)),
                      entry='0x0051D6F0', stop='RET_MAGIC', game_speed_index=read32(0xA8EB60))
    return output


def constructor_and_retail_receipts():
    """Reuse the existing live Foot setup; do not create another reader/mapper."""
    from tools.spatial_oracle.anytown_damage.foot_missions import FootMissions
    from tools.spatial_oracle.anytown_damage import mission as native_owner
    from tools.rules_oracle.bridge_child_sound import Sound, sections as sound_sections
    from tools.projectile_oracle.bridge_render_inputs import lexical
    from tools.spatial_oracle.building_body_rules import INI, RULES, dwords

    class Water(FootMissions):
        def observe(self, u, address, size, data):
            if address == 0x7509E0:
                # Execute the original body at its supplied disabled gate,
                # rather than the existing mission owner's audio callback.
                index = u.reg_read(UC_X86_REG_ECX)
                name = None
                if index < self.m.read32(0xB1D388):
                    voc = self.m.read32(self.m.read32(0xB1D37C) + index * 4)
                    name = self.m.string(self.m.read32(voc) + 0x6C)
                if hasattr(self, 'water_sounds'):
                    self.water_sounds.append(dict(index=index, name=name,
                        xyz=native_owner.base.xyz(u, u.reg_read(UC_X86_REG_EDX)),
                        context=native_owner.base.i32(u, u.reg_read(UC_X86_REG_ESP) + 4),
                        water_state_at_call=native_owner.base.i32(u, self.water_actor + 0x6E8),
                        doing_at_call=native_owner.base.i32(u, self.water_actor + 0x6C4)))
                return
            super().observe(u, address, size, data)

    q = Water()
    m, u = q.m, q.u
    u.mem_write(0x8464AC, b'\0')
    original_invoke = m.invoke
    constructors = []
    active = [None]

    def rng():
        return {key: native_owner.base.sr.rng_state(u, ptr)
                for key, ptr in q.resident.rngs.items()}

    def signed(address):
        return native_owner.base.i32(u, address)

    def ctor_snap(owner, entry):
        offsets = (0xEA4, 0xEA8) if entry == 0x5236A0 else (0x6C4, 0x6E8)
        return {hex(off): signed(owner + off) for off in offsets}

    def invoke(entry, owner, args=()):
        if entry not in (0x517A50, 0x5236A0):
            return original_invoke(entry, owner, args)
        row = dict(entry=hex(entry), args=list(args), before=ctor_snap(owner, entry),
                   rng_before=rng(), writes=[])
        if entry == 0x5236A0:
            row['type_name'] = m.string(args[0])
        else:
            row['type_name'] = m.string(args[0] + 0x24)
        active[0] = (owner, entry, row)
        answer = original_invoke(entry, owner, args)
        row.update(after=ctor_snap(owner, entry), returned_eax=answer,
                   vtable=hex(m.read32(owner)), rng_after=rng())
        constructors.append(row)
        active[0] = None
        return answer

    def ctor_write(_u, _access, address, size, value, _data):
        if active[0] is None:
            return
        owner, entry, row = active[0]
        offsets = (0xEA4, 0xEA8) if entry == 0x5236A0 else (0x6C4, 0x6E8)
        if address - owner in offsets:
            row['writes'].append(dict(pc=hex(u.reg_read(UC_X86_REG_EIP)),
                offset=hex(address - owner), size=size,
                old=int.from_bytes(u.mem_read(address, size), 'little', signed=True),
                new=value))

    hook = u.hook_add(UC_HOOK_MEM_WRITE, ctor_write)
    m.invoke = invoke
    q.initialize_companion()
    text_before = hashlib.sha256(bytes(u.mem_read(0x401000, 0x3E0000))).hexdigest()
    root = Path(os.environ['VERA20K_SHRAPNEL_INPUTS'])
    sound_raw = (root / 'SOUNDMD.INI').read_bytes()
    physical_sound = sound_sections(sound_raw)
    wanted = {'TanyaEntersWater', 'TanyaLeavesWater'}
    selected_sound = {'SoundList': {key: name for key, name in physical_sound['SoundList'].items()
                                   if name in wanted}}
    assert set(selected_sound['SoundList'].values()) == wanted
    # Original registration and lookup execute. Voc entry configuration,
    # sample IO and playback are excluded; the native audio gate is false.
    proxy = Sound.__new__(Sound)
    proxy.__dict__ = m.__dict__
    Sound.make_ini(proxy, selected_sound)
    m.invoke(0x7510D0, INI)
    sound_bindings = {name: m.invoke(0x7514D0, m.cstring(name)) for name in sorted(wanted)}
    assert all(index != 0xFFFFFFFF for index in sound_bindings.values())
    art_raw = (root / 'ARTMD.INI').read_bytes()
    art, art_lines = lexical(art_raw, {'SEAL', 'TANY', 'SealSequence', 'TanyaSequence'})
    m.make_ini(art)
    types, readers = {}, []
    for name in ('GHOST', 'TANY'):
        typ = m.alloc(0x1900)
        m.invoke(0x5236A0, typ, (m.cstring(name),))
        defaults = dict(enter_sound=signed(typ + 0xEA4), leave_sound=signed(typ + 0xEA8))
        history = []
        for layer, path in native_owner.base.layers():
            if not path.exists():
                assert layer == 'LANGRULE.INI'
                history.append(dict(file=layer, absent=True))
                continue
            raw = path.read_bytes()
            selected, lines = lexical(raw, {'GHOST', 'TANY'})
            m.rules_cache(selected)
            before = dict(enter_sound=signed(typ + 0xEA4), leave_sound=signed(typ + 0xEA8))
            answer = m.invoke(0x5240A0, typ, (RULES,))
            history.append(dict(file=layer, sha256=hashlib.sha256(raw).hexdigest(),
                inputs=selected, lines=lines, returned_al=answer & 255, before=before,
                after=dict(enter_sound=signed(typ + 0xEA4), leave_sound=signed(typ + 0xEA8))))
        table = m.read32(typ + 0xE3C)
        assert table
        records = [list(struct.unpack('<9i', u.mem_read(table + action * 36, 36)))
                   for action in range(42)]
        actor = m.alloc(0x1000)
        m.invoke(0x517A50, actor, (typ, 0))
        u.mem_write(actor + 0x21C, dwords(q.house))
        u.mem_write(actor + 0x9C, bytes(u.mem_read(q.e1 + 0x9C, 12)))
        types[name] = (typ, actor)
        readers.append(dict(kind='retail_reader', type_name=name, defaults=defaults,
            history=history, movement_zone=signed(typ + 0x5B4),
            sound_bindings=sound_bindings, sounds=dict(enter=signed(typ + 0xEA4),
                leave=signed(typ + 0xEA8)), records=records,
            art_sha256=hashlib.sha256(art_raw).hexdigest(), art=art, art_lines=art_lines,
            sound_sha256=hashlib.sha256(sound_raw).hexdigest(), selected_sound=selected_sound))
    u.hook_del(hook)
    m.invoke = original_invoke
    q.phase = 'logic'
    cell = q.resident.ptrs[tuple(q.e1_placement['xyz'][:2][i] // 256 for i in range(2))]
    cell_before = bytes(u.mem_read(cell, 0x200))
    rows = []

    def snap(actor):
        return dict(doing=signed(actor + 0x6C4), water_state=signed(actor + 0x6E8),
            frame=signed(actor + 0xF8), timer_start=signed(actor + 0x100),
            timer_duration=signed(actor + 0x108), timer_repeat=signed(actor + 0x10C),
            health=signed(actor + 0x6C), prone=u.mem_read(actor + 0x6DB, 1)[0])

    for name, (typ, actor) in types.items():
        baseline = bytes(u.mem_read(actor, 0x1000))
        loco = m.read32(actor + 0x674) - 4
        loco_before = bytes(u.mem_read(loco, 0x100))
        cases = [dict(request=request, land=land, bridge=False, old_water_state=old)
                 for request in (4, 8, 9, 10, 11, 12) for land in (2, 6) for old in (1, 2)]
        cases += [dict(request=request, land=2, bridge=True, old_water_state=0)
                  for request in (11, 12)]
        cases += [dict(request=0, land=0, bridge=False, old_water_state=0)]
        cases += [dict(request=request, land=2, bridge=False, old_water_state=2, health=0)
                  for request in (11, 12)]
        for input in cases:
            u.mem_write(actor, baseline)
            u.mem_write(loco, loco_before)
            u.mem_write(cell, cell_before)
            u.mem_write(cell + 0xEC, dwords(input['land']))
            u.mem_write(actor + 0x8C, bytes([input['bridge']]))
            u.mem_write(actor + 0x6C4, dwords(-1))
            if 'health' in input:
                u.mem_write(actor + 0x6C, dwords(input['health']))
            u.mem_write(actor + 0x6E8, dwords(input['old_water_state']))
            u.mem_write(actor + 0xF8, dwords(7))
            u.mem_write(actor + 0x100, dwords(17, 0, 91, 92))
            u.mem_write(0xA8ED84, dwords(100))
            for ptr in q.resident.rngs.values():
                m.invoke(0x65C6D0, ptr, (0,))
            q.water_actor, q.water_sounds = actor, []
            before, rng_before = snap(actor), rng()
            answer = m.invoke(0x51D6F0, actor, (input['request'], 0, 0))
            rng_after = rng()
            assert rng_after == rng_before
            rows.append(dict(kind='retail_action', type_name=name,
                input=dict(input, current=-1, seed=0), before=before, after=snap(actor),
                accepted=answer & 255, sounds=list(q.water_sounds),
                rng_before=rng_before, rng_after=rng_after,
                game_speed_index=signed(0xA8EB60), vtable=hex(m.read32(actor)),
                physical_xyz=native_owner.base.xyz(u, actor + 0x9C),
                cell=list(struct.unpack('<hh', u.mem_read(cell + 0x24, 4))),
                entry='0x0051D6F0', stop='RET_MAGIC'))
        u.mem_write(actor, baseline)
        u.mem_write(loco, loco_before)
    u.mem_write(cell, cell_before)
    assert text_before == hashlib.sha256(bytes(u.mem_read(0x401000, 0x3E0000))).hexdigest()
    return ([dict(kind='constructors', constructors=constructors,
                 e1_after_ready=dict(movement_zone=signed(q.e1_type + 0x5B4),
                     water_state=signed(q.e1 + 0x6E8),
                     doing=signed(q.e1 + 0x6C4),
                     ready_returned_al=q.e1_ready & 255),
                 text_sha256=text_before,
                 sound_registry_boundary='Original selected physical SoundList registration; Voc configuration/sample IO/playback excluded, audio gate8464ACfalse.')]
        + readers + rows)


def generate():
    rows = [{'request': request, 'land': land, 'bridge': bridge, 'old_water_state': old, 'current': current}
            for request in (0, 2, 3, 6) for land in (0, 2, 6) for bridge in (False, True)
            for old in (0, 1) for current in (-1, 17, 31)]
    rows += [{'request': 3, 'absent': 3}, {'request': 3, 'absent': 17},
             {'request': 3, 'zone': 0}, {'request': 3, 'fear': 200},
             {'request': 3, 'current': 31, 'force': True},
             {'request': 3, 'old_water_state': -1}, {'request': 3, 'old_water_state': 2}]
    legacy = [query(row) for row in rows]
    # The first151 rows retain their original inputs and complete payloads.
    extended = [dict(extended=True, request=request, land=land, bridge=bridge,
                     old_water_state=old, current=-1)
                for request in (4, 8, 9, 10, 11, 12)
                for land in (0, 2, 6) for bridge in (False, True) for old in (0, 1, 2)]
    extended += [dict(extended=True, request=request, land=2, old_water_state=old,
                      current=current, force=force)
                 for request, current in ((4, 22), (9, 18), (11, 20), (12, 11))
                 for old in (1, 2) for force in (False, True)]
    extended += [dict(extended=True, request=request, land=land, old_water_state=2,
                      current=-1)
                 for request in (1, 13, 14, 15, 20, 21) for land in (0, 2)]
    extended += [dict(extended=True, request=request, absent=absent, land=2,
                      old_water_state=1, current=-1)
                 for request, absent in ((4, 4), (4, 22), (9, 9), (9, 18),
                                         (11, 11), (11, 20), (12, 12), (12, 21))]
    return legacy + [query(row) for row in extended] + constructor_and_retail_receipts()


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=provenance(
        scope='Original zone3 Infantry action remaps and pre-admission water/sound ordering, full Infantry constructor plus layered physical GHOST/TANY type/ART readers and bounded DoAction; no sound playback, scenario loader, full wet death lifecycle or Rust parity.',
        assumptions=[
            'Supplied original Infantry and Cell vtables, currentXYZ2688,2688,0, real Cell10,10, flat level/slope0. Original coordinate and Map5657A0 calls execute; map construction/terrain producers are not executed.',
            'Historical and synthetic action rows supply MovementZone+5B4=3 except zone0 contrast; actor+2DC=0,+74=false, alive+90=true,+6C=100. They exclude carried/highflight/falling/zero-health callbacks and supply all42 sequence counts6 except declared absent slot.',
            'Direct requests0/2/3/6 cross LandType0/2/6, OnBridge false/true, old+6E8=0/1 and currentDoing-1/17/31. Additional contrasts test absent requested/mapped sequence, fear200, force and old+6E8=-1/2.',
            'Frame100, old logical timer17/91/92 and image frame7; ignored middle timer word+104 is excluded. Random-start false. No later timer/sequence advancement.',
            'Type+EA4/+EA8 sound indices supplied101/102. Original global8464AC is explicitly false, so original7509E0 returns at its disable gate. Observed entry arguments prove caller request ordering only, not audible output or stock sound index identity.',
            'Native zone identity: ReadINI71605E..716081 writes type+5B4 from474E40; its names table81BA88 has index3 pointer81BB38=AmphibiousDestroyer.',
            'The first151 payload rows retain their historical complete values. New action rows execute original request4/8/9/10/11/12 remaps, unmapped/direct raw records, absent requested versus mapped counts, and unchanged/noninterruptible/force contrasts. Three complete RNG streams are initialized with original65C6D0 and compared before/after; no gameplay result is supplied.',
            'Constructor/read receipts reuse FootMissions original setup and actual Infantry517A50/InfantryType5236A0 calls. Original517AC2 writes6E8=2; original523748/52374E write EA4/EA8=-1. The original E1 Ready receiver returns with its non-zone3 water state2 unchanged. Original Foot/Techno/Walk constructors and supplied allocation/House/OS boundaries remain inherited; no native scenario-load or whole House construction claim.',
            'Physical GHOST/TANY ReadINI5240A0 executes across RULESMD, absent LANGRULE, MPBattleMD and XMP03T4 cached layers; physical ARTMD SEAL/TANY/SealSequence/TanyaSequence produces all42 signed records. Original52440B/524447 ReadString128 then7514D0 resolve exact EnterWaterSound/LeaveWaterSound keys against original selected physical SoundList registration. Voc configuration, sample IO and playback are excluded, and fixture-relative indices are named explicitly.',
            'Full retail DoAction rows supply constructor-built GHOST/TANY actors physical XYZ22272,12544,416 on the existing real Cell87,49, LandType0/2/6 and OnBridge controls. They do not execute their Unlimbo or prove deck occupancy. Health0 controls execute the real Stop_Driver re-entry, while random-start is false and speed index A8EB60 is supplied0. Audio7509E0 itself executes unchanged through its supplied disabled gate; three full RNG streams remain unchanged.',
        ], substitutions=[
            'Only constructor/retail receipts inherit existing FootMissions allocation, prepared physical INI/ART caches, cropped map, supplied House and OS callbacks. The existing mission audio callback is bypassed so original7509E0 executes at its explicit disabled gate; no DoAction/map/sound decision is replaced.'
        ], entry_points={'do_action':0x51D6F0,'packed_cell_query':0x5657A0,'sound_request':0x7509E0,'walk_constructor':0x75AA90,
                         'infantry_constructor':0x517A50,'infantry_type_constructor':0x5236A0,'infantry_type_reader':0x5240A0,
                         'sequence_reader':0x523D00,'sound_list':0x7510D0,'sound_lookup':0x7514D0,'rng_seed':0x65C6D0}),
        source_paths={'generator':Path(__file__), 'foot_setup':Path(__file__).parent/'anytown_damage/foot_missions.py',
                      'mission_setup':Path(__file__).parent/'anytown_damage/mission.py',
                      'physical_reader':Path(__file__).parents[1]/'projectile_oracle/bridge_render_inputs.py',
                      'sound_owner':Path(__file__).parents[1]/'rules_oracle/bridge_child_sound.py'})
