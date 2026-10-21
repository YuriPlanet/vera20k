"""Original tactical modifier and infantry weapon-cursor controls.

Reuses bunker_attack_action's immutable-code VM and environmental seams.
Modifier controls execute DetermineAction and full Display. Infantry controls
start at 51E9C1 with base action and weapon index supplied; they establish the
suffix, not preceding weapon selection or fire legality. For actions16/64/71,
Foot ClickedAction runs to the original ClickedMission argument boundary.
"""
import argparse
import json
from itertools import product
from pathlib import Path

from tools.spatial_oracle.bunker_attack_action import (
    ACTOR, COORD, RET_MAGIC, SP, TARGET, TARGET_TYPE, TYPE, WEAPON,
    UC_HOOK_CODE, UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP, dwords, run, run_checked,
)


def weapon_control(spec):
    def continuation(u, read, ret, calls, result, receiver):
        u.mem_write(ACTOR, dwords(0x7EB058))  # Infantry vtable
        u.mem_write(TYPE, dwords(0x7EB610))  # InfantryType vtable
        u.mem_write(TYPE + 0xEC2, bytes([spec.get('c4', False)]))
        u.mem_write(WEAPON + 0x139, bytes([
            spec.get('sabotage', False), spec.get('mig', False)]))
        if spec.get('building'):
            u.mem_write(TARGET, dwords(0x7E3EBC))
            u.mem_write(TARGET + 0x520, dwords(TARGET_TYPE))
            u.mem_write(TARGET_TYPE, dwords(0x7E43E8))
            u.mem_write(TARGET_TYPE + 0x1577, bytes([spec.get('can_c4', True)]))
            u.mem_write(TARGET_TYPE + 0x1701, bytes([spec.get('invisible', False)]))
        u.mem_write(TARGET + 0x2E4, dwords(ACTOR if spec.get('bunkered') else 0))
        visited = []

        def observer(machine, pc, *_):
            if pc == 0x5218E0:
                visited.append(dict(seam='supplied Infantry SelectWeapon index', index=0))
                ret(0, 1)
            if pc in (0x51E9C1, 0x51EA59, 0x51EA80, 0x51EAD2, 0x51EB24):
                visited.append(hex(pc))

        hook = u.hook_add(UC_HOOK_CODE, observer)
        # Real Infantry51E3B0 saved-register/local stack shape, as in the
        # shared VM's existing infantry_bomb_control. No game bytes patched.
        u.mem_write(SP, dwords(0, 0, 0, 0))
        u.mem_write(SP + 0x38, dwords(RET_MAGIC, TARGET, 0))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_EDI, ACTOR)
        u.reg_write(UC_X86_REG_ESI, TARGET)
        u.reg_write(UC_X86_REG_EBP, spec.get('base_action', 5))
        try:
            stop = run_checked(u, 0x51E9C1, (RET_MAGIC, 0x51EB24), count=5000,
                               required_addresses=[0x51E9C1])
        finally:
            u.hook_del(hook)
        returned = stop == RET_MAGIC
        if returned:
            assert u.reg_read(UC_X86_REG_ESP) == SP + 0x44
        action = u.reg_read(UC_X86_REG_EAX if returned else UC_X86_REG_EBP)
        u.mem_write(SP + 0x20, dwords(RET_MAGIC, COORD, 0, TARGET, action, 0))
        u.reg_write(UC_X86_REG_ESP, SP + 0x20)
        u.reg_write(UC_X86_REG_ECX, receiver)
        run_checked(u, 0x4AAE90, RET_MAGIC + 0x100, count=10000,
                    required_addresses=[0x4AAE90, 0x4AB449])
        row = read(u.reg_read(UC_X86_REG_ESP) + 4)
        mission = None
        if action in (16, 64, 71):
            u.mem_write(SP, dwords(RET_MAGIC, action, TARGET, 0))
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_ECX, ACTOR)
            run_checked(u, 0x4D74E0, 0x6FFBE0, count=10000,
                        required_addresses=[0x4D74E0])
            sp = u.reg_read(UC_X86_REG_ESP)
            mission = dict(mission=read(sp + 4), target_is_object=read(sp + 8) == TARGET,
                           destination_is_object=read(sp + 12) == TARGET)
        return dict(weapon_output=dict(action=action, cursor_row=row, returned=returned,
                                       visited=visited, clicked_mission=mission))

    case = dict(cursor_contract=True, mouse_over=True, route='object', relation='enemy',
                installed=False, in_range=True, ctrl=False)
    result = run(case, continuation)
    return dict(input=spec, output=result['weapon_output'])


def generate():
    controls = []
    routes = [('cell', 'none', False), ('object', 'enemy', False),
              ('object', 'own', False), ('object', 'own', True)]
    for route, relation, self_target in routes:
        for ctrl, shift, alt in product((False, True), repeat=3):
            case = dict(cursor_contract=True, mouse_over=True, route=route,
                        relation=relation, self_target=self_target, installed=False,
                        in_range=True, ctrl=ctrl, shift=shift, alt=alt)
            result = run(case)
            controls.append(dict(input=case, action=result['action'],
                                 cursor_row=result['cursor']['output']['cursor_row']))
    for option in ['unarmed', 'mixed_unarmed_selection']:
        for ctrl, shift, alt in product((False, True), repeat=3):
            case = dict(cursor_contract=True, mouse_over=True, route='cell', relation='none',
                        installed=False, in_range=True, ctrl=ctrl, shift=shift, alt=alt,
                        **{option: True})
            if option == 'mixed_unarmed_selection':
                case['display_selected_count'] = 2
            result = run(case)
            controls.append(dict(input=case, action=result['action'],
                                 cursor_row=result['cursor']['output']['cursor_row']))
    specs = [
        dict(name='sapper', sabotage=True),
        dict(name='sapper_bunkered', sabotage=True, bunkered=True),
        dict(name='sapper_not_attack', sabotage=True, base_action=7),
        dict(name='c4_building', c4=True, building=True),
        dict(name='c4_barred', c4=True, building=True, can_c4=False),
        dict(name='flare', mig=True, building=True),
        dict(name='flare_vehicle', mig=True),
        dict(name='flare_barred', mig=True, building=True, can_c4=False),
        dict(name='flare_invisible', mig=True, building=True, invisible=True),
        dict(name='plain', building=True),
    ]
    return dict(schema=1, modifiers=controls,
                weapons=[weapon_control(spec) for spec in specs])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--check', action='store_true')
    mode.add_argument('--write', action='store_true')
    parser.add_argument('--output', type=Path, default=Path(__file__).with_suffix('.json'))
    args = parser.parse_args()
    payload = json.dumps(generate(), indent=2) + '\n'
    if args.check:
        if args.output.read_text() != payload:
            raise SystemExit('native tactical cursor corpus changed')
        print('58 native tactical cursor controls match')
    else:
        args.output.write_text(payload)


if __name__ == '__main__':
    main()
