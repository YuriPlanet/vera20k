"""Original Airburst parent tail, fixed nine-child fan, Construct and Fire.

Synthetic registered types and a flat, empty supplied world. Reuses BulletReader's
VM/INI transport and the existing COM, admission and RNG transport owners.
No mod files or assets are read. See airburst.md for coverage and boundaries.
"""
from collections import deque
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import *

from tools.native_oracle import finish_vectors, provenance, initialize_empty_windows_seh
from tools.projectile_oracle.bridge_render_inputs import BulletReader
from tools.projectile_oracle.guided_step import i32, xyz, vec
from tools.projectile_oracle.ifv_launch import (
    bullet_addref_transport, bullet_world_admission_transport,
)
from tools.projectile_oracle.ifv_impact import (
    initialize_effect_registries, initialize_effect_world, retirement_transport,
)
from tools.procedural_drawing_oracle.electric_bolt import RngTrace
from tools.spatial_oracle.building_body_rules import RULES, SP, dwords


def prepare():
    # Existing reader owns the VM and native scalar readers. An absent asset
    # directory deliberately prevents incidental physical resource inputs.
    m = BulletReader({}, Path(__file__).parent / 'no-airburst-assets')
    u = m.u
    for base in (0x8127F4, 0x8129FC, 0x813C2C):
        for address in struct.unpack('<13I', u.mem_read(base, 52)):
            m.invoke(address, 0)
    m.invoke(0x49F2F0, 0)
    directions = list(map(list, struct.iter_unpack('<hh', u.mem_read(0x89F688, 32))))
    for address in (0x887568, 0xA8ED40):
        u.mem_write(address, dwords(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
    runtime_rules = m.alloc(0x2000)
    u.mem_write(0x8871E0, dwords(runtime_rules))
    # The actual runtime scalars are supplied, not a second Rules parser.
    u.mem_write(runtime_rules + 0x16B8, dwords(6))
    child_type = m.construct('AIR_CHILD')
    parent_type = m.construct('AIR_PARENT')
    child_wh = m.alloc(0x200)
    parent_wh = m.alloc(0x200)
    weapon = m.invoke(0x772FA0, m.cstring('AIR_WEAPON'))
    u.mem_write(weapon + 0xA0, dwords(child_type, 37, 73, child_wh))
    u.mem_write(weapon + 0x12F, b'\1')
    # Deliberately distinct from the fan's literal speed50/brightfalse.
    u.mem_write(parent_type + 0x294, b'\1')
    u.mem_write(parent_type + 0x2AC, dwords(2, weapon))
    u.mem_write(child_type + 0x2DC, dwords(0))
    u.mem_write(child_type + 0x2A9, b'\1')
    u.mem_write(child_type + 0x2B8, dwords(0))
    table = 0x26000000
    u.mem_map(table, 0x100000)
    u.mem_write(0x87F924, dwords(table, 262144))
    cells = {}
    for y in range(18, 23):
        for x in range(8, 13):
            cell = m.alloc(0x160)
            cells[x, y] = cell
            u.mem_write(cell, dwords(0x7E4EEC))
            u.mem_write(cell + 0x24, struct.pack('<hh', x, y))
            u.mem_write(cell + 0x11B, b'\0\0')
            u.mem_write(table + (y * 512 + x) * 4, dwords(cell))
    u.mem_write(0xABDC50, dwords(0x7E4EEC))
    initialize_effect_registries(m)
    initialize_effect_world(m, cells)
    house = m.alloc(0x16100)
    u.mem_write(house + 0x16054, dwords(7))
    source = m.alloc(0x800)
    u.mem_write(source + 0x21C, dwords(house))
    parent = m.alloc(0x180)
    m.invoke(0x466380, parent)
    m.invoke(0x4664C0, parent, (parent_type, cells[10, 20], source, 0,
                             parent_wh, 19, 1))
    u.mem_write(parent + 0x9C, dwords(2688, 5248, 600))
    u.mem_write(parent + 0x90, b'\1')
    u.mem_write(parent + 0x150, dwords(768))
    u.mem_write(0xA8ED84, dwords(100))
    return m, parent, cells, source, directions


def execute(fail_first=False, lifecycle=False):
    m, parent, cells, source, directions = prepare()
    u = m.u
    scenario = m.read32(0xA8B230)
    rng = RngTrace(u, scenario, 31)
    before = rng.states()
    children, events, pending_fire = [], [], {}
    recent = deque(maxlen=16)
    allocations = 0
    area_handoffs, uninit_order, logic_add_order = [], [], []
    center = cells[10, 20]
    impact = m.alloc(12)
    u.mem_write(impact, dwords(2699, 5259, 610))
    by_cell = {p: list(xy) for xy, p in cells.items()}
    identity_before = m.read32(scenario + 0x214)

    def child_state(child):
        return dict(target=by_cell[m.read32(child + 0x10C)],
                    native_id=i32(u, child + 0x10),
                    source_is_parent_source=m.read32(child + 0xB0) == source,
                    damage=i32(u, child + 0x6C), maximum_speed=i32(u, child + 0x110),
                    bright=bool(u.mem_read(child + 0xE0, 1)[0]),
                    damage_multiplier=i32(u, child + 0x150),
                    installed_weapon_null=m.read32(child + 0x130) == 0,
                    firer_palette=i32(u, child + 0x114),
                    position=xyz(u, child + 0x9C), velocity=vec(u, child + 0xE8),
                    target_coord=xyz(u, child + 0x140),
                    detector_first=[i32(u, child + 0xB8), i32(u, child + 0xC0)],
                    detector_arm=[i32(u, child + 0xC4), i32(u, child + 0xCC)],
                    in_limbo=bool(u.mem_read(child + 0x81, 1)[0]))

    def observe(_uc, pc, _size, _data):
        nonlocal allocations
        recent.append(hex(pc))
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc == 0x489280:
            area_handoffs.append(dict(source_is_parent_source=m.read32(sp + 4) == source,
                                      damage=u.reg_read(UC_X86_REG_EDX),
                                      position=xyz(u, u.reg_read(UC_X86_REG_ECX)),
                                      source_house_null=m.read32(sp + 16) == 0))
        elif pc == 0x5F65F0:
            pointer = u.reg_read(UC_X86_REG_ECX)
            uninit_order.append('parent' if pointer == parent else
                                next(i for i, (p, _) in enumerate(children) if p == pointer))
        elif pc == 0x55BAA0:
            pointer = m.read32(sp + 4)
            logic_add_order.append(next(i for i, (p, _) in enumerate(children) if p == pointer))
        if (pc, sp) in pending_fire:
            child, row = pending_fire.pop((pc, sp))
            row['after_fire'] = child_state(child)
        if pc in (0x469F4B, 0x46A123):
            allocations += 1
            assert bytes(u.mem_read(pc, 6)) == bytes.fromhex('ff15fc157e00')
            if fail_first and allocations == 1:
                u.reg_write(UC_X86_REG_EAX, 0x80004005)
                u.reg_write(UC_X86_REG_ESP, sp + 20)
                u.reg_write(UC_X86_REG_EIP, pc + 6)
                events.append('supplied_COM_activation_failure')
            else:
                clsid, outer, context, iid, ppv = struct.unpack('<5I', u.mem_read(sp, 20))
                assert (clsid, outer, context, iid) == (0x7E96E0, 0, 7, 0x7F7C90)
                u.mem_write(sp, dwords(pc + 6, 0, outer, iid, ppv))
                u.reg_write(UC_X86_REG_EIP, 0x6C5090)
        elif bullet_addref_transport(m, pc, events):
            pass
        elif pc == 0x4664C0 and u.reg_read(UC_X86_REG_ECX) != parent:
            child = u.reg_read(UC_X86_REG_ECX)
            children.append((child, {}))
        elif pc == 0x468670:
            child = u.reg_read(UC_X86_REG_ECX)
            row = next(row for pointer, row in children if pointer == child)
            row.update(target=by_cell[m.read32(child + 0x10C)],
                       origin=xyz(u, m.read32(sp + 4)),
                       launch_velocity=vec(u, m.read32(sp + 8)))
            pending_fire[(m.read32(sp), sp + 12)] = (child, row)
        elif bullet_world_admission_transport(m, pc, u.reg_read(UC_X86_REG_ECX), events):
            pass
        retirement_transport(m, pc, events)

    handle = u.hook_add(UC_HOOK_CODE, observe)
    try:
        m.invoke(0x4690B0, parent, (impact,))
        if lifecycle:
            # This is the original AI caller's continuation467FB4, not part
            # of DetonateAtCoord itself. No Bullet retirement body is replaced.
            m.invoke(0x5F65F0, parent)
            for frame in range(101, 251):
                u.mem_write(0xA8ED84, dwords(frame))
                live = [p for p, _ in children if u.mem_read(p + 0x90, 1)[0]]
                if not live:
                    break
                for child in live:
                    m.invoke(0x4666E0, child)
            assert all(not u.mem_read(p + 0x90, 1)[0] for p, _ in children)
            initialize_empty_windows_seh(u)
            m.invoke(0x725C70, 0)
    except Exception as exc:
        raise RuntimeError((str(exc), list(recent), hex(u.reg_read(UC_X86_REG_ECX)),
                            allocations, len(children), events[-3:],
                            hex(u.reg_read(UC_X86_REG_ESP)))) from exc
    finally:
        u.hook_del(handle)
    assert not pending_fire
    result = dict(name='off_map_cleanup' if lifecycle else 'allocation_failure' if fail_first else 'nine_children',
                input=dict(cluster=2, parent_multiplier=768, child_damage=37,
                           child_weapon_speed=73, child_weapon_bright=True,
                           center=[10, 20], parent_location=[2688, 5248, 600],
                           impact=[2699, 5259, 610], seed=31),
                initialized_directions=directions,
                children=[row for _, row in children], allocations=allocations,
                native_ids_consumed=(m.read32(scenario + 0x214) - identity_before) & 0xFFFFFFFF,
                rng_before=before, rng_after=rng.states(), rng_requests=rng.requests,
                services=events)
    if lifecycle:
        result.update(area_handoffs=area_handoffs, uninit_order=uninit_order,
                      logic_add_order=logic_add_order,
                      remaining_bullet_count=i32(u, 0xA8ED50),
                      remaining_delete_count=i32(u, 0xB0F6A8))
    return result


def generate():
    return [execute(False), execute(True), execute(False, True)]


def metadata():
    return provenance(
        scope='Original stock Airburst fan/Construct/Fire; fixed-nine positive, allocation failure and off-map AI retirement controls. No child damage parity.',
        assumptions=['Synthetic empty flat cell table; runtime Map+F4/F8 usable diamond remains0. Off-map history enters native AI467A35/568350 rejection, mode2, UnInit and deferred drain; no child AreaDamage is reached.',
                     'Synthetic quiet parent/child warheads and source pointer/house palette supplied. No source Unit lifecycle/type is supplied or claimed. Native direction initializer49F2F0 executes.',
                     'Child weapon Damage37 Speed73 Brighttrue, parent multiplier768. No Ares, mod names, assets or data. Initial two histories recorded in one batch; third off-map retirement history appended in a separate new VM because the prior process had closed. Old two payloads retained unchanged.'],
        substitutions=['Existing Windows COM activation to original registered Bullet factory; one declared activation failure.',
                       'Existing Bullet world-admission guards/display and OS Interlocked/IsBadReadPtr transports. Actual Construct, Fire, coordinate fixup, Logic append, RNG, UnInit and drain execute.'],
        entry_points={'parent_detonate': 0x4690B0, 'fan': 0x469EBA, 'construct': 0x4664C0,
                      'fire': 0x468670, 'directions': 0x49F2F0, 'child_ai': 0x4666E0,
                      'usable_map_check': 0x568350, 'uninit': 0x5F65F0, 'drain': 0x725C70},
    )


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
