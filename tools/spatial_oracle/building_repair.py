"""Original building repair: BuildingClass::ToggleRepair, the repair step's
cost, BuildingClass::UpdateRepairAndPower's auto-repair start and repair tick,
and HouseClass::Update's release of the owner's auto-repair latch.

- `cost` rows: BuildingTypeClass's repair step cost (vt+0xB0 = 0x7120D0)
  natively on the slave_manager fixture's refinery type: GetCost (vt+0xAC =
  0x45ED50: Cost through TechnoTypeClass::GetCost 0x711EB0, less the average
  of the two PadAircraft= types' costs when this type is the first pad
  aircraft's first Dock= and SeparateAircraft= is clear, less the FreeUnit='s
  cost floored at 0), Strength / RepairStep and the cost over those steps
  (IDIV), times RepairPercent under the ambient PC53/chop x87 word, through
  ftol, at least 1.
- `toggle` rows: BuildingClass::ToggleRepair (0x446FF0, vt+0x19C) over its
  control (-1 toggles, 0 stops, 1 starts, any other keeps), the repair byte
  (+0x6E8), Health at or below Strength and the local player (IsHumanPlayer
  0x50B6F0: PlayerPtr in a multiplayer game, +0x1EC or +0x1ED in a campaign):
  ScoldSound= or GenericClick= through VocClass::PlayAt (0x7509E0) at the
  building's Location, the flash (vt+0x148 = 0x456E00), the wrench byte
  (+0x6DE) and EVA_Repairing (0x752700). Flash, EVA and PlayAt are observed
  and answered.
- `update` rows: BuildingClass::UpdateRepairAndPower (0x450630) from its entry
  to its return on the refinery: the admission (CurrentIQ against [IQ]
  RepairSell=, Get_Mission, Can_Repair, Available_Money against [AI]
  CreditReserve=), the computer's auto-repair start (0x4506B2: the owner's
  latch +0x245, the repair byte, HasBeenCaptured +0x6E3, AIRepairable +0x6CB,
  IsControlledByHuman 0x50B730; the latch, ToggleRepair(1), and for a house
  no human controls the latch timer +0x280 = {Frame, TimeLeft =
  RandomRanged(ftol(RepairDelay * 225), ftol(RepairDelay * 1800))} on the
  Scenario RNG, RepairDelay = House+0x1C0) and the repair tick (0x450813:
  Frame % ftol(RepairRate * 900), the wrench byte, the step cost against
  Available_Money, HouseClass::Spend_Money 0x4F9790, RepairStep added to
  Health +0x6C and the estimate +0x70, the clamp at Strength that ends the
  repair, the damage-state slots and the smoke's retirement).
- `build` rows: a damaged building's build-up on the routes of
  building_construction's `route` rows (a deploy, a computer house's
  placement, a human player's placement), per frame BuildingClass::Update's
  construction pieces then UpdateRepairAndPower, until two frames after
  Grand_Opening: Get_Mission reads Construction (current, or only queued
  before a human player's placement commences) through the build-up and
  Guard on the completion frame, whose UpdateRepairAndPower starts the
  repair. The fixture's observers also see the pieces' calls and the Guard
  mission's draws; each frame keeps only what UpdateRepairAndPower calls and
  draws.
- `wrench` rows: TechnoClass::DrawExtras' repair wrench frame (0x6F52D8..
  0x6F532D) over the stored game speed (0xA8EB60) and Frame: the cycle
  SpeedNormalize(14) / 4 (signed, at least 2) and WRENCH.SHP's frame
  ((Frame % cycle) * 6) / (cycle - 1).
- `release` rows: HouseClass::Update's release of the latch (0x4F9302..
  0x4F9338): the latch clears once its timer expired (Start -1: TimeLeft 0;
  otherwise Frame - Start >= TimeLeft, signed).

Usage: python -m tools.spatial_oracle.building_repair [--check|--write]
       python -m tools.spatial_oracle.building_repair --depot-service [--check|--write]
       python -m tools.spatial_oracle.building_repair --depot-waiters [--check|--write]
"""
import hashlib
import os
from pathlib import Path
import struct
import sys

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX,
                               UC_X86_REG_ESP, UC_X86_REG_ESI, UC_X86_REG_EDI,
                               UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_EIP,
                               UC_X86_REG_FPCW)
from tools.native_oracle import RET_MAGIC, finish_vectors, provenance, run_checked
from tools.native_oracle import require
from tools.spatial_oracle import building_construction as bc
from tools.spatial_oracle import slave_manager as sm
from tools.spatial_oracle import refinery_dock as rd
from tools.spatial_oracle.building_sale import MISSION, coord, ret, signed
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.refinery_dock import HOUSE, RULES
from tools.spatial_oracle.unit_source_scatter import SCENARIO

TOGGLE_REPAIR, REPAIR_STEP_COST, UPDATE_REPAIR_AND_POWER = 0x446FF0, 0x7120D0, 0x450630
HOUSE_RELEASE = (0x4F9302, 0x4F9338)
# RandomRanged, the returns of the latch timer's draw and of the sale's roll.
RANDOM_RANGED, TIMER_DRAW_RETURN, SALE_ROLL_RETURN = 0x65C7E0, 0x45075E, 0x4507C9
PLAY_AT, PLAY_EVA, BUILDING_FLASH = 0x7509E0, 0x752700, 0x456E00
# BuildingClass::GetCurrentFrame (only the redraw byte +0x80 reads it here),
# the damage-state slot anim and Sell_Back.
CURRENT_FRAME, CREATE_ANIM_FOR_SLOT, SELL_BACK = 0x43EF90, 0x451890, 0x447110
PLAYER_PTR, FRAME = 0xA83D4C, 0xA8ED84
# TechnoClass::DrawExtras' wrench frame and the stored game speed
# (GameOptionsClass 0xA8EB60, SpeedNormalize's receiver).
WRENCH_FRAME, GAME_SPEED = (0x6F52D8, 0x6F532D), 0xA8EB60
HOUSE_MONEY_VTABLE = 0x7EA834
# AircraftTypeClass and UnitTypeClass vtables (GetCost vt+0xAC = 0x711EB0) for
# the PadAircraft= pair and the FreeUnit=; ParticleSystemClass's for the smoke
# (+0x310), whose vt+0xF8 sets its done byte (+0xF8).
AIRCRAFT_TYPE_VTABLE, UNIT_TYPE_VTABLE, PARTICLE_SYSTEM_VTABLE = 0x7E2868, 0x7F6218, 0x7EFB9C
# Scratch after the slave_manager fixture's region: the pad pair and its
# vector, the free unit's type and the smoke.
REGION = sm.REGION + 0x40000
PAD_ITEMS, PAD_TYPES, PAD_DOCKS = REGION, REGION + 0x1000, REGION + 0x3000
FREE_UNIT = REGION + 0x4000
SMOKE = REGION + 0x6000
TYPE_SIZE = 0x1000
# Sound indices for [AudioVisual] ScoldSound= and GenericClick=.
SCOLD_SOUND, GENERIC_CLICK = 43, 42
# Retail `RepairPercent=15%` (ReadDouble: 15.0f widened, times .01) and the
# RulesClass constructor's .25; retail `RepairRate=.016` and the difficulty
# rows' `RepairDelay=.02`/`.05` (a `%f` float widened); the ReadDifficulty
# default .02 (0x0066D317).
PERCENT_15, PERCENT_25 = 0x3FC3333333333333, 0x3FD0000000000000


def widened(text):
    return struct.unpack('<Q', struct.pack('<d', struct.unpack('<f', struct.pack('<f', float(text)))[0]))[0]


RATE_016, DELAY_02, DELAY_05, DELAY_02_DEFAULT = widened('.016'), widened('.02'), widened('.05'), 0x3F947AE147AE147B


def double_bits(bits):
    return struct.pack('<Q', bits)


def fixture(case):
    """The slave_manager fixture's refinery (a Building over YTYPE, 2x2 at NW
    (12, 12)) owned by HOUSE, with the pad vector the type cost reads."""
    u, call, read32, events = sm.make_fixture(dict(
        name=case['name'], manager_state=0, nodes=[], ore=[], seed=case.get('seed', 1),
        human=case.get('human', False), game_mode=case.get('game_mode', 1)))
    u.mem_map(REGION, 0x10000)
    kind = sm.YTYPE
    # Cost=, Strength=, FreeUnit=, the Rules repair keys.
    u.mem_write(kind + 0x610, dwords(case.get('cost', 2500)))
    u.mem_write(kind + 0xA0, dwords(case.get('strength', 1000)))
    free_unit = case.get('free_unit')
    u.mem_write(kind + 0xEA0, dwords(FREE_UNIT if free_unit is not None else 0))
    if free_unit is not None:
        u.mem_write(FREE_UNIT, dwords(UNIT_TYPE_VTABLE))
        u.mem_write(FREE_UNIT + 0x610, dwords(free_unit))
    u.mem_write(RULES + 0x16CC, dwords(case.get('step', 8)))
    u.mem_write(RULES + 0x16D0, double_bits(case.get('percent', PERCENT_15)))
    u.mem_write(RULES + 0x16E0, double_bits(case.get('rate', RATE_016)))
    # [General] PadAircraft= (the vector's items at Rules+0xB5C), each type's
    # Cost= and Dock= list (+0x3EC), and SeparateAircraft= (+0x17E8).
    pads = case.get('pads', [1000, 1200])
    u.mem_write(RULES + 0xB5C, dwords(PAD_ITEMS))
    u.mem_write(PAD_ITEMS, dwords(*[PAD_TYPES + index * TYPE_SIZE for index in range(len(pads))]))
    for index, pad_cost in enumerate(pads):
        pad = PAD_TYPES + index * TYPE_SIZE
        u.mem_write(pad, dwords(AIRCRAFT_TYPE_VTABLE))
        u.mem_write(pad + 0x610, dwords(pad_cost))
        u.mem_write(pad + 0x3EC, dwords(PAD_DOCKS + index * 0x10))
    u.mem_write(PAD_DOCKS, dwords(kind if case.get('pad_dock') else sm.STYPE))
    u.mem_write(RULES + 0x17E8, bytes([case.get('separate_aircraft', True)]))
    return u, call, read32, events


# --- cost ----------------------------------------------------------------


def cost(case):
    u, _call, _read32, _events = fixture(case)
    value = bc.invoke(u, REPAIR_STEP_COST, sm.YTYPE)
    return dict(input=case, cost=struct.unpack('<i', dwords(value))[0])


def cost_cases():
    rows = []
    # Strength 1000 over RepairStep 8: 125 steps. Per-step costs 20, 40 and
    # 100 times 15% land just below 3, 6 and 15 in double precision.
    for base_cost in (0, 100, 750, 800, 875, 1000, 2000, 2500, 5000, 12500, 100000):
        rows.append(dict(name=f'k_{base_cost}', cost=base_cost))
    rows += [
        # The RulesClass constructor's 25%.
        dict(name='k_2500_default_percent', cost=2500, percent=PERCENT_25),
        # Strength not a multiple of the step, a step of 1, a large step.
        dict(name='k_uneven_strength', cost=2000, strength=1100),
        dict(name='k_step_one', cost=2000, step=1),
        dict(name='k_step_strength', cost=2000, step=1000),
        # A negative cost (the per-step IDIV truncates toward zero).
        dict(name='k_negative', cost=-5000),
        # FreeUnit=: less its cost, floored at 0 only there.
        dict(name='k_free_unit', cost=3000, free_unit=1400),
        dict(name='k_free_unit_exceeds', cost=3000, free_unit=5000),
        dict(name='k_free_unit_equal', cost=3000, free_unit=3000),
        # The first pad aircraft docks at this type: less the pair's average
        # (signed division) unless SeparateAircraft=.
        dict(name='k_pad_dock', cost=3000, pad_dock=True, separate_aircraft=False),
        dict(name='k_pad_dock_odd_sum', cost=3000, pad_dock=True, separate_aircraft=False, pads=[1000, 1201]),
        dict(name='k_pad_dock_separate', cost=3000, pad_dock=True),
        dict(name='k_pad_other_dock', cost=3000, separate_aircraft=False),
        dict(name='k_pad_dock_below_zero', cost=500, pad_dock=True, separate_aircraft=False),
        dict(name='k_pad_dock_free_unit', cost=3000, pad_dock=True, separate_aircraft=False, free_unit=1000),
    ]
    return rows


# --- toggle --------------------------------------------------------------


def observe_toggle(u, read32, events):
    def hook(_u, address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if address == BUILDING_FLASH:
            events.append(['flash', read32(sp + 4)])
            ret(u, read32, 4)
        elif address == PLAY_EVA:
            name = bytes(u.mem_read(u.reg_read(UC_X86_REG_ECX), 32)).split(b'\0')[0].decode('ascii')
            events.append(['eva', name, struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EDX)))[0],
                           struct.unpack('<i', dwords(read32(sp + 4)))[0]])
            ret(u, read32, 4)
        elif address == PLAY_AT:
            events.append(['play_at', u.reg_read(UC_X86_REG_ECX), coord(u, u.reg_read(UC_X86_REG_EDX)),
                           read32(sp + 4)])
            ret(u, read32, 4)

    u.hook_add(UC_HOOK_CODE, hook)


def building_state(u, read32):
    building = sm.YAREFN
    return dict(health=signed(u, building + 0x6C), estimate=signed(u, building + 0x70),
                repairing=u.mem_read(building + 0x6E8, 1)[0], wrench=u.mem_read(building + 0x6DE, 1)[0],
                damaged=u.mem_read(building + 0x6E6, 1)[0])


def toggle(case):
    u, _call, read32, events = fixture(case)
    building = sm.YAREFN
    u.mem_write(building + 0x6C, dwords(case['health']))
    u.mem_write(building + 0x6E8, bytes([case['repairing']]))
    u.mem_write(building + 0x6DE, bytes([case.get('wrench', 0)]))
    u.mem_write(RULES + 0x700, dwords(SCOLD_SOUND))
    u.mem_write(RULES + 0x70C, dwords(GENERIC_CLICK))
    u.mem_write(HOUSE + 0x1ED, bytes([case.get('player_control', False)]))
    u.mem_write(PLAYER_PTR, dwords(HOUSE if case.get('player') else 0))
    observe_toggle(u, read32, events)
    bc.invoke(u, TOGGLE_REPAIR, building, case['control'] & 0xFFFFFFFF)
    return dict(input=case, events=events, location=coord(u, building + 0x9C), **building_state(u, read32))


def toggle_cases():
    rows = []
    for control in (-1, 0, 1, 2):
        for repairing in (0, 1):
            for health, label in ((500, 'damaged'), (1000, 'full')):
                for player in (False, True):
                    rows.append(dict(name=f't_{control}_{repairing}_{label}_{"player" if player else "other"}',
                                     control=control, repairing=repairing, health=health, player=player))
    # A campaign: IsHumanPlayer is House+0x1EC or +0x1ED, not PlayerPtr.
    for human, player_control in ((True, False), (False, True), (False, False)):
        rows.append(dict(name=f't_campaign_{int(human)}{int(player_control)}', control=-1, repairing=0,
                         health=500, game_mode=0, human=human, player_control=player_control))
    # Health above Strength: GenericClick and the local player's EVA.
    rows.append(dict(name='t_above_strength', control=1, repairing=0, health=1001, player=True))
    return rows


# --- update --------------------------------------------------------------


# Slots whose occupant the damage-state change replaces: 0 and 2 name both
# anims, 1 is occupied without names, 3 has names and no occupant.
SLOT_OCCUPANTS = {0: True, 1: True, 2: True, 3: False}
SLOT_NAMES = {0: True, 1: False, 2: True, 3: True}


def prepare_update(u, read32, events, case):
    """The update rows' building, owner and Rules at the row's frame, with
    UpdateRepairAndPower's callees observed; returns the draws and the hook's
    state (GetCurrentFrame is answered only inside UpdateRepairAndPower)."""
    building, kind = sm.YAREFN, sm.YTYPE
    frame = case.get('frame', 196)
    u.mem_write(FRAME, dwords(frame))
    health = case.get('health', 300)
    u.mem_write(building + 0x6C, dwords(health, case.get('estimate', health)))
    u.mem_write(building + 0xAC, dwords(MISSION[case.get('mission', 'guard')]))
    u.mem_write(building + 0xB4, dwords(MISSION[case.get('queue', 'none')]))
    u.mem_write(building + 0x6E8, bytes([case.get('repairing', False)]))
    u.mem_write(building + 0x6DE, bytes([case.get('wrench', 0)]))
    u.mem_write(building + 0x6E3, bytes([case.get('captured', False)]))
    u.mem_write(building + 0x6CB, bytes([case.get('ai_repairable', True)]))
    u.mem_write(building + 0x3D1, bytes([case.get('attacked', False)]))
    u.mem_write(building + 0x6DC, b'\x00')
    u.mem_write(building + 0x34, dwords(0))
    u.mem_write(building + 0x80, b'\x00')
    # The retained damage state (+0x6E6) as the last change left it: damaged
    # at or below ConditionYellow.
    strength = case.get('strength', 1000)
    u.mem_write(building + 0x6E6, bytes([case.get('damaged', health * 2 <= strength)]))
    for slot in range(21):
        occupied = SLOT_OCCUPANTS.get(slot, False)
        u.mem_write(building + 0x55C + slot * 4, dwords(REGION + 0x8000 + slot * 0x10 if occupied else 0))
        for offset, prefix in ((0xF4C, 'N'), (0xF5C, 'D')):
            named = SLOT_NAMES.get(slot, False)
            u.mem_write(kind + slot * 0x44 + offset, (f'{prefix}{slot:02}' if named else '').encode() + b'\0')
    u.mem_write(building + 0x310, dwords(SMOKE if case.get('smoke', True) else 0))
    u.mem_write(SMOKE, dwords(PARTICLE_SYSTEM_VTABLE))
    # ClickRepairable=, Repairable=, no UndeploysInto=, a 2x2 foundation,
    # not a yard.
    u.mem_write(kind + 0x157A, bytes([case.get('click_repairable', True)]))
    u.mem_write(kind + 0xCCC, b'\x01')
    u.mem_write(kind + 0x408, dwords(0))
    u.mem_write(kind + 0xEF0, dwords(3))
    u.mem_write(kind + 0xEB8, dwords(-1))
    # The owner: money interface, Balance, CurrentIQ, the authored IQ and
    # TechLevel, the latch and its timer, RepairDelay, player control.
    u.mem_write(HOUSE + 0x24, dwords(HOUSE_MONEY_VTABLE))
    u.mem_write(HOUSE + 0x30C, dwords(case.get('balance', 5000)))
    u.mem_write(HOUSE + 0x2DC, dwords(0))
    u.mem_write(HOUSE + 0x24C, dwords(case.get('current_iq', 2)))
    u.mem_write(HOUSE + 0x1D0, dwords(2, 10))
    u.mem_write(HOUSE + 0x245, bytes([case.get('latched', False)]))
    u.mem_write(HOUSE + 0x280, dwords(*case.get('timer', [0, 0x5A5A5A5A, 0])))
    u.mem_write(HOUSE + 0x1C0, double_bits(case.get('delay', DELAY_02)))
    u.mem_write(HOUSE + 0x1ED, bytes([case.get('player_control', False)]))
    u.mem_write(PLAYER_PTR, dwords(HOUSE if case.get('player') else 0))
    # [IQ] RepairSell=/SellBack=, [AI] CreditReserve=, [AudioVisual]
    # ConditionYellow= (the fixture's .5) and ConditionRed=, the sounds.
    u.mem_write(RULES + 0x1444, dwords(1))
    u.mem_write(RULES + 0x145C, dwords(2))
    u.mem_write(RULES + 0x1758, dwords(case.get('credit_reserve', 100)))
    u.mem_write(RULES + 0x1708, struct.pack('<d', 0.25))
    u.mem_write(RULES + 0x700, dwords(SCOLD_SOUND))
    u.mem_write(RULES + 0x70C, dwords(GENERIC_CLICK))
    observe_toggle(u, read32, events)
    draws = []
    state = dict(in_update=False)

    def hook(_u, address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if address == RANDOM_RANGED:
            draws.append([signed(u, sp + 4), signed(u, sp + 8)])
        elif address in (TIMER_DRAW_RETURN, SALE_ROLL_RETURN):
            draws[-1].append(u.reg_read(UC_X86_REG_EAX))
        elif address == TOGGLE_REPAIR:
            events.append(['toggle_repair', signed(u, sp + 4)])
        elif address == CURRENT_FRAME and state['in_update']:
            ret(u, read32, 0, 0)
        elif address == CREATE_ANIM_FOR_SLOT:
            name = bytes(u.mem_read(read32(sp + 4), 16)).split(b'\0')[0].decode('ascii')
            events.append(['slot_anim', name, *[signed(u, sp + offset) for offset in (8, 12, 16, 20)]])
            ret(u, read32, 20)
        elif address == SELL_BACK:
            events.append(['sell_back', signed(u, sp + 4)])
            ret(u, read32, 4, 1)

    u.hook_add(UC_HOOK_CODE, hook)
    return draws, state


def run_update(u, state):
    state['in_update'] = True
    bc.invoke(u, UPDATE_REPAIR_AND_POWER, sm.YAREFN)
    state['in_update'] = False


def owner_state(u, read32):
    return dict(balance=signed(u, HOUSE + 0x30C), spent=signed(u, HOUSE + 0x2DC),
                latched=u.mem_read(HOUSE + 0x245, 1)[0],
                timer=[signed(u, HOUSE + 0x280), signed(u, HOUSE + 0x288)])


def random_indices(read32):
    return [read32(SCENARIO + 0x21C), read32(SCENARIO + 0x220)]


def update(case):
    """UpdateRepairAndPower (module doc) from its entry to its return."""
    u, _call, read32, events = fixture(case)
    draws, state = prepare_update(u, read32, events, case)
    before = random_indices(read32)
    run_update(u, state)
    return dict(input=case, draws=draws, events=events, **building_state(u, read32), **owner_state(u, read32),
                smoke_done=u.mem_read(SMOKE + 0xF8, 1)[0], redraw=u.mem_read(sm.YAREFN + 0x80, 1)[0],
                random_indices=dict(before=before, after=random_indices(read32)))


def update_cases():
    return [
        # A computer house's AIRepairable building below Strength: the latch,
        # ToggleRepair(1), the latch timer's draw, then the tick on a frame
        # the period (ftol(.016f * 900) = 14) divides.
        dict(name='u_start'),
        dict(name='u_start_seed_7', seed=7),
        dict(name='u_start_off_cadence', frame=200),
        dict(name='u_start_frame_zero', frame=0),
        # The latch holds any start; the repair tick still runs.
        dict(name='u_latched', latched=True),
        dict(name='u_latched_repairing', latched=True, repairing=True),
        # Already repairing: past the start to the tick (0x450821).
        dict(name='u_repairing', repairing=True),
        dict(name='u_repairing_off_cadence', repairing=True, frame=197),
        # Not AIRepairable: only a capture or a human's control starts it.
        dict(name='u_not_flagged', ai_repairable=False),
        dict(name='u_captured', ai_repairable=False, captured=True),
        dict(name='u_human', ai_repairable=False, human=True),
        dict(name='u_human_player', ai_repairable=False, human=True, player=True),
        dict(name='u_campaign', ai_repairable=False, game_mode=0),
        dict(name='u_campaign_flagged', game_mode=0),
        dict(name='u_campaign_player_control', ai_repairable=False, game_mode=0, player_control=True),
        # The admission: CurrentIQ, the mission, Can_Repair, money below the
        # reserve (the sale arm, not attacked here).
        dict(name='u_iq_below', current_iq=0),
        dict(name='u_iq_below_repairing', current_iq=0, repairing=True),
        dict(name='u_selling_repairing', mission='selling', repairing=True),
        dict(name='u_construction_repairing', mission='construction', repairing=True),
        # Construction or Selling, current or only queued (Get_Mission),
        # holds the start (0x450679 -> 0x450813).
        dict(name='u_construction_start', mission='construction'),
        dict(name='u_construction_queued_start', mission='none', queue='construction'),
        dict(name='u_selling_start', mission='selling'),
        dict(name='u_not_click_repairable', click_repairable=False),
        dict(name='u_below_reserve', balance=99),
        dict(name='u_below_reserve_repairing', balance=99, repairing=True),
        dict(name='u_below_reserve_attacked', balance=99, attacked=True, health=200),
        dict(name='u_at_reserve', balance=100),
        # Full Strength: no start; a repair already on spends one step and
        # clamps.
        dict(name='u_full', health=1000),
        dict(name='u_full_repairing', health=1000, repairing=True),
        # The step cost against the money: 100000 over 125 steps is 800,
        # times 15% is 119.
        dict(name='u_cannot_afford', cost=100000, balance=118),
        dict(name='u_affords_exactly', cost=100000, balance=119, credit_reserve=0),
        dict(name='u_affords_exactly_repairing', cost=100000, balance=119, repairing=True),
        # The clamp: past, at and just below Strength.
        dict(name='u_completes', health=995, repairing=True),
        dict(name='u_exact', health=992, repairing=True),
        dict(name='u_one_short', health=991, repairing=True),
        # The estimate (+0x70) takes the same step, and Strength at the clamp.
        dict(name='u_estimate_differs', estimate=250, repairing=True),
        dict(name='u_estimate_differs_completes', health=995, estimate=900, repairing=True),
        # The damage state at ConditionYellow (.5): crossing above replaces
        # the occupied named slots and retires the smoke; at yellow it stays.
        dict(name='u_crosses_yellow', health=496, repairing=True),
        dict(name='u_to_yellow', health=492, repairing=True),
        dict(name='u_above_yellow', health=600, repairing=True),
        dict(name='u_crosses_yellow_no_smoke', health=496, repairing=True, smoke=False),
        dict(name='u_stale_damaged_state', health=600, repairing=True, damaged=True),
        # RepairDelay: the difficulty rows' .02/.05 as ReadDouble stores
        # them, ReadDifficulty's default, and 0 (the House constructor's).
        dict(name='u_delay_05', delay=DELAY_05),
        dict(name='u_delay_default', delay=DELAY_02_DEFAULT),
        dict(name='u_delay_zero', delay=0),
        # A wrench byte already on flips back.
        dict(name='u_wrench_on', repairing=True, wrench=1),
        # The latch timer overwritten while an older one runs.
        dict(name='u_timer_overwritten', timer=[150, 0, 40]),
    ]


# --- build ---------------------------------------------------------------


def build(case):
    """A damaged building's build-up (the `build` rows): the update rows'
    building, owner and Rules at the row's start frame, created there on the
    row's route as building_construction's `route` rows create it, then per
    frame BuildingClass::Update's construction pieces (bc.building_update)
    followed by UpdateRepairAndPower, until two frames after Grand_Opening."""
    u, _call, read32, events = fixture(case)
    building, kind = sm.YAREFN, sm.YTYPE
    start = case['frame']
    draws, state = prepare_update(u, read32, events, dict(case, mission='none'))
    calls = []

    def hook(_u, address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if address in bc.PRESENTATION:
            ret(u, read32, bc.PRESENTATION[address])
        elif address in (bc.RADIO_BROADCAST, bc.RADIO_BROADCAST_ALL):
            calls.append(['radio', read32(sp + 4)])
            ret(u, read32, 4)
        elif address == bc.GRAND_OPENING:
            calls.append(['grand_opening', read32(sp + 4)])
            ret(u, read32, 4)
        elif address in (bc.LOOP_UPDATE, bc.SOUND_RELEASE):
            ret(u, read32, 0)
        elif address == bc.TECHNO_RECEIVE_RADIO:
            ret(u, read32, 12, 1)

    u.hook_add(UC_HOOK_CODE, hook)
    # The route rows' creation state (building_construction.building_fixture
    # and route): the control, no UndeploysInto, the TechnoClass
    # constructor's stage state, BState and queued BState -1, in play, +0x6E9.
    u.mem_write(kind + 0xF04, dwords(*case['control']))
    u.mem_write(building + 0xBC, dwords(0))
    u.mem_write(building + 0x218, dwords(0))
    u.mem_write(building + 0x534, dwords(-1))
    u.mem_write(building + 0x538, dwords(-1))
    u.mem_write(building + 0x6DD, bytes([0]))
    u.mem_write(building + 0xF8, dwords(0))
    u.mem_write(building + 0x100, dwords(start, 0, 0, 0, 1))
    u.mem_write(building + 0xC8, dwords(start, 0, 0))
    u.mem_write(building + 0x90, bytes([1]))
    u.mem_write(building + 0x6E9, bytes([1]))
    u.mem_write(bc.SCENARIO_INIT, dwords(0))
    u.mem_write(bc.SCENARIO_FLAG_ED6B, bytes([0]))
    route = case['route']
    bc.invoke(u, bc.ENTER_CONSTRUCTION, building, 1, 1)
    if route == 'computer':
        bc.invoke(u, bc.COMMENCE, building)
    elif route == 'player':
        bc.invoke(u, bc.RECEIVE_RADIO, building, sm.YAREFN + 0x1000, 3, 0)
    elif route == 'deploy':
        bc.invoke(u, bc.QUEUE_MISSION, building, 0x12, 0)
        u.mem_write(building + 0x6DD, bytes([1]))
    frames = []
    completed = None
    # A deployed or computer-placed building's first Update is in its
    # creation frame (building_construction's route rows).
    k = 0 if route in ('deploy', 'computer') else 1
    while completed is None or k <= completed + 2:
        assert k <= case['frames'], case['name']
        u.mem_write(FRAME, dwords(start + k))
        first_call = len(calls)
        bc.building_update(u, building)
        grand = any(call[0] == 'grand_opening' for call in calls[first_call:])
        if grand:
            completed = k
        # Get_Mission's two words as UpdateRepairAndPower reads them, and
        # what it calls and draws (the fixture's observers also see the
        # pieces' calls and the Guard mission's draws).
        mission = [signed(u, building + 0xAC), signed(u, building + 0xB4)]
        first_event, first_draw, before = len(events), len(draws), random_indices(read32)
        run_update(u, state)
        frames.append(dict(frame=start + k, mission=mission, grand_opening=grand, events=events[first_event:],
                           draws=draws[first_draw:], random_indices=dict(before=before, after=random_indices(read32)),
                           **building_state(u, read32), **owner_state(u, read32)))
        k += 1
    return dict(input=case, frames=frames)


def build_cases():
    return [
        # A computer's deployed yard: complete at D + 1 + (count - 1) * rate,
        # here 196, a repair-step frame (196 % 14 == 0).
        dict(name='b_deploy_3x2', route='deploy', control=[0, 3, 2], frame=191, frames=12),
        dict(name='b_deploy_1x0', route='deploy', control=[0, 1, 0], frame=191, frames=12),
        # A computer house's factory placement: complete at N +
        # (count - 1) * rate, off the step period.
        dict(name='b_computer_3x2', route='computer', control=[0, 3, 2], frame=190, frames=12),
        dict(name='b_computer_4x1', route='computer', control=[0, 4, 1], frame=190, frames=12),
        # A human player's placement (IsControlledByHuman starts it without
        # the timer; the local player hears it): complete at N + 2 +
        # (count - 1) * rate.
        dict(name='b_player_3x2', route='player', control=[0, 3, 2], frame=190, frames=12, human=True,
             ai_repairable=False, player=True),
    ]


# --- wrench --------------------------------------------------------------


def wrench(case):
    """TechnoClass::DrawExtras' repair wrench frame (0x6F52D8..0x6F532D): the
    cycle SpeedNormalize(14) / 4 (signed), at least 2, and WRENCH.SHP's frame
    ((Frame % cycle) * 6) / (cycle - 1), both signed IDIVs."""
    u, _call, _read32, _events = fixture(case)
    u.mem_write(GAME_SPEED, dwords(case['speed']))
    u.mem_write(FRAME, dwords(case['frame']))
    bc.run_block(u, sm.YAREFN, WRENCH_FRAME)
    return dict(input=case, frame_index=struct.unpack('<i', dwords(u.reg_read(UC_X86_REG_EAX)))[0])


def wrench_cases():
    return [dict(name=f'w_{speed}_{frame}', speed=speed, frame=frame)
            for speed in range(8)
            for frame in (0, 1, 2, 3, 4, 5, 6, 13, 14, 27, 28, 55, 196, 1000, 123457, 0x7FFFFFFF, -1, -30)]


# --- release -------------------------------------------------------------


def release(case):
    u, _call, read32, _events = fixture(case)
    u.mem_write(FRAME, dwords(case['frame']))
    u.mem_write(HOUSE + 0x245, bytes([case['latched']]))
    start, time_left = case['timer']
    u.mem_write(HOUSE + 0x280, dwords(start, 0, time_left))
    bc.run_block(u, HOUSE, HOUSE_RELEASE)
    return dict(input=case, latched=u.mem_read(HOUSE + 0x245, 1)[0])


def release_cases():
    rows = []
    for latched in (0, 1):
        for start, time_left, frame in ((100, 30, 129), (100, 30, 130), (100, 30, 131), (-1, 0, 50),
                                        (-1, 5, 50), (300, 30, 200), (200, 0, 200), (100, -5, 90),
                                        (0, 0, 0), (0x7FFFFFF0, 30, 16)):
            rows.append(dict(name=f'r_{latched}_{start}_{time_left}_{frame}', latched=latched,
                             timer=[start, time_left], frame=frame))
    return rows


# --- repair-depot service (additive corpus) ------------------------------

DEPOT_SERVICE, UNIT_REPAIR_STEP = 0x44B780, 0x747F20
UNIT_RECEIVE_RADIO, FOOT_RECEIVE_RADIO = 0x737430, 0x4D8FB0
DEPOT_CONTROLS = 0xA8E3A8
DEPOT_REPAIR_MISSION = 20
DEPOT_FOOT_INPUT_INITIALIZERS = {
    'tube_index': (0x4D31F1, 0x4D31FB),
    'saved_megamission': (0x4D32EC, 0x4D3308),
}
DEPOT_EVENT_SLICES = {
    'idle_event_calls': (0x4C75DA, 0x4C762A),
    'idle_event_foot_prefix': (0x4C757D, 0x4C762A),
    'idle_event_after_actor_admission': (0x4C7504, 0x4C762A),
    'deploy_event_calls': (0x4C77F0, 0x4C7818),
    'idle_tether_gate': (0x4C7504, 0x4C8109),
    'idle_mission_gate': (0x4C7504, 0x4C8109),
    'megamission_clear': (0x4C72E8, 0x4C7385),
}
DEPOT_GENERAL_READS = ((0x670DA3, 0x670DCA), (0x670DCA, 0x670DE9),
                       (0x670E30, 0x670E57))
DEPOT_SELECTED = {
    'General': {'RepairPercent', 'RepairStep', 'URepairRate'},
    'AudioVisual': {'ConditionYellow'},
    **{name: {'Strength', 'Cost', 'MovementZone', 'Harvester', 'Weeder', 'ManualReload'}
       for name in ('MTNK', 'HTNK')},
    **{name: {'Strength', 'Cost', 'UnitRepair', 'NumberOfDocks', 'HasStupidGuardMode', 'ManualReload'}
       for name in ('GADEPT', 'NADEPT')},
    **{name: {'Rate', 'AARate', 'NoThreat', 'Zombie', 'Recruitable',
              'Paralyzed', 'Retaliate', 'Scatter'}
       for name in ('Repair', 'Sleep', 'Move', 'Guard')},
}


def depot_inputs_root():
    return Path(os.environ.get('VERA20K_DEPOT_SERVICE_INPUTS',
                               'target/asset/depot-service/extract'))


class DepotInputReader:
    """Depot-specific reads over the shared Landing allocator/CRC/INI owner.

    Original full Rules, UnitType and BuildingType constructors execute. Only
    selected original read/store slices run; physical INI loading is supplied.
    """
    def __init__(self, *, waiters=False):
        from tools.rules_oracle.bridge_landing_inputs import Landing
        self.waiters = waiters
        self.selected = {name: set(keys) for name, keys in DEPOT_SELECTED.items()}
        if waiters:
            self.selected['General'].add('CloseEnough')
            self.selected['AI'] = {'PathDelay', 'BlockagePathDelay'}
            self.selected['Enter'] = set(self.selected['Move'])
            for name in ('MTNK', 'HTNK'):
                self.selected[name].update(('SpeedType', 'Crusher'))
            for name in ('GADEPT', 'NADEPT'):
                self.selected[name].add('NumberImpassableRows')
        self.m = Landing()
        m, u = self.m, self.m.u
        self.u = u
        for registry in (0x887568, 0xA8EB00, 0xA83CE0, 0xA8ED40, 0xA83C68):
            u.mem_write(registry, dwords(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
        self.rules = m.alloc(0x5000)
        m.invoke(0x665650, self.rules)
        m.invoke(0x4E7CF0, 0)
        self.units = {name: m.alloc(0xF00) for name in ('MTNK', 'HTNK')}
        for name, ptr in self.units.items():
            m.invoke(0x7470D0, ptr, [m.cstring(name)])
        self.btypes = {name: m.invoke(0x4653C0, m.cstring(name))
                       for name in ('GADEPT', 'NADEPT')}
        m.invoke(0x45C300, 0)
        self.constructor = self.snap()
        self.dock_array_constructor = {}
        for name, ptr in self.btypes.items():
            count, items = m.read32(ptr + 0x1780), m.read32(ptr + 0x1788)
            self.dock_array_constructor[name] = dict(
                array_bytes=bytes(u.mem_read(ptr + 0x1780, 0x12)).hex(),
                offset_bytes=bytes(u.mem_read(items, count * 12)).hex(),
                writes=[], original_constructor='0x45dd90')
        self.calls, self.writes, self.layers = [], [], []
        self.current = None
        self.watched = {self.rules + off: name for name, off in
                        (('repair_step', 0x16CC), ('repair_percent', 0x16D0),
                         ('unit_repair_rate', 0x16E8), ('full_health_threshold', 0x16F8),
                         ('condition_yellow', 0x1700))}
        for name, ptr in (self.units | self.btypes).items():
            for field, off in (('strength', 0xA0), ('cost', 0x610), ('manual_reload', 0xD24)):
                self.watched[ptr + off] = name + '.' + field
        if waiters:
            self.watched[self.rules + 0x1718] = 'close_enough'
            self.watched[self.rules + 0x1760] = 'path_delay'
            self.watched[self.rules + 0x1768] = 'blockage_path_delay'
            for name, ptr in self.units.items():
                self.watched[ptr + 0x67C] = name + '.speed_type'
                self.watched[ptr + 0xD28] = name + '.crusher'
            for name, ptr in self.btypes.items():
                self.watched[ptr + 0x1620] = name + '.number_impassable_rows'
        for name, ptr in self.units.items():
            for field, off in (('movement_zone', 0x5B4), ('harvester', 0xE0E), ('weeder', 0xE0F)):
                self.watched[ptr + off] = name + '.' + field
        for name, ptr in self.btypes.items():
            for field, off in (('unit_repair', 0x16A9), ('number_of_docks', 0x1780),
                               ('stupid_guard_mode', 0x16B5),
                               ('foundation', 0xEF0), ('outside_list', 0xED4)):
                self.watched[ptr + off] = name + '.' + field
        self.hooks = [u.hook_add(UC_HOOK_CODE, self.observe),
                      u.hook_add(UC_HOOK_MEM_WRITE, self.written)]

    def snap(self):
        u, m, rules = self.u, self.m, self.rules
        result = dict(repair_step=signed(u, rules + 0x16CC),
                      repair_percent_bits=bytes(u.mem_read(rules + 0x16D0, 8)).hex(),
                      unit_repair_rate_bits=bytes(u.mem_read(rules + 0x16E8, 8)).hex(),
                      full_health_threshold_bits=bytes(u.mem_read(rules + 0x16F8, 8)).hex(),
                      condition_yellow_bits=bytes(u.mem_read(rules + 0x1700, 8)).hex(),
                      x87_control_word=f'{u.reg_read(UC_X86_REG_FPCW):04x}',
                      units={name: dict(strength=signed(u, ptr + 0xA0),
                                        cost=signed(u, ptr + 0x610),
                                        movement_zone=signed(u, ptr + 0x5B4),
                                        manual_reload=u.mem_read(ptr + 0xD24, 1)[0],
                                        harvester=u.mem_read(ptr + 0xE0E, 1)[0],
                                        weeder=u.mem_read(ptr + 0xE0F, 1)[0])
                             for name, ptr in self.units.items()},
                      buildings={name: dict(strength=signed(u, ptr + 0xA0),
                                            cost=signed(u, ptr + 0x610),
                                            manual_reload=u.mem_read(ptr + 0xD24, 1)[0],
                                            unit_repair=u.mem_read(ptr + 0x16A9, 1)[0],
                                            stupid_guard_mode=u.mem_read(ptr + 0x16B5, 1)[0],
                                            number_of_docks=signed(u, ptr + 0x1780),
                                            foundation=m.read32(ptr + 0xEF0))
                                 for name, ptr in self.btypes.items()},
                      mission_controls={name: bytes(u.mem_read(DEPOT_CONTROLS + number * 32, 32)).hex()
                                        for name, number in
                                        (('repair', 20), ('sleep', 0), ('move', 2), ('guard', 5))})
        if self.waiters:
            result['close_enough'] = signed(u, rules + 0x1718)
            result['path_delay_bits'] = bytes(u.mem_read(rules + 0x1760, 8)).hex()
            result['blockage_path_delay'] = signed(u, rules + 0x1768)
            result['mission_controls']['enter'] = bytes(
                u.mem_read(DEPOT_CONTROLS + 7 * 32, 32)).hex()
            for name, ptr in self.units.items():
                result['units'][name].update(speed_type=signed(u, ptr + 0x67C),
                                            crusher=u.mem_read(ptr + 0xD28, 1)[0])
            for name, ptr in self.btypes.items():
                result['buildings'][name].update(
                    number_impassable_rows=signed(u, ptr + 0x1620),
                    constructor_byte_235=u.mem_read(ptr + 0x235, 1)[0])
        return result

    def observe(self, u, pc, size, data):
        if pc in (0x5276D0, 0x5283D0, 0x5295F0, 0x474DA0, 0x474E40, 0x528A10, 0x529CA0):
            sp, m = u.reg_read(UC_X86_REG_ESP), self.m
            count = {0x5283D0: 4, 0x528A10: 5, 0x529CA0: 4}.get(pc, 3)
            self.calls.append(dict(layer=self.current, pc=hex(pc), caller=hex(m.read32(sp)),
                                   args=[m.read32(sp + n * 4) for n in range(1, count + 1)]))

    def written(self, u, access, address, size, value, data):
        if address in self.watched or DEPOT_CONTROLS <= address < DEPOT_CONTROLS + 32 * 32:
            self.writes.append(dict(layer=self.current, pc=hex(u.reg_read(UC_X86_REG_EIP)),
                                    field=self.watched.get(address, 'mission_control'),
                                    size=size, value=value))

    def block(self, begin, end, registers, required=()):
        from tools.spatial_oracle.building_body_rules import SP as READER_SP
        self.u.reg_write(UC_X86_REG_ESP, READER_SP)
        for register, value in registers:
            self.u.reg_write(register, value)
        run_checked(self.u, begin, end, required_addresses=required)

    def read_layer(self, filename, raw, sections, lines):
        from tools.spatial_oracle.building_body_rules import INI, SP as READER_SP
        m, u, rules = self.m, self.u, self.rules
        self.current = filename
        m.make_ini(sections)
        before, first_call, first_write = self.snap(), len(self.calls), len(self.writes)
        for name, ptr in (self.units | self.btypes).items():
            if not m.invoke(0x526810, INI, [m.cstring(name)]):
                continue
            self.block(0x5F94D3, 0x5F94F3,
                       ((UC_X86_REG_EBX, ptr), (UC_X86_REG_ESI, INI), (UC_X86_REG_EBP, ptr + 0x24)),
                       (0x5276D0,))
            techno_registers = ((UC_X86_REG_EBP, ptr), (UC_X86_REG_ESI, INI),
                                (UC_X86_REG_EDI, INI), (UC_X86_REG_EBX, ptr + 0x24))
            # Unit74763F calls Techno712170 before its own flags/postpass.
            # Preserve the original selected order in the additive mode;
            # legacy corpora retain their historical independent slices.
            if self.waiters and name in self.units:
                self.block(0x7121D1, 0x7121EB, techno_registers)
            cost = (0x71469F, 0x7146B9, (0x5276D0,))
            manual_reload = (0x713343, 0x71336C, (0x5295F0, 0x713366))
            for begin, end, required in ((manual_reload, cost) if self.waiters
                                         else (cost, manual_reload)):
                self.block(begin, end, techno_registers, required)
            if name in self.units:
                if self.waiters:
                    self.block(0x714CC8, 0x714CE9, techno_registers)
                # The enclosing TechnoType reader holds INI at this local;
                # this is the existing harvest reader's MovementZone slice.
                u.mem_write(READER_SP + 0x380, dwords(INI))
                movement_zone = (0x71605E, 0x716090,
                                 ((UC_X86_REG_EBP, ptr), (UC_X86_REG_EBX, ptr + 0x24)),
                                 (0x474E40, 0x716081))
                unit_flags = (0x74769F, 0x7476D3,
                              ((UC_X86_REG_EDI, ptr), (UC_X86_REG_EBX, INI),
                               (UC_X86_REG_EBP, ptr + 0x24)), (0x7476AE, 0x7476C8))
                speed_postpass = (0x7476D3, 0x747711,
                                  ((UC_X86_REG_EDI, ptr), (UC_X86_REG_EBX, INI),
                                   (UC_X86_REG_EBP, ptr + 0x24)), ())
                for args in ((movement_zone, unit_flags, speed_postpass) if self.waiters
                             else (unit_flags, movement_zone)):
                    self.block(*args)
            if name in self.btypes:
                if self.waiters:
                    self.block(0x460133, 0x46014D,
                               ((UC_X86_REG_EBP, ptr), (UC_X86_REG_ESI, m.cstring(name)),
                                (UC_X86_REG_EDI, INI)), (0x5276D0, 0x460147))
                self.block(0x460906, 0x46092F,
                           ((UC_X86_REG_EBP, ptr), (UC_X86_REG_ESI, INI), (UC_X86_REG_EBX, ptr + 0x24)),
                           (0x5295F0,))
                self.block(0x460EA0, 0x460EBA,
                           ((UC_X86_REG_EBP, ptr), (UC_X86_REG_ESI, INI), (UC_X86_REG_EBX, ptr + 0x24)),
                           (0x5295F0, 0x460EB4))
                self.block(0x46492E, 0x46494B,
                           ((UC_X86_REG_EBP, ptr), (UC_X86_REG_ESI, INI), (UC_X86_REG_EBX, ptr + 0x24)),
                           (0x5276D0,))
        if m.invoke(0x526810, INI, [m.cstring('General')]):
            for begin, end in DEPOT_GENERAL_READS:
                self.block(begin, end, ((UC_X86_REG_ESI, rules), (UC_X86_REG_EDI, INI)))
        if self.waiters:
            self.block(0x670EDD, 0x670EFD,
                       ((UC_X86_REG_ESI, rules), (UC_X86_REG_EDI, INI)),
                       (0x474620, 0x670EF7))
            # Existing path_delay_rules owner identifies these original
            # [AI] read/store slices; execute them on each physical layer.
            self.block(0x6739E5, 0x673A37,
                       ((UC_X86_REG_ESI, rules), (UC_X86_REG_EDI, INI)),
                       (0x5283D0, 0x5276D0))
        # ReadAudioVisual's unconditional 1.0 assignment is included before
        # the selected ConditionYellow read; it is never an INI default.
        if m.invoke(0x526810, INI, [m.cstring('AudioVisual')]):
            self.block(0x66B323, 0x66B337, ((UC_X86_REG_ESI, rules), (UC_X86_REG_EDI, INI)))
            self.block(0x66B35E, 0x66B385, ((UC_X86_REG_ESI, rules), (UC_X86_REG_EDI, INI)))
        self.block(0x679C92, 0x679CAF, ((UC_X86_REG_ESI, INI),), (0x5B3760,))
        self.layers.append(dict(file=filename, sha256=hashlib.sha256(raw).hexdigest(), bytes=len(raw),
                                selected={s: {k: v for k, v in keys.items() if k in self.selected[s]}
                                          for s, keys in sections.items()},
                                lines=[line for line in lines if line['key'] in self.selected[line['section']]],
                                before=before, after=self.snap(), calls=self.calls[first_call:],
                                writes=self.writes[first_write:]))

    def read_art(self, path):
        from tools.projectile_oracle.bridge_render_inputs import lexical
        from tools.spatial_oracle.building_body_rules import SP as READER_SP
        raw = path.read_bytes()
        sections, lines = lexical(raw, set(self.btypes))
        m, u = self.m, self.u
        m.make_ini(sections)
        self.current = 'ARTMD.INI'
        first_call, first_write, before = len(self.calls), len(self.writes), self.snap()
        offsets = {}
        for name, ptr in self.btypes.items():
            u.mem_write(ptr + 0x1F8, name.encode('ascii') + b'\0')
            self.block(0x461225, 0x46125D,
                       ((UC_X86_REG_EBP, ptr), (UC_X86_REG_EDI, ptr + 0x1F8),
                        (UC_X86_REG_EBX, ptr + 0x24)), (0x474DA0, 0x528A10))
            self.block(0x461547, 0x461570,
                       ((UC_X86_REG_EBP, ptr), (UC_X86_REG_EDI, ptr + 0x1F8)),
                       (0x46156A,))
            u.reg_write(UC_X86_REG_ESP, READER_SP)
            offsets[name] = rd.DockInputReader.read_dock_offsets(self, name)
        return dict(file='ARTMD.INI', sha256=hashlib.sha256(raw).hexdigest(), bytes=len(raw),
                    selected={name: {key: value for key, value in keys.items()
                                     if key in ('Foundation', 'DockingOffset0')}
                              for name, keys in sections.items()},
                    lines=[line for line in lines if line['key'] in ('Foundation', 'DockingOffset0')],
                    supplied_effective_images=list(self.btypes),
                    before=before, after=self.snap(), docking_offsets=offsets,
                    outside_lists={name: bytes(u.mem_read(0x89D368 + m.read32(ptr + 0xEF0) * 0x78, 0x78)).hex()
                                   for name, ptr in self.btypes.items()},
                    calls=self.calls[first_call:], writes=self.writes[first_write:])

    def close(self):
        for hook in self.hooks:
            self.u.hook_del(hook)


def depot_input_receipts(*, waiters=False):
    from tools.projectile_oracle.bridge_render_inputs import lexical
    root, reader = depot_inputs_root(), DepotInputReader(waiters=waiters)
    try:
        for filename in ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'XMP03T4.MAP'):
            path = root / filename
            if not path.is_file():
                assert filename == 'LANGRULE.INI', str(path)
                reader.layers.append(dict(file=filename, absent=True))
                continue
            raw = path.read_bytes()
            sections, lines = lexical(raw, set(reader.selected))
            reader.read_layer(filename, raw, sections, lines)
        art = reader.read_art(root / 'ARTMD.INI' if (root / 'ARTMD.INI').is_file()
                              else Path('ini/ARTMD.INI'))
        return dict(constructor=reader.constructor, layers=reader.layers,
                    art=art, after=reader.snap())
    finally:
        reader.close()


class DepotService:
    """One original-byte VM for paid repair and its retained mission state.

    Arrival, in-play objects and their contact/House membership are supplied.
    The stock reader's bytes initialize type/rules fields; radio, money,
    dispatcher, Stage update, power and departure setters execute originally.
    """
    def __init__(self, case, inputs):
        self.case, self.inputs = case, inputs
        self.u, self.call, self.read32 = rd.make_dock_fixture(dict(
            case, harvester=False, mission='guard', linked=case.get('linked', True),
            moving=False,
            nav='refinery' if case.get('nav') else None, frame=case.get('frame', 200)))
        u, r, stock = self.u, self.read32, inputs['after']
        unit_name, building_name = case.get('unit', 'HTNK'), case.get('building', 'NADEPT')
        unit, building = stock['units'][unit_name], stock['buildings'][building_name]
        self.unit_name, self.building_name = unit_name, building_name
        # The inherited source map supplies width 16. Its unused height was
        # zero; native IsUsableArea needs both for the original exit search.
        # Supply the matching 16x16 playable geometry over its 32x32 cell grid.
        u.mem_write(0x87F7E8 + 0xF8, dwords(case.get('map_height', 16)))
        strength = case.get('strength', unit['strength'])
        health = case.get('health', strength // 2)
        for pointer, value in ((rd.TYPE + 0x610, case.get('cost', unit['cost'])),
                               (rd.TYPE + 0xA0, strength),
                               (rd.TYPE + 0x5B4, unit['movement_zone']),
                               (rd.ACTOR + 0x6C, health), (rd.ACTOR + 0x70, case.get('estimate', health)),
                               (rd.ACTOR + 0xAC, case.get('unit_mission', 0)),
                               (rd.ACTOR + 0xB0, -1), (rd.ACTOR + 0xB4, -1),
                               (rd.ACTOR + 0x5A0, rd.OTHER if case.get('nav_aux') else 0),
                               (rd.ACTOR + 0x5A4, rd.BLD if case.get('nav') else 0),
                               (HOUSE + 0x24, HOUSE_MONEY_VTABLE),
                               (HOUSE + 0x30C, case.get('balance', 1000)),
                               (HOUSE + 0x2DC, case.get('spent', 0)),
                               (rd.BTYPE, 0x7E4570), (rd.BTYPE + 0xA0, building['strength']),
                               (rd.BTYPE + 0x610, building['cost']),
                               (rd.BTYPE + 0xEF0, building['foundation']),
                               (rd.BTYPE + 0x1780, building['number_of_docks']),
                               (rd.BTYPE + 0x1788, rd.EXTRA + 0x2D000), (rd.BTYPE + 0x178C, 1),
                               (rd.BLD + 0x6C, building['strength']), (rd.BLD + 0x70, building['strength']),
                               (rd.BLD + 0xAC, case.get('building_mission', DEPOT_REPAIR_MISSION)),
                               (rd.BLD + 0xB0, -1), (rd.BLD + 0xB4, -1),
                               (rd.BLD + 0xBC, case.get('status', 1)),
                               (rd.BLD + 0xC8, -1), (rd.BLD + 0xD0, 0),
                               (RULES + 0x16CC, case.get('step', stock['repair_step']))):
            u.mem_write(pointer, dwords(value))
        for pointer, value in ((rd.BTYPE + 0x16A9, building['unit_repair']),
                               (rd.BTYPE + 0x16B5, case.get('stupid_guard_mode', building['stupid_guard_mode'])),
                               (rd.TYPE + 0xD24, case.get('manual_reload', unit['manual_reload'])),
                               (rd.BTYPE + 0x16B3, 0), (rd.BTYPE + 0x16BB, 0),
                               (rd.ACTOR + 0x90, 1), (rd.BLD + 0x90, 1),
                               (rd.ACTOR + 0x418, 1), (rd.BLD + 0x418, 1),
                               (rd.LOCO + 0x10, case.get('powered', False))):
            u.mem_write(pointer, bytes([value]))
        if 'ready' in case:
            u.mem_write(rd.BLD + 0x6DD, bytes([case['ready']]))
        if 'speed_type' in unit:
            u.mem_write(rd.TYPE + 0x67C, dwords(unit['speed_type']))
            u.mem_write(rd.TYPE + 0xD28, bytes([unit['crusher']]))
            u.mem_write(rd.BTYPE + 0x1620, dwords(building['number_impassable_rows']))
            u.mem_write(rd.BTYPE + 0x235, bytes([building['constructor_byte_235']]))
            u.mem_write(RULES + 0x1718, dwords(stock['close_enough']))
            u.mem_write(RULES + 0x1760, bytes.fromhex(stock['path_delay_bits']))
            u.mem_write(RULES + 0x1768, dwords(stock['blockage_path_delay']))
        u.mem_write(RULES + 0x16D0, bytes.fromhex(case.get('percent_bits', stock['repair_percent_bits'])))
        u.mem_write(RULES + 0x16E8, bytes.fromhex(case.get('unit_repair_rate_bits', stock['unit_repair_rate_bits'])))
        u.mem_write(RULES + 0x16F8, bytes.fromhex(stock['full_health_threshold_bits']))
        u.mem_write(RULES + 0x1700, bytes.fromhex(stock['condition_yellow_bits']))
        for name, number in (('repair', 20), ('sleep', 0), ('move', 2), ('guard', 5)):
            u.mem_write(DEPOT_CONTROLS + number * 32, bytes.fromhex(stock['mission_controls'][name]))
        if 'enter' in stock['mission_controls']:
            u.mem_write(DEPOT_CONTROLS + 7 * 32, bytes.fromhex(stock['mission_controls']['enter']))
        if 'mission_rate_bits' in case:
            u.mem_write(DEPOT_CONTROLS + DEPOT_REPAIR_MISSION * 32 + 0x10,
                        bytes.fromhex(case['mission_rate_bits']))
        u.mem_write(rd.EXTRA + 0x2D000,
                    bytes.fromhex(inputs['art']['docking_offsets'][building_name]['offset_bytes']))
        # Transfer the actual original startup initializer's outside-list row;
        # execute the original ART post-read pointer assignment in this VM.
        u.mem_write(0x89D368 + building['foundation'] * 0x78,
                    bytes.fromhex(inputs['art']['outside_lists'][building_name]))
        from tools.spatial_oracle.unit_scatter_state import SP
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_EBP, rd.BTYPE)
        u.reg_write(UC_X86_REG_EDI, rd.BTYPE + 0x1F8)
        run_checked(u, 0x461547, 0x461570, required_addresses=(0x46156A,))
        # Original Building constructor's independent StageClass at +0x620.
        u.reg_write(UC_X86_REG_ESI, rd.BLD)
        u.reg_write(UC_X86_REG_EBX, 0)
        run_checked(u, 0x43B7F5, 0x43B823, required_addresses=(0x43B819,))
        if 'stage' in case:
            progress, changed, start, duration, rate, step = case['stage']
            u.mem_write(rd.BLD + 0x620, dwords(progress))
            u.mem_write(rd.BLD + 0x624, bytes([changed]))
            u.mem_write(rd.BLD + 0x628, dwords(start, 0, duration, rate, step))
        if 'rally' in case:
            u.mem_write(rd.BLD + 0x218, dwords(rd.cell(*case['rally'])))
        u.mem_write(0xA8E7AC, dwords(0))
        self.spatial_startup = depot_spatial_startup(self) if case.get('spatial_startup') else None
        # GetDockCoords is native. The independent physical arrival at this
        # coordinate is supplied; no Drive::Process/path history is claimed.
        output = rd.EXTRA + 0x2D100
        center = bc.invoke(u, r(0x7E3EBC + 0x48), rd.BLD, output)
        self.center_coordinate = coord(u, center)
        coord_pointer = bc.invoke(u, r(0x7E3EBC + 0x4C), rd.BLD, output, rd.ACTOR)
        self.dock_coordinate = coord(u, coord_pointer)
        if not case.get('off_pad'):
            u.mem_write(rd.ACTOR + 0x9C, dwords(*self.dock_coordinate))
        if case.get('arrival') == 'center':
            u.mem_write(rd.ACTOR + 0x9C, dwords(*self.center_coordinate))
        if 'delta' in case:
            at = coord(u, rd.ACTOR + 0x9C)
            u.mem_write(rd.ACTOR + 0x9C, dwords(*[v + dv for v, dv in zip(at, case['delta'])]))
        if case.get('moving'):
            at = coord(u, rd.ACTOR + 0x9C)
            u.mem_write(rd.LOCO + 0x34, dwords(at[0] + 256, at[1], at[2]))
        self.events, self.writes, self.pending, self.steps = [], [], {}, []
        self.original_text = bytes(u.mem_read(0x401000, 0x3E0000))
        self.original_vtables = [bytes(u.mem_read(address, 0x600))
                                 for address in (0x7F5C70, 0x7F6218, 0x7E3EBC, 0x7E4570)]
        self.hooks = [u.hook_add(UC_HOOK_CODE, self.observe),
                      u.hook_add(UC_HOOK_MEM_WRITE, self.written)]
        if self.spatial_startup is not None:
            self.hooks.append(self.spatial_startup_hook)
        self.before = self.state()

    def state(self):
        from tools.spatial_oracle.shrapnel_repair.shrapnel_repair import rng_state
        u, r = self.u, self.read32
        def target(pointer):
            return rd.name_of(pointer) if pointer in (0, rd.ACTOR, rd.BLD, rd.OTHER) else rd.cell_xy(pointer)
        return dict(frame=signed(u, FRAME), health=signed(u, rd.ACTOR + 0x6C),
                    unit_coordinate=coord(u, rd.ACTOR + 0x9C),
                    estimate=signed(u, rd.ACTOR + 0x70), balance=signed(u, HOUSE + 0x30C),
                    spent=signed(u, HOUSE + 0x2DC), unit_mission=signed(u, rd.ACTOR + 0xAC),
                    unit_queued=signed(u, rd.ACTOR + 0xB4), unit_nav=target(r(rd.ACTOR + 0x5A4)),
                    unit_nav_aux=target(r(rd.ACTOR + 0x5A0)),
                    unit_archive=target(r(rd.ACTOR + 0x218)), pending_entry=target(r(rd.ACTOR + 0x500)),
                    unit_contacts=[target(r(r(rd.ACTOR + 0xE4) + n * 4)) for n in range(r(rd.ACTOR + 0xE8))],
                    building_contacts=[target(r(r(rd.BLD + 0xE4) + n * 4)) for n in range(r(rd.BLD + 0xE8))],
                    unit_tether=u.mem_read(rd.ACTOR + 0x418, 1)[0],
                    building_tether=u.mem_read(rd.BLD + 0x418, 1)[0],
                    building_mission=signed(u, rd.BLD + 0xAC),
                    building_queued=signed(u, rd.BLD + 0xB4), status=signed(u, rd.BLD + 0xBC),
                    mission_visits=signed(u, rd.BLD + 0xC4),
                    dispatch_words=[signed(u, rd.BLD + off) for off in (0xC8, 0xD0)],
                    stage=[signed(u, rd.BLD + 0x620), u.mem_read(rd.BLD + 0x624, 1)[0],
                           *[signed(u, rd.BLD + off) for off in (0x628, 0x630, 0x634, 0x638)]],
                    repairing=u.mem_read(rd.BLD + 0x6DD, 1)[0],
                    locomotor_powered=u.mem_read(rd.LOCO + 0x10, 1)[0],
                    locomotor_destination=coord(u, rd.LOCO + 0x34),
                    scenario_rng=rng_state(u, SCENARIO + 0x218))

    def observe(self, u, pc, size, data):
        for event in self.pending.pop(pc, []):
            event['returned_eax'] = u.reg_read(UC_X86_REG_EAX)
            if event['kind'] == 'exit_cell':
                event['returned_cell'] = list(struct.unpack('<hh', bytes(u.mem_read(event['args'][0], 4))))
        if u.reg_read(UC_X86_REG_EIP) != pc:
            return
        sp, this = u.reg_read(UC_X86_REG_ESP), u.reg_read(UC_X86_REG_ECX)
        destination_calls = getattr(self, 'destination_calls', {})
        if pc in destination_calls:
            event = dict(kind=destination_calls[pc], pc=hex(pc))
            if pc == 0x742F48:
                # Interior label, not a function entry or a new caller.
                event['actor'] = rd.name_of(u.reg_read(UC_X86_REG_EBP))
            else:
                event['caller'] = hex(self.read32(sp))
                event['this'] = (hex(self.read32(sp + 4))
                                 if pc in self.destination_power_slots else rd.name_of(this))
            if pc in self.destination_power_slots:
                event['power_before'] = u.mem_read(rd.LOCO + 0x10, 1)[0]
            self.events.append(event)
        sinks = {rd.DESTROY_ANIM: 4, CREATE_ANIM_FOR_SLOT: 20,
                 PLAY_EVA: 4, rd.PLAY_ANIM: 4} | bc.PRESENTATION
        if pc in sinks:
            self.events.append(dict(kind='presentation_sink', pc=hex(pc),
                                    args=[self.read32(sp + n * 4) for n in range(1, sinks[pc] // 4 + 1)]))
            ret(u, self.read32, sinks[pc])
            return
        calls = {DEPOT_SERVICE: ('depot_service', 0), 0x4496B0: ('depot_guard', 0),
                 bc.MISSION_AI: ('mission_dispatch', 0), bc.COMMENCE: ('commence', 0),
                 bc.RECEIVE_RADIO: ('building_receive_radio', 3), 0x454250: ('building_ready', 0),
                 UNIT_RECEIVE_RADIO: ('unit_receive_radio', 3),
                 FOOT_RECEIVE_RADIO: ('foot_receive_radio', 3),
                 bc.TECHNO_RECEIVE_RADIO: ('techno_receive_radio', 3),
                 REPAIR_STEP_COST: ('repair_step_cost', 0), UNIT_REPAIR_STEP: ('repair_step', 0),
                 0x711EB0: ('unit_base_cost', 0), 0x4F9790: ('spend_money', 1),
                 0x4F9950: ('add_credits', 1),
                 rd.TRANSMIT: ('radio_transmit', 3), 0x65ACB0: ('radio_contact', 1),
                 0x65C7E0: ('random_range', 2), 0x65C780: ('random_raw', 0),
                 0x65C6D0: ('scenario_seed', 1),
                 rd.QUEUE: ('queue_mission', 2), rd.ASSIGN: ('assign_destination', 2),
                 0x70C610: ('archive_setter', 1), 0x44EFB0: ('exit_cell', 3)}
        if pc in calls:
            kind, count = calls[pc]
            event = dict(kind=kind, pc=hex(pc), this=rd.name_of(this),
                         args=[self.read32(sp + n * 4) for n in range(1, count + 1)])
            self.events.append(event)
            self.pending.setdefault(self.read32(sp), []).append(event)

    def written(self, u, access, address, size, value, data):
        fields = {rd.ACTOR + 0x6C: 'health', rd.ACTOR + 0x70: 'estimate',
                  HOUSE + 0x30C: 'balance', HOUSE + 0x2DC: 'spent',
                  rd.BLD + 0xBC: 'status', rd.BLD + 0xC8: 'dispatch_start', rd.BLD + 0xD0: 'dispatch_delay',
                  rd.BLD + 0x620: 'stage_progress', rd.BLD + 0x624: 'stage_changed',
                  rd.BLD + 0x628: 'stage_start', rd.BLD + 0x630: 'stage_duration',
                  rd.BLD + 0x634: 'stage_rate', rd.BLD + 0x638: 'stage_step'}
        if address in fields:
            self.writes.append(dict(pc=hex(u.reg_read(UC_X86_REG_EIP)),
                                    field=fields[address], size=size, value=value))

    def invoke(self, entry, this, *args):
        before, first_event, first_write = self.state(), len(self.events), len(self.writes)
        value = bc.invoke(self.u, entry, this, *args)
        # The outer function returns directly to the fixture sentinel, which
        # run_checked stops before; resolve its measured return here.
        for event in self.pending.pop(RET_MAGIC, []):
            event['returned_eax'] = value
        row = dict(entry=hex(entry), before=before, after=self.state(), returned_eax=value,
                   events=self.events[first_event:], writes=self.writes[first_write:])
        self.steps.append(row)
        return row

    def block(self, bounds, *, esi=rd.BLD, instruction_count=10_000):
        before, first_event, first_write = self.state(), len(self.events), len(self.writes)
        bc.run_block(self.u, esi, bounds, instruction_count=instruction_count)
        row = dict(entry=hex(bounds[0]), end_exclusive=hex(bounds[1]), before=before,
                   after=self.state(), events=self.events[first_event:], writes=self.writes[first_write:])
        self.steps.append(row)
        return row

    def finish(self):
        assert bytes(self.u.mem_read(0x401000, 0x3E0000)) == self.original_text
        assert [bytes(self.u.mem_read(address, 0x600))
                for address in (0x7F5C70, 0x7F6218, 0x7E3EBC, 0x7E4570)] == self.original_vtables
        result = dict(input=self.case, unit=self.unit_name, building=self.building_name,
                    dock_coordinate=self.dock_coordinate, center_coordinate=self.center_coordinate,
                    before=self.before, steps=self.steps,
                    original_code_and_vtables_unchanged=True,
                    native_text_sha256=hashlib.sha256(self.original_text).hexdigest(),
                    ambient_x87_control_word=f'{self.u.reg_read(UC_X86_REG_FPCW):04x}')
        for hook in self.hooks:
            self.u.hook_del(hook)
        self.hooks = []
        return result


def depot_spatial_startup(vm):
    """Original spatial CRT before the first retained Cell height getter.

    The exit-registration seam is shared with the Engineer joined fixture;
    it only excludes process-exit callback registration, never a map result.
    Keep it through Mark, whose lazy globals can register exit callbacks.
    """
    from types import SimpleNamespace
    from tools.spatial_oracle.mapgen_range import Machine
    u, r = vm.u, vm.read32
    receipt = dict(exit_registration_calls=[])

    def registration(_u, pc, size, data):
        if pc == 0x7C978A:
            receipt['exit_registration_calls'].append(dict(
                pc=hex(pc), callback=hex(r(u.reg_read(UC_X86_REG_ESP) + 4))))
            ret(u, r, 0)

    vm.spatial_startup_hook = u.hook_add(UC_HOOK_CODE, registration)
    receipt['x87'] = Machine.startup(SimpleNamespace(u=u))
    receipt['initializer_tables'] = []
    for table, count in ((0x8141D8, 14), (0x8129FC, 13)):
        entries = struct.unpack(f'<{count}I', u.mem_read(table, count * 4))
        for entry in entries:
            bc.invoke(u, entry, 0)
        receipt['initializer_tables'].append(dict(table=hex(table), entries=list(map(hex, entries))))
    for entry in (0x561710, 0x5617A0, 0x5617C0, 0x5617E0):
        bc.invoke(u, entry, 0)
    receipt['map_initializers'] = ['0x561710', '0x5617A0', '0x5617C0', '0x5617E0']
    # Shared track_fresh_response startup; leave candidate direction deltas
    # to the original initializer instead of a supplied lookup table.
    receipt['direction_initializer'] = dict(entry='0x49f3a0',
                                           returned_eax=bc.invoke(u, 0x49F3A0, 0))
    receipt['object_height_globals'] = {hex(a): r(a) for a in (0xAC13C8, 0xAC13BC)}
    receipt['map_ground_height'] = r(0xABDE88)
    receipt['before_first_height_getter'] = dict(cache_gate_89e770=r(0x89E770))
    return receipt


def depot_waiter_case(inputs, level):
    """Produced Mark -> parking/entry/retry boundaries, excluding travel.

    Object/House admission and flat Cell/zone priors are declared. Mark,
    Nearby, class entry and radio/destination bodies run unchanged.
    """
    from types import SimpleNamespace
    from tools.spatial_oracle.anytown_damage.navigation import Navigation, MAP
    from tools.spatial_oracle.unit_scatter_state import SP
    case = dict(name=f'marked_gadept_level{level}', unit='MTNK', building='GADEPT',
                linked=False, off_pad=True, miner_cell=[16, 16], health=40,
                spatial_startup=True, frame=200, terrain_level=level,
                map_local_size=[-16, -16, 64, 64], unit_in_playfield=True)
    vm = DepotService(case, inputs)
    u, r = vm.u, vm.read32
    for stream in (0x886B88, 0xABE890):
        bc.invoke(u, 0x65C6D0, stream, 1)
    u.mem_write(MAP + 0xEC, dwords(0, 0, 16, 16))
    u.mem_write(MAP + 0xFC, dwords(*case['map_local_size']))
    # This corpus starts after object admission, like the Rust comparison's
    # production Unlimbo. Bind consequential membership explicitly.
    u.mem_write(rd.ACTOR + 0x3D5, bytes([case['unit_in_playfield']]))
    plane = bytearray(b'\x07\x00\x00\x00' * (33 * 33))
    for y in range(1, 32):
        for x in range(1, 32):
            struct.pack_into('<BBH', plane, (y * 33 + x) * 4, 0, level, 0)
    Navigation.on_existing_map(SimpleNamespace(u=u, allocate=None),
                               size=(16, 16), class_height_plane=bytes(plane))
    for y in range(32):
        for x in range(32):
            bc.invoke(u, 0x47BBF0, rd.cell(x, y))
            u.mem_write(rd.cell(x, y) + 0x24, struct.pack('<2h', x, y))
            u.mem_write(rd.cell(x, y) + 0x11B, bytes([level]))
    bc.invoke(u, 0x45B1C0, 0)
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_EBP, rd.BTYPE)
    u.reg_write(UC_X86_REG_EDI, rd.BTYPE + 0x1F8)
    run_checked(u, 0x46152C, 0x461570, required_addresses=(0x461541, 0x46156A))
    u.reg_write(UC_X86_REG_ESI, rd.BLD)
    u.reg_write(UC_X86_REG_EBX, 0)
    run_checked(u, 0x43B762, 0x43B783, required_addresses=(0x43B768, 0x43B777, 0x43B77D))
    u.mem_write(SP, dwords(RET_MAGIC, 1))
    u.reg_write(UC_X86_REG_ECX, rd.BLD)
    u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(u, 0x447780, 0x4477C7, required_addresses=(0x4477C1,))
    u.mem_write(rd.ACTOR + 0xA4, dwords(level * r(0xAC13C8)))
    u.mem_write(rd.BLD + 0xA4, dwords(level * r(0xAC13C8)))
    u.mem_write(0x887324, dwords(rd.EXTRA + 0x5000))
    u.mem_write(0xA8E9A0, b'\x01')
    u.mem_write(rd.ACTOR + 0x418, b'\0')
    u.mem_write(rd.BLD + 0x418, b'\0')

    def rngs():
        return {name: bytes(u.mem_read(pointer, 0x3F4)).hex()
                for name, pointer in (('main', 0x886B88), ('scenario', SCENARIO + 0x218),
                                      ('mapgen', 0xABE890))}

    def foundation():
        return [dict(cell=[x, y], head=rd.name_of(r(rd.cell(x, y) + 0xE4)),
                     ground=r(rd.cell(x, y) + 0x124), deck=r(rd.cell(x, y) + 0x128),
                     level=u.mem_read(rd.cell(x, y) + 0x11B, 1)[0])
                for y in range(rd.NW[1], rd.NW[1] + 3)
                for x in range(rd.NW[0], rd.NW[0] + 3)]

    def foot_state():
        return dict(movement_timer=[signed(u, rd.ACTOR + off) for off in (0x640, 0x648)],
                    blocked_timer=[signed(u, rd.ACTOR + off) for off in (0x668, 0x670)],
                    retries_left=r(rd.ACTOR + 0x64C),
                    path_blocked=bool(u.mem_read(rd.ACTOR + 0x6B7, 1)[0]),
                    nav_queue_count=r(rd.ACTOR + 0x598),
                    path=[signed(u, rd.ACTOR + 0x5E0 + i * 4) for i in range(24)],
                    head=coord(u, rd.LOCO + 0x40), selector=signed(u, rd.LOCO + 0x58),
                    valid=u.mem_read(rd.LOCO + 0x63, 1)[0],
                    in_playfield=bool(u.mem_read(rd.ACTOR + 0x3D5, 1)[0]),
                    mission_only=bool(u.mem_read(rd.ACTOR + 0x3D4, 1)[0]))

    vm.steps, vm.events, vm.writes, vm.pending = [], [], [], {}
    mark_before, mark_rng = foundation(), rngs()
    marked = vm.invoke(0x43F180, rd.BLD, 3)
    assert marked['returned_eax'] & 255 == 1
    mark_after = foundation()
    assert all(cell['head'] == 'refinery' and cell['ground'] == 0x80
               and cell['deck'] == 0 for cell in mark_after)
    assert mark_rng == rngs()
    # Same explicit uniform sector/zone prior as DockContinuation; Mark/Recalc
    # produced the class/height plane. Full connectivity is not executed.
    zone = rd.EXTRA + 0x2D800
    u.mem_write(zone, bytes(2))
    for offset in range(0x18, 0x2C, 4):
        u.mem_write(MAP + offset, dwords(zone))
    u.mem_write(rd.BLD_ITEMS, dwords(rd.OTHER))
    u.mem_write(rd.OTHER_ITEMS, dwords(rd.BLD))
    u.mem_write(rd.MINER_ITEMS, dwords(0))
    controls = []

    def invoke(name, entry, this, *args):
        before_rng = rngs()
        before_timer = [signed(u, rd.ACTOR + off) for off in (0xC8, 0xD0)]
        row = vm.invoke(entry, this, *args)
        controls.append(dict(name=name, **row, rng_before=before_rng, rng_after=rngs(),
                             unit_dispatch_before=before_timer,
                             unit_dispatch_after=[signed(u, rd.ACTOR + off) for off in (0xC8, 0xD0)]))
        return row

    invoke('queue_enter', rd.QUEUE, rd.ACTOR, 7, 1)
    invoke('busy_destination', rd.ASSIGN, rd.ACTOR, rd.BLD, 1)
    invoke('park_dispatch', bc.MISSION_AI, rd.ACTOR)
    saved_actor = bytes(u.mem_read(rd.ACTOR, 0x800))
    saved_loco = bytes(u.mem_read(rd.LOCO, 0x100))
    entry_rng = rngs()
    entry_cells = []
    for x, y in (tuple(cell['cell']) for cell in mark_after):
        direction, height = (4, level) if level else (-1, -1)
        result = bc.invoke(u, 0x73F0A0, rd.ACTOR, rd.cell(x, y), direction, height, 0, 1)
        entry_cells.append(dict(cell=[x, y], args=[direction, height], result=result))
    assert entry_rng == rngs()

    def retry(name):
        before, before_rng = vm.state(), rngs()
        # Adapt the existing VM's row-return interface to the shared typed
        # observer's EAX interface; both invocation and radio receipts retain
        # their existing owners.
        measured = rd.DockContinuation.try_pending_entry(SimpleNamespace(
            u=u, read32=r, invoke=lambda entry: vm.invoke(entry, rd.ACTOR)['returned_eax']))
        controls.append(dict(name=name, entry='0x70d7e0', before=before, after=vm.state(),
                             measurement=measured, rng_before=before_rng, rng_after=rngs()))

    retry('busy_retry')
    invoke('release_by_break', rd.TRANSMIT, rd.OTHER, 3, 0, rd.BLD)
    retry('free_outside_retry')
    u.mem_write(rd.ACTOR, saved_actor)
    u.mem_write(rd.LOCO, saved_loco)
    u.mem_write(rd.MINER_ITEMS, dwords(0))
    u.mem_write(rd.BLD_ITEMS, dwords(0))
    u.mem_write(rd.ACTOR + 0x9C, dwords(8 * 256 + 128, 11 * 256 + 128, level * r(0xAC13C8)))
    invoke('null_destination_on_foundation', rd.ASSIGN, rd.ACTOR, 0, 1)
    retry('free_foundation_retry')
    blocked_goal = None
    if level == 4:
        # An admitted stationary second MTNK at the common goal. Reuse the
        # existing Unit fields, type and House, with its own originally
        # constructed Drive and radio buffer. Full Unit ctor/Unlimbo omitted.
        peer, peer_loco, peer_contacts = rd.EXTRA + 0x20000, rd.EXTRA + 0x22000, rd.EXTRA + 0x23000
        goal = rd.cell(8, 12)
        u.mem_write(peer, saved_actor)
        u.mem_write(peer + 0x9C, dwords(2176, 3200, level * r(0xAC13C8)))
        for off, value in ((0x500, 0), (0x5A4, 0), (0x5A0, 0), (0xAC, 5), (0xB4, -1),
                           (0xE4, peer_contacts)):
            u.mem_write(peer + off, dwords(value))
        u.mem_write(peer + 0x74, b'\0')
        u.mem_write(peer_contacts, dwords(0))
        bc.invoke(u, 0x4AF540, peer_loco)
        u.mem_write(peer_loco + 0xC, dwords(peer))
        u.mem_write(peer_loco + 0x14, dwords(1))
        u.mem_write(peer + 0x674, dwords(peer_loco + 4))
        u.mem_write(peer + 0x6B6, b'\x01')
        peer_put = bc.invoke(u, 0x47E8A0, goal, peer, 0)
        assert r(goal + 0xE4) == peer and r(goal + 0x124) == 0x20
        u.mem_write(rd.ACTOR, saved_actor)
        u.mem_write(rd.LOCO, saved_loco)
        u.mem_write(rd.ACTOR + 0x9C, dwords(2176, 2944, level * r(0xAC13C8)))
        u.mem_write(rd.MINER_ITEMS, dwords(0))
        u.mem_write(rd.BLD_ITEMS, dwords(rd.OTHER))
        u.mem_write(rd.OTHER_ITEMS, dwords(rd.BLD))
        entry = bc.invoke(u, 0x73F0A0, rd.ACTOR, goal, 4, level, 0, 1)
        assert entry == 6
        invoke('shared_goal_destination', rd.ASSIGN, rd.ACTOR, goal, 1)
        bc.invoke(u, r(r(rd.LOCO + 4) + 0x38), 0, rd.LOCO + 4)
        u.mem_write(rd.ACTOR + 0x5E0, dwords(4, 4, 4, *([-1] * 21)))
        u.mem_write(rd.ACTOR + 0x388, dwords(0x8000, 0x8000, -1, 0, 0))
        u.mem_write(rd.LOCO + 0x40, dwords(0, 0, 0))
        u.mem_write(rd.LOCO + 0x58, dwords(-1))
        u.mem_write(rd.LOCO + 0x63, b'\0')
        u.mem_write(FRAME, dwords(201))
        before, before_rng, foot_before = vm.state(), rngs(), foot_state()
        trace, requests, returns = [], [], {}

        def observe_drive(_u, pc, size, data):
            if u.reg_read(UC_X86_REG_EIP) != pc:
                return
            sp = u.reg_read(UC_X86_REG_ESP)
            for event in returns.pop(pc, []):
                event['returned_eax'] = u.reg_read(UC_X86_REG_EAX)
            if pc == 0x73F0A0:
                event = dict(entry=hex(pc), caller=hex(r(sp)),
                             cell=list(rd.cell_xy(r(sp + 4))),
                             args=[r(sp + n * 4) for n in range(2, 6)])
                trace.append(event)
                returns.setdefault(r(sp), []).append(event)
            elif pc == 0x4D3920:
                assert not requests, 'unexpected second bounded path request'
                event = dict(entry=hex(pc), caller=hex(r(sp)),
                             args=[r(sp + n * 4) for n in range(1, 4)],
                             supplied_route=[4, 4, 4], body_executed=False)
                requests.append(event)
                trace.append(event)
                u.mem_write(rd.ACTOR + 0x5E0, dwords(4, 4, 4, *([-1] * 21)))
                ret(u, r, 12, 1)
            elif pc in (0x741970, 0x4D94B0, 0x4AFE00, 0x7C5F00):
                event = dict(entry=hex(pc), caller=hex(r(sp)))
                if pc == 0x741970:
                    event['destination'] = r(sp + 4)
                if pc == 0x7C5F00:
                    returns.setdefault(r(sp), []).append(event)
                trace.append(event)

        hook = u.hook_add(UC_HOOK_CODE, observe_drive)
        try:
            u.mem_write(SP, dwords(RET_MAGIC, rd.LOCO + 4))
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_ECX, 0)
            process = r(r(rd.LOCO + 4) + 0x40)
            assert process == 0x4B0500
            stop = run_checked(u, process, (0x4B0A7E, RET_MAGIC), count=4_000_000,
                               required_addresses=(process, 0x4B2630))
        finally:
            u.hook_del(hook)
        assert stop == 0x4B0A7E and len(requests) == 1 and not returns
        assert [e['returned_eax'] for e in trace if e['entry'] == '0x73f0a0'] == [6, 6]
        assert r(rd.ACTOR + 0x5A4) == 0 and r(rd.ACTOR + 0x500) == rd.BLD
        assert before_rng == rngs()
        blocked_goal = dict(before=before, after=vm.state(), entry=hex(process), stop=hex(stop),
                            stationary_ally_entry=entry, trace=trace, requests=requests,
                            peer_put_return=peer_put, goal=[8, 12],
                            foot_before=foot_before, foot_after=foot_state(),
                            rng_before=before_rng, rng_after=rngs())
        invoke('blocked_goal_release_by_break', rd.TRANSMIT, rd.OTHER, 3, 0, rd.BLD)
        retry('blocked_goal_foundation_retry')
    before_remove, remove_rng = foundation(), rngs()
    removed = vm.invoke(0x43F180, rd.BLD, 0)
    after_remove = foundation()
    assert removed['returned_eax'] & 255 == 1
    assert all(cell['head'] is None and cell['ground'] == 0 for cell in after_remove)
    assert remove_rng == rngs()
    vm.before = marked['after']
    result = vm.finish()
    return dict(input=case, startup=vm.spatial_startup, mark_before=mark_before,
                mark_after=mark_after, mark_rng=mark_rng, marked=marked,
                entry_cells=entry_cells, entry_rng=entry_rng, controls=controls,
                blocked_goal=blocked_goal,
                removal=dict(before=before_remove, after=after_remove, row=removed, rng=remove_rng),
                original_code_and_vtables_unchanged=result['original_code_and_vtables_unchanged'],
                native_text_sha256=result['native_text_sha256'])


def generate_depot_waiters():
    inputs = depot_input_receipts(waiters=True)
    return dict(inputs=inputs, cases=[depot_waiter_case(inputs, level) for level in (0, 4)])


def depot_waiters_provenance():
    return provenance(
        scope='Original GADEPT Mark3/0, MTNK busy parking, Unit cell entry and pending-entry '
              'admission/refusal on marked flat levels0/4; bounded original Drive blocked-goal '
              'response with real Unit entry and a supplied FindPath route.',
        entry_points=dict(mark=0x43F180, place_down=0x5683C0, cell_put=0x47E8A0,
                          occupation=0x453D60, recalc=0x47D2B0, nearby=0x703590,
                          mission_ai=bc.MISSION_AI, parking=0x70D8F0, retry=0x70D7E0,
                          unit_entry=0x73F0A0, foot_entry=0x4D9C10, building_rows=0x458A00,
                          unit_destination=rd.ASSIGN, number_impassable_rows_read=0x460133,
                          close_enough_read=0x670EDD, path_delay_read=0x6739E5,
                          direction_initializer=0x49F3A0, drive_process=0x4B0500,
                          drive_process_movement=0x4B2630, find_path=0x4D3920),
        assumptions=[
            'Existing DepotService supplies admitted Unit/Building/House/Drive, no complete constructors '
            'or admission history. Flat32x32 Cell coordinate/level priors and33x33 class/height storage; '
            'whole original Cell constructors, Building Mark and Recalc produce footprint bits/lists.',
            'Original process x87/Object14/Cell13/Map CRT initialize before the first Cell height getter; '
            'no retrospective cache reset. Building constructor BState slice and BeginMode Idle prefix '
            'exclude animation creation. BuildingType+235 is an opaque original constructor byte.',
            'Uniform sector/zone0 arrays and zero-extents Tactical storage are supplied as existing '
            'DockContinuation/Engineer joined fixtures. Full connectivity/Logic/path search/travel excluded.',
            'Original whole Mark, Nearby/FNPC/raw clearance, Unit entry, Queue/Commence/radio and '
            'pending-entry bodies execute. Actor coordinates remain prepared16,16 until the explicit '
            '8,11 foundation control; incumbent is the retained OTHER Techno, not a moving second tank.',
            'Physical RULESMD, optional LANGRULE, MPBattleMD, XMP03T4, fixed ARTMD selected original '
            'read slices establish inputs; read transport is supplied. All three full RNG streams retained.',
            'Unit entry result cells are measured, not supplied. Mark returns/raw bits, parking choice, '
            'cadence draw, pending/contact cleanup are not replaced. This is not a whole native match '
            'or proof of the captured three-tank travel path.',
            'Level4 blocked-goal control prepares a stationary second MTNK by copying admitted Unit '
            'fields with separate original Drive and radio storage; original CellPUT places it at8,12. '
            'Actor8,11, facing8000, path4,4,4, empty NavQueue and powered Drive are declared priors. '
            'Original Drive executes until4B0A7E, before ProcessTrack; actual UnitCanEnter and '
            'near-stop distance/NULL/timer effects execute. The single FindPath call alone supplies '
            'AL1 and path4,4,4. Then original BREAK and retry execute foundation refusal.'
        ],
        substitutions=['Existing DepotService presentation sinks; original CRT exit-registration '
                       '7C978A seam returns0, excluding process-exit callback registration only.',
                       'One level4 Foot FindPath4D3920 call supplies AL1/path4,4,4; all Unit entry '
                       'classifications remain original.'])


def depot_initialize_foot_inputs(vm, names, supplied_state, trace):
    """Execute the shared ordinary Foot constructor input slices once.

    The caller owns its read-only raw-state/trace projections. Reuse the same
    initializer receipt for lifecycle and terminal-arrival compositions.
    """
    initializers = []
    for name in names:
        raw_before, trace_start = supplied_state(), len(trace)
        # Original Foot ctor4D31EF supplies EBX=0. ESI is the actor;
        # the shared slice driver supplies the bounded stack and EBP=0.
        vm.u.reg_write(UC_X86_REG_EBX, 0)
        initialized = vm.block(DEPOT_FOOT_INPUT_INITIALIZERS[name], esi=rd.ACTOR)
        assert initialized['before'] == initialized['after']
        require(vm.steps.pop() is initialized, f'{name}: initializer is not the last VM step')
        initializers.append(dict(name=name, entry=initialized['entry'],
                                 end_exclusive=initialized['end_exclusive'],
                                 registers=dict(esi=hex(rd.ACTOR), ebx='0x0', ebp='0x0',
                                                esp=hex(bc.SP), zero_stack_bytes=0x80),
                                 before=raw_before, after=supplied_state(),
                                 writes=trace[trace_start:], full_observed_state_unchanged=True))
    return initializers


def depot_arrival_input_receipts():
    """Original selected idle prerequisites in the existing reader VM.

    Reuse the whole TechnoType weapons read block already executed by the
    Anytown MTNK owner. Only Primary pointer presence is consumed here;
    WeaponType callbacks, firing and their data are outside this composition.
    """
    from tools.projectile_oracle.bridge_render_inputs import lexical
    from tools.spatial_oracle.building_body_rules import INI
    reader = DepotInputReader()
    m, u, calls = reader.m, reader.u, []
    text = bytes(u.mem_read(0x401000, 0x3E0000))
    vtables = {a: bytes(u.mem_read(a, 0x600)) for a in (0x7F5C70, 0x7F6218)}
    layer = 'constructor'

    def snapshot():
        types = {}
        for name, ptr in reader.units.items():
            weapon = m.read32(ptr + 0x898)
            types[name] = dict(primary_pointer=weapon,
                               primary_name=m.string(weapon + 0x24) if weapon else None,
                               has_turret=u.mem_read(ptr + 0x806, 1)[0],
                               turret_count=signed(u, ptr + 0x808),
                               weapon_count=signed(u, ptr + 0x80C),
                               default_to_guard_area=u.mem_read(ptr + 0xD39, 1)[0])
        return dict(types=types, guard_area_iq=signed(u, reader.rules + 0x1440),
                    mission_control_bytes=bytes(u.mem_read(DEPOT_CONTROLS, 32 * 32)).hex(),
                    mission_controls={name: dict(
                        raw_bytes=bytes(u.mem_read(DEPOT_CONTROLS + number * 32, 32)).hex(),
                        raw_mission=signed(u, DEPOT_CONTROLS + number * 32),
                        no_threat=u.mem_read(DEPOT_CONTROLS + number * 32 + 4, 1)[0],
                        zombie=u.mem_read(DEPOT_CONTROLS + number * 32 + 5, 1)[0],
                        paralyzed=u.mem_read(DEPOT_CONTROLS + number * 32 + 7, 1)[0])
                        for name, number in (('Enter', 7), ('Sleep', 0), ('Move', 2), ('Guard', 5))})

    def observe(machine, pc, size, data):
        if machine.reg_read(UC_X86_REG_EIP) != pc:
            return
        if pc in (0x5276D0, 0x528A10, 0x5295F0):
            sp = machine.reg_read(UC_X86_REG_ESP)
            default = m.read32(sp + 12)
            calls.append(dict(layer=layer, entry=hex(pc), caller=hex(m.read32(sp)),
                              section=m.string(m.read32(sp + 4)), key=m.string(m.read32(sp + 8)),
                              raw_default=default,
                              bool_default_low_byte=default & 255 if pc == 0x5295F0 else None,
                              string_default=m.string(default) if pc == 0x528A10 and default else None))

    hook = u.hook_add(UC_HOOK_CODE, observe)
    constructor, layers = snapshot(), []
    try:
        for filename in ('RULESMD.INI', 'LANGRULE.INI', 'MPBattleMD.ini', 'XMP03T4.MAP'):
            path = depot_inputs_root() / filename
            if not path.is_file():
                assert filename == 'LANGRULE.INI', str(path)
                layers.append(dict(file=filename, absent=True))
                continue
            raw = path.read_bytes()
            sections, lines = lexical(raw, {'HTNK', 'MTNK', 'IQ', 'Enter', 'Sleep',
                                            'Move', 'Guard', 'Repair'})
            m.make_ini(sections)
            layer, start, before = filename, len(calls), snapshot()
            for name, ptr in reader.units.items():
                if not m.invoke(0x526810, INI, [m.cstring(name)]):
                    continue
                reader.block(0x71284A, 0x712A8F, (
                    (UC_X86_REG_EBP, ptr), (UC_X86_REG_EBX, ptr + 0x24),
                    (UC_X86_REG_ESI, INI), (UC_X86_REG_EDI, INI),
                    (UC_X86_REG_EAX, u.mem_read(ptr + 0xD22, 1)[0])), (0x5276D0, 0x528A10))
                reader.block(0x714F3D, 0x714F5E, (
                    (UC_X86_REG_EBP, ptr), (UC_X86_REG_EBX, ptr + 0x24),
                    (UC_X86_REG_EDI, INI)), (0x5295F0, 0x714F58))
            m.invoke(0x674240, reader.rules, (INI,))
            reader.block(0x679C92, 0x679CAF, ((UC_X86_REG_ESI, INI),), (0x5B3760,))
            layers.append(dict(file=filename, sha256=hashlib.sha256(raw).hexdigest(), bytes=len(raw),
                               source_lines=lines, before=before, after=snapshot(), calls=calls[start:]))
        actor = m.alloc(0x1000)
        u.mem_write(actor, dwords(0x7F5C70))
        getters = []
        for name, ptr in reader.units.items():
            u.mem_write(actor + 0x6C4, dwords(ptr))
            u.mem_write(actor + 0x138, dwords(0))
            u.mem_write(actor + 0x150, struct.pack('<d', 0))
            getters.append(dict(type=name, unit_vtable=hex(m.read32(actor)),
                                is_armed_slot=hex(m.read32(m.read32(actor) + 0x2AC)),
                                current_weapon_slot=hex(m.read32(m.read32(actor) + 0x3F4)),
                                weapon_slot=hex(m.read32(m.read32(actor) + 0x3F8)),
                                type_slot=hex(m.read32(m.read32(actor) + 0x84)),
                                is_armed_eax=m.invoke(0x701120, actor)))
        assert text == bytes(u.mem_read(0x401000, 0x3E0000))
        assert all(raw == bytes(u.mem_read(a, 0x600)) for a, raw in vtables.items())
        return dict(constructor=constructor, layers=layers, after=snapshot(), original_getters=getters,
                    original_text_and_unit_vtables_unchanged=True,
                    weapon_binding_boundary='Original selected type reader and IsArmed consume the '
                    'allocated Primary identity/presence; WeaponType readers, callbacks and firing are excluded')
    finally:
        u.hook_del(hook)
        reader.close()


def depot_arrival_cases():
    return [dict(name='stock_enter_terminal_untethered'),
            dict(name='stock_enter_terminal_tethered', tether=True),
            dict(name='post_stop_no_contact', entry='navigation_gate', linked=False),
            dict(name='terminal_nonservice', unit_repair=False),
            dict(name='terminal_target_nonnull', target=True),
            dict(name='post_stop_idle_args00', entry='unit_idle', unit_idle_args=[0, 0]),
            dict(name='post_stop_idle_args01', entry='unit_idle', unit_idle_args=[0, 1])]


def depot_arrival_controls(inputs, idle_inputs):
    """Original terminal Drive arm and complete PerCell/idle call composition.

    Travel, speed/admission and prior track selection are supplied. The exact
    Process_Track prologue frame reaches its original RET4; no terminal,
    navigation, radio, power or idle function is replaced by this observer.
    """
    rows = []
    for selected in depot_arrival_cases():
        direct_idle = selected.get('entry') == 'unit_idle'
        post_stop = selected.get('entry') in ('unit_idle', 'navigation_gate')
        case = dict(selected, entry=selected.get('entry', 'terminal_drive'), unit='HTNK', building='NADEPT',
                    unit_mission=7, unit_queued=0 if post_stop else -1,
                    building_mission=5, status=0, health=200, linked=selected.get('linked', True),
                    nav=not post_stop, powered=not post_stop, tether=selected.get('tether', False),
                    unit_repair=selected.get('unit_repair', True),
                    foot_constructor_inputs=list(DEPOT_FOOT_INPUT_INITIALIZERS))
        vm = DepotService(case, inputs)
        u, r, native = vm.u, vm.read32, idle_inputs['after']
        unit = native['types'][case['unit']]
        for address, value in ((rd.TYPE + 0x898, unit['primary_pointer']),
                               (rd.TYPE + 0x808, unit['turret_count']),
                               (rd.TYPE + 0x80C, unit['weapon_count']),
                               (RULES + 0x1440, native['guard_area_iq']),
                               (rd.ACTOR + 0xB4, case['unit_queued']),
                               (rd.ACTOR + 0x2B4, rd.OTHER if selected.get('target') else 0),
                               (rd.ACTOR + 0x5E0, -1)):
            u.mem_write(address, dwords(value))
        for address, value in ((rd.TYPE + 0x806, unit['has_turret']),
                               (rd.TYPE + 0xD39, unit['default_to_guard_area']),
                               (rd.ACTOR + 0x418, case['tether']), (rd.BLD + 0x418, case['tether']),
                               (rd.BTYPE + 0x16A9, case['unit_repair']),
                               (rd.BTYPE + 0x16AA, 0), (rd.BTYPE + 0x16AB, 0),
                               (rd.ACTOR + 0x81, 0), (rd.ACTOR + 0x8D, 0),
                               (rd.ACTOR + 0x6B3, 0)):
            u.mem_write(address, bytes([value]))
        u.mem_write(DEPOT_CONTROLS, bytes.fromhex(native['mission_control_bytes']))
        destination = [0, 0, 0] if post_stop else vm.dock_coordinate
        u.mem_write(rd.LOCO + 0x34, dwords(*destination))
        u.mem_write(rd.LOCO + 0x40, dwords(*destination))
        sample = idle_inputs['terminal_sample']
        u.mem_write(rd.LOCO + 0x58, dwords(-1 if post_stop else sample['selector'],
                                         0 if post_stop else sample['cursor']))
        u.mem_write(rd.LOCO + 0x63, bytes([not post_stop]))
        vm.steps, vm.events, vm.writes, vm.pending = [], [], [], {}
        phase = 'input_initialization'
        snapshots, trace, pending = [], [], {}
        calls = {
            0x4DF0D0: ('foot_stop_moving', 0), 0x4DB9B0: ('foot_navigation_gate', 0),
            0x739EC0: ('unit_per_cell', 1), 0x4D85D0: ('foot_per_cell', 1),
            rd.IDLE: ('unit_idle', 2), 0x4D82B0: ('foot_idle', 2), 0x709A40: ('techno_idle', 2),
            rd.TRANSMIT: ('radio_transmit', 3), bc.RECEIVE_RADIO: ('building_radio', 3),
            rd.QUEUE: ('queue_mission', 2), 0x55A910: ('drive_power_off', 1),
            0x701120: ('is_armed', 0), 0x4DF1C0: ('saved_megamission_query', 0),
        }
        markers = {0x4B21B1: 'reached_true_writer', 0x4B2215: 'after_per_cell',
                   0x4B2247: 'after_stop_moving', 0x4B2291: 'after_navigation_gate',
                   0x73A540: 'successful_dock_before_unit_return', 0x4B25F9: 'track_return'}
        fields = {rd.ACTOR + 0xAC: 'unit_mission', rd.ACTOR + 0xB4: 'unit_queued',
                  rd.ACTOR + 0x5A0: 'unit_nav_aux', rd.ACTOR + 0x5A4: 'unit_nav',
                  rd.LOCO + 0x10: 'locomotor_powered', rd.BLD + 0xAC: 'building_mission',
                  rd.BLD + 0xB4: 'building_queued', rd.BLD + 0x6DD: 'repairing',
                  rd.ACTOR + 0x520: 'tube_index', rd.ACTOR + 0x5C4: 'saved_megamission',
                  rd.ACTOR + 0x5C8: 'saved_target', rd.ACTOR + 0x5CC: 'saved_destination',
                  rd.ACTOR + 0x5D1: 'saved_flag', rd.ACTOR + 0x5E0: 'path_head',
                  rd.ACTOR + 0x6B6: 'occupation_6b6', rd.ACTOR + 0x6B7: 'occupation_6b7',
                  rd.LOCO + 0x58: 'track_selector', rd.LOCO + 0x5C: 'track_cursor',
                  rd.LOCO + 0x63: 'track_valid'}
        if not post_stop:
            fields[bc.SP - 0x100 + 0x13] = 'terminal_reached'
            fields[bc.SP - 0x100 + 0x38] = 'terminal_budget'
        for offset, name in ((0x34, 'drive_destination_'), (0x40, 'drive_head_')):
            fields.update({rd.LOCO + offset + n * 4: name + axis
                           for n, axis in enumerate(('x', 'y', 'z'))})

        def supplied_state():
            return dict(tube_index=signed(u, rd.ACTOR + 0x520),
                        saved_megamission=signed(u, rd.ACTOR + 0x5C4),
                        saved_target=rd.name_of(r(rd.ACTOR + 0x5C8)),
                        saved_destination=rd.name_of(r(rd.ACTOR + 0x5CC)),
                        saved_flag=u.mem_read(rd.ACTOR + 0x5D1, 1)[0],
                        retained_path_queue_count=signed(u, rd.ACTOR + 0x5BC),
                        idle_guard=u.mem_read(rd.ACTOR + 0x6B3, 1)[0],
                        occupation_6b6_6b7=[u.mem_read(rd.ACTOR + off, 1)[0] for off in (0x6B6, 0x6B7)],
                        actor_flags_81_8d=[u.mem_read(rd.ACTOR + off, 1)[0] for off in (0x81, 0x8D)],
                        target=rd.name_of(r(rd.ACTOR + 0x2B4)),
                        current_weapon=signed(u, rd.ACTOR + 0x138),
                        primary_pointer=r(rd.TYPE + 0x898), primary_name=unit['primary_name'],
                        turret_count=signed(u, rd.TYPE + 0x808),
                        weapon_count=signed(u, rd.TYPE + 0x80C),
                        default_to_guard_area=u.mem_read(rd.TYPE + 0xD39, 1)[0],
                        guard_area_iq=signed(u, RULES + 0x1440), house_iq=signed(u, HOUSE + 0x24C),
                        service_flags=[u.mem_read(rd.BTYPE + off, 1)[0] for off in (0x16A9, 0x16AA, 0x16AB)],
                        drive_head=coord(u, rd.LOCO + 0x40),
                        track_selector=signed(u, rd.LOCO + 0x58), track_cursor=signed(u, rd.LOCO + 0x5C),
                        track_valid=u.mem_read(rd.LOCO + 0x63, 1)[0])

        def code(machine, pc, size, data):
            for event in pending.pop(pc, []):
                event['returned_eax'] = machine.reg_read(UC_X86_REG_EAX)
                event['returned_al'] = event['returned_eax'] & 0xFF
            if machine.reg_read(UC_X86_REG_EIP) != pc:
                return
            if pc in markers:
                snapshots.append(dict(pc=hex(pc), kind='instruction_marker', marker=markers[pc],
                                      state=vm.state(), supplied_state=supplied_state()))
            if pc in calls:
                name, count = calls[pc]
                sp = machine.reg_read(UC_X86_REG_ESP)
                # Actual ILoco methods use stdcall; first stack argument is
                # their receiver. ECX at PowerOff is a stale mission value.
                event = dict(pc=hex(pc), phase=phase, kind='function_entry', function=name,
                             receiver=hex(r(sp + 4) if pc == 0x55A910 else machine.reg_read(UC_X86_REG_ECX)),
                             receiver_source='first_stack_argument' if pc == 0x55A910 else 'ecx',
                             caller=hex(r(sp)), args=[r(sp + n * 4) for n in range(1, count + 1)])
                trace.append(event)
                pending.setdefault(r(sp), []).append(event)

        def written(machine, access, address, size, value, data):
            if address in fields:
                trace.append(dict(kind='memory_write', phase=phase,
                                  pc=hex(machine.reg_read(UC_X86_REG_EIP)), field=fields[address],
                                  address=hex(address), size=size, value=value))

        hooks = [u.hook_add(UC_HOOK_CODE, code), u.hook_add(UC_HOOK_MEM_WRITE, written)]
        initializers = depot_initialize_foot_inputs(vm, case['foot_constructor_inputs'], supplied_state, trace)
        supplied_before, vm.before = supplied_state(), vm.state()
        phase = 'entry'
        entry_registers = None
        if direct_idle:
            step = vm.invoke(rd.IDLE, rd.ACTOR, *case['unit_idle_args'])
        elif post_stop:
            step = vm.invoke(0x4DB9B0, rd.ACTOR)
        else:
            # Original 4B0F20 prologue: F0 local bytes, four saved registers,
            # return at SP and one argument; terminal entry reads EBP and SP.
            frame = bc.SP - 0x100
            saved_registers = [0x11111111, 0x22222222, 0x33333333, 0x44444444]
            u.mem_write(frame, bytes(0x108))
            u.mem_write(frame, dwords(*saved_registers))
            u.mem_write(frame + 0x38, dwords(1))
            u.mem_write(bc.SP, dwords(RET_MAGIC, 0))
            u.reg_write(UC_X86_REG_EBP, rd.LOCO)
            u.reg_write(UC_X86_REG_ESP, frame)
            entry_registers = dict(ebp=hex(rd.LOCO), esp=hex(frame), zero_stack_bytes=0x108,
                                   saved_registers=saved_registers, local_budget_offset='0x38',
                                   local_budget=1, return_address=hex(RET_MAGIC), caller_args=[0])
            run_checked(u, 0x4B1F97, RET_MAGIC, count=2_000_000,
                        required_addresses=(0x4B1F97, 0x4B220F, 0x739EC0, 0x4B2242, 0x4DB9B0))
            value = u.reg_read(UC_X86_REG_EAX)
            assert u.reg_read(UC_X86_REG_ESP) == bc.SP + 8
            for event in vm.pending.pop(RET_MAGIC, []):
                event['returned_eax'] = value
            step = dict(entry='0x4b1f97', before=vm.before, after=vm.state(), returned_eax=value,
                        events=vm.events[:], writes=vm.writes[:])
            vm.steps.append(step)
        value = step['returned_eax']
        for event in pending.pop(RET_MAGIC, []):
            event['returned_eax'], event['returned_al'] = value, value & 0xFF
        assert not pending, pending
        for hook in hooks:
            u.hook_del(hook)
        result = vm.finish()
        result.update(after=step['after'], entry_registers=entry_registers, input_initializers=initializers,
                      supplied_before=supplied_before, supplied_after=supplied_state(),
                      interior_snapshots=snapshots, ordered_trace=trace,
                      terminal_frame_after=None if post_stop else dict(
                          local_budget=signed(u, frame + 0x38), reached=u.mem_read(frame + 0x13, 1)[0],
                          esp=hex(u.reg_read(UC_X86_REG_ESP)),
                          restored_registers={name: hex(u.reg_read(register)) for name, register in
                                              (('edi', UC_X86_REG_EDI), ('esi', UC_X86_REG_ESI),
                                               ('ebp', UC_X86_REG_EBP), ('ebx', UC_X86_REG_EBX))}),
                      returned_eax=value, returned_al=value & 0xFF, instruction_limit=2_000_000,
                      actual_per_cell_slot=hex(r(r(rd.ACTOR) + 0x18C)),
                      actual_navigation_gate_slot=hex(r(r(rd.ACTOR) + 0x504)),
                      actual_power_off_slot=hex(r(r(rd.LOCO + 4) + 0x5C)))
        assert result['before']['scenario_rng'] == result['after']['scenario_rng']
        rows.append(result)
    return rows


def depot_unit_cost_cases(inputs):
    return [dict(name='stock_htnk'), dict(name='stock_mtnk', unit='MTNK', building='GADEPT'),
            dict(name='zero_cost', cost=0), dict(name='negative_cost', cost=-900),
            dict(name='uneven_strength', strength=401), dict(name='step_one', step=1),
            dict(name='step_strength', step=400), dict(name='negative_step', step=-8),
            dict(name='negative_strength', strength=-400),
            dict(name='signed_max_step', strength=0x7FFFFFFF, step=0x7FFFFFFF),
            dict(name='constructor_percent', percent_bits=inputs['constructor']['repair_percent_bits']),
            dict(name='zero_percent', percent_bits='0000000000000000'),
            dict(name='negative_percent', percent_bits=struct.pack('<d', -0.15).hex())]


def depot_category_cost_controls():
    """Use the existing cost fixture for both unchanged original getters.

    The same authored FreeUnit/PadAircraft fields remain while the supplied
    type object's actual category vtable changes. No native table is patched.
    """
    rows = []
    for case in (dict(name='free_unit_category', cost=900, strength=400, free_unit=300),
                 dict(name='pad_category', cost=900, strength=400, pad_dock=True,
                      separate_aircraft=False),
                 dict(name='pad_and_free_unit_category', cost=3000, strength=1000,
                      pad_dock=True, separate_aircraft=False, free_unit=1000)):
        u, call, read32, events = fixture(case)
        text = bytes(u.mem_read(0x401000, 0x3E0000))
        vtables = [bytes(u.mem_read(address, 0x600)) for address in (0x7E4570, 0x7F6218)]
        building_cost = bc.invoke(u, 0x45ED50, sm.YTYPE)
        building_repair_cost = bc.invoke(u, REPAIR_STEP_COST, sm.YTYPE)
        u.mem_write(sm.YTYPE, dwords(UNIT_TYPE_VTABLE))
        unit_cost = bc.invoke(u, 0x711EB0, sm.YTYPE)
        unit_repair_cost = bc.invoke(u, REPAIR_STEP_COST, sm.YTYPE)
        assert text == bytes(u.mem_read(0x401000, 0x3E0000))
        assert vtables == [bytes(u.mem_read(address, 0x600)) for address in (0x7E4570, 0x7F6218)]
        as_signed = lambda value: struct.unpack('<i', dwords(value))[0]
        rows.append(dict(input=case, building_cost=as_signed(building_cost),
                         building_repair_cost=as_signed(building_repair_cost),
                         unit_cost=as_signed(unit_cost), unit_repair_cost=as_signed(unit_repair_cost),
                         original_code_and_vtables_unchanged=True))
    return rows


def depot_radio_cases():
    return [dict(name='stock_htnk_paid', health=200),
            dict(name='stock_mtnk_paid', unit='MTNK', building='GADEPT',
                 health=100, estimate=100, balance=10000),
            dict(name='exact_money', health=200, balance=2),
            dict(name='short_money', health=200, balance=1),
            dict(name='empty_wallet', health=200, balance=0),
            dict(name='newly_full', health=395, balance=2),
            dict(name='exactly_full_after_step', health=392, balance=2),
            dict(name='one_short_after_step', health=391, balance=2),
            dict(name='already_full', health=400, balance=0),
            dict(name='above_strength', health=401, balance=0),
            dict(name='nav_nonnull', health=200, nav=True),
            dict(name='estimate_differs', health=200, estimate=50),
            dict(name='complete_clamps_estimate', health=395, estimate=50),
            dict(name='negative_step_heals_one', health=200, step=-8),
            dict(name='negative_health', health=-10, estimate=-20),
            dict(name='negative_strength', strength=-400, health=200),
            dict(name='signed_max_step_no_overflow', strength=0x7FFFFFFF,
                 step=0x7FFFFFFF, health=0, estimate=0),
            dict(name='signed_max_step_wraps', strength=0x7FFFFFFF,
                 step=0x7FFFFFFF, health=1, estimate=1)]


def depot_guard_cases():
    base = dict(building_mission=5, unit_mission=7, arrival='center')
    return [dict(base, name='stock_state0_center', status=0),
            dict(base, name='stock_state1_center', status=1),
            dict(base, name='stock_state1_dock_point', status=1, arrival='dock'),
            dict(base, name='center_delta_x63', status=1, delta=[63, 0, 0]),
            dict(base, name='center_delta_x64', status=1, delta=[64, 0, 0]),
            dict(base, name='center_delta_z63', status=1, delta=[0, 0, 63]),
            dict(base, name='center_delta_z64', status=1, delta=[0, 0, 64]),
            dict(base, name='center_powered_moving', status=1, moving=True, powered=True),
            dict(base, name='center_not_enter', status=1, unit_mission=5),
            dict(base, name='center_no_contact', status=1, linked=False),
            dict(base, name='stupid_guard_mode', status=1, stupid_guard_mode=True),
            dict(base, name='allied_center', status=1, unit='MTNK', building='GADEPT')]


def depot_mission_cases():
    return [dict(name='state1_paid', health=200),
            dict(name='state1_insufficient', health=200, balance=1),
            dict(name='state1_newly_full', health=395),
            dict(name='state1_already_full', health=400),
            dict(name='state1_full_rally', health=400, rally=[13, 13]),
            dict(name='state1_nav_nonnull', health=200, nav=True),
            dict(name='state1_nav_null_aux_nonnull', health=200, nav_aux=True),
            dict(name='state1_nav_and_aux_nonnull', health=200, nav=True, nav_aux=True),
            dict(name='state1_powered_moving', health=200, moving=True, powered=True),
            dict(name='state1_no_contact', linked=False),
            dict(name='state1_manual_reload', health=395, manual_reload=True),
            dict(name='state2_progress14_below_gate', health=200, status=2, stage=[13, 0, 199, 1, 1, 1]),
            dict(name='state2_progress15_paid', health=200, status=2, stage=[14, 0, 199, 1, 1, 1]),
            dict(name='state2_progress15_insufficient', health=200, balance=1,
                 status=2, stage=[14, 0, 199, 1, 1, 1]),
            dict(name='state2_unexpired_timer', health=200, status=2, stage=[14, 0, 200, 1, 1, 1]),
            dict(name='state2_rate0_clamps_timer', health=200, status=2, stage=[13, 0, 199, 1, 0, 1]),
            dict(name='state2_stopped_timer', health=200, status=2, stage=[13, 0, -1, 0, 1, 1]),
            dict(name='state2_completes', health=395, status=2, stage=[14, 0, 199, 1, 1, 1]),
            dict(name='state2_already_full', health=400, status=2, stage=[14, 0, 199, 1, 1, 1]),
            dict(name='state2_full_rally', health=400, rally=[13, 13],
                 status=2, stage=[14, 0, 199, 1, 1, 1]),
            dict(name='allied_state1_paid', unit='MTNK', building='GADEPT', health=100)]


def depot_execute_cases(cases, inputs, entry, this, args=()):
    rows = []
    for case in cases:
        vm = DepotService(case, inputs)
        vm.invoke(entry, this, *args)
        rows.append(vm.finish())
    return rows


def depot_repair_fallback_cases():
    rows = []
    for status in (0, 1):
        for distance in (99, 100, 199, 200):
            for powered in (False, True):
                rows.append(dict(name=f'state{status}_distance{distance}_power{int(powered)}',
                                 status=status, delta=[distance, 0, 0], powered=powered,
                                 nav=bool(status), moving=bool(status), linked=True, ready=0))
    for powered in (False, True):
        rows.append(dict(name=f'state0_need_move_negative_distance99_power{int(powered)}',
                         status=0, delta=[99, 0, 0], powered=powered,
                         nav=True, moving=True, linked=True, ready=0))
    for status in (0, 1):
        for ready in (0, 1):
            rows.append(dict(name=f'state{status}_no_contact_ready{ready}', status=status,
                             linked=False, powered=False, nav=False, moving=False, ready=ready))
    return rows


def depot_repair_fallback_controls(inputs):
    """Original repair fallback branches with observations scoped to these rows.

    Coordinates/navigation/power and readiness are declared prestate. Actual
    distance and NEED_MOVE results, all ILoco bodies and MissionAI execute;
    this observer neither chooses replies nor substitutes a power/readiness
    writer. Existing corpus event and write projections remain unchanged.
    """
    rows = []
    for selected in depot_repair_fallback_cases():
        case = dict(selected, unit='HTNK', building='NADEPT', unit_mission=7,
                    building_mission=DEPOT_REPAIR_MISSION, health=200)
        vm = DepotService(case, inputs)
        u, r = vm.u, vm.read32
        interface_vtable = r(rd.LOCO + 4)
        persist_vtable = r(rd.LOCO)
        slots = {name: r(interface_vtable + offset) for name, offset in
                 (('is_moving', 0x10), ('power_on', 0x58), ('power_off', 0x5C), ('is_powered', 0x60))}
        # The state0 QueryInterface returns IPersist. Its GetClassID/Release
        # slots belong to the base interface, distinct from ILoco+4.
        slots.update(class_id=r(persist_vtable + 0xC), release=r(persist_vtable + 0x8))
        distance_entry = r(r(rd.BLD) + 0x4D8)
        calls = {entry: ('drive_' + name, 2 if name == 'class_id' else 1, True)
                 for name, entry in slots.items()}
        calls.update({distance_entry: ('building_distance_to', 1, False),
                      0x53A130: ('house_power_gate', 0, False),
                      0x65ACB0: ('radio_first_contact', 1, False),
                      rd.TRANSMIT: ('radio_transmit', 3, False),
                      UNIT_RECEIVE_RADIO: ('unit_radio', 3, False),
                      FOOT_RECEIVE_RADIO: ('foot_radio', 3, False),
                      bc.TECHNO_RECEIVE_RADIO: ('techno_radio', 3, False),
                      rd.QUEUE: ('queue_mission', 2, False),
                      0x4DF0D0: ('foot_stop_moving', 0, False)})
        markers = {0x44C5AB: 'state1_need_move_fallback', 0x44C7E5: 'state0_fallback',
                   0x44C623: 'state0_contact_gate', 0x44C18E: 'state1_no_contact_ready_writer',
                   0x44C6EA: 'state0_no_contact_queue', 0x44C61B: 'state1_power_on_call',
                   0x44C808: 'state0_power_on_call'}
        fields = {rd.LOCO + 0x10: 'locomotor_powered', rd.BLD + 0x6DD: 'repairing',
                  rd.BLD + 0xAC: 'building_mission', rd.BLD + 0xB4: 'building_queued',
                  rd.BLD + 0xBC: 'status', rd.BLD + 0xC8: 'dispatch_start',
                  rd.BLD + 0xD0: 'dispatch_delay', rd.ACTOR + 0x5A0: 'unit_nav_aux',
                  rd.ACTOR + 0x5A4: 'unit_nav'}
        trace, snapshots, pending = [], [], {}

        def supplied_state():
            return dict(building_ready_byte=u.mem_read(rd.BLD + 0x6DD, 1)[0],
                        contact_has_locomotor=r(rd.ACTOR + 0x674) != 0,
                        locomotor_interface=hex(r(rd.ACTOR + 0x674)),
                        locomotor_interface_vtable=hex(interface_vtable),
                        locomotor_persist_vtable=hex(persist_vtable),
                        locomotor_destination=coord(u, rd.LOCO + 0x34),
                        tube_index=signed(u, rd.ACTOR + 0x520),
                        saved_megamission=signed(u, rd.ACTOR + 0x5C4),
                        building_slots_8_11_12=[hex(r(rd.BLD + 0x55C + n * 4)) for n in (8, 11, 12)])

        def code(machine, pc, size, data):
            for event in pending.pop(pc, []):
                event['returned_eax'] = machine.reg_read(UC_X86_REG_EAX)
                event['returned_al'] = event['returned_eax'] & 0xFF
                if event['function'] == 'drive_class_id':
                    event['returned_class_id_bytes'] = bytes(machine.mem_read(event['args'][1], 16)).hex()
            if machine.reg_read(UC_X86_REG_EIP) != pc:
                return
            if pc in markers:
                marker = dict(kind='instruction_marker', pc=hex(pc), marker=markers[pc],
                              state=vm.state(), supplied_state=supplied_state())
                snapshots.append(marker)
                trace.append(dict(kind='instruction_marker', pc=hex(pc), marker=markers[pc]))
            if pc in calls:
                name, count, com_receiver = calls[pc]
                sp = machine.reg_read(UC_X86_REG_ESP)
                event = dict(kind='function_entry', pc=hex(pc), function=name,
                             receiver=hex(r(sp + 4) if com_receiver else machine.reg_read(UC_X86_REG_ECX)),
                             receiver_source='first_stack_argument' if com_receiver else 'ecx',
                             caller=hex(r(sp)), args=[r(sp + n * 4) for n in range(1, count + 1)])
                trace.append(event)
                pending.setdefault(r(sp), []).append(event)

        def written(machine, access, address, size, value, data):
            if address in fields:
                trace.append(dict(kind='memory_write', pc=hex(machine.reg_read(UC_X86_REG_EIP)),
                                  field=fields[address], address=hex(address), size=size, value=value))

        hooks = [u.hook_add(UC_HOOK_CODE, code), u.hook_add(UC_HOOK_MEM_WRITE, written)]
        supplied_before = supplied_state()
        step = vm.invoke(bc.MISSION_AI, rd.BLD)
        for event in pending.pop(RET_MAGIC, []):
            event['returned_eax'], event['returned_al'] = step['returned_eax'], step['returned_eax'] & 0xFF
        assert not pending, pending
        for hook in hooks:
            u.hook_del(hook)
        result = vm.finish()
        result.update(after=step['after'], supplied_before=supplied_before, supplied_after=supplied_state(),
                      actual_locomotor_slots={name: hex(entry) for name, entry in slots.items()},
                      actual_distance_entry=hex(distance_entry), fallback_trace=trace,
                      interior_snapshots=snapshots, instruction_limit=2_000_000)
        assert result['before']['scenario_rng'] == result['after']['scenario_rng']
        rows.append(result)
    return rows


def depot_destination_cases():
    return [dict(name='repair_current_cell'),
            dict(name='empty_current_cell', current_list=[]),
            dict(name='high_bridge_current_cell', flags=0x100),
            dict(name='no_service_flags', unit_repair=False),
            dict(name='bunker_current_cell', unit_repair=False, bunker=True),
            dict(name='reload_only_current_cell', unit_repair=False, unit_reload=True),
            dict(name='actor_then_repair_current_cell', current_list=['miner', 'refinery']),
            dict(name='unlinked_repair_current_cell', linked=False),
            dict(name='already_powered_repair_current_cell', powered=True),
            dict(name='null_prior_nav_repair_current_cell', null=True, prior_nav=True),
            dict(name='null_no_prior_nav_early_return', null=True),
            dict(name='null_force_reassign_repair_current_cell', null=True, force_reassign=True),
            dict(name='repair_current_cell_flag0', flag=0),
            dict(name='depot_target_fast_return', target='depot'),
            dict(name='same_nav_depot_target_early_return', target='depot', prior_nav=True),
            dict(name='deployed_destination_early_return', deployed=True)]


def depot_destination_controls(inputs):
    """Original Unit741970 with declared raw current-cell/list prerequisites.

    This extends the existing scene owner. It supplies no setter/power result,
    native callback or movement Process and never changes executable bytes.
    """
    rows = []
    for case in depot_destination_cases():
        vm = DepotService(dict(case, nav=case.get('prior_nav', False)), inputs)
        u, r = vm.u, vm.read32
        at = vm.state()['unit_coordinate']
        current = rd.cell(at[0] // 256, at[1] // 256)
        u.mem_write(current + 0x140, dwords(case.get('flags', 0)))
        u.mem_write(rd.BTYPE + 0x16A9, bytes([case.get('unit_repair', True)]))
        u.mem_write(rd.BTYPE + 0x16AA, bytes([case.get('unit_reload', False)]))
        u.mem_write(rd.BTYPE + 0x16AB, bytes([case.get('bunker', False)]))
        items = case.get('current_list', ['refinery'])
        pointers = dict(miner=rd.ACTOR, refinery=rd.BLD, other=rd.OTHER)
        u.mem_write(current + 0xE4, dwords(pointers[items[0]] if items else 0))
        for index, name in enumerate(items):
            following = pointers[items[index + 1]] if index + 1 < len(items) else 0
            u.mem_write(pointers[name] + 0x30, dwords(following))
        u.mem_write(rd.ACTOR + 0x1F8, bytes([case.get('force_reassign', False)]))
        u.mem_write(rd.ACTOR + 0x6E0, bytes([case.get('deployed', False)]))
        power_on = r(r(rd.LOCO + 4) + 0x58)
        is_powered = r(r(rd.LOCO + 4) + 0x60)
        vm.destination_power_slots = (power_on, is_powered)
        vm.destination_calls = {0x742F48: 'unit_power_gate', 0x53A130: 'house_always_false',
                                0x5F6960: 'raw_current_cell', power_on: 'locomotor_power_on',
                                is_powered: 'locomotor_is_powered',
                                0x4D94B0: 'foot_destination', 0x4AFD40: 'drive_move_to'}
        flags = lambda: dict(force_reassign=u.mem_read(rd.ACTOR + 0x1F8, 1)[0],
                             deployed=u.mem_read(rd.ACTOR + 0x6E0, 1)[0])
        flags_before = flags()
        vm.before = vm.state()
        target = (0 if case.get('null') else rd.BLD if case.get('target') == 'depot'
                  else rd.cell(13, 13))
        vm.invoke(rd.ASSIGN, rd.ACTOR, target, case.get('flag', 1))
        flags_after = flags()
        result = vm.finish()
        result.update(raw_current_cell=rd.cell_xy(current), current_list=items,
                      raw_current_flags=case.get('flags', 0),
                      supplied_service_flags=dict(unit_repair=case.get('unit_repair', True),
                                                  unit_reload=case.get('unit_reload', False),
                                                  bunker=case.get('bunker', False)),
                      actual_ilo_vtable=hex(r(rd.LOCO + 4)), actual_power_on=hex(power_on),
                      actual_is_powered=hex(is_powered),
                      setter_flags_before=flags_before, setter_flags_after=flags_after)
        rows.append(result)
    return rows


def depot_pending_entry_cases():
    return [
        dict(name='unit_null_no_nav', entry='unit_null'),
        dict(name='unit_null_prior_nav', entry='unit_null', nav=True),
        dict(name='unit_null_force_no_nav', entry='unit_null', force=True),
        dict(name='ordinary_unit_cell', entry='unit_cell'),
        dict(name='unit_idle_no_nav', entry='unit_idle'),
        dict(name='unit_idle_prior_nav', entry='unit_idle', nav=True),
        dict(name='idle_event_null_no_nav', entry='idle_event_calls'),
        dict(name='idle_event_null_prior_nav', entry='idle_event_calls', nav=True),
        dict(name='idle_event_linked_untethered', entry='idle_event_calls', linked=True),
        dict(name='deploy_event_null_no_nav', entry='deploy_event_calls'),
        dict(name='deploy_event_null_prior_nav', entry='deploy_event_calls', nav=True),
        dict(name='megamission_untethered_no_nav', entry='megamission_clear'),
        dict(name='megamission_linked_untethered', entry='megamission_clear', linked=True),
        dict(name='megamission_linked_tethered_depot', entry='megamission_clear', linked=True, tether=True),
        dict(name='megamission_tethered_live_dock_unload', entry='megamission_clear', linked=True,
             tether=True, dock_unload=True),
        dict(name='megamission_tethered_dead_dock_unload', entry='megamission_clear', linked=True,
             tether=True, dock_unload=True, contact_alive=False),
        dict(name='idle_event_two_contacts', entry='idle_event_calls', linked=True, two_contacts=True),
        dict(name='megamission_two_contacts', entry='megamission_clear', linked=True, two_contacts=True),
        dict(name='idle_tethered_skips_all', entry='idle_tether_gate', linked=True, tether=True),
        dict(name='idle_foot_prefix_no_nav', entry='idle_event_foot_prefix'),
        dict(name='idle_foot_prefix_prior_nav', entry='idle_event_foot_prefix', nav=True),
        dict(name='idle_after_actor_admission_no_nav', entry='idle_event_after_actor_admission'),
        dict(name='idle_after_actor_admission_prior_nav', entry='idle_event_after_actor_admission',
             nav=True),
        dict(name='idle_after_actor_admission_two_contacts', entry='idle_event_after_actor_admission',
             linked=True, two_contacts=True),
        dict(name='megamission_tethered_no_contact', entry='megamission_clear', tether=True),
        dict(name='megamission_tethered_unit_contact', entry='megamission_clear', linked=True, tether=True,
             dock_unload=True, contact_is_unit=True),
        dict(name='megamission_tethered_alive_zero_health', entry='megamission_clear', linked=True,
             tether=True, dock_unload=True, contact_health=0),
        dict(name='megamission_null_pending_no_store', entry='megamission_clear', pending_null=True),
        dict(name='idle_mission18_skips_all', entry='idle_mission_gate', linked=True, mission=18),
        dict(name='idle_mission19_skips_all', entry='idle_mission_gate', linked=True, mission=19),
    ]


def depot_pending_entry_controls(inputs, cases=None):
    """Original class bodies and admitted Event slices over the shared VM.

    Event token decoding/admission, Team membership and later translated
    queue/target work are outside the supplied slice boundaries. The observer
    records original calls and raw writes together without replacing bodies.
    """
    rows = []
    for case in depot_pending_entry_cases() if cases is None else cases:
        case = dict(case, foot_constructor_inputs=case.get(
            'foot_constructor_inputs', list(DEPOT_FOOT_INPUT_INITIALIZERS)))
        if case['entry'] == 'unit_idle':
            case['unit_idle_args'] = case.get('unit_idle_args', [0, 1])
        vm = DepotService(dict(case, unit_mission=case.get('mission', 5),
                               nav=case.get('nav', False), linked=case.get('linked', False),
                               powered=True), inputs)
        u, r = vm.u, vm.read32
        u.mem_write(rd.ACTOR + 0x500, dwords(0 if case.get('pending_null') else rd.OTHER))
        for address, value in ((rd.ACTOR + 0x418, case.get('tether', False)),
                               (rd.BLD + 0x418, case.get('tether', False)),
                               (rd.BTYPE + 0x16B3, case.get('dock_unload', False)),
                               (rd.BLD + 0x90, case.get('contact_alive', True)),
                               (rd.ACTOR + 0x1F8, case.get('force', False))):
            u.mem_write(address, bytes([value]))
        if 'contact_health' in case:
            u.mem_write(rd.BLD + 0x6C, dwords(case['contact_health']))
        if case.get('contact_is_unit'):
            # A supplied contact using the unchanged original Unit vtable.
            # This only exercises the original WhatAmI gate; no Unit AI runs.
            u.mem_write(rd.BLD, dwords(0x7F5C70))
        u.mem_write(rd.ACTOR + 0x5D4, dwords(0))
        if case.get('two_contacts'):
            u.mem_write(rd.ACTOR + 0xE8, dwords(2, 2))
            u.mem_write(rd.MINER_ITEMS, dwords(rd.BLD, rd.OTHER))
            u.mem_write(rd.OTHER_ITEMS, dwords(rd.ACTOR))
        first_contact_send = r(r(rd.ACTOR) + 0x274)
        broadcast_send = r(r(rd.ACTOR) + 0x280)
        path_vector_clear = r(r(rd.ACTOR + 0x5AC) + 0xC)
        saved_megamission_query = r(r(rd.ACTOR) + 0x4AC)
        phase = 'input_initialization'
        trace, pending_returns = [], {}
        calls = {
            rd.ASSIGN: ('unit_destination', 2), 0x4D94B0: ('foot_destination', 2),
            rd.IDLE: ('unit_idle', 2), 0x4D82B0: ('foot_idle', 2),
            0x709A40: ('techno_idle', 2), 0x6FCDB0: ('techno_target', 1),
            first_contact_send: ('radio_first_contact', 1),
            broadcast_send: ('radio_broadcast_all', 1),
            rd.TRANSMIT: ('radio_transmit', 3),
            0x4DA1C0: ('foot_clear_path', 0),
            0x4DA030: ('foot_process_retained_path', 0),
            path_vector_clear: ('path_vector_clear', 0),
            saved_megamission_query: ('saved_megamission_query', 0),
        }
        markers = {0x4C7342: 'event_tether_clear', 0x4C7353: 'event_pending_entry_clear',
                   0x4C7597: 'stop_path_vector_call', 0x4C759C: 'stop_foot_path_call'}
        fields = {rd.ACTOR + 0x500: 'pending_entry', rd.ACTOR + 0x418: 'unit_tether',
                  rd.BLD + 0x418: 'building_tether', rd.ACTOR + 0x5A0: 'unit_nav_aux',
                  rd.ACTOR + 0x5A4: 'unit_nav', rd.MINER_ITEMS: 'unit_contact_slot0',
                  rd.MINER_ITEMS + 4: 'unit_contact_slot1', rd.BLD_ITEMS: 'building_contact_slot0',
                  rd.OTHER_ITEMS: 'other_contact_slot0'}
        fields.update({rd.ACTOR + 0x520: 'tube_index', rd.ACTOR + 0x5C4: 'saved_megamission',
                       rd.ACTOR + 0x5C8: 'saved_target', rd.ACTOR + 0x5CC: 'saved_destination',
                       rd.ACTOR + 0x5D1: 'saved_flag'})

        def code(machine, pc, size, data):
            for event in pending_returns.pop(pc, []):
                event['returned_eax'] = machine.reg_read(UC_X86_REG_EAX)
                if event['function'] == 'saved_megamission_query':
                    event['returned_al'] = event['returned_eax'] & 0xFF
            if machine.reg_read(UC_X86_REG_EIP) != pc:
                return
            common = dict(pc=hex(pc), phase=phase,
                          pending_entry_before=rd.name_of(r(rd.ACTOR + 0x500)))
            if pc in markers:
                trace.append(dict(common, kind='instruction_marker', instruction=markers[pc],
                                  registers={name: hex(machine.reg_read(register)) for name, register in
                                             (('esi', UC_X86_REG_ESI), ('edi', UC_X86_REG_EDI),
                                              ('ebp', UC_X86_REG_EBP))}))
            if pc in calls:
                kind, count = calls[pc]
                sp = machine.reg_read(UC_X86_REG_ESP)
                event = dict(common, kind='function_entry', function=kind,
                             this=rd.name_of(machine.reg_read(UC_X86_REG_ECX)),
                             caller=hex(r(sp)), args=[r(sp + n * 4) for n in range(1, count + 1)])
                trace.append(event)
                pending_returns.setdefault(r(sp), []).append(event)

        def written(machine, access, address, size, value, data):
            if address in fields:
                trace.append(dict(kind='memory_write', phase=phase,
                                  pc=hex(machine.reg_read(UC_X86_REG_EIP)),
                                  field=fields[address], address=hex(address), size=size, value=value,
                                  pending_entry_before=rd.name_of(r(rd.ACTOR + 0x500))))

        def supplied_state():
            return dict(object_flags_14=u.mem_read(rd.ACTOR + 0x14, 1)[0],
                        force_reassign=u.mem_read(rd.ACTOR + 0x1F8, 1)[0],
                        tube_index=signed(u, rd.ACTOR + 0x520),
                        saved_megamission=signed(u, rd.ACTOR + 0x5C4),
                        saved_target=rd.name_of(r(rd.ACTOR + 0x5C8)),
                        saved_destination=rd.name_of(r(rd.ACTOR + 0x5CC)),
                        saved_flag=u.mem_read(rd.ACTOR + 0x5D1, 1)[0],
                        retained_path_queue_count=signed(u, rd.ACTOR + 0x5BC),
                        team=rd.name_of(r(rd.ACTOR + 0x5D4)),
                        path_vector_bytes=bytes(u.mem_read(rd.ACTOR + 0x5AC, 0x18)).hex(),
                        contact_vtable=hex(r(rd.BLD)),
                        contact_alive=u.mem_read(rd.BLD + 0x90, 1)[0],
                        contact_health=signed(u, rd.BLD + 0x6C),
                        contact_dock_unload=u.mem_read(rd.BTYPE + 0x16B3, 1)[0])

        hooks = [u.hook_add(UC_HOOK_CODE, code), u.hook_add(UC_HOOK_MEM_WRITE, written)]
        initializers = depot_initialize_foot_inputs(vm, case['foot_constructor_inputs'], supplied_state, trace)
        supplied_before = supplied_state()
        vm.before = vm.state()
        phase = 'entry'
        entry, registers = case['entry'], None
        if entry == 'unit_null':
            step = vm.invoke(rd.ASSIGN, rd.ACTOR, 0, 1)
        elif entry == 'unit_cell':
            step = vm.invoke(rd.ASSIGN, rd.ACTOR, rd.cell(13, 13), 1)
        elif entry == 'unit_idle':
            step = vm.invoke(rd.IDLE, rd.ACTOR, *case['unit_idle_args'])
        else:
            if entry == 'megamission_clear':
                esi, edi = rd.EXTRA + 0x2E000, rd.ACTOR
                u.mem_write(esi, bytes(0x30))
            else:
                esi, edi = rd.ACTOR, 0
            u.reg_write(UC_X86_REG_EBX, 0)
            u.reg_write(UC_X86_REG_EDI, edi)
            registers = dict(esi=hex(esi), edi=hex(edi), ebx='0x0', ebp='0x0',
                             esp=hex(bc.SP), zero_stack_bytes=0x80)
            step = vm.block(DEPOT_EVENT_SLICES[entry], esi=esi, instruction_count=2_000_000)
        step['instruction_limit'] = 2_000_000
        if 'returned_eax' in step:
            for event in pending_returns.pop(RET_MAGIC, []):
                event['returned_eax'] = step['returned_eax']
        assert not pending_returns, pending_returns
        for hook in hooks:
            u.hook_del(hook)
        supplied_after = supplied_state()
        result = vm.finish()
        result.update(after=step['after'], entry_registers=registers,
                      input_initializers=initializers,
                      supplied_before=supplied_before, supplied_after=supplied_after,
                      actual_first_contact_send=hex(first_contact_send),
                      actual_broadcast_send=hex(broadcast_send),
                      actual_path_vector_clear=hex(path_vector_clear),
                      actual_saved_megamission_query=hex(saved_megamission_query),
                      lifecycle_trace=trace)
        assert result['before']['scenario_rng'] == result['after']['scenario_rng']
        rows.append(result)
    return rows


def depot_history(case, inputs):
    """Every frame invokes the unchanged MissionAI, including idle timers.

    Quiet dispatches have no service/Guard call and identical complete before
    and after snapshots. Retain their frame/result and a hash over every full
    native step; omit only the duplicated full snapshot from the payload.
    """
    vm = DepotService(case, inputs)
    quiet, digest = [], hashlib.sha256()
    from tools.native_oracle import _canonical
    if case.get('dock_now'):
        vm.invoke(bc.RECEIVE_RADIO, rd.BLD, rd.ACTOR, 0x15, 0)
    for frame in range(case.get('frame', 200), case['to_frame'] + 1):
        vm.u.mem_write(FRAME, dwords(frame))
        if frame == case.get('deposit_frame'):
            vm.invoke(0x4F9950, HOUSE, case['deposit'])
        if frame == case.get('restore_frame'):
            # Declared native continuation boundary for the existing Rust
            # snapshot policy: original Random::Seed(0), no native disk load.
            vm.invoke(0x65C6D0, SCENARIO + 0x218, 0)
        step = vm.invoke(bc.MISSION_AI, rd.BLD)
        digest.update(_canonical(step))
        if step['before'] == step['after'] and not any(
                event['kind'] in ('depot_service', 'depot_guard') for event in step['events']):
            assert all(event['kind'] == 'mission_dispatch' for event in step['events'])
            quiet.append([frame, step['returned_eax']])
            vm.steps.pop()
    row = vm.finish()
    row.update(quiet_dispatches=quiet, all_dispatch_steps_sha256=digest.hexdigest(),
               dispatched_frames=case['to_frame'] - case.get('frame', 200) + 1)
    return row


def depot_prerequisite_controls(inputs):
    rows = []
    for name, dock_now in (('guard_queue_waits_ready0', False), ('dock_now_promotes_queued_repair', True)):
        vm = DepotService(dict(name=name, building_mission=5, unit_mission=7,
                               arrival='center', status=1), inputs)
        vm.invoke(bc.MISSION_AI, rd.BLD)
        vm.block(bc.READY_COMMENCE)
        if dock_now:
            vm.invoke(bc.RECEIVE_RADIO, rd.BLD, rd.ACTOR, 0x15, 0)
            vm.block(bc.READY_COMMENCE)
        rows.append(vm.finish())
    return rows


def depot_input_controls():
    from tools.projectile_oracle.bridge_render_inputs import lexical
    rows = []
    for name, text in (
            ('constructor_defaults_then_absent_keys', '[General]\nUnrelated=1\n[HTNK]\nUnrelated=1\n[NADEPT]\nUnrelated=1\n'),
            ('exact_keys_then_missing', '[General]\nRepairStep=8\nRepairPercent=15%\nURepairRate=.016\n[HTNK]\nStrength=400\nCost=900\nManualReload=yes\n[NADEPT]\nUnitRepair=yes\nHasStupidGuardMode=no\n[Repair]\nRate=.08\n'),
            ('wrong_case_keys', '[General]\nrepairstep=9\nrepairpercent=50%\nurepairrate=.03\n[HTNK]\nstrength=99\ncost=123\nmanualreload=yes\n[NADEPT]\nunitrepair=yes\nhasstupidguardmode=no\n[Repair]\nrate=.1\n'),
            ('native_signed_and_fractional', '[General]\nRepairStep=-8\nRepairPercent=-15%\nURepairRate=.001111111\n[HTNK]\nStrength=-400\nCost=-900\n[NADEPT]\nUnitRepair=yes\nHasStupidGuardMode=no\n[Repair]\nRate=.08\n')):
        reader = DepotInputReader()
        try:
            raw = text.encode('ascii')
            sections, lines = lexical(raw, set(DEPOT_SELECTED))
            reader.read_layer(name + '.INI', raw, sections, lines)
            if name == 'exact_keys_then_missing':
                missing = '[General]\nUnrelated=1\n[HTNK]\nUnrelated=1\n[NADEPT]\nUnrelated=1\n[Repair]\nUnrelated=1\n'.encode('ascii')
                sections, lines = lexical(missing, set(DEPOT_SELECTED))
                reader.read_layer('missing_keys.INI', missing, sections, lines)
            rows.append(dict(name=name, source=text, constructor=reader.constructor,
                             layers=reader.layers, after=reader.snap()))
        finally:
            reader.close()
    return rows


def generate_depot_service():
    inputs = depot_input_receipts()
    unit_cost = []
    for case in depot_unit_cost_cases(inputs):
        vm = DepotService(case, inputs)
        for slot in (0xAC, 0xB0, 0xB4):
            vm.invoke(vm.read32(UNIT_TYPE_VTABLE + slot), rd.TYPE)
        unit_cost.append(vm.finish())
    return dict(schema_version=1, source='unicorn/gamemd.exe',
                pointer_aliases=dict(miner='selected tank', refinery='selected repair depot', other='fixture second building'),
                inputs=inputs, input_controls=depot_input_controls(), unit_cost=unit_cost,
                category_cost=depot_category_cost_controls(),
                radio=depot_execute_cases(depot_radio_cases(), inputs, UNIT_RECEIVE_RADIO,
                                          rd.ACTOR, (rd.BLD, 0x1C, 0)),
                destination=depot_destination_controls(inputs),
                pending_entry_lifecycle=depot_pending_entry_controls(inputs),
                guard=depot_execute_cases(depot_guard_cases(), inputs, bc.MISSION_AI, rd.BLD),
                prerequisites=depot_prerequisite_controls(inputs),
                mission=depot_execute_cases(depot_mission_cases(), inputs, bc.MISSION_AI, rd.BLD),
                histories=[depot_history(case, inputs) for case in (
                    dict(name='paid_complete', health=360, to_frame=332),
                    dict(name='paid_complete_restore_seed0', health=360, to_frame=332, restore_frame=280),
                    dict(name='first_step_complete', health=395, to_frame=288),
                    dict(name='dock_now_state0_to_service', health=395, status=0,
                         unit_mission=7, dock_now=True, to_frame=292),
                    dict(name='full_release_rally', health=400, rally=[13, 13], to_frame=203),
                    dict(name='insufficient_then_deposit', health=384, balance=2, to_frame=445,
                         deposit_frame=300, deposit=2),
                    dict(name='no_funds_200_mission_visits', health=384, balance=0, to_frame=14400))],
                arrival_terminal=depot_arrival_terminal(inputs),
                repair_fallback=depot_repair_fallback_controls(inputs))


def depot_arrival_terminal(inputs):
    from tools.spatial_oracle.locomotor_track_cursor import OriginalCursor
    idle_inputs = depot_arrival_input_receipts()
    cursor = OriginalCursor('drive')
    turn = cursor.table(0)
    raw = cursor.raw(turn['normal'])
    terminal_cursor = len(raw['points']) - 1
    idle_inputs['terminal_sample'] = dict(
        selector=0, raw_index=turn['normal'], turn_table=turn, raw_track=raw,
        cursor=terminal_cursor, supplied_budget=8,
        original_sample=cursor.sample(0, False, terminal_cursor, 8))
    return dict(native_inputs=idle_inputs, controls=depot_arrival_controls(inputs, idle_inputs))


def depot_source_paths():
    owner = Path('tools/spatial_oracle')
    return {'generator': Path(__file__),
            **{name: owner / f'{name}.py' for name in ('building_construction', 'building_sale',
                                                      'slave_manager', 'refinery_dock', 'harvest_field',
                                                      'track_destination', 'unit_source_scatter',
                                                      'unit_scatter_state', 'unit_entry', 'map_queries',
                                                      'locomotor_track_cursor')},
            'landing_reader': Path('tools/rules_oracle/bridge_landing_inputs.py'),
            'reader_heap': Path('tools/rules_oracle/bridge_anim_lists.py'),
            'ini_index': owner / 'building_body_rules.py',
            'physical_lexical': Path('tools/projectile_oracle/bridge_render_inputs.py'),
            'signed_word': Path('tools/projectile_oracle/guided_step.py'),
            'rng_snapshot': owner / 'shrapnel_repair/shrapnel_repair.py',
            'native_runner': Path('tools/native_oracle.py')}


def depot_provenance():
    result = provenance(
        scope='Stock HTNK/NADEPT repair-depot service with MTNK/GADEPT controls: original selected '
              'constructor/INI/ART reads, UnitType cost/heal virtual getters, Unit→Foot→Techno radio1C, '
              'Building Guard admission and MissionRepairAndProduce, MissionAI timer dispatch, '
              'independent Building Stage, payment/retry, native outside-list exit and radio detach; '
              'additive pending-entry lifecycle controls over original Unit bodies and admitted '
              'MegaMission/Stop/Deploy Event slices, and complete original terminal Drive/PerCell/idle '
              'composition with separately read retail idle prerequisites, plus repair fallback '
              'power/readiness controls over the same original MissionAI owner.',
        entry_points=dict(rules_constructor=0x665650, unit_type_constructor=0x7470D0,
                          building_type_constructor=0x45DD90,
                          building_type_find_or_allocate=0x4653C0, mission_control_constructor=0x4E7CF0,
                          outside_list_initializer=0x45C300, strength_read=0x5F94D3, cost_read=0x71469F,
                          manual_reload_read=0x713343, stupid_guard_read=0x460EA0,
                          unit_repair_read=0x460906, movement_zone_read=0x71605E,
                          repair_percent_read=0x670DA3, repair_step_read=0x670DCA,
                          unit_repair_rate_read=0x670E30, mission_controls_read=0x679C92,
                          foundation_read=0x461225, outside_list_assignment=0x461547,
                          building_stage_constructor=0x43B7F5, get_dock_coordinates=0x447E90,
                          unit_cost=0x711EB0, building_cost=0x45ED50, repair_cost=REPAIR_STEP_COST,
                          unit_heal=UNIT_REPAIR_STEP, unit_radio=UNIT_RECEIVE_RADIO,
                          foot_radio=FOOT_RECEIVE_RADIO, techno_radio=bc.TECHNO_RECEIVE_RADIO,
                          depot_guard=0x4496B0, depot_service=DEPOT_SERVICE,
                          building_ready=0x454250, postdispatch_ready_slice=bc.READY_COMMENCE[0],
                          mission_ai=bc.MISSION_AI, queue_mission=rd.QUEUE, commence=bc.COMMENCE,
                          spend_money=0x4F9790, add_credits=0x4F9950, exit_cell=0x44EFB0,
                          scenario_seed=0x65C6D0,
                          unit_destination_power_gate=0x742F48, house_always_false=0x53A130,
                          raw_current_cell=0x5F6960, drive_power_on=0x55A8F0,
                          drive_is_powered=0x55A930, foot_assign_destination=0x4D94B0,
                          drive_move_to=0x4AFD40,
                          unit_assign_destination=rd.ASSIGN, archive_setter=0x70C610,
                          unit_enter_idle=rd.IDLE, foot_clear_path=0x4DA1C0,
                          foot_process_retained_path=0x4DA030,
                          path_vector_clear=0x4E0190, radio_first_contact=0x65ACB0,
                          radio_broadcast_all=0x65ACE0, techno_assign_target=0x6FCDB0,
                          event_megamission_prefix=0x4C72E8, event_pending_entry_clear=0x4C7353,
                          event_stop_admitted_prefix=0x4C7504, event_stop_class_calls=0x4C75DA,
                          event_deploy_class_calls=0x4C77F0,
                          arrival_weapon_reader=0x71284A, default_to_guard_area_read=0x714F3D,
                          rules_iq_read=0x674240, unit_is_armed=0x701120,
                          unit_current_weapon=0x70E1A0, unit_weapon=0x70E140,
                          type_weapon=0x7177C0, type_turret_count=0x717880,
                          terminal_drive_arm=0x4B1F97, unit_per_cell=0x739EC0,
                          foot_per_cell=0x4D85D0, foot_stop_moving=0x4DF0D0,
                          foot_navigation_gate=0x4DB9B0, drive_power_off=0x55A910,
                          drive_is_moving=0x4AFB80, drive_persist_class_id=0x4B4830,
                          drive_persist_release=0x4B4CC0, persist_query=0x45AEA0,
                          depot_state0_power_fallback=0x44C7E5,
                          depot_state1_need_move_fallback=0x44C5AB,
                          depot_state0_no_contact=0x44C623,
                          depot_state1_no_contact_ready_writer=0x44C18E),
        assumptions=[
            'Physical cached INI index construction is supplied by the shared Landing owner; selected '
            'original readers execute in RULESMD→optional LANGRULE→MPBattleMD→XMP03T4 order. ARTMD is '
            'not layered. Effective Image names equal the two depot IDs; selected native constructors '
            'and original startup outside-list initializer execute, and produced bytes transfer to the scene VM.',
            'Shared refinery_dock map/Drive/Scenario fixture: 32x32 supplied cells, playable width16/height16, '
            'depot NW(6,9), actual Unit/UnitType/Building/BuildingType vtables, in-play actor and depot '
            'membership and contacts supplied. No combat, parasite or live smoke object is present; '
            'the additive terminal section binds an original-read Primary identity solely for IsArmed presence. '
            'House money interface uses its original vtable, empty storage, no silos, cash/Spent at row inputs.',
            'Arrival is supplied at original GetDockCoords, or at original GetCoords plus a declared delta '
            'for Guard controls. Stock NADEPT DockingOffset0=(128,0,0) separates these points. The corpus '
            'does not establish natural Drive/path/Enter arrival satisfying Guard center-distance<64.',
            'Independent Stage constructor slices execute; state2 controls explicitly supply prior Stage '
            'state. MissionAI executes on every history frame, while unchanged quiet snapshots are omitted '
            'only with exact state equality, retained frame/EAX and a hash of every complete native dispatch.',
            'Prerequisite rows execute original DOCK_NOW and the original postdispatch Building Update '
            'ReadyCommence slice. Service histories otherwise begin on the declared mission/status. The '
            'full Building/Techno AI prefix, its C4 visit counter, passive target work, animation producers '
            'and the general Techno Stage are not executed by these mission-dispatch histories.',
            'The restore-seed0 history supplies a mid-progress continuation boundary and executes original '
            'Random::Seed(0), matching the existing Rust snapshot-load policy. Native disk serialization '
            'and full LoadGame are not executed. The Rust consumer performs its actual snapshot roundtrip '
            'then continues against this original-dispatch/reseed history.',
            'Ambient x87 FPCW0x0E7F is inherited from the existing native fixture. Signed repair inputs are '
            'controls over the original getters/receiver, not valid healthy in-play stock objects. IDIV zero '
            'divisors and signed division overflow are outside the callable cost rows. No Rust outputs generate goldens.',
            'Category cost controls reuse existing building_repair.fixture; the same authored type fields '
            'execute under separately supplied actual BuildingType/UnitType category vtables. The old building '
            'payload and provenance remain unchanged. New scene VMs assert original text and vtables unchanged.',
            'Destination controls supply the raw current Cell ground list, UnitRepair/Bunker flags '
            'and a UnitReload-only negative control (actual16AA versus Bunker16AB), '
            'Cell+140 high-bridge bit, deployment and force-reassign prior state. Original Unit741970, '
            'House53A130, raw GetCell5F6960, actual ILoco power slots and the Foot/Drive destination bodies '
            'execute. Ordered actor-first and no-contact controls are separate from physical placement; '
            'no movement Process runs. Native EAX is retained but is not treated as a Boolean setter result.',
            'Thirty pending-entry controls reuse the same scene VM with explicit pending pointer, '
            'contacts/tethers, force/Nav state and a stopped powered Drive. Whole Unit destination/idle '
            'bodies execute, or admitted Event interior slices execute with the saved register/stack '
            'inputs and a 2,000,000 instruction cap. Stop prefix rows use the existing original empty '
            'Foot+5AC vector; narrower class-call rows omit the earlier path resets. Team is NULL. '
            'MegaMission stops before translation/Queue at4C7385, Stop stops before later work at4C762A '
            'or its proven early exit4C8109, and Deploy stops after Queue(Unload16) at4C7818. '
            'Event token decoding, full actor admission, network/input dispatch, Team internals, '
            'later mission work and subsequent FootAI retries are excluded. DockUnload/alive/health '
            'controls are declared raw type/object inputs; the non-Building gate uses an unchanged '
            'original Unit vtable on the supplied contact body without executing its Unit AI.',
            'The lifecycle and additive terminal scenes execute original Foot constructor tube-index '
            '4D31F1..4D31FB and saved-MegaMission4D32EC..4D3308 stores with ESI=actor, '
            'EBX=0 as at4D31EF, and the shared slice stack. They establish tube_index=-1, '
            'saved_megamission=-1, saved target/destination NULL and saved flag0. Lifecycle direct '
            'Unit EnterIdle controls supply exact ordinary args(0,1); arrival args are separately retained. '
            'Raw inputs before/after and '
            'initializer writes are retained. The unchanged inherited raw0 +5C4/+520 states '
            'remain the explicit prestate of every pre-lifecycle section; this addition does '
            'not silently change those prior native comparisons. Nondefault saved-MegaMission '
            'execution and tube callbacks are outside the ordinary lifecycle controls.',
            'The lifecycle trace is read-only and interleaves original function entries/returns, '
            'instruction markers and raw memory writes. Interior PCs4C7342/4C7353 have explicit '
            'register observations rather than fabricated call arguments. Every row retains full '
            'Scenario RNG and asserts its exact equality; no frame or movement advance is supplied. '
            'Original instruction packets establish +500 clear ordering before later conditional '
            'TeamRemove and translated Queue; those later effects do not execute in the Mega rows.',
            'Seven additive arrival controls have their own native-input receipt, preserving every '
            'prior top-level value. The full original TechnoType weapons block71284A..712A8F, '
            'DefaultToGuardArea read714F3D..714F5E, full Rules ReadIQ674240 and layered MissionControl '
            'reader establish HTNK Primary120mm, signed TurretCount0, DefaultToGuardArea0, GuardAreaIQ2 '
            'and committed Enter Zombie0/Paralyzed0. Whole original IsArmed701120 executes in the reader '
            'and terminal scene through the unchanged actual virtual slots. Primary allocation/presence '
            'is bound to the scene; WeaponType data readers, weapon callbacks and firing are excluded.',
            'Four terminal controls begin at original4B1F97, with the exact4B0F20 prologue frame '
            '(F0 locals/four saved registers, caller argument0 and RET4). The live EBP=Drive and ESP, '
            'zeroed frame, budget1, selector0/cursor23/valid1 and pad/head coordinates are supplied. '
            'A separate existing OriginalCursor witness reads TurnTrack0→RawTrack1 and executes '
            'its appended zero terminal point atcursor23 withbudget8, reaching4B1F97 withbudget1. '
            'Prior speed, admission, payment, track selection and travel do not execute in the joined VM. Original '
            'terminal refund/coordinate work, reached test, complete UnitPerCell739EC0(2), DOCK_NOW, '
            'PowerOff55A910, StopMoving4DF0D0 and virtual5044DB9B0 through UnitIdle738970 execute '
            'to the original return. In-play membership, human HouseIQ0, idleGuard0, target and '
            'reciprocal contacts are prepared. The selected route has both tether bytes0; a separate '
            'tethered row controls that input. Nonservice/nonNULL-target terminal rows are prepared '
            'gates; a no-contact control executes only post-Stop virtual504, excluding PerCell path '
            'and zone work for a broken contact. Two post-Stop direct UnitIdle rows supply '
            'queuedSleep/currentEnter/NavNULL and '
            'compare args(0,0)/(0,1), with constructor tube_index=-1 making retained tube work inert.',
            'Arrival observations retain full before/interior/after mission/navigation/power/Drive/Foot '
            'inputs and Scenario RNG, raw writer/caller order and actual returns. The PowerOff stdcall '
            'receiver comes from its first stack argument, never stale ECX. The complete terminal '
            'returns currentEnter7/queuedGuard5 on the selected route; later mission promotion, full '
            'Drive dispatch and natural path reachability are outside this supplied terminal frame.',
            'Twenty-two additive repair_fallback rows reuse original MissionAI and the same supplied '
            'stopped/moving Drive scene. Eighteen linked rows declare status0/1, coordinate deltas '
            '99/100/199/200, power0/1 and Nav/moving state; two of these are state0 Nav+moving '
            'NEED_MOVE-rejection controls atdelta99. Four no-contact rows declare status0/1 and '
            'ready0/1. Readiness is written as input only when the new case declares its key; old '
            'case initialization and results are preserved. The fallback-only observer retains '
            'actual distance/NEED_MOVE returns, IPersist class ID/Release, ILoco PowerOn/Off, '
            'IsPowered/IsMoving, House gate, raw readiness/power/queue/status/timer writes and '
            'full before/interior/after Scenario RNG. None of those original bodies are substituted.',
            'The supplied actual Drive class ID follows the state0 default threshold100: delta99 '
            'admits status1/delay3, delta100 falls back with PowerOn/delay71. Nominal delta199 '
            'measures198 through original447E00; its result is retained rather than replaced with '
            'hand arithmetic. State0 radio rejection returns10, skips the distance getter and '
            'takes the same unconditional PowerOn arm. State1 atnative distance200 avoids '
            'StopMoving, gets NEED_MOVE10 and powers on only an unpowered contact. No-contact '
            'state0 preserves ready and Queue(Guard5,1) promotes only when already ready; state1 '
            'writes ready1 before the same Queue. Existing raw Foot constructor fields, empty '
            'slot objects and presentation boundaries remain explicitly supplied in these controls.',
        ],
        substitutions=[
            'Inherited source map fixture supplies scene/path reachability and bare field recalculation. '
            'Presentation calls (Building slot/play/destroy animation, EVA and building_construction presentation '
            'callbacks) are observed service sinks. Repair math, radio, wallet, idle/Guard, Stage, timer dispatch, '
            'queue/destination/archive, original exit search and radio contact removal are not substituted.',
            'Reader VM inherits Landing allocator/delete/TLS/Interlocked services. It runs selected original '
            'read/store slices with explicitly supplied receiver/register/live-stack prerequisites, resetting '
            'ESP between slices where original following-reader arguments remain pushed.',
        ])
    from tools.native_oracle import file_span, image_bytes
    native = image_bytes()
    result['pending_entry_lifecycle'] = dict(
        row_count=len(depot_pending_entry_cases()), instruction_limit=2_000_000,
        foot_constructor_inputs=dict(
            original_constructor='0x004D31E0', executed_full_constructor=False,
            default_idle_args=[0, 1],
            slices={name: dict(start=hex(start), end_exclusive=hex(end),
                               registers=dict(esi='selected actor', ebx=0, ebp=0,
                                              esp=hex(bc.SP), zero_stack_bytes=0x80),
                               original_bytes=file_span(native, start, end - start)[1].hex())
                    for name, (start, end) in DEPOT_FOOT_INPUT_INITIALIZERS.items()},
            preserved_old_sections='inherited raw saved_megamission0/tube_index0',
            nondefault_saved_megamission_boundary='Existing inherited raw0 witnesses are valid bounded calls; the retained-MegaMission producer/resume mechanism is excluded'),
        event_slices={name: dict(start=hex(start), end_exclusive=hex(end),
                                size=end - start,
                                original_bytes_sha256=hashlib.sha256(
                                    file_span(native, start, end - start)[1]).hexdigest())
                      for name, (start, end) in DEPOT_EVENT_SLICES.items()},
        instruction_order=dict(
            evidence='original instructions; constructor and post-Mega-prefix effects are not executed here',
            packets={name: dict(pc=hex(pc), original_bytes=file_span(native, pc, size)[1].hex())
                     for name, pc, size in (
                         ('constructor_pending_zero', 0x6F310C, 6),
                         ('megamission_pending_clear', 0x4C7353, 10),
                         ('megamission_team_remove_call', 0x4C7380, 5),
                         ('megamission_translate_call', 0x4C73AA, 6),
                         ('megamission_queue_call', 0x4C73B9, 6),
                         ('stop_broadcast_call', 0x4C75E0, 6),
                         ('stop_destination_call', 0x4C75ED, 6),
                         ('stop_target_call', 0x4C75F8, 6))}))
    result['arrival_terminal'] = dict(
        row_count=len(depot_arrival_cases()), complete_terminal_rows=4,
        direct_navigation_gate_rows=1, direct_unit_idle_rows=2,
        instruction_limit=2_000_000, original_process_reference='0x004B0F20',
        executed_terminal_start='0x004B1F97', executed_end='original RET4 to fixture sentinel',
        selected_contact_inputs=dict(reciprocal=True, unit_tether=0, building_tether=0),
        reader_slices={name: dict(start=hex(start), end_exclusive=hex(end),
                                  original_bytes_sha256=hashlib.sha256(
                                      file_span(native, start, end - start)[1]).hexdigest())
                       for name, start, end in (
                           ('type_weapons', 0x71284A, 0x712A8F),
                           ('default_to_guard_area', 0x714F3D, 0x714F5E),
                           ('mission_controls', 0x679C92, 0x679CAF))},
        whole_rules_iq_entry='0x00674240',
        reader_register_inputs=dict(
            type_weapons=dict(ebp='type', ebx='type+0x24', esi='INI', edi='INI',
                              eax='current Type+0xD22 byte', esp='shared reader SP'),
            default_to_guard_area=dict(ebp='type', ebx='type+0x24', edi='INI', esp='shared reader SP'),
            mission_controls=dict(esi='INI', esp='shared reader SP')),
        terminal_sample_owner='tools/spatial_oracle/locomotor_track_cursor.py::OriginalCursor',
        terminal_sample_bound=dict(selector=0, raw_index=1, cursor=23,
                                   supplied_budget=8, post_admission_budget=1,
                                   original_sample_instruction_limit=300),
        foot_inputs_owner='depot_initialize_foot_inputs',
        instruction_windows={name: dict(pc=hex(pc), bytes=16,
                                        original_bytes=file_span(native, pc, 16)[1].hex())
                             for name, pc in (
                                 ('type_turret_count_constructor', 0x71136F),
                                 ('type_default_to_guard_area_constructor', 0x71153C),
                                 ('terminal_reached_store', 0x4B21B1),
                                 ('terminal_per_cell_call', 0x4B220F),
                                 ('terminal_stop_moving_call', 0x4B2242),
                                 ('terminal_navigation_gate_call', 0x4B228B))},
        limits=['Physical approach, speed, prior terminal admission and complete Process_Track dispatch '
                'are supplied; only the original terminal arm and its complete downstream class calls execute.',
                'The separate raw1 sample witness establishes the selector/cursor/budget prestate, '
                'without executing travel or occupation callbacks for the earlier real points.',
                'No-contact control executes only post-Stop Foot virtual504; broken-contact PerCell '
                'path/zone work is excluded from this sparse scene.',
                'Direct UnitIdle args00/01 are prepared post-Stop controls, with original constructor '
                'TubeIndex=-1 and savedMegaMission=-1; retained tube/saved-order execution is excluded.',
                'The full terminal returns committed Enter7 and queuedGuard5; subsequent mission '
                'promotion and full-frame object scheduling are outside this caller boundary.'])
    result['repair_fallback'] = dict(
        row_count=len(depot_repair_fallback_cases()), linked_rows=18, no_contact_rows=4,
        seam='original MissionAI5B3060 -> MissionRepairAndProduce44B780',
        instruction_limit=2_000_000, default_frame=200,
        input_readiness='Building+6DD is explicitly supplied from the new ready key only; older cases unchanged',
        scope_observations='Only repair_fallback VMs receive extra read-only call/write observers',
        original_slices={name: dict(start=hex(start), end_exclusive=hex(end),
                                    original_bytes=file_span(native, start, end - start)[1].hex())
                         for name, start, end in (
                             ('state0_fallback_power_on', 0x44C7E5, 0x44C819),
                             ('state1_need_move_fallback', 0x44C5AB, 0x44C623),
                             ('state0_no_contact_queue', 0x44C623, 0x44C6FD),
                             ('state1_no_contact_ready_queue', 0x44C17D, 0x44C1A8),
                             ('state1_distance_and_power_admission', 0x44C1B9, 0x44C2A6))},
        state0_class_comparison_constants={hex(address): file_span(native, address, 16)[1].hex()
                                           for address in (0x7E9A40, 0x7E9AB0)},
        limits=['The scene supplies contact membership, stopped/moving Drive and exact coordinate offsets; '
                'prior travel, Guard admission and command production do not execute.',
                'Empty native animation slot fields are supplied; presentation constructor/audio '
                'services retain the existing owner boundaries.',
                'Nominal coordinate delta and original measured distance are distinct; delta199 '
                'executes as distance198 under the retained x87 state.',
                'Readiness controls execute original Queue(Guard5,1) and its immediate promotion '
                'through the existing queue/commence owners; later object visits are excluded.'])
    return result


def generate():
    return {'source': 'unicorn/gamemd.exe',
            # The Rules and House doubles the rows write, as bits.
            'constants': {name: f'{bits:016x}' for name, bits in (
                ('percent_15', PERCENT_15), ('percent_25', PERCENT_25), ('rate_016', RATE_016),
                ('delay_02', DELAY_02), ('delay_05', DELAY_05), ('delay_02_default', DELAY_02_DEFAULT))},
            'cost': [cost(case) for case in cost_cases()],
            'toggle': [toggle(case) for case in toggle_cases()],
            'update': [update(case) for case in update_cases()],
            'build': [build(case) for case in build_cases()],
            'wrench': [wrench(case) for case in wrench_cases()],
            'release': [release(case) for case in release_cases()]}


def main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    if '--depot-waiters' in argv:
        argv.remove('--depot-waiters')
        finish_vectors(generate_depot_waiters,
                       Path(__file__).with_suffix('.depot_waiters.json'),
                       provenance=depot_waiters_provenance,
                       source_paths={**depot_source_paths(),
                                     'spatial_startup': Path('tools/spatial_oracle/mapgen_range.py'),
                                     'direction_startup_reference': Path('tools/spatial_oracle/track_fresh_response.py'),
                                     'path_delay_reader_reference': Path('tools/spatial_oracle/path_delay_rules.py'),
                                     'navigation_storage': Path('tools/spatial_oracle/anytown_damage/navigation.py')},
                       argv=argv)
        return
    if '--depot-service' in argv:
        argv.remove('--depot-service')
        finish_vectors(generate_depot_service,
                       Path(__file__).with_suffix('.depot_service.json'),
                       provenance=depot_provenance, source_paths=depot_source_paths(), argv=argv)
        return
    finish_vectors(
        generate, Path(__file__).with_suffix('.json'),
        provenance=lambda: provenance(
            scope='BuildingTypeClass repair step cost 0x7120D0 with GetCost 0x45ED50; BuildingClass::'
                  'ToggleRepair 0x446FF0; BuildingClass::UpdateRepairAndPower 0x450630 from entry to return '
                  '(admission, the computer\'s auto-repair start and latch timer, the repair tick), alone and '
                  'after BuildingClass::Update\'s construction pieces through a build-up; '
                  'HouseClass::Update\'s latch release 0x4F9302..0x4F9338; TechnoClass::DrawExtras\' repair '
                  'wrench frame 0x6F52D8..0x6F532D',
            entry_points={'repair_step_cost': REPAIR_STEP_COST, 'toggle_repair': TOGGLE_REPAIR,
                          'update_repair_and_power': UPDATE_REPAIR_AND_POWER,
                          'house_release': HOUSE_RELEASE[0], 'enter_construction': bc.ENTER_CONSTRUCTION,
                          'commence': bc.COMMENCE, 'queue_mission': bc.QUEUE_MISSION,
                          'receive_radio': bc.RECEIVE_RADIO, 'update_animation': bc.UPDATE_ANIMATION,
                          'mission_ai': bc.MISSION_AI, 'update_ready_commence_unless_building': 0x43FE27,
                          'update_ready_commence': 0x43FF91, 'update_queued_bstate': 0x43FFB4,
                          'draw_extras_wrench_frame': WRENCH_FRAME[0]},
            assumptions=['the slave_manager fixture refinery (Building vtables over a BuildingType-vtable '
                         'type, 2x2 at NW (12, 12)) owned by the fixture House; multiplayer game mode unless '
                         '`game_mode` is 0; PlayerPtr the owner only when `player`; the Scenario RNG seeded '
                         'through the original seeder',
                         'Rules: RepairStep 8, RepairPercent 15% (0x3FC3333333333333) unless the row sets it, '
                         'RepairRate .016f widened, ConditionYellow .5, ConditionRed .25, [IQ] RepairSell 1 and '
                         'SellBack 2, [AI] CreditReserve 100 unless the row sets it; the PadAircraft= vector holds '
                         'two AircraftType-vtable types (Cost 1000/1200 unless the row sets them) whose first\'s '
                         'first Dock= is this type only when `pad_dock`; SeparateAircraft= set unless the row '
                         'clears it; a FreeUnit= is a UnitType-vtable type',
                         'update rows: the owner\'s money interface House+0x24 holds the constructor\'s vtable '
                         '0x7EA834, its storage empty (Available_Money = Balance); authored IQ 2, TechLevel 10; '
                         'the latch timer\'s unread +0x284 word preset; damage-state slots 0..3 as SLOT_OCCUPANTS/'
                         'SLOT_NAMES name them; the smoke (+0x310) a ParticleSystemClass-vtable object',
                         'build rows: the update rows\' state with no mission, then building_construction\'s route '
                         'creation (the control at Type+0xF04, the TechnoClass constructor stage state, BState and '
                         'queued BState -1, +0x90 and +0x6E9 set) at the row\'s start frame'],
            substitutions=['toggle and update rows: the flash (vt+0x148 = 0x456E00), EVA 0x752700 and '
                           'VocClass::PlayAt 0x7509E0 observed and answered',
                           'update rows: BuildingClass::GetCurrentFrame 0x43EF90 answered 0 (only the redraw '
                           'byte +0x80 reads it), the damage-state slot anim 0x451890 observed and answered '
                           '(ToggleRepair, Health_Ratio, Spend_Money, Available_Money and the smoke\'s vt+0xF8 '
                           'native), Sell_Back 0x447110 observed and answered',
                           'build rows: UpdateRepairAndPower invoked after the queued-BState block (0x440042), '
                           'the rest of BuildingClass::Update before 0x4401B6 and of TechnoClass::AI not run; '
                           'the route rows\' substitutions (UpdateAnimation presentation callees, the radio '
                           'broadcasts 0x65ACB0/0x65ACE0, Grand_Opening 0x445F80, the loop update 0x750D40, '
                           'SoundEvent::Release 0x406060, TechnoClass::Receive_Radio 0x6F4AB0 answered 1); '
                           'GetCurrentFrame answered only inside UpdateRepairAndPower',
                           'release rows: the block run alone with ESI = the House',
                           'wrench rows: the block run alone (SpeedNormalize 0x5FB2E0 native)']),
        argv=argv)


if __name__ == '__main__':
    main()
