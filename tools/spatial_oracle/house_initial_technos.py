"""House50D610 records and Action130 after a real foreign-owner transfer."""
import struct
from pathlib import Path

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX,
                              UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_ESI,
                              UC_X86_REG_ESP)

from tools import native_oracle as native
from tools.spatial_oracle import building_construction as construction
from tools.spatial_oracle import building_sale_triggers as sale
from tools.spatial_oracle import engineer_repair_admission as engineer
from tools.spatial_oracle.tag_lifecycle import REGISTRIES


def definitions():
    result = sale.synthetic_definitions('OWNER', [(48, 14, 1)])
    restore = sale.synthetic_definitions('RESTORE', [(13, 130, 0)])
    # This component directly dispatches the reader-created action. It does
    # not poll an elapsed event or claim a natural scenario-startup history.
    restore['Events']['RESTORE_0'] = '1,13,0,999'
    for section, entries in restore.items():
        result[section].update(entries)
    return result


def initialize_records(fixture, house):
    """Original member constructor and HouseCtor's +16C growth-step tail."""
    member = house + 0x16C
    padding = bytes(fixture.u.mem_read(house + 0x17A, 2))
    native.require(construction.invoke(fixture.u, 0x510690, member, 0, 0) == member,
                   "original initial-record member constructor refused")
    for register, value in ((UC_X86_REG_ESI, member), (UC_X86_REG_EBX, 0),
                            (UC_X86_REG_EDI, 10)):
        fixture.u.reg_write(register, value)
    native.run_checked(fixture.u, 0x4F562B, 0x4F5637, count=16)
    assert bytes(fixture.u.mem_read(house + 0x17A, 2)) == padding


def records(fixture, house):
    member = house + 0x16C
    count = sale.signed(fixture.u, member + 16)
    assert 0 <= count <= 16
    result = []
    for index in range(count):
        record = fixture.read32(fixture.read32(member + 4) + index * 4)
        typ = fixture.read32(record)
        assert typ == fixture.types['GAPOWR']
        result.append(dict(type='GAPOWR', cell=list(struct.unpack(
            '<2h', fixture.u.mem_read(record + 4, 4)))))
    return result


def snapshot(fixture, objects):
    rows = []
    for pointer in objects:
        assert fixture.read32(pointer + 0x520) == fixture.types['GAPOWR']
        rows.append(dict(owner=fixture.owner_name(fixture.read32(pointer + 0x21C)),
            type='GAPOWR', health=sale.signed(fixture.u, pointer + 0x6C),
            alive=bool(fixture.u.mem_read(pointer + 0x90, 1)[0]),
            marked=bool(fixture.u.mem_read(pointer + 0x74, 1)[0]),
            limbo=bool(fixture.u.mem_read(pointer + 0x81, 1)[0]),
            location=list(struct.unpack('<3i', fixture.u.mem_read(pointer + 0x9C, 12)))))
    pending = REGISTRIES['pending']
    count = sale.signed(fixture.u, pending + 16)
    assert 0 <= count <= 16
    pending_objects = [fixture.read32(fixture.read32(pending + 4) + i * 4)
                       for i in range(count)]
    assert all(pointer in objects for pointer in pending_objects)
    return dict(objects=rows, pending=[objects.index(pointer) for pointer in pending_objects],
                records={name: records(fixture, house)
                         for name, house in fixture.houses.items()})


def execute(inputs, tags, *, clear_foreign_resident):
    image = native.image_bytes()
    def cold_houses(fixture):
        # add_house clones the supplied cold House prior, before any Building
        # or Base can allocate buffers. Never clone or reset an active Base.
        for offset in (4, 0x20, 0x38):
            member = sale.HOUSE + 0x5700 + offset
            assert fixture.read32(member + 4) == fixture.read32(member + 16) == 0
        fixture.add_house(inputs.sale_new_country)
        assert len(set(fixture.owner_houses)) == 2
    f = sale.Fixture(inputs, prepare_cold_houses=cold_houses, native_base_reservations=True)
    for house in f.owner_houses:
        initialize_records(f, house)
        for offset in (4, 0x20, 0x38):
            vptr = f.read32(house + 0x5700 + offset)
            raw = native.file_span(image, vptr, 20)[1]
            assert f.read32(vptr + 0x10)
            assert bytes(f.u.mem_read(vptr, 20)) == raw
            f.checked_vtables.append((vptr, raw))
    f.attach(tags['OWNER'])
    construction.invoke(f.u, 0x50D610, f.houses['OLD'])
    captured = snapshot(f, [sale.BLD])
    assert len(captured['records']['OLD']) == 1
    assert captured['records']['NEW'] == []
    f.phase, f.recording = 'sale_completion_owner_transfer', True
    f.u.mem_write(sale.SP, bytes(0x1000))
    f.u.reg_write(UC_X86_REG_ESP, sale.SP)
    f.u.reg_write(UC_X86_REG_EBP, sale.BLD)
    native.run_checked(f.u, sale.BEGIN, sale.END)
    assert f.u.reg_read(UC_X86_REG_ESP) == sale.SP
    assert f.read32(sale.BLD + 0x21C) == f.houses['NEW']
    after_transfer = snapshot(f, [sale.BLD])

    live_tag = construction.invoke(f.u, 0x6E52A0, tags['RESTORE'])
    live_trigger = f.read32(live_tag + 0x28)
    action = inputs.read32(inputs.read32(tags['RESTORE'] + 0xA0) + 0xB0)
    assert live_trigger and inputs.read32(action + 0x2C) == 130
    assert inputs.read32(action + 0x90) == 0
    record = f.read32(f.read32(f.houses['OLD'] + 0x170))
    actual_cell = construction.invoke(f.u, 0x5657A0, 0x87F7E8, record + 4)
    native.require(construction.invoke(f.u, 0x47C520, actual_cell) == sale.BLD,
                   "expected original foreign resident before restoration")
    clear_before = f.rng()
    if clear_foreign_resident:
        f.phase = 'original_foreign_resident_uninit'
        construction.invoke(f.u, f.read32(f.read32(sale.BLD) + 0xF8), sale.BLD)
        native.require(construction.invoke(f.u, 0x47C520, actual_cell) == 0,
                       "original UnInit did not clear the resident")
    clear_rng_unchanged = f.rng() == clear_before
    before = snapshot(f, [sale.BLD])
    assert before['records'] == captured['records']

    created, placements, reached = [], [], set()
    watched = {0x6DD8B0, 0x6E3120, 0x50D320, 0x43B740, 0x458A80,
               0x440580, 0x5F65F0, 0x7258D0}
    def observe(u, pc, size, data):
        if pc in watched:
            reached.add(pc)
        if pc == 0x43B740:
            created.append(u.reg_read(UC_X86_REG_ECX))
        elif pc == 0x50D419:
            placements.append(u.reg_read(UC_X86_REG_EAX) & 255)
    hook = f.u.hook_add(UC_HOOK_CODE, observe)
    rng_before, draws_start = f.rng(), len(f.draws)
    f.phase = 'original_action130'
    returned = construction.invoke(f.u, 0x6DD8B0, action,
                                  f.houses['OLD'], 0, live_trigger, 0) & 255
    f.u.hook_del(hook)
    assert len(created) == len(placements) == 1
    assert {0x6DD8B0, 0x6E3120, 0x50D320, 0x43B740, 0x458A80} <= reached
    objects = [sale.BLD, *created]
    resident = construction.invoke(f.u, 0x47C520, actual_cell)
    assert resident in (0, *objects)
    code_unchanged = f.code_unchanged()
    vtables_unchanged = all(bytes(f.u.mem_read(at, len(raw))) == raw
                           for at, raw in f.checked_vtables)
    assert code_unchanged and vtables_unchanged
    return dict(input=dict(clear_foreign_resident=clear_foreign_resident,
                           game_mode=f.read32(0xA8B238)),
        captured=captured, after_transfer=after_transfer, before_restore=before,
        after_restore=snapshot(f, objects), action_return=returned,
        placement_return=placements[0], resident=objects.index(resident) if resident else None,
        clear_rng_unchanged=clear_rng_unchanged,
        rng_before=rng_before, rng_after=f.rng(),
        draws=[dict(entry=f"0x{draw['entry']:08X}", site=f"0x{draw['site']:08X}",
                    stream=next(name for name in rng_before
                                if draw['state_before'] == rng_before[name]),
                    result=draw['result']) for draw in f.draws[draws_start:]],
        reached=[f'0x{pc:08X}' for pc in sorted(reached)],
        code_unchanged=code_unchanged, vtables_unchanged=vtables_unchanged)


def generate():
    inputs = engineer.prepare_joined_inputs(engineer.JOINED_ROOT)
    sale.prepare_new_country(inputs)
    authored = definitions()
    tags = sale.read_definitions(inputs, authored)
    return dict(schema_version=1, source='unicorn/gamemd.exe', seed=31,
        definitions=authored,
        cases=[execute(inputs, tags, clear_foreign_resident=clear)
               for clear in (False, True)])


def metadata():
    return native.provenance(
        scope='House50D610 capture, real sale/Action14 ownership transfer, and direct live Action130: foreign resident blocked versus cleared by original virtual F8, before deferred deletion drains.',
        entry_points=dict(capture=0x50D610, restore_buildings=0x50D320,
            action_dispatch=0x6DD8B0, action130=0x6E3120, base_constructor=0x42E6F0,
            records_constructor=0x510690, house_records_tail=0x4F562B,
            building_constructor=0x43B740, mark_reservation=0x455F10,
            sale_completion=sale.BEGIN, change_owner=0x448260,
            resident=0x47C520, force_create=0x458A80, unlimbo=0x440580,
            uninit=0x5F65F0, deferred_expiry=0x7258D0),
        assumptions=[
            'Uses the existing Engineer input reader and sale Fixture. Two separate cold House/Base instances are prepared before the first Building; original Base constructors and Mark reservation execute. This is not a full House constructor, retail scenario load or HighExpR timing proof.',
            'The inherited declared 32x32 clear-map admission prior contains one retail GAPOWR at10,10. Original construction and GrandOpening execute; mission19/status2 is a supplied sale-completion boundary. The real live Action14 changes owner, before the outer sale Limbo tail.',
            'Initial-record values come only from actual House50D610. Original member construction and HouseCtor tail initialize empty capacity. No record, resident, success return, executable code or vtable is replaced.',
            'Original readers create the supplied synthetic Tag/Trigger/TEvent/TAction definitions. Action130 selector0 uses country index0, with live Trigger context. Its event13=999 is not polled; direct dispatch excludes event registration/timing and startup-mode selection claims.',
            'The cleared control executes original virtual F8 through deferred expiry and original Limbo/Mark; deletion is not drained. Both controls execute real constructor/ForceCreate and retain refused objects until pending deletion. Only normalized lifecycle/records/cell/RNG outputs are compared; later AI/body completion and whole458A80 parity remain outside scope.'],
        substitutions=[
            'Allocator/free, TLS/interlocked, OS/audio/presentation and the declared House/map admission prior are inherited from the existing shared Engineer/sale fixture. No parallel VM owner or native gameplay return stub is introduced.'])


if __name__ == '__main__':
    native.finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
