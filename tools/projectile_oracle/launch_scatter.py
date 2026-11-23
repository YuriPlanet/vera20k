"""Original launch scatter, shrapnel launch velocity and cluster direction math.

The optional --shrapnel-chain mode executes full SpawnShrapnel46A310,
including original selection, RNG, factory/Construct, Fire, Logic admission and
first-generation Comet/SuperComet lasers. Its supplied-world and excluded
lifetime boundary is recorded in shrapnel_chain.meta.json. Rust consumer:
src/sim/combat/shrapnel_chain_tests.rs.

Default numeric mode executes, case by case:
- TechnoClass::FireAt's launch scatter, 0x006FE663..0x006FE8EE, both arms
  (flak: FlakScatter and not Inviso; plain). Sqrt_Approx 0x004CAC40,
  Math::sin 0x004CACB0, Math::cos 0x004CAD00 and ftol 0x007C5F00 run.
- BulletClass::Fire's flak placement, 0x0046874E..0x004688A9, which a
  FlakScatter and Inviso bullet takes before its Inviso arm. The same
  helpers run.
- BulletClass::SpawnShrapnel's child velocity, from the zeroed velocity to the
  child's launch call: the hostile-object branch 0x0046A5B2..0x0046A875 and
  the random-cell branch 0x0046AA66..0x0046AD29. Math::atan2 0x004CAE30,
  the vector lengths 0x0041C430/0x0041C350/0x0041C3C0, Sqrt_Approx,
  Math::sin/cos and ftol run.
- The random-direction coordinate helper 0x0049F420 (no cell snap), as
  BulletClass::ResolveImpactCoordAndDetonate calls it for each cluster at
  0x00469067, at the cluster distances 0x100..0x200.
- That cluster loop itself, 0x00469008..0x00469091: which coordinate each
  cluster's BulletClass::DetonateAtCoord (0x004690B0) receives.

Supplied at entry and returned without running: Random__RandomRanged
0x0065C7E0 (the case's draw results, arguments recorded), Random 0x0065C780
(the case's raw word), TechnoClass::GetWeaponRange (vt+0x168; the case's
range, argument recorded), the target's and the cell's GetCoords (vt+0x48;
the case's coordinate). Execution stops on the child bullet's launch call
(vt+0x1F0), whose velocity argument is the observed result. The cell branch
starts after its own prologue zeroed the velocity (0x0046AA31..0x0046AA59); the
freshly mapped stack is zero, which stands in for that. In the cluster loop,
DetonateAtCoord is recorded (its coordinate argument) and returns at once,
leaving the bullet's +0x90 flag set.

Rust consumers: src/sim/projectile/launch.rs (fireat_launch_scatter,
bullet_fire_flak_placement, shrapnel_launch_velocity) and
src/sim/combat/inviso_scatter.rs
(random_direction_coord_for_byte).
"""
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
                               UC_X86_REG_EDI, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESI,
                               UC_X86_REG_ESP, UC_X86_REG_FPCW)

from tools.native_oracle import (load_image, run_checked, SCRATCH, SCRATCH_SIZE, STACK_BASE,
                                 STACK_SIZE, NATIVE_FPCW, finish_vectors, provenance)
from tools.native_oracle import require

RULES_PTR, SCENARIO_PTR = 0x8871E0, 0xA8B230
RANDOM_RANGED, RANDOM = 0x65C7E0, 0x65C780
RULES, SCENARIO = SCRATCH + 0x1000, SCRATCH + 0x3000
TECHNO, TECHNO_VT = SCRATCH + 0x4000, SCRATCH + 0x4800
BULLET_TYPE, FRAME = SCRATCH + 0x5000, SCRATCH + 0x5800
PARENT, TARGET, TARGET_VT = SCRATCH + 0x6000, SCRATCH + 0x6400, SCRATCH + 0x6800
CHILD, CHILD_VT, WEAPON = SCRATCH + 0x7000, SCRATCH + 0x7400, SCRATCH + 0x7800
STUB = SCRATCH + 0x9000                     # one address per supplied call
FIRE_BULLET, FIRE_TYPE = SCRATCH + 0xD000, SCRATCH + 0xD400
FIRE_WEAPON, FIRE_LAUNCH = SCRATCH + 0xD800, SCRATCH + 0xDC00
RANGE_STUB, COORDS_STUB = STUB, STUB + 0x10
SP = STACK_BASE + STACK_SIZE - 0x4000


def u32(value):
    return struct.pack("<I", value & 0xFFFFFFFF)


def read32(uc, address):
    return struct.unpack("<i", uc.mem_read(address, 4))[0]


def machine():
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(SCRATCH, SCRATCH_SIZE)
    uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
    uc.mem_write(RULES_PTR, u32(RULES))
    uc.mem_write(SCENARIO_PTR, u32(SCENARIO))
    return uc


def returns(uc, value, stack_bytes):
    """Return from the intercepted call as a callee-cleanup function would."""
    sp = uc.reg_read(UC_X86_REG_ESP)
    uc.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
    uc.reg_write(UC_X86_REG_EIP, read32(uc, sp) & 0xFFFFFFFF)
    uc.reg_write(UC_X86_REG_ESP, sp + 4 + stack_bytes)


def scatter(case):
    """FireAt 0x006FE663..0x006FE8EE with the delta at [ESP+0x30]."""
    uc = machine()
    x, y, z = case["delta"]
    uc.mem_write(RULES + 0x1734, u32(case["scatter"]))
    uc.mem_write(TECHNO, u32(TECHNO_VT))
    uc.mem_write(TECHNO_VT + 0x168, u32(RANGE_STUB))
    uc.mem_write(FRAME + 0xC, u32(case["weapon"]))
    for offset, flag in ((0x2A2, 1), (0x29B, 1), (0x2A3, case["flak_scatter"]),
                         (0x29E, case["inviso"])):
        uc.mem_write(BULLET_TYPE + offset, bytes([flag]))
    for offset, value in ((0x30, x), (0x34, y), (0x38, z), (0x94, x), (0x98, y), (0x9C, z)):
        uc.mem_write(SP + offset, u32(value))
    for register, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_ECX, BULLET_TYPE),
                            (UC_X86_REG_EAX, y), (UC_X86_REG_EDI, z),
                            (UC_X86_REG_ESI, TECHNO), (UC_X86_REG_EBP, FRAME)):
        uc.reg_write(register, value & 0xFFFFFFFF)
    draws = list(case["draws"])
    calls = []

    def hook(_uc, address, _size, _data):
        sp = uc.reg_read(UC_X86_REG_ESP)
        if address == RANDOM_RANGED:
            calls.append(["random_ranged", read32(uc, sp + 4), read32(uc, sp + 8)])
            returns(uc, draws.pop(0), 8)
        elif address == RANGE_STUB:
            calls.append(["weapon_range", read32(uc, sp + 4)])
            returns(uc, case["range"], 4)

    uc.hook_add(UC_HOOK_CODE, hook)
    run_checked(uc, 0x6FE663, 0x6FE8EE, count=200_000,
                required_addresses=[0x4CACB0, 0x4CAD00])
    assert not draws, "every supplied draw is consumed"
    assert uc.reg_read(UC_X86_REG_ESP) == SP
    return dict(input=case, calls=calls,
                result=[read32(uc, SP + 0x94), read32(uc, SP + 0x98), read32(uc, SP + 0x9C)])


def bullet_fire_flak(case):
    """Fire 0x0046874E..0x004688A9: the launch at [EDI], the target in
    ECX/EAX/EDX and at [ESP+0x44]; the placement is left at [ESP+0x44]."""
    uc = machine()
    target = case["target"]
    uc.mem_write(RULES + 0x1734, u32(case["scatter"]))
    uc.mem_write(FIRE_BULLET + 0xAC, u32(FIRE_TYPE))
    uc.mem_write(FIRE_BULLET + 0x130, u32(FIRE_WEAPON))
    uc.mem_write(FIRE_WEAPON + 0xB4, u32(case["range"]))
    for offset in (0x2A3, 0x29E):
        uc.mem_write(FIRE_TYPE + offset, bytes([1]))
    uc.mem_write(FIRE_LAUNCH, struct.pack("<iii", *case["launch"]))
    for offset, value in zip((0x44, 0x48, 0x4C), target):
        uc.mem_write(SP + offset, u32(value))
    for register, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EBX, FIRE_BULLET),
                            (UC_X86_REG_EDI, FIRE_LAUNCH), (UC_X86_REG_ECX, target[0]),
                            (UC_X86_REG_EAX, target[1]), (UC_X86_REG_EDX, target[2])):
        uc.reg_write(register, value & 0xFFFFFFFF)
    draws = list(case["draws"])
    calls = []

    def hook(_uc, address, _size, _data):
        sp = uc.reg_read(UC_X86_REG_ESP)
        if address == RANDOM_RANGED:
            calls.append(["random_ranged", read32(uc, sp + 4), read32(uc, sp + 8)])
            returns(uc, draws.pop(0), 8)

    uc.hook_add(UC_HOOK_CODE, hook)
    run_checked(uc, 0x46874E, 0x4688A9, count=200_000,
                required_addresses=[0x4CAC40, 0x4CACB0, 0x4CAD00])
    assert not draws, "every supplied draw is consumed"
    assert uc.reg_read(UC_X86_REG_ESP) == SP
    return dict(input=case, calls=calls,
                result=[read32(uc, SP + 0x44), read32(uc, SP + 0x48), read32(uc, SP + 0x4C)])


def shrapnel(case):
    """SpawnShrapnel's child velocity, up to the child's launch call."""
    uc = machine()
    uc.mem_write(PARENT + 0x9C, struct.pack("<iii", *case["origin"]))
    uc.mem_write(WEAPON + 0xA8, u32(case["speed"]))
    uc.mem_write(TARGET, u32(TARGET_VT))
    uc.mem_write(TARGET_VT + 0x48, u32(COORDS_STUB))
    uc.mem_write(CHILD, u32(CHILD_VT))
    random_cell = case["random_cell"]
    uc.mem_write(SP + (0x44 if random_cell else 0x4C), u32(CHILD))
    registers = [(UC_X86_REG_ESP, SP), (UC_X86_REG_EDI, PARENT), (UC_X86_REG_ESI, WEAPON)]
    # The object branch reads its target from EBP; the cell branch from the
    # MapClass cell lookup's result in EAX.
    registers.append((UC_X86_REG_EAX, TARGET) if random_cell else (UC_X86_REG_EBP, TARGET))
    for register, value in registers:
        uc.reg_write(register, value)

    def hook(_uc, address, _size, _data):
        if address == COORDS_STUB:
            out = read32(uc, uc.reg_read(UC_X86_REG_ESP) + 4) & 0xFFFFFFFF
            uc.mem_write(out, struct.pack("<iii", *case["target"]))
            returns(uc, out, 4)

    uc.hook_add(UC_HOOK_CODE, hook)
    # Stop on the child's launch call (vt+0x1F0), its two arguments pushed.
    begin, launch = (0x46AA66, 0x46AD29) if random_cell else (0x46A5B2, 0x46A875)
    run_checked(uc, begin, launch, count=200_000, required_addresses=[0x4CAE30, 0x4CAC40])
    sp = uc.reg_read(UC_X86_REG_ESP)
    coords, velocity = read32(uc, sp) & 0xFFFFFFFF, read32(uc, sp + 4) & 0xFFFFFFFF
    return dict(input=case, coords=list(struct.unpack("<iii", uc.mem_read(coords, 12))),
                bits=[f"{bits:016x}" for bits in struct.unpack("<QQQ", uc.mem_read(velocity, 24))])


def shrapnel_chain(case):
    """Full46A310, child Fire and laser births; supplied incoming world only.

    The existing Bullet world/reader owns initialization, RngTrace owns stream
    observation and the registered native factory owns each child allocation.
    No selection, random result, launch or effect result is supplied here.
    """
    import hashlib
    from collections import deque
    from tools.native_oracle import initialize_empty_windows_seh
    from tools.projectile_oracle.bridge_render_inputs import BulletReader, assets_root, lexical
    from tools.projectile_oracle.guided_step import create, xyz, vec, i32
    from tools.projectile_oracle.ifv_impact import initialize_effect_registries, initialize_effect_world
    from tools.projectile_oracle.ifv_launch import bullet_activation_transport, bullet_addref_transport
    from tools.procedural_drawing_oracle.electric_bolt import RngTrace
    from tools.spatial_oracle.building_prism import LASERS, LASER_VECTOR_INIT, laser_state
    from tools.spatial_oracle.building_body_rules import RULES as INI_RULES, SP as CALL_SP, dwords

    root = assets_root()
    art, _ = lexical((root / 'ARTMD.INI').read_bytes(),
                     {'AAHeatSeeker2', 'DRAGON', 'FV', 'LargeCometP', 'SmallCometP',
                      'SuperCometP', 'SuperSmallCometP'})
    m, _, cells, _ = create(False, reader=BulletReader(art))
    u = m.u
    initialize_effect_registries(m)
    initialize_effect_world(m, cells)
    initialize_empty_windows_seh(u)
    transports = []

    def platform(_u, pc, _size, _data):
        if pc == 0x7C978A:  # CRT atexit (no process teardown in the fixture)
            m.ret(0)
        else:
            bullet_activation_transport(m, pc, transports)
            bullet_addref_transport(m, pc, transports)

    u.hook_add(UC_HOOK_CODE, platform)
    # The trailer-only world does not walk Logic. Run its original vector
    # constructor so ObjectUnlimbo can allocate/insert instead of returning0.
    m.invoke(0x40CB80, 0)
    m.invoke(LASER_VECTOR_INIT, 0)
    # Native initializer plus a supplied spare-capacity backing array, as in
    # the existing LaserScene; original constructor performs every insertion.
    u.mem_write(LASERS + 4, dwords(m.alloc(64), 16))
    u.mem_write(LASERS + 0xD, b'\0')
    names = ['Comet', 'CometFragment', 'SuperComet', 'SuperCometFragment']
    weapons = {name: m.invoke(0x772FA0, m.cstring(name)) for name in names}
    wanted = set(names) | {'LargeCometP', 'SmallCometP', 'SuperCometP',
                            'SuperSmallCometP', 'CometWH', 'AudioVisual'}
    layers = []
    for filename in ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'Hills.map'):
        path = root / filename
        if not path.exists():
            assert filename == 'LANGRULE.INI'
            layers.append(dict(file=filename, absent=True))
            continue
        raw = path.read_bytes()
        sections, _ = lexical(raw, wanted)
        m.rules_cache(sections)
        # Rules::ReadAudioVisual precedes type postprocessing. Gravity belongs
        # to AudioVisual (not General); preserve the native retained default
        # across absent layers, as weapon_speed.physical does.
        rules_object = m.read32(0x8871E0)
        gravity_before = i32(u, rules_object + 0x16B8)
        u.reg_write(UC_X86_REG_ESP, CALL_SP)
        u.reg_write(UC_X86_REG_ESI, rules_object)
        u.reg_write(UC_X86_REG_EDI, INI_RULES)
        run_checked(u, 0x66B3C4, 0x66B3E4, required_addresses=(0x5276D0,))
        gravity = i32(u, rules_object + 0x16B8)
        postpass = {}
        # Referenced projectiles and weapons are allocated by native readers;
        # repeated explicit reads reproduce retained values in each layer.
        for w in weapons.values():
            m.invoke(0x772080, w, (INI_RULES,))
            if m.read32(w + 0xA0):
                m.invoke(0x46BEE0, m.read32(w + 0xA0), (INI_RULES,))
            if m.read32(w + 0xAC):
                m.invoke(0x75D3A0, m.read32(w + 0xAC), (INI_RULES,))
            m.invoke(0x7729F0, w)
            postpass[m.string(w + 0x24)] = dict(
                gravity=gravity, floater=bool(u.mem_read(m.read32(w + 0xA0) + 0x295, 1)[0]),
                speed=i32(u, w + 0xA8))
        layers.append(dict(file=filename, sha256=hashlib.sha256(raw).hexdigest(),
                           physical_sections=sections, gravity_before=gravity_before,
                           gravity=gravity, weapon_postpass=postpass))
    w = weapons[case['parent_weapon']]
    p = m.read32(w + 0xA0)
    child_w = m.read32(p + 0x2B4)
    assert child_w

    # Explicit preexisting actors and ground head order; real Unit vtable
    # methods read these fields. Actor construction/death dispatch is excluded.
    houses = []
    for index in range(2):
        h = m.alloc(0x6000)
        u.mem_write(h + 0x30, dwords(index))
        u.mem_write(h + 0x56FC, bytes((18, 52, 86)))
        houses.append(h)
    actors, identities, actor_inputs = {}, {}, []
    layout = [('source', (15, 20), 0), ('victim', (16, 20), 1),
              ('ally', (16, 19), 0), ('hidden_enemy', (16, 19), 1),
              ('enemy_a', (17, 20), 1), ('enemy_b', (16, 21), 1)]
    if case.get('terrain'):
        layout += [('tree', (17, 19), None)]
    if case.get('all_selected'):
        layout += [('enemy_c', (17, 19), 1), ('enemy_d', (17, 21), 1),
                   ('enemy_e', (15, 21), 1)]
    for name, cell, house in layout:
        actor = m.alloc(0x1000)
        location = [cell[0] * 256 + 128, cell[1] * 256 + 128, 624]
        if name == 'tree':
            location[2] = case.get('retained_z', 624)
        u.mem_write(actor, dwords(0x7F522C if name == 'tree' else 0x7F5C70))
        u.mem_write(actor + 0x14, dwords(2))
        u.mem_write(actor + 0x9C, dwords(*location))
        if house is not None:
            u.mem_write(actor + 0x21C, dwords(houses[house]))
        actors[name], identities[actor] = actor, name
        actor_inputs.append(dict(name=name, cell=list(cell), house=house, location=location,
                                 head=name != 'hidden_enemy',
                                 native_class='Terrain' if name == 'tree' else 'Unit',
                                 next_object='hidden_enemy' if name == 'ally' else None))
        if name == 'hidden_enemy':
            u.mem_write(actors['ally'] + 0x30, dwords(actor))
        else:
            u.mem_write(cells[cell] + 0xE4, dwords(actor))
    if case.get('lower_tree_ground'):
        # Preserve the Terrain object's retained location while its cell drops.
        u.mem_write(cells[17, 19] + 0x11B, b'\0')
    if case.get('refused'):
        u.mem_write(cells[16, 20] + 0xE4, dwords(0))
    b = m.alloc(0x180)
    m.invoke(0x466380, b)
    m.invoke(0x4664C0, b, (p, actors['victim'], actors['source'], m.read32(w + 0xA4),
                          m.read32(w + 0xAC), m.read32(w + 0xA8), 0))
    if 'count' in case:
        u.mem_write(p + 0x2B8, dwords(case['count']))
    origin = xyz(u, actors['tree'] + 0x9C) if case.get('tree_center') else [4224, 5248, 624]
    u.mem_write(b + 0x9C, dwords(*origin))
    u.mem_write(b + 0x130, dwords(w))
    if case.get('detached'):
        m.invoke(0x4684E0, b, (actors['source'], 0))
        assert m.read32(b + 0xB0) == 0
    scenario = m.read32(0xA8B230)
    rng = RngTrace(u, scenario, case['seed'])
    before = rng.states()
    id_before = m.read32(scenario + 0x214)
    events, children, lasers, recent = [], [], [], deque(maxlen=12)
    child_pointers, visited = [], set()
    pending_fire, pending_laser = {}, {}
    by_cell = {pointer: list(cell) for cell, pointer in cells.items()}

    def identity(ptr):
        if ptr == 0:
            return None
        if ptr in identities:
            return dict(kind='terrain' if identities[ptr] == 'tree' else 'entity',
                        name=identities[ptr])
        if ptr in by_cell:
            return dict(kind='cell', cell=by_cell[ptr])
        raise AssertionError(('unrecognized native target', hex(ptr)))

    def observe(_u, pc, _size, _data):
        recent.append(hex(pc))
        if pc in (0x6C5090, 0x466380, 0x4664C0, 0x468670, 0x5F4EC0,
                  0x55BAA0, 0x6FD210, 0x54FE60, 0x4A9720):
            visited.add(hex(pc))
        sp = u.reg_read(UC_X86_REG_ESP)
        if (pc, sp) in pending_fire:
            row = children[pending_fire.pop((pc, sp))]
            child = row.pop('_pointer')
            row.update(placed_location=xyz(u, child + 0x9C),
                       fired_velocity=vec(u, child + 0xE8),
                       fire_location=xyz(u, child + 0x134),
                       target_location=xyz(u, child + 0x140),
                       on_bridge=bool(u.mem_read(child + 0x8C, 1)[0]),
                       in_limbo=bool(u.mem_read(child + 0x81, 1)[0]),
                       alive=bool(u.mem_read(child + 0x90, 1)[0]),
                       logic_registered=bool(u.mem_read(child + 0x98, 1)[0]),
                       returned=u.reg_read(UC_X86_REG_EAX) & 255,
                       rng_after_fire=rng.states(),
                       arm=[i32(u, child + 0xC4), i32(u, child + 0xCC)],
                       weapon_is_null=m.read32(child + 0x130) == 0)
            if case.get('terrain'):
                row['retained_target'] = identity(m.read32(child + 0x10C))
            events.append(dict(kind='fire_return', child=row['order']))
        if (pc, sp) in pending_laser:
            index = pending_laser.pop((pc, sp))
            ptr = u.reg_read(UC_X86_REG_EAX)
            lasers.append(dict(child=index, **laser_state(u, ptr)))
            events.append(dict(kind='laser_return', child=index))
        if pc == 0x65C7E0:
            events.append(dict(kind='rng', request=len(rng.requests) - 1))
        elif pc == 0x4664C0:
            args = struct.unpack('<7I', u.mem_read(sp + 4, 28))
            child = u.reg_read(UC_X86_REG_ECX)
            child_pointers.append(child)
            children.append(dict(order=len(children), _pointer=child,
                                 native_id=m.read32(child + 0x10),
                                 projectile=m.string(args[0] + 0x24), target=identity(args[1]),
                                 source=identity(args[2]), damage=args[3],
                                 warhead=m.string(args[4] + 0x24), speed=args[5], bright=args[6],
                                 rng_request_count=len(rng.requests)))
            events.append(dict(kind='construct', child=len(children) - 1))
        elif pc == 0x468670:
            row = children[-1]
            assert u.reg_read(UC_X86_REG_ECX) == row['_pointer']
            coord, velocity = struct.unpack('<2I', u.mem_read(sp + 4, 8))
            row.update(launch_location=xyz(u, coord), launch_velocity=vec(u, velocity))
            pending_fire[(m.read32(sp), sp + 12)] = row['order']
            events.append(dict(kind='fire', child=row['order']))
        elif pc == 0x6FD210:
            target, index, weapon, source = struct.unpack('<4I', u.mem_read(sp + 4, 16))
            assert index == 0 and weapon == child_w
            events.append(dict(kind='laser', child=len(children)-1,
                               source=identity(u.reg_read(UC_X86_REG_ECX)),
                               target=identity(target), origin=xyz(u, source), weapon=m.string(weapon+0x24)))
            pending_laser[(m.read32(sp), sp + 20)] = len(children) - 1
        elif pc == 0x6FD460:
            raise AssertionError('Unexpected electric child effect in Prism control')

    hook = u.hook_add(UC_HOOK_CODE, observe)
    try:
        m.invoke(0x46A310, b, context={'case': case})
    except Exception:
        print('SpawnShrapnel last instructions:', list(recent))
        raise
    finally:
        u.hook_del(hook)
    assert not pending_fire and not pending_laser
    after = rng.states()
    logic_count = m.read32(0x87F788)
    logic_items = m.read32(0x87F77C)
    logic_pointers = [m.read32(logic_items + i*4) for i in range(logic_count)]
    assert logic_pointers == child_pointers
    assert all(c['returned'] == 1 and c['logic_registered'] for c in children)
    if children:
        assert {'0x6c5090', '0x466380', '0x4664c0', '0x468670', '0x5f4ec0', '0x55baa0'} <= visited
    if lasers:
        assert {'0x6fd210', '0x54fe60'} <= visited
    assert m.read32(LASERS + 0x10) == len(lasers)
    assert before['main'] == after['main'] and before['mapgen'] == after['mapgen']
    return dict(input=case, origin=origin, actors=actor_inputs, children=children,
                logic_child_order=[child_pointers.index(p) for p in logic_pointers],
                original_calls=sorted(visited),
                lasers=lasers, events=events, requests=rng.requests,
                rng_before=before, rng_after=after, native_id_before=id_before,
                native_id_after=m.read32(scenario + 0x214),
                effective=dict(child_weapon=m.string(child_w + 0x24),
                               child_count=i32(u, p + 0x2B8),
                               child_range=i32(u, child_w + 0xB4),
                               gravity=i32(u, m.read32(0x8871E0) + 0x16B8),
                               child_floater=bool(u.mem_read(m.read32(child_w + 0xA0) + 0x295, 1)[0]),
                               child_type_is_logic=bool(u.mem_read(m.read32(child_w + 0xA0) + 0x234, 1)[0])), layers=layers)


def shrapnel_chain_generate():
    rows = [shrapnel_chain(case) for case in [
        dict(name='normal_mixed', parent_weapon='Comet', seed=31),
        dict(name='normal_selected', parent_weapon='Comet', seed=31, all_selected=True),
        dict(name='normal_detached', parent_weapon='Comet', seed=31, detached=True),
        dict(name='elite_mixed', parent_weapon='SuperComet', seed=31),
        dict(name='refused_empty_center', parent_weapon='Comet', seed=31, refused=True),
    ]]
    layers = rows[0]['layers']
    for row in rows:
        require(row.pop('layers') == layers, 'layers differs from the shared value')
    return dict(layers=layers, cases=rows,
                input_environment='VERA20K_PROJECTILE_RENDER_ASSETS points to the extracted physical files identified in layers; RA2_DIR or VERA20K_GAMEMD_EXE selects gamemd.exe')


def shrapnel_chain_metadata():
    return provenance(
        scope='Full original SpawnShrapnel46A310 through return, native child constructors/Fire and first-generation Prism laser births',
        assumptions=[
            'Selected physical Battle+Hills AudioVisual.Gravity and weapon/projectile/warhead layers read through original native readers; hashes, physical strings and each native Gravity/Floater/Speed postpass input/output are in payload. Original66B3C4 reads AudioVisual.Gravity before7729F0 recomputes nonROT speed; the inherited default3 alone is not a retail input.',
            'Existing guided Bullet world plus original Logic40CB80 initializer supplies flat level6 cells; actor fields/ground list heads and parent incoming location are supplied, native vtables and selection/coordinate getters execute.',
            'Detached row executes original PointerExpired4684E0 before measured entry. Whole actor death dispatch, parent FireAt/flight/AreaDamage, child AI/recursive damage and pixels are outside this boundary.',
            'Main/Scenario/MapGen seeded by existing native RngTrace; all complete stream states and draw requests retained. No RNG/geometry/selection/effect result is supplied.',
            'Supplied parent and actors have real Bullet/Unit vtables; only stock positive shrapnel counts and first-generation Comet/SuperComet are compared. No allocation failure, dummy cell or recursive child AI claim.',
            'Native x87 control0E7F; existing54 shrapnel numeric controls remain separate and unchanged.',
        ],
        substitutions=[
            'Inherited BulletReader allocator, CRT TLS, cached INI/archive IO; supplied mapped cell/actor state and laser spare capacity.',
            'Existing Windows CoCreateInstance transport to native registered factory6C5090 and InterlockedIncrement; CRT atexit returns0.',
        ], entry_points=dict(gravity_reader=0x66B3C4, weapon_postpass=0x7729F0,
                             spawn_shrapnel=0x46A310, construct=0x4664C0,
                             fire=0x468670, laser=0x6FD210, laser_ctor=0x54FE60))


def terrain_shrapnel_generate():
    rows = [shrapnel_chain(case) for case in (
        dict(name='normal_tree', parent_weapon='Comet', seed=31, terrain=True),
        dict(name='elite_tree_retained_ground', parent_weapon='SuperComet', seed=31,
             terrain=True, lower_tree_ground=True),
        dict(name='tree_detached', parent_weapon='Comet', seed=31, terrain=True, detached=True),
        dict(name='tree_count_zero', parent_weapon='Comet', seed=31, terrain=True, count=0),
        dict(name='elite_tree_child_center', parent_weapon='SuperCometFragment', seed=31,
             terrain=True, tree_center=True),
    )]
    layers = rows[0]['layers']
    for row in rows:
        require(row.pop('layers') == layers, 'layers differs from the shared value')
    return dict(layers=layers, cases=rows)


def terrain_shrapnel_metadata():
    result = shrapnel_chain_metadata()
    result['scope'] = ('Original SpawnShrapnel target/RNG/child/effect order with a real Terrain '
                       'vtable head, no-firer and count0 controls, and a prepared elite '
                       'second-generation parent centered on the tree; no child AI execution')
    result['assumptions'][1] = (
        'Existing flat level6 map, supplied Unit and Terrain object states. Terrain7F522C '
        'has retained Location, Object flag2, null house and WhatAmI36; retained-ground '
        'row lowers only tree cell17,19 from level6 to0 after supplying LocationZ624. Original virtual '
        '+48/+58/+A4 and alliance gate run. Terrain construction/ground admission excluded.')
    result['assumptions'][4] = (
        'Count0 is an explicit synthetic override after physical reads. Other counts and '
        'weapons are native-reader outputs. Elite child-center row supplies an already '
        'detonating SuperSmallCometP at the retained tree location; it does not execute '
        'the preceding child AI/damage or certify complete recursive lifetime.')
    return result


def direction(case):
    """0x0049F420(out, in, distance, snap=0) with one supplied raw draw."""
    uc = machine()
    source, result = SCRATCH + 0xA000, SCRATCH + 0xA100
    uc.mem_write(source, struct.pack("<iii", *case["base"]))
    ret = SCRATCH + 0xB000
    uc.mem_write(SP, u32(ret) + u32(case["distance"]) + u32(0))
    for register, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_ECX, result),
                            (UC_X86_REG_EDX, source)):
        uc.reg_write(register, value)

    def hook(_uc, address, _size, _data):
        if address == RANDOM:
            returns(uc, case["raw"], 0)

    uc.hook_add(UC_HOOK_CODE, hook)
    run_checked(uc, 0x49F420, ret, count=100_000, required_addresses=[0x4CACB0, 0x4CAD00])
    return dict(input=case, result=list(struct.unpack("<iii", uc.mem_read(result, 12))))


def cluster_loop(case):
    """0x00469008..0x00469091: the impact at [ESP+0xC], Cluster= from the type."""
    uc = machine()
    bullet, bullet_type = SCRATCH + 0xC000, SCRATCH + 0xC400
    uc.mem_write(bullet + 0xAC, u32(bullet_type))
    uc.mem_write(bullet + 0x90, bytes([1]))
    uc.mem_write(bullet_type + 0x2AC, u32(case["cluster"]))
    uc.mem_write(SP + 0xC, struct.pack("<iii", *case["impact"]))
    for register, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_ESI, bullet),
                            (UC_X86_REG_EAX, bullet_type)):
        uc.reg_write(register, value)
    distances, raws = list(case["distances"]), list(case["raws"])
    detonations = []

    def hook(_uc, address, _size, _data):
        sp = uc.reg_read(UC_X86_REG_ESP)
        if address == 0x4690B0:
            coord = read32(uc, sp + 4) & 0xFFFFFFFF
            detonations.append(list(struct.unpack("<iii", uc.mem_read(coord, 12))))
            returns(uc, 0, 4)
        elif address == RANDOM_RANGED:
            assert (read32(uc, sp + 4), read32(uc, sp + 8)) == (0x100, 0x200)
            returns(uc, distances.pop(0), 8)
        elif address == RANDOM:
            returns(uc, raws.pop(0), 0)

    uc.hook_add(UC_HOOK_CODE, hook)
    run_checked(uc, 0x469008, (0x469091, 0x4690A6), count=100_000,
                required_addresses=[0x49F420])
    assert not distances and not raws, "one distance and one raw draw per cluster"
    return dict(input=case, detonations=detonations)


def cluster_cases():
    rows = []
    impact = [40 * 256 + 64, 50 * 256 + 200, 104]
    for count in (1, 2, 3, 5):
        rows.append(dict(impact=impact, cluster=count,
                         distances=[0x100 + 0x40 * i for i in range(count)],
                         raws=[0x40 * (i + 1) + 7 for i in range(count)]))
    # The critic's case: equal draws put clusters 2 and 3 on one point.
    rows.append(dict(impact=[10304, 13000, 104], cluster=3,
                     distances=[0x200] * 3, raws=[0x40] * 3))
    # Beside the map's edge: a step off the map falls back to the impact.
    rows.append(dict(impact=[100, 100, 0], cluster=4,
                     distances=[0x200] * 4, raws=[0x80, 0xC0, 0x00, 0x40]))
    return rows


def scatter_cases():
    rows = []
    deltas = [(1280, 0, 0), (1024, 0, 77), (-900, 600, -300), (37, -2211, 512),
              (0, 0, 0), (3000, 3000, 0), (-1, 1, 1), (255, -256, 104)]
    raws = [0, 1, 0x3FFFFFFF, 0x7FFFFFFE, 0x12345678, 0x6D0A2C11, 0x00010000]
    for index, delta in enumerate(deltas):
        for raw in raws[index % 3::3] + [raws[(index * 5) % len(raws)]]:
            for flak, inviso in ((1, 0), (0, 0), (1, 1)):
                scatter_value = [256, 0, 384][index % 3]
                # The flak arm's draws are RandomRanged(0, scatter) then the raw
                # angle; the plain arm's RandomRanged(scatter/2, scatter).
                low = 0 if flak and not inviso else scatter_value // 2
                roll = [low, scatter_value, (low + scatter_value) // 2][raw % 3]
                rows.append(dict(delta=list(delta), scatter=scatter_value, flak_scatter=flak,
                                 inviso=inviso, weapon=index % 2, range=[1280, 1536, 2560][index % 3],
                                 draws=[roll, raw]))
    # Boundaries of the draw-to-word chain the Rust constants must reproduce.
    for raw in (0x55555554, 0x55555555, 0x2AAAAAAA, 0x7FFFFFFD, 0x40000000, 0x3FFFFFFE):
        rows.append(dict(delta=[512, -512, 0], scatter=256, flak_scatter=0, inviso=0,
                         weapon=0, range=1280, draws=[256, raw]))
    return rows


def bullet_fire_flak_cases():
    rows = []
    launch = [30 * 256 + 128, 40 * 256 + 128, 0]
    # Target minus launch: aircraft at stock flak ranges and heights, a ground
    # case, no distance, one-lepton steps and a target below the launch.
    offsets = [(1792, 0, 1500), (0, -2304, 1500), (-1100, 900, 1500), (2900, 1450, 1500),
               (300, -200, 0), (0, 0, 0), (-1, 1, 1), (2048, 2048, -104)]
    raws = [0, 1, 0x1FFFFFFF, 0x2AAAAAAA, 0x3FFFFFFF, 0x40000000, 0x55555555, 0x6D0A2C11,
            0x12345678, 0x7FFFFFFE]
    for index, (dx, dy, dz) in enumerate(offsets):
        target = [launch[0] + dx, launch[1] + dy, launch[2] + dz]
        for step, raw in enumerate(raws):
            # Stock BallisticScatter=1.0: the magnitude draw's ceiling is 512.
            roll = [512, 257, 1, 0][(index + step) % 4]
            rows.append(dict(launch=launch, target=target, scatter=256,
                             range=[3072, 2560, 2048][index % 3], draws=[roll, raw]))
    # No BallisticScatter, and 1.5 cells.
    for scatter_value in (0, 384):
        for raw in (0x12345678, 0x55555555):
            rows.append(dict(launch=launch, target=[launch[0] + 2900, launch[1] + 1450, 1500],
                             scatter=scatter_value, range=2560,
                             draws=[scatter_value * 2, raw]))
    return rows


def shrapnel_cases():
    rows = []
    origin = [20 * 256 + 128, 30 * 256 + 128, 0]
    offsets = [(512, 0, 0), (0, 512, 0), (-384, 256, 0), (300, -700, 208), (1, 1, 0),
               (0, 0, 0), (-1536, -1536, -104), (2048, 5, 0), (-3, 1200, 416)]
    for random_cell in (False, True):
        for speed in (10, 40, 0):
            for dx, dy, dz in offsets:
                rows.append(dict(random_cell=random_cell, speed=speed, origin=origin,
                                 target=[origin[0] + dx, origin[1] + dy, origin[2] + dz]))
    return rows


def direction_cases():
    rows = []
    for distance in (0x100, 0x155, 0x200):
        for byte in range(256):
            rows.append(dict(base=[40 * 256 + 64, 50 * 256 + 200, 104], distance=distance,
                             raw=byte | (0x5A5A00 if byte & 1 else 0x7F00)))
    # Near the map's edges: a coordinate leaving the 512-cell square falls back.
    for base in ([100, 100, 0], [511 * 256 + 200, 300, 0], [300, 511 * 256 + 250, 0]):
        for byte in (0, 32, 64, 96, 128, 160, 192, 224):
            rows.append(dict(base=base, distance=0x200, raw=byte))
    return rows


def generate():
    return dict(scatter=[scatter(row) for row in scatter_cases()],
                bullet_fire_flak=[bullet_fire_flak(row) for row in bullet_fire_flak_cases()],
                shrapnel=[shrapnel(row) for row in shrapnel_cases()],
                direction=[direction(row) for row in direction_cases()],
                cluster_loop=[cluster_loop(row) for row in cluster_cases()])


if __name__ == "__main__":
    import sys
    if '--terrain-shrapnel' in sys.argv:
        args = [arg for arg in sys.argv[1:] if arg != '--terrain-shrapnel']
        finish_vectors(terrain_shrapnel_generate, Path(__file__).with_name('terrain_shrapnel.json'),
                       provenance=terrain_shrapnel_metadata, argv=args)
        raise SystemExit(0)
    if '--shrapnel-chain' in sys.argv:
        args = [arg for arg in sys.argv[1:] if arg != '--shrapnel-chain']
        finish_vectors(shrapnel_chain_generate, Path(__file__).with_name('shrapnel_chain.json'),
                       provenance=shrapnel_chain_metadata, argv=args)
        raise SystemExit(0)
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope="FireAt launch scatter 0x006FE663..0x006FE8EE (both arms), BulletClass::Fire's flak "
              "placement 0x0046874E..0x004688A9, SpawnShrapnel child "
              "velocity (object branch 0x0046A5B2..0x0046A875, cell branch 0x0046AA66..0x0046AD29), "
              "the random-direction helper 0x0049F420 at cluster distances, and the cluster loop "
              "0x00469008..0x00469091",
        assumptions=["x87 control word 0x0E7F (53-bit, chop) as WinMain sets it",
                     "the retail sine, atan and Sqrt_Approx tables are the image's initialised data",
                     "the shrapnel cell branch starts after its prologue zeroed the velocity; the "
                     "fresh stack is zero"],
        substitutions=["Random__RandomRanged 0x0065C7E0 and Random 0x0065C780 return the case's "
                       "values; their arguments are recorded",
                       "GetWeaponRange (vt+0x168) returns the case's range",
                       "the target's/cell's GetCoords (vt+0x48) returns the case's coordinate",
                       "the child's launch call (vt+0x1F0) is observed, not run",
                       "in the cluster loop, DetonateAtCoord 0x004690B0 records its coordinate "
                       "argument and returns"],
        entry_points=dict(scatter=0x6FE663, bullet_fire_flak=0x46874E,
                          shrapnel_object=0x46A5B2,
                          shrapnel_cell=0x46AA66, direction=0x49F420,
                          cluster_loop=0x469008)))
