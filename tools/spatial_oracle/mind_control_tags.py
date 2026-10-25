"""Original Bullet MindControl entry prefix and live object Tag callbacks.

This is deliberately not a full capture/world/campaign oracle. The Tag fixture owns actual
Tag/Trigger construction, dispatch, event predicates, actions and allocation.
Only its final HOST dispatch request is adapted into a call of 4690B0; native
callbacks are neither intercepted nor replaced. See mind_control_tags.md.
"""
from __future__ import annotations

import copy
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EIP, UC_X86_REG_ESP,
)
from tools import native_oracle as native
from tools.spatial_oracle import tag_lifecycle as tag
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.team_creation import u32, i32


SOURCE, SOURCE_TYPE, TARGET_TYPE, WARHEAD, MANAGER, BULLET, OTHER_HOUSE, XYZ = (
    native.SCRATCH + n for n in
    (0xA0000, 0xA2000, 0xA4000, 0xA6000, 0xA8000, 0xAA000, 0xAC000, 0xAE000)
)
DETONATE, MIND_CONTROL, CAPTURE, COMMON_END = 0x4690B0, 0x46920B, 0x471D40, 0x469AA4
ORIGINAL_FIXTURE = tag.Fixture


def cases():
    base = dict(name="live_tag", seed=31, game_mode=1, repeat=2, lifecycle=True,
        triggers=[dict(events=[[6, 0]], actions=[[28, 7]]),
                  dict(events=[[44, 0]], actions=[[28, 8]])],
        operations=[["shared"], ["object"], ["attach_object", 0, 0],
                    ["dispatch", 0, 6, [0, 0], 0, 0]],
        probe=dict(firer=True, manager=True, target=True, alive=True,
                   attached=True, same_owner=False, human_target=False,
                   execute_rejection=False))
    rows = [base]
    for name, delta in (
        ("no_firer", dict(firer=False)),
        ("no_manager", dict(manager=False)),
        ("no_tag", dict(attached=False)),
        ("dead_target", dict(alive=False)),
        ("human_target", dict(human_target=True)),
        ("null_target_rejected", dict(target=False, execute_rejection=True)),
        ("same_owner_rejected", dict(same_owner=True, execute_rejection=True)),
    ):
        row = copy.deepcopy(base)
        row["name"] = name
        row["probe"].update(delta)
        rows.append(row)
    for name, action in (("event6_disables_self", 54), ("event6_removes_self", 12)):
        row = copy.deepcopy(base)
        row["name"] = name
        row["triggers"][0]["actions"].insert(0, [action, 0, 0])
        rows.append(row)
    row = copy.deepcopy(base)
    row["name"] = "event44_other_house_index"
    row["triggers"][1]["events"] = [[44, 1]]
    rows.append(row)
    return rows


def execute(case):
    observations = {}

    class PrefixFixture(ORIGINAL_FIXTURE):
        def call(self, entry, receiver, args, count=500000):
            if entry != 0x6E53A0:
                return super().call(entry, receiver, args, count=count)
            assert not observations, "exactly one host ingress per fresh VM"
            assert len(args) == 5 and args[0] == 6
            self.probe(receiver, args[1])
            # tag.execute uses a byte return for its ordinary HOST dispatch.
            # Detonate has not returned. The result is explicitly nulled below.
            return 0

        def probe(self, attached_tag, target):
            u, prior = self.u, case["probe"]
            # Keep Object's actual base construction and attachment. Everything
            # above Object is declared supplied Unit/Techno state, not a Unit
            # constructor, map admission, projectile launch or type-reader result.
            u.mem_write(target + 0xA8, bytes(0x1000 - 0xA8))
            for actor, typ, house in ((SOURCE, SOURCE_TYPE, tag.HOUSE),
                    (target, TARGET_TYPE, tag.HOUSE if prior["same_owner"] else OTHER_HOUSE)):
                u.mem_write(actor, dwords(0x7F5C70))
                u.mem_write(actor + 0x14, dwords(3))  # actual Object/Techno bits used here
                u.mem_write(actor + 0x90, b"\1")
                u.mem_write(actor + 0x9C, dwords(2688, 2688, 0))
                u.mem_write(actor + 0xAC, dwords(5))
                u.mem_write(actor + 0x21C, dwords(house))
                u.mem_write(actor + 0x6C4, dwords(typ))
                u.mem_write(typ, dwords(0x7F6218))
            u.mem_write(target + 0x90, bytes([prior["alive"]]))
            if not prior["attached"]:
                # Actual Object::ReplaceTag unlinks the constructed attachment.
                super().call(0x5F5B50, target, [0])
            u.mem_write(OTHER_HOUSE + 0x34, dwords(tag.HOUSE_TYPE))
            u.mem_write(tag.HOUSE_TYPE + 0x98, dwords(0))
            u.mem_write(0xA83D4C, dwords(OTHER_HOUSE if prior["human_target"] else tag.HOUSE))
            # Existing original file-static zero Cell initializer, also used by
            # Bullet::PointerExpired. Do not infer its runtime value from BSS.
            super().call(0x466270, 0, [])
            assert u32(u, 0x89DDF0) == 0
            # Actual CaptureManager constructor prefix; its final global append
            # is not needed by this direct Bullet-owned pointer chain.
            u.mem_write(self.sp, dwords(native.RET_MAGIC, SOURCE, 3, 0))
            u.reg_write(UC_X86_REG_ESP, self.sp)
            u.reg_write(UC_X86_REG_ECX, MANAGER)
            native.run_checked(u, 0x4717D0, 0x471835,
                required_addresses=(0x4717D0, 0x410170),
                context=dict(case=case["name"], stage="manager_constructor_prefix"))
            assert u32(u, MANAGER + 0x48) == SOURCE
            assert i32(u, MANAGER + 0x3C) == 3 and i32(u, MANAGER + 0x34) == 0
            u.mem_write(SOURCE + 0x2BC, dwords(MANAGER if prior["manager"] else 0))
            u.mem_write(WARHEAD + 0x155, b"\1")
            # Explicit zero shake bounds, absent Weapon/RadLevel prior. These
            # skip unrelated Detonate prefix effects through original branches.
            u.mem_write(WARHEAD + 0x17C, bytes(16))
            u.mem_write(BULLET + 0x128, dwords(WARHEAD))
            u.mem_write(BULLET + 0x130, dwords(0))
            u.mem_write(BULLET + 0xB0, dwords(SOURCE if prior["firer"] else 0))
            u.mem_write(BULLET + 0x10C, dwords(target if prior["target"] else 0))
            u.mem_write(XYZ, dwords(2688, 2688, 0))
            trace, active, visits = [], [], set()
            watch = {DETONATE, MIND_CONTROL, 0x469276, 0x46929D, 0x4692A2, 0x4692B8,
                     0x4692BD, 0x4692C3, 0x4692CA,
                     0x4692D0, 0x4692D5, 0x50B6F0, 0x6E53A0, 0x7264C0,
                     0x7265C0, 0x71E940, 0x6DD8B0, CAPTURE, 0x471C90, 0x65C837}
            original_ranges = [(native.IMAGE_BASE + rva, raw_size)
                for rva, _, raw_size, _, flags in native._sections(native.image_bytes())
                if flags & 0x20000000]
            original_ranges += [(0x7F5C70, 0x500), (0x7F6218, 0x100)]
            original_bytes = [bytes(u.mem_read(a, n)) for a, n in original_ranges]

            def state():
                live_firer = u32(u, BULLET + 0xB0)
                return dict(target=target, target_tag=u32(u, target + 0x34),
                    target_owner=u32(u, target + 0x21C),
                    bullet_raw_target=u32(u, BULLET + 0x10C), firer=live_firer,
                    firer_owner=u32(u, live_firer + 0x21C) if live_firer else None,
                    manager=u32(u, SOURCE + 0x2BC), target_alive=bool(u.mem_read(target + 0x90, 1)[0]))

            def observe(_u, pc, _size, _data):
                sp = u.reg_read(UC_X86_REG_ESP)
                if active and pc == active[-1]["return_pc"] and sp == active[-1]["return_sp"]:
                    item = active.pop()
                    trace.append(dict(kind="return", callee=item["callee"], pc=pc,
                                      al=u.reg_read(UC_X86_REG_EAX) & 255, state=state()))
                if pc not in watch:
                    return
                visits.add(pc)
                item = dict(kind="entry", pc=pc, ecx=u.reg_read(UC_X86_REG_ECX), state=state())
                if pc == 0x6E53A0:
                    item.update(event=i32(u, sp + 4), object=u32(u, sp + 8),
                        cell=list(struct.unpack("<hh", u.mem_read(sp + 12, 4))),
                        force=u32(u, sp + 16), source=u32(u, sp + 20))
                    active.append(dict(callee=pc, return_pc=u32(u, sp), return_sp=sp + 24))
                elif pc == 0x6DD8B0:
                    item.update(action=i32(u, item["ecx"] + 0x2C),
                                value=i32(u, item["ecx"] + 0x90))
                elif pc == CAPTURE:
                    item.update(raw_target=u32(u, sp + 4),
                                target_was_human=u.reg_read(UC_X86_REG_EBX) & 255)
                    # Null/same-owner rows continue through the real rejection.
                    if prior["execute_rejection"]:
                        active.append(dict(callee=pc, return_pc=u32(u, sp), return_sp=sp + 8))
                trace.append(item)

            hook = u.hook_add(UC_HOOK_CODE, observe)
            before = state()
            rng_before = bytes(u.mem_read(tag.SCENARIO + 0x218, 0x3F4))
            u.mem_write(self.sp, dwords(native.RET_MAGIC, XYZ))
            u.reg_write(UC_X86_REG_ESP, self.sp)
            u.reg_write(UC_X86_REG_ECX, BULLET)
            can_attempt = prior["firer"] and prior["manager"]
            end = COMMON_END if not can_attempt or prior["execute_rejection"] else CAPTURE
            required = [DETONATE, MIND_CONTROL]
            should_dispatch = can_attempt and prior["target"] and prior["alive"] and prior["attached"]
            if should_dispatch:
                required += [0x46929D, 0x4692B8, 0x6E53A0, 0x7264C0, 0x71E940, 0x7265C0, 0x6DD8B0]
            if prior["execute_rejection"]:
                required += [CAPTURE, 0x4692D5]
            try:
                stopped = native.run_checked(u, DETONATE, end,
                    required_addresses=required, count=1_000_000,
                    context=dict(case=case["name"], stage="detonate_entry_prefix", prior=prior))
            finally:
                u.hook_del(hook)
            assert original_bytes == [bytes(u.mem_read(a, n)) for a, n in original_ranges]
            assert not active, active
            callbacks = [t for t in trace if t["kind"] == "entry" and t["pc"] == 0x6E53A0]
            assert [t["event"] for t in callbacks] == ([6, 44] if should_dispatch else [])
            for t in callbacks:
                assert (t["object"], t["source"], t["cell"], t["force"]) == (target, SOURCE, [0, 0], 0)
            boundary = dict(pc=stopped, ecx=u.reg_read(UC_X86_REG_ECX),
                            state=state())
            if stopped == CAPTURE:
                # Unicorn's primary end address is not executed or hooked.
                # Observe the actual incoming ABI state without claiming entry.
                boundary.update(raw_target=u32(u, u.reg_read(UC_X86_REG_ESP) + 4),
                    return_pc=u32(u, u.reg_read(UC_X86_REG_ESP)),
                    target_was_human=u.reg_read(UC_X86_REG_EBX) & 255)
                assert boundary["ecx"] == MANAGER and boundary["raw_target"] == target
                assert boundary["return_pc"] == 0x4692D5
            observations.update(before=before, after=state(), trace=trace,
                boundary=boundary,
                stop_pc=stopped, stop_before_capture_body=stopped == CAPTURE,
                reached_addresses=sorted(visits), original_code_and_vtables_unchanged=True,
                scenario_full_rng_equal=rng_before == bytes(u.mem_read(tag.SCENARIO + 0x218, 0x3F4)))

    tag.Fixture = PrefixFixture
    try:
        result = tag.execute(case)
    finally:
        tag.Fixture = ORIGINAL_FIXTURE
    result["history"][-1]["returned"] = None
    result.pop("next_random")  # existing fixture's separate terminal probe is not this boundary
    return compact(case, result, observations)


def compact(case, result, observed):
    """Keep measured callback/state boundaries; omit raw pointers and RNG words."""
    target = observed["before"]["target"]
    pointers = {0: None, SOURCE: "firer", target: "target", MANAGER: "manager",
                tag.HOUSE: "firer_house", OTHER_HOUSE: "other_house"}
    tag_pointer = next((t["ecx"] for t in observed["trace"]
                        if t["kind"] == "entry" and t["pc"] == 0x6E53A0), None)
    # No-tag/gated rows can have no callback. The remaining original attachment
    # is still observable without guessing the allocator's actual address.
    if tag_pointer is None:
        tag_pointer = observed["before"]["target_tag"] or observed["after"]["target_tag"]
    if tag_pointer:
        pointers[tag_pointer] = "tag"

    def pointer(value):
        if value is None:
            return None
        assert value in pointers, hex(value)
        return pointers[value]

    def state(value):
        return {key: (raw if key == "target_alive" else pointer(raw))
                for key, raw in value.items()}

    calls, reloads = [], []
    names = {0x6E53A0: "tag", 0x6DD8B0: "action", CAPTURE: "capture"}
    for event in observed["trace"]:
        pc = event["pc"]
        if event["kind"] == "return" and event["callee"] in names:
            calls.append(dict(kind=names[event["callee"]] + "_return", pc=pc,
                              al=event["al"], state=state(event["state"])))
        elif event["kind"] == "entry" and pc in names:
            item = dict(kind=names[pc], pc=pc)
            if pc == 0x6E53A0:
                item.update(tag=pointer(event["ecx"]), event=event["event"],
                    object=pointer(event["object"]), source=pointer(event["source"]),
                    cell=event["cell"], force=event["force"])
            elif pc == 0x6DD8B0:
                item.update(action=event["action"], value=event["value"])
            else:
                item.update(manager=pointer(event["ecx"]), target=pointer(event["raw_target"]),
                            target_was_human=bool(event["target_was_human"]))
            calls.append(item)
        elif pc in (0x4692A2, 0x4692BD, 0x4692C3, 0x4692CA):
            reloads.append(dict(pc=pc, state=state(event["state"])))
    boundary = observed["boundary"]
    endpoint = dict(pc=boundary["pc"], before_capture_entry=observed["stop_before_capture_body"])
    if observed["stop_before_capture_body"]:
        endpoint.update(manager=pointer(boundary["ecx"]), target=pointer(boundary["raw_target"]),
                        return_pc=boundary["return_pc"],
                        target_was_human=bool(boundary["target_was_human"]))
    final = result["history"][-1]
    return dict(input={k: case[k] for k in ("name", "seed", "game_mode", "repeat", "triggers", "probe")},
        before=state(observed["before"]), after=state(observed["after"]),
        tag_state={k: final[k] for k in ("tags", "actions", "pending_count", "pending_tags",
                                        "globals", "locals", "changed", "object_tags")},
        calls=calls, live_pointer_reloads=reloads, boundary=endpoint,
        rng=dict(scenario_draws=sum(t["pc"] == 0x65C837 for t in observed["trace"]),
                 full_scenario_state_equal=observed["scenario_full_rng_equal"]),
        reached_addresses=observed["reached_addresses"],
        original_code_and_vtables_unchanged=observed["original_code_and_vtables_unchanged"])


def generate():
    return dict(schema_version=1, cases=[execute(case) for case in cases()])


def metadata():
    return native.provenance(
        scope='Eleven original4690B0 entry-prefix controls through live object Tag6/44 dispatch, predicate and actions before CaptureUnit471D40. Null-target and same-owner controls execute genuine capture refusal and stop before469AA4; other admitted-looking rows stop before CaptureUnit entry. No successful capture, completed Detonate, firing/flight, sound, full owner-change or physical campaign replay claim.',
        entry_points=dict(detonate=DETONATE, mind_control=MIND_CONTROL,
            tag_event6=0x46929D, tag_event44=0x4692B8, tag_dispatch=0x6E53A0,
            trigger_predicate=0x7264C0, event_predicate=0x71E940, spring=0x7265C0,
            action=0x6DD8B0, capture=CAPTURE, can_capture=0x471C90,
            common_end=COMMON_END, manager_constructor=0x4717D0,
            manager_registration_boundary=0x471835, empty_cell_initializer=0x466270),
        assumptions=[
            'Each row is a fresh tag_lifecycle.execute VM: original Tag/Trigger/Object construction and ObjectReplaceTag run with synthetic resolved definitions, spare native registry vectors, game mode1, frame123, Scenario seed31 and inherited false live-start flags. One linked event6/action28 and event44/action28 graph distinguishes actual predicate/actions; controls use original54 self-disable and12 self-removal. These are not altered stock map definitions.',
            'Bullet, source and target Techno extensions, types and Houses are supplied minimal state with original Unit vtables. The target base Object and attachment are actual constructor products. Unit/House/Bullet constructors, map admission, type readers and complete world listeners are excluded. Explicit zero shake bounds and absent Weapon skip unrelated Detonate prefix branches naturally.',
            'Original CaptureManager constructor4717D0 runs to471835 before its global append, producing owner/capacity/vector defaults. Original466270 initializes the empty Cell read by both callbacks. All executable image sections and relevant original vtables remain unchanged.',
            'The original caller checks live target+90 and Tag+34, calls6 then44 with the original source pointers, and reloads Bullet+B0/+10C and manager before capture. Callback order/returns, actual actions, pending/enabled state, zero-cell/force arguments and live reload state are retained. The pre-callback target-human byte is observed at CaptureUnit boundary. No cached sound-owner claim is made because successful capture/sound is outside this execution.',
            'Single-use Tag disposal is excluded: it broadcasts into TechnoPointerExpired, which needs constructor-owned distributed-fire vector+470 and full listener lifecycle absent in this sparse fixture. No vtable/gameplay return was substituted to bypass that prerequisite. Complete owner-changing callbacks and destroyed-source behavior likewise remain outside these controls; Action14 ChangeHouse uses TechnoTargetSweep, not Bullet PointerExpired.',
            'The compact corpus translates observed pointer identities to labels and records full1012-byte Scenario RNG equality plus actual draw count rather than the RNG words. Inherited fixture terminal RNG continuation is outside the measured boundary and omitted. Every row requires the declared original dispatch/predicate/rejection PCs and fails on a native fault or unexpected stop.'],
        substitutions=[
            'Reuse tag_lifecycle validated Windows IsBadReadPtr transport and bounded poisoned operator-new/free storage. No gameplay function, branch, vtable slot, callback or native return is replaced.',
            'The final HOST Tag-dispatch request is adapted into a call at original4690B0. Nested native Tag/capture calls execute directly; its legacy host return slot is omitted because Detonate has not returned. Sparse supplied actor state is declared above, not produced by a native launch.'])


if __name__ == "__main__":
    native.finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
