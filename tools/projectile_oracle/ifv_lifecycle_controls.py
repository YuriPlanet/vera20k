"""Original guided states isolating lifecycle regression fixture boundaries."""
import argparse
from pathlib import Path

from tools.native_oracle import finish_vectors, provenance
from tools.projectile_oracle import ifv_guided_controls as controls


def specs():
    for height in (1, 0):
        yield dict(
            name=f"ground_old{height}_level_no_acceleration",
            old=[2688, 5248, 624 + height],
            velocity=[4.0, 0.0, -2.0],
            target=(17, 20),
            maximum=4,
            type_keys={"Level": "yes", "Acceleration": "0"},
        )
    yield dict(
        name="expired_null_below_safety",
        old=[2688, 5248, 1024],
        velocity=[64.0, 0.0, 0.0],
        target=None,
        maximum=0,
        locked=True,
        safety=500,
    )


def generate():
    original_specs = controls.specs
    try:
        controls.specs = specs
        result = controls.generate()
    finally:
        controls.specs = original_specs
    result["scope"] = (
        "Three supplied incoming guided states through original4668BD..467B7A: "
        "Level/no-acceleration ground crossing and target-null below safety. "
        "No pointer-expiry, snapshot, scheduler, damage or retirement execution."
    )
    return result


def metadata():
    return provenance(
        scope="Original guided controls for two lifecycle-test fixture assumptions",
        assumptions=[
            "Tracked ifv_guided_controls/guided_step setup executes original Bullet construction/configure and physical layered Weapon/Bullet/General readers. This is not complete Rules Process chronology.",
            "Map cells are supplied flat level6. Native ID100/frame1, incoming XYZ/velocity, maximum speed, counters/lock, null source and target kind are supplied. Level and Acceleration overrides execute the full original BulletType reader. Safety500 is a supplied retained Rules scalar.",
            "All three controls stop before world coordinate commit. No original pointer expiry, save/load, Unit scheduling, collision receiver or deletion is executed here.",
            "Rust ground fixture translates native floor624 to208 (level6 to2), and cells[10,20]/[17,20] to[5,5]/[12,5]. Its first step compares the native relative crossing; phase-independent Level/yaw-aligned candidate is the bounded comparison.",
            "Rust null fixture translates floor624 to0 and places the Bullet at[1024,1024,400]. ID100/frame1, incoming velocity, maximum, latch and guidance flags match. Velocity and height deltas are comparable in the zero-target pitch arm; its coordinate-dependent closing accumulator is not compared.",
        ],
        substitutions=[
            "Inherits tracked BulletReader physical lexical INI caches and archive byte delivery, allocation/delete/CRT TLS and verified OS imports. Observation records original calls and rejects RNG. Only the Python supplied-state table differs; original instructions and callees are unchanged.",
        ],
        entry_points={
            "guided_ai": 0x4668BD,
            "homing": 0x5B20F0,
            "null_safety": 0x466E70,
            "precommit_stop": 0x467B7A,
        },
    )



def teleport_targets():
    """Execute both target-loss bodies and one subsequent guided step."""
    import struct

    from unicorn import UC_HOOK_CODE
    from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EBP
    from tools.native_oracle import run_checked
    from tools.projectile_oracle import guided_step as g
    from tools.spatial_oracle.building_body_rules import dwords, SP
    from tools.spatial_oracle.object_flight_height import initialize_object_scalars

    reader, bullet, cells, initial = g.create()
    u = reader.u
    initial["object_initializers"] = [
        hex(address) for address in initialize_object_scalars(u, reader.invoke)
    ]
    # Extra real slots distinguish signed division and fixed-stride aliasing.
    for x, y in [(0, 20), (511, 20)]:
        cell = reader.alloc(0x160)
        cells[x, y] = cell
        u.mem_write(cell, dwords(0x7E4EEC))
        u.mem_write(cell + 0x24, struct.pack("<2h", x, y))
        u.mem_write(cell + 0x11B, bytes((6, 0)))
        u.mem_write(0x26000000 + (y * 512 + x) * 4, dwords(cell))
    unit = reader.alloc(0x800)
    u.mem_write(unit, dwords(0x7F5C70))
    fresh = bytes(u.mem_read(bullet, 0x180))

    def retained_target():
        pointer = reader.read32(bullet + 0x10C)
        if not pointer:
            return {"kind": "none"}
        return {
            "kind": "dummy" if pointer == 0xABDC50 else "cell",
            "cell": list(struct.unpack("<2h", u.mem_read(pointer + 0x24, 4))),
        }

    cases = [
        dict(name="ground", xyz=[3200, 5248, 624]),
        dict(name="below_high", xyz=[3200, 5248, 831]),
        dict(name="high", xyz=[3200, 5248, 832]),
        dict(name="unmarked_high", xyz=[3200, 5248, 832], marked=False),
        dict(name="placement_scope", xyz=[3200, 5248, 624], scope=True),
        dict(name="empty_sentinel", xyz=[128, 128, 0]),
        dict(name="negative_fraction", xyz=[-1, 5248, 624]),
        dict(name="signed_alias", xyz=[-256, 5504, 624]),
        dict(name="unallocated_cell", xyz=[6528, 5248, 0]),
        dict(name="word_wrap", xyz=[(65536 + 12) * 256 + 128, 5248, 624], marked=False),
    ]
    rows = []
    for case in cases:
        supplied = dict(marked=True, scope=False)
        supplied.update(case)
        u.mem_write(bullet, fresh)
        u.mem_write(unit + 0x9C, dwords(*supplied["xyz"]))
        u.mem_write(unit + 0x74, bytes((supplied["marked"],)))
        u.mem_write(0xA8E7AC, dwords(int(supplied["scope"])))
        u.mem_write(bullet + 0x10C, dwords(unit))
        u.mem_write(bullet + 0xB0, dwords(unit))
        before = bytes(u.mem_read(bullet, 0x180))
        queries = []

        def observe(_uc, address, _size, _data):
            if address in (0x65C640, 0x65C660, 0x65C780, 0x65C7E0):
                raise AssertionError(("RNG", hex(address)))
            if address == 0x5657A0:
                pointer = reader.read32(u.reg_read(UC_X86_REG_ESP) + 4)
                queries.append(list(struct.unpack("<2h", u.mem_read(pointer, 4))))

        hook = u.hook_add(UC_HOOK_CODE, observe)
        try:
            reader.invoke(0x468430, bullet)
        finally:
            u.hook_del(hook)
        after = bytes(u.mem_read(bullet, 0x180))
        result = retained_target()
        changed = [i for i, (a, b) in enumerate(zip(before, after)) if a != b]
        assert all(0x10C <= i < 0x110 for i in changed)
        u.mem_write(bullet, before)
        reader.invoke(0x4684E0, bullet, (unit, 0))
        assert retained_target() == result
        rows.append(dict(
            input=supplied, target=result, map_queries=queries,
            lose_changed_bytes=changed, source_preserved=True,
            expiry_source_cleared=reader.read32(bullet + 0xB0) == 0,
        ))

    # The original guided AI must read the retained Cell after the Unit moves.
    u.mem_write(bullet, fresh)
    u.mem_write(0xA8E7AC, dwords(0))
    u.mem_write(unit + 0x9C, dwords(3200, 5248, 624))
    u.mem_write(unit + 0x74, b"\1")
    u.mem_write(bullet + 0x10C, dwords(unit))
    u.mem_write(bullet + 0xB0, dwords(0))
    old, velocity = [2688, 5248, 800], [1., 0., 0.]
    u.mem_write(bullet + 0x10, dwords(100))
    u.mem_write(bullet + 0x9C, dwords(*old))
    u.mem_write(bullet + 0xE8, struct.pack("<3d", *velocity))
    reference = reader.alloc(12)
    reader.invoke(0x5F65A0, unit, (reference,))
    u.mem_write(0xA8ED84, dwords(0))
    reader.invoke(0x4E1130, bullet + 0xB8, (bullet + 0x9C, reference, 2, 0x7FFFFFFF))
    reader.invoke(0x468430, bullet)
    retained = retained_target()
    u.mem_write(unit + 0x9C, dwords(4224, 6016, 624))
    u.mem_write(0xA8ED84, dwords(1))
    u.mem_write(SP, bytes(0x200))
    u.mem_write(SP + 0x24, dwords(*old))
    targets = []

    def observe_flight(_uc, address, _size, _data):
        if address in (0x65C640, 0x65C660, 0x65C780, 0x65C7E0):
            raise AssertionError(("RNG", hex(address)))
        if address == 0x5B20F0:
            targets.append(g.xyz(u, reader.read32(u.reg_read(UC_X86_REG_ESP) + 4)))

    hook = u.hook_add(UC_HOOK_CODE, observe_flight)
    try:
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_EBP, bullet)
        run_checked(u, 0x4668BD, 0x467B7A, required_addresses=(0x5B20F0,))
    finally:
        u.hook_del(hook)
    flight = dict(
        old=old, incoming_velocity=velocity, destination=[4224, 6016, 624],
        target=retained, homing_targets=targets, candidate=g.xyz(u, SP + 0x24),
        velocity=g.vec(u, bullet + 0xE8),
        course_locked=bool(u.mem_read(bullet + 0x105, 1)[0]),
        course_frames=g.i32(u, bullet + 0x108),
        closing_count=g.i32(u, bullet + 0x118),
        closing_bits=bytes(u.mem_read(bullet + 0x120, 8)).hex(),
        impact=u.mem_read(SP + 0x18, 1)[0],
    )
    return dict(initial=initial, rows=rows, flight=flight)


def teleport_metadata():
    return provenance(
        scope="Original LoseTarget468430 and PointerExpired4684E0; retained departure Cell through guided AI",
        assumptions=[
            "Ten supplied Unit poses cover marked ground, high threshold, unmarked high, placement scope, sentinel, negative fraction, signed cell alias, missing slot and signed16 wrap. Native getters, Map5657A0 and both Bullet callbacks execute unchanged.",
            "Original Object CRT entries initialize the height constants. Sparse flat level6 cells, Unit7F5C70 vtable and incoming Bullet fields are supplied; no Unit constructor, native Teleport Process, map loader or full Rules Process is executed.",
            "Source equals target in callback controls. LoseTarget may change only +10C; PointerExpired additionally clears +B0. No RNG or timer draws/writes occur in these bodies.",
            "One guided continuation moves the Unit only after native LoseTarget. Source is absent, ID100/frame1, XYZ2688,5248,800 and velocity1,0,0 are supplied. Original4668BD..467B7A observes the old Cell after the Unit moves to4224,6016,624; no world commit/damage/deletion claim.",
            "Selected physical HoverMissile/AAHeatSeeker2 readers and Object/Bullet/Cell/Homing initializers are inherited from existing owners. MissileLauncher uses the same stock BulletType; Rust also tests its final retail projection. x87 PC53/chop0E7F.",
        ],
        substitutions=[
            "Existing guided_step BulletReader supplies INI cache/archive, allocation and OS/CRT boundaries. Map allocation and retained Unit state are supplied. No getter, target-repair, map lookup or guided result is substituted; hooks observe and reject RNG.",
        ],
        entry_points={"lose_target": 0x468430, "pointer_expired": 0x4684E0,
                      "unit_coords": 0x5F65A0, "unit_high": 0x4DE620,
                      "map_cell": 0x5657A0, "guided_ai": 0x4668BD,
                      "homing": 0x5B20F0, "precommit_stop": 0x467B7A},
    )


if __name__ == "__main__":
    selector = argparse.ArgumentParser(add_help=False)
    selector.add_argument("--teleport-targets", action="store_true")
    selected, remaining = selector.parse_known_args()
    if selected.teleport_targets:
        finish_vectors(teleport_targets, Path(__file__).with_name("teleport_targets.json"),
                       provenance=teleport_metadata, argv=remaining)
    else:
        finish_vectors(generate, Path(__file__).with_suffix(".json"),
                       provenance=metadata, argv=remaining)
