"""Original building sale: Sell_Back and BuildingClass::Sell for a building
that does not undeploy.

- `route` rows: a sale of an idle building from the SELL event
  (`BuildingClass::Sell_Back(-1)` 0x447110) through BuildingClass::Update's
  construction pieces (building_construction's `route`) until the stage-2
  visit that finds +0x6DD (0x449CA7, where the sale completes). Rows cover
  the construction controls, a building without a Buildup SHP (+0x6E9 clear:
  Sell_Back acts only on a FirestormWall type), a second Sell_Back while
  Selling, the local player's click (VocClass::PlayAtPos 0x750920) and stage-1
  sounds, and a tethered building (+0x418) whose stage 1 waits while its
  OVER_OUT broadcast leaves the tether set. The survivor count (0x451330) is
  answered 0 and the occupy list empty; the `crew` rows run them.
- `crew` rows: Sell's stage 1 (0x44A2EE..0x44A8DE) natively on the
  slave_manager fixture's refinery (the harvest_field map, House, Rules and
  Scenario RNG), entered through BuildingClass::Sell with +0xBC = 1: the
  survivor count How_Many_Survivors (0x451330) with the refund
  (TechnoTypeClass::GetRefund 0x711F60) over the row's cost, side and owner;
  the occupy list (vt+0x108: BuildingType+0xDFC into the table the static
  initialiser 0x45B1C0 builds); absorbed passengers (+0x114) out through
  InfantryClass::Unlimbo (a passenger past the list's end reads its 0x7FFF
  terminator and places in MapClass's off-map cell 0xABDC50, constructed as
  the static initialiser 0x565060 does); per survivor Crew_Type (0x44EB10, TechnoClass::GetCrew
  0x707D20) with the Engineer-once re-roll, the InfantryClass constructor
  (0x517A50: a prepared object, the TechnoClass constructor's one Scenario
  draw (0x6F3254 -> 0x65C780) taken through a native trampoline), the
  RandomRanged(0, n - 1) cell pick, PlaceInfantryInCell (0x481180),
  InfantryClass::Unlimbo (0x51DFF0, its FootClass::Unlimbo answered after
  writing the placement), Scatter (0x51D0D0) with the Infantry setter
  (0x51AA40), Walk's MoveTo and the immediate first Process (0x51D478: the
  head, FindSubCellDest 0x75C240 and its raw-occupation leaves) native, and
  Queue_Mission(Move); then the sale's sounds (VocClass::PlayAt 0x7509E0,
  observed) and Begin_Mode(0). The building's occupy-list cells carry its
  AddContent mark (0x80), and the step table (0x49F3A0) and the Infantry
  startup heights run as the static initialisers leave them. Answered: the
  OVER_OUT broadcast (0x65ACE0), the building's IsArmed (vt+0x2AC =
  0x458DB0), FootClass::Find_Path (0x4D3920: the one-step route to the
  neighbouring Scatter cell), Select (vt+0x14C) and the infantry deletes.
- `refund` rows: the sale's credit, TechnoClass vt+0x2BC (0x70ADA0 ->
  TechnoTypeClass::GetRefund 0x711F60), over cost, owner and game mode.
- `ai_sale` rows: the computer's low-credit sale, BuildingClass::
  UpdateRepairAndPower (0x450630) natively on the same refinery from its entry
  to the repair tick (0x450813) or the computer's auto-repair start
  (0x4506B2): CurrentIQ (House+0x24C) against [IQ] RepairSell, Get_Mission
  (vt+0x184) on Construction or Selling, Can_Repair (vt+0x94 = 0x452630, with
  the 1x1 UndeploysInto test 0x465D40 and TechnoClass 0x701140), the owner's
  Available_Money (House+0x24 vt+0x18 = 0x4F6990) against [AI] CreditReserve,
  the campaign's AI sale byte (+0x6DC), WasAttackedByEnemy (+0x3D1), the
  authored IQ (House+0x1D0, unsigned) against SellBack, RandomRanged(0, 50)
  on the Scenario RNG against TechLevel (House+0x1D4, unsigned), the
  AttachedTag (+0x34), Factory=BuildingType and Health_Ratio (0x5F5C60)
  against ConditionRed. Sell_Back (0x447110) is observed and answered.

Usage: python -m tools.spatial_oracle.building_sale [--check|--write]
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP
from tools.native_oracle import RET_MAGIC, finish_vectors, provenance, run_checked
from tools.spatial_oracle import building_construction as bc
from tools.spatial_oracle import infantry_entry_raw as entry_raw
from tools.spatial_oracle import slave_manager as sm
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.refinery_dock import HOUSE, HTYPE, RULES, cell_xy
from tools.spatial_oracle.unit_source_scatter import SCENARIO

SELL, SELL_BACK = 0x449C30, 0x447110
SELL_CONVERTS = 0x449CA7
RANDOM = 0x65C780
INFANTRY_CTOR, NEW_CALL, NEW_RETURN = 0x517A50, 0x44A635, 0x44A63A
BUILDING_IS_ARMED, TECHNO_SELECT = 0x458DB0, 0x6FBFA0
INFANTRY_DELETE, INFANTRY_KILL_CREDIT = 0x523350, 0x702D40
BROADCAST_ALL, PLAY_AT, PLAY_AT_POS = 0x65ACE0, 0x7509E0, 0x750920
PLACE_INFANTRY, UNLIMBO, FOOT_UNLIMBO, SCATTER = 0x481180, 0x51DFF0, 0x4D7170, 0x51D0D0
# InfantryClass::Scatter's Find_Nearby_Passable_Cell call and return.
FNPC, SCATTER_FNPC_CALL, SCATTER_FNPC_RETURN = 0x56DC20, 0x51D41D, 0x51D422
SETTER, LIMBO, PLAY_ANIM, QUEUE = 0x51AA40, 0x51DF10, 0x51D6F0, 0x5B35E0
# FootClass::Find_Path, which Walk's first Process (Scatter's 0x51D478) calls,
# and the static initialiser of the eight step offsets (0x89F6D8) the Process
# adds to the coordinate for the route's next cell (0x75B5AC).
FIND_PATH, STEP_OFFSETS_INIT = 0x4D3920, 0x49F3A0
# FacingType order of a route step: N, NE, E, SE, S, SW, W, NW.
STEP_FACINGS = [(0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1)]
REFUND = 0x70ADA0
OCCUPY_INIT, OCCUPY_LISTS, OCCUPY_STRIDE = 0x45B1C0, 0x89C900, 120
# MapClass's cell for coordinates off the map (0x565730 returns it), built by
# the static initialiser 0x565060 through CellClass's constructor.
DUMMY_CELL, CELL_CTOR = 0xABDC50, 0x47BBF0
PLAYER_PTR = 0xA83D4C
# Scratch after the slave_manager fixture's region: crew types, the
# constructor trampoline and its slot pointer, prepared infantry objects.
REGION = sm.REGION + 0x40000
CREW_TYPES = REGION
TYPE_SIZE = 0x1000
TRAMPOLINE, SLOT_PTR = REGION + 0x7F00, REGION + 0x7FF0
TRAMPOLINE_DRAW = TRAMPOLINE + 16
PAD = REGION + 0x8000
SLOTS = REGION + 0x10000
SLOT_SIZE = 0x2000  # the object, then its Walk locomotor at +0x1000
CREW_SLOTS, PASSENGER_SLOTS = 8, 5
CREW_NAMES = ['E1', 'E2', 'INIT', 'CTECH', 'ENGINEER']
# Rules crew slots: AlliedCrew, SovietCrew, ThirdCrew, Technician, Engineer.
RULES_CREW = {'E1': 0xF78, 'E2': 0xF7C, 'INIT': 0xF80, 'CTECH': 0xF6C, 'ENGINEER': 0xF70}
# Retail [General] Allied/Soviet/ThirdSurvivorDivisor.
DIVISORS = (500, 250, 750)
SELL_SOUND, GENERIC_CLICK = 41, 42
MISSION = {'move': 2, 'guard': 5, 'hunt': 15, 'construction': 18, 'selling': 19, 'none': -1}
# BuildingClass::UpdateRepairAndPower, its two exits from the admission (the
# computer's auto-repair start and the repair tick), RandomRanged and the
# sale roll's return, and the House money interface vtable.
UPDATE_REPAIR_AND_POWER = 0x450630
AI_SALE_ENDS = {0x4506B2: 'auto_repair_start', 0x450813: 'repair_tick'}
RANDOM_RANGED, SALE_ROLL_RETURN = 0x65C7E0, 0x4507C9
HOUSE_MONEY_VTABLE = 0x7EA834
# Any non-null AttachedTag (+0x34): the admission only tests it.
AI_SALE_TAG = 0x7FFF0000


def crew_type_address(name):
    return CREW_TYPES + CREW_NAMES.index(name) * TYPE_SIZE


def crew_name(pointer):
    return next((name for name in CREW_NAMES if crew_type_address(name) == pointer), hex(pointer))


def slot_address(index):
    return SLOTS + index * SLOT_SIZE


def slot_name(pointer):
    index = (pointer - SLOTS) // SLOT_SIZE
    if pointer and 0 <= index < CREW_SLOTS + PASSENGER_SLOTS and (pointer - SLOTS) % SLOT_SIZE == 0:
        return f'crew{index}' if index < CREW_SLOTS else f'passenger{index - CREW_SLOTS}'
    return {sm.YAREFN: 'building', 0: None}.get(pointer, hex(pointer))


def signed(u, address):
    return struct.unpack('<i', u.mem_read(address, 4))[0]


def ret(u, read32, cleanup, value=0):
    sp = u.reg_read(UC_X86_REG_ESP)
    u.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
    u.reg_write(UC_X86_REG_EIP, read32(sp))
    u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)


def coord(u, pointer):
    return list(struct.unpack('<3i', u.mem_read(pointer, 12)))


# --- route ---------------------------------------------------------------


def route(case):
    """An idle building's sale from Sell_Back(-1) through BuildingClass::Update's
    construction pieces (building_construction.route's order), per frame: BState,
    stage, +0x6DD, mission, queue, Sell's status, and the observed calls."""
    u, read32, building, frame, calls = bc.building_fixture(dict(case, undeploys=False))
    stop = {'converts': False}

    def hook(_u, address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if address == BROADCAST_ALL:
            calls.append(['radio', read32(sp + 4)])
            ret(u, read32, 4)
        elif address == bc.IS_HUMAN_PLAYER:
            ret(u, read32, 0, case.get('player', False))
        elif address == bc.SURVIVOR_COUNT:
            calls.append(['survivors'])
            ret(u, read32, 0, 0)
        elif address == bc.OCCUPY_LIST:
            u.mem_write(RET_MAGIC + 0x800, bytes([0xFF, 0x7F, 0xFF, 0x7F]))
            ret(u, read32, 4, RET_MAGIC + 0x800)
        elif address == PLAY_AT_POS:
            calls.append(['play_global', u.reg_read(UC_X86_REG_ECX), u.reg_read(UC_X86_REG_EDX)])
            ret(u, read32, 8)
        elif address == SELL_CONVERTS:
            stop['converts'] = True
            u.emu_stop()

    u.hook_add(UC_HOOK_CODE, hook)
    u.mem_write(bc.SCENARIO_INIT, dwords(0))
    u.mem_write(bc.SCENARIO_FLAG_ED6B, bytes([0]))
    u.mem_write(building + 0x90, bytes([1]))
    # +0x6E9: BuildingClass::Init_Managers sets it for a type with a Buildup
    # SHP (0x442CCF).
    u.mem_write(building + 0x6E9, bytes([case.get('buildup', True)]))
    u.mem_write(building + 0x538, dwords(-1))
    u.mem_write(building + 0xC8, dwords(frame, 0, 0))
    # [AudioVisual] SellSound/GenericClick (sound indices) and the type's
    # PackupSound (the BuildingType constructor's -1 unless the row sets one).
    u.mem_write(RULES + 0x6A4, dwords(SELL_SOUND))
    u.mem_write(RULES + 0x70C, dwords(GENERIC_CLICK))
    u.mem_write(sm.YTYPE + 0xE70, dwords(case.get('packup_sound', -1)))
    # ObjectClass::AI's two positional sounds (Type+0x1F4 and +0x64), which
    # the constructors leave at -1.
    u.mem_write(sm.YTYPE + 0x1F4, dwords(-1))
    u.mem_write(building + 0x64, dwords(-1))
    # An idle building: BState 1 (the idle control), Guard current.
    bc.invoke(u, bc.BEGIN_MODE, building, 1)
    start = len(calls)
    bc.invoke(u, SELL_BACK, building, 0xFFFFFFFF)
    order = dict(calls=calls[start:], mission=signed(u, building + 0xAC), queue=signed(u, building + 0xB4),
                 status=signed(u, building + 0xBC))
    tether_until = case.get('tether_until')
    if tether_until is not None:
        u.mem_write(building + 0x418, b'\x01')
    frames = []
    for k in range(1, case['frames'] + 1):
        u.mem_write(bc.FRAME, dwords(frame + k))
        before = len(calls)
        if k == tether_until:
            u.mem_write(building + 0x418, b'\x00')
        if k == case.get('sell_again_at'):
            calls.append(['sell_back'])
            bc.invoke(u, SELL_BACK, building, 0xFFFFFFFF)
        try:
            bc.building_update(u, building)
        except Exception:
            if not stop['converts']:
                raise
        frames.append(dict(frame=k, bstate=signed(u, building + 0x534), queued_bstate=signed(u, building + 0x538),
                           stage=signed(u, building + 0xF8), done=u.mem_read(building + 0x6DD, 1)[0],
                           mission=signed(u, building + 0xAC), queue=signed(u, building + 0xB4),
                           status=signed(u, building + 0xBC), converts=stop['converts'], calls=calls[before:]))
        if stop['converts']:
            break
    return dict(input=case, order=order, frames=frames)


def route_cases():
    cases = []
    for control, frames in (([0, 3, 2], 12), ([0, 4, 1], 10), ([0, 1, 0], 6), ([0, 26, 2], 60),
                            ([0, 25, 2], 60), ([0, 17, 3], 60)):
        cases.append(dict(name=f'sale_{control[1]}x{control[2]}', control=control, frames=frames,
                          mission='guard'))
    return cases + [
        # The local player: the click at the order, SellSound and PackupSound at
        # stage 1.
        dict(name='sale_player_sounds', control=[0, 3, 2], frames=12, mission='guard', player=True,
             packup_sound=7),
        # No Buildup SHP (+0x6E9 clear): Sell_Back does nothing.
        dict(name='sale_without_buildup', control=[0, 1, 0], frames=4, mission='guard', buildup=False,
             player=True),
        # A second order while Selling: only the click.
        dict(name='sale_ordered_twice', control=[0, 4, 1], frames=10, mission='guard', player=True,
             sell_again_at=2),
        # A tether (+0x418) holds stage 1 until it clears.
        dict(name='sale_tethered', control=[0, 3, 2], frames=16, mission='guard', tether_until=5),
    ]


# --- crew ----------------------------------------------------------------


def trampoline():
    """The TechnoClass constructor's tail (0x6F3249..0x6F3266) for a prepared
    object: its Scenario draw into +0x3C8, then `RET 8` with the object."""
    after_call = TRAMPOLINE_DRAW
    return (bytes([0xA1]) + dwords(0xA8B230)                           # mov eax, [0xA8B230]
            + bytes([0x8D, 0x88]) + dwords(0x218)                      # lea ecx, [eax + 0x218]
            + bytes([0xE8]) + struct.pack('<i', RANDOM - after_call)   # call 0x65C780
            + bytes([0x8B, 0x0D]) + dwords(SLOT_PTR)                   # mov ecx, [SLOT_PTR]
            + bytes([0x66, 0x89, 0x81]) + dwords(0x3C8)                # mov [ecx + 0x3C8], ax
            + bytes([0x89, 0xC8])                                      # mov eax, ecx
            + bytes([0xC2, 0x08, 0x00]))                               # ret 8


def crew_fixture(case):
    u, call, read32, events = sm.make_fixture(dict(
        name=case['name'], manager_state=0, nodes=[], ore=[], seed=case.get('seed', 1),
        human=case.get('human', True), game_mode=case.get('game_mode', 1), passable=case.get('passable', [])))
    u.mem_map(REGION, 0x10000 + SLOT_SIZE * (CREW_SLOTS + PASSENGER_SLOTS))
    building, kind = sm.YAREFN, sm.YTYPE
    # The per-Foundation occupy lists, as the static initialiser leaves them,
    # and the lazy statics of 0x5F5B90/0x45EC20 already constructed (their
    # first call registers an atexit destructor the fixture cannot run).
    call(OCCUPY_INIT, 0, [])
    call(STEP_OFFSETS_INIT, 0, [])
    # The Infantry startup initialisers (CRT table 0x813490): the level height
    # (0xA8F240) and the bridge-deck threshold (0xA8F234) the raw-occupation
    # leaves 0x5217C0/0x521850 compare against.
    for entry in entry_raw.STARTUP:
        call(entry, 0, [])
    assert read32(0xA8F234) == 416
    call(CELL_CTOR, DUMMY_CELL, [])
    for flag, value in ((0xAC1398, 0xAC139C), (0x89C890, 0x89C8E8)):
        u.mem_write(flag, bytes([u.mem_read(flag, 1)[0] | 1]))
        u.mem_write(value, dwords(0x7FFF7FFF))
    foundation = case.get('foundation', 3)
    u.mem_write(kind + 0xEF0, dwords(foundation))
    u.mem_write(kind + 0xDFC, dwords(OCCUPY_LISTS + foundation * OCCUPY_STRIDE))
    # Place_Down's AddContent (0x47E8A0) lists the building in each cell of
    # its occupy list and marks the cell through vt+0xF0 (0x453D60:
    # CellClass+0x124 |= 0x80), which Scatter's Find_Nearby_Passable_Cell reads.
    for dx, dy in occupy_offsets(read32, foundation):
        at = sm.cell(sm.YAREFN_NW[0] + dx, sm.YAREFN_NW[1] + dy)
        u.mem_write(at + 0xE4, dwords(building))
        u.mem_write(at + 0x124, dwords(read32(at + 0x124) | 0x80))
    # Sell's stage 1 with Selling current and nothing queued.
    u.mem_write(building + 0xAC, dwords(MISSION['selling']))
    u.mem_write(building + 0xB4, dwords(-1))
    u.mem_write(building + 0xBC, dwords(1))
    u.mem_write(building + 0x83, bytes([case.get('selected', False)]))
    u.mem_write(building + 0x6E0, bytes([case.get('no_survivor', False)]))
    u.mem_write(building + 0x6E3, bytes([case.get('captured', False)]))
    u.mem_write(building + 0x90, bytes([1]))
    # Crewed=, Factory=, PackupSound= and the absorb flags.
    u.mem_write(kind + 0xCCD, bytes([case.get('crewed', True)]))
    u.mem_write(kind + 0xEB8, dwords(7 if case.get('yard') else -1))
    u.mem_write(kind + 0xE70, dwords(case.get('packup_sound', -1)))
    u.mem_write(kind + 0x16AF, bytes([bool(case.get('passengers'))]))
    # GetRefund's inputs (repair_refund's fixture): Cost, RefundPercent .5,
    # the country and house cost multipliers 1.0, the aircraft pad lists.
    u.mem_write(kind + 0x610, dwords(case['cost']))
    u.mem_write(RULES + 0x1738, struct.pack('<d', 0.5))
    u.mem_write(HTYPE + 276, struct.pack('<5f', 1, 1, 1, 1, 1))
    u.mem_write(HOUSE + 21392, struct.pack('<5f', 1, 1, 1, 1, 1))
    u.mem_write(HOUSE + 0x1ED, bytes([case.get('player_control', False)]))
    u.mem_write(RULES + 2908, dwords(PAD))
    u.mem_write(RULES + 6120, b'\x01')
    u.mem_write(PAD, dwords(PAD + 256, PAD + 256))
    u.mem_write(PAD + 256 + 1004, dwords(PAD + 0x800))
    # The owner's side (House+0x1E8) and its country's Side (HouseType+0xBC).
    u.mem_write(HOUSE + 0x1E8, dwords(case.get('side', 0)))
    u.mem_write(HTYPE + 0xBC, dwords(case.get('country_side', case.get('side', 0))))
    u.mem_write(RULES + 0x14F8, dwords(*case.get('divisors', DIVISORS)))
    # The crew types (InfantryType vtable, Strength, MovementZone Infantry;
    # ENGINEER's Engineer= +0xEC3) and the Rules pointers to them.
    for name in CREW_NAMES:
        address = crew_type_address(name)
        u.mem_write(address, dwords(0x7EB610))
        u.mem_write(address + 0xA0, dwords(125))
        u.mem_write(address + 0x5B4, dwords(7))
        u.mem_write(address + 0xEC3, bytes([name == 'ENGINEER']))
        u.mem_write(RULES + RULES_CREW[name], dwords(address))
    u.mem_write(RULES + 0x6A4, dwords(SELL_SOUND))
    # IsHumanPlayer (0x50B6F0) in a multiplayer game: the owner is PlayerPtr.
    u.mem_write(PLAYER_PTR, dwords(HOUSE if case.get('player', True) else 0))
    # Prepared infantry: every slot a limbo object with a constructed Walk.
    for index in range(CREW_SLOTS + PASSENGER_SLOTS):
        prepare_infantry(u, call, slot_address(index))
    passengers = case.get('passengers', [])
    for index, name in enumerate(passengers):
        slot = slot_address(CREW_SLOTS + index)
        u.mem_write(slot + 0x6C0, dwords(crew_type_address(name)))
        u.mem_write(slot + 0x21C, dwords(HOUSE))
        following = slot_address(CREW_SLOTS + index + 1) if index + 1 < len(passengers) else 0
        u.mem_write(slot + 0x30, dwords(following))
    u.mem_write(building + 0x114, dwords(len(passengers), slot_address(CREW_SLOTS) if passengers else 0))
    u.mem_write(TRAMPOLINE, trampoline())
    u.mem_write(bc.SCENARIO_INIT, dwords(0))
    return u, call, read32, events


def occupy_offsets(read32, foundation):
    """The occupy list the static initialiser 0x45B1C0 builds for
    `foundation`: CellStruct offsets up to the 0x7FFF7FFF terminator."""
    offsets, address = [], OCCUPY_LISTS + foundation * OCCUPY_STRIDE
    while (value := read32(address)) != 0x7FFF7FFF:
        offsets.append(tuple(struct.unpack('<hh', struct.pack('<I', value))))
        address += 4
    return offsets


def prepare_infantry(u, call, slot):
    """An InfantryClass as the constructor leaves it, in limbo (slave_manager's
    make_slave): the four vtables, no mission, Doing -1, a constructed Walk."""
    loco = slot + 0x1000
    u.mem_write(slot, dwords(0x7EB058, 0x7EB03C, 0x7EB034, 0x7EB02C))
    u.mem_write(slot + 0x14, dwords(5))
    u.mem_write(slot + 0x6C4, dwords(-1))
    u.mem_write(slot + 0x6C, dwords(125, 125))
    u.mem_write(slot + 0x90, bytes([1]))
    u.mem_write(slot + 0x94, dwords(-1))
    u.mem_write(slot + 0x74, b'\x00')
    u.mem_write(slot + 0x81, b'\x01')
    u.mem_write(slot + 0xAC, dwords(-1))
    u.mem_write(slot + 0xB4, dwords(-1))
    u.mem_write(slot + 0x684, b'\xff')
    call(sm.WALK_CTOR, loco, [])
    u.mem_write(loco + 0xC, dwords(slot))
    u.mem_write(slot + 0x674, dwords(loco + 4))
    # The owner's COM reference (LocomotionClass +0x14), which the setter's
    # QueryInterface/Release pair (0x4D957C, 0x55A970) must not release.
    u.mem_write(loco + 0x14, dwords(1))


def observe_crew(u, read32, events, case):
    free = {'next': 0}
    armed = case.get('armed', False)

    def hook(_u, address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        this = u.reg_read(UC_X86_REG_ECX)
        if address == NEW_CALL:
            slot = slot_address(free['next'])
            free['next'] += 1
            u.reg_write(UC_X86_REG_EAX, slot)
            u.reg_write(UC_X86_REG_EIP, NEW_RETURN)
        elif address == INFANTRY_CTOR:
            kind, owner = read32(sp + 4), read32(sp + 8)
            events.append(['construct', slot_name(this), crew_name(kind), owner == HOUSE])
            u.mem_write(this + 0x6C0, dwords(kind))
            u.mem_write(this + 0x21C, dwords(owner))
            u.mem_write(SLOT_PTR, dwords(this))
            u.reg_write(UC_X86_REG_EIP, TRAMPOLINE)
        elif address == BUILDING_IS_ARMED:
            events.append(['is_armed', armed])
            ret(u, read32, 0, armed)
        elif address == BROADCAST_ALL:
            events.append(['radio', read32(sp + 4)])
            ret(u, read32, 4)
        elif address == PLACE_INFANTRY:
            events.append(['place_infantry', 'dummy' if this == DUMMY_CELL else cell_xy(this),
                           coord(u, read32(sp + 8))])
        elif address == UNLIMBO:
            events.append(['unlimbo', slot_name(this), coord(u, read32(sp + 4)), read32(sp + 8),
                           read32(0xA8E7AC)])
        elif address == FOOT_UNLIMBO:
            placed = coord(u, read32(sp + 4))
            events.append(['foot_unlimbo', slot_name(this), placed])
            u.mem_write(this + 0x9C, dwords(*placed))
            u.mem_write(this + 0x81, b'\x00')
            u.mem_write(this + 0x74, b'\x01')
            ret(u, read32, 8, 1)
        elif address == SCATTER:
            events.append(['scatter', slot_name(this), coord(u, read32(sp + 4)), read32(sp + 8) & 0xFF,
                           read32(sp + 12) & 0xFF, read32(0xA8E7AC)])
        elif address == SETTER:
            target = read32(sp + 4)
            events.append(['set_destination', slot_name(this), cell_xy(target) if target else None])
        elif address == FIND_PATH:
            # The AStar route to Scatter's destination, a neighbouring cell:
            # its one step, written to Foot+5E0 with AL = 1 (track_fresh_response).
            to = struct.unpack('<hh', dwords(read32(sp + 4)))
            at = [value // 256 for value in coord(u, this + 0x9C)[:2]]
            facing = STEP_FACINGS.index((to[0] - at[0], to[1] - at[1]))
            events.append(['find_path', slot_name(this), list(to), read32(sp + 8), read32(sp + 12), facing])
            u.mem_write(this + 0x5E0, dwords(facing, *([-1] * 23)))
            ret(u, read32, 12, 1)
        elif address == LIMBO:
            events.append(['limbo', slot_name(this)])
            u.mem_write(this + 0x81, b'\x01')
            ret(u, read32, 0, 1)
        elif address == PLAY_ANIM:
            u.mem_write(this + 0x6C4, dwords(read32(sp + 4)))
            ret(u, read32, 12, 1)
        elif address == INFANTRY_DELETE:
            events.append(['delete', slot_name(this)])
            ret(u, read32, 4)
        elif address == INFANTRY_KILL_CREDIT:
            events.append(['kill_credit', slot_name(this), read32(sp + 4)])
            ret(u, read32, 4)
        elif address == PLAY_AT:
            handle = read32(sp + 4)
            events.append(['play_at', u.reg_read(UC_X86_REG_ECX),
                           coord(u, u.reg_read(UC_X86_REG_EDX)), 'handle' if handle else None])
            ret(u, read32, 4)
        elif address == TECHNO_SELECT:
            events.append(['select', slot_name(this)])
            ret(u, read32, 0, 1)
        elif address == TRAMPOLINE_DRAW:
            events.append(['constructor_draw', u.reg_read(UC_X86_REG_EAX) & 0xFFFF])
        elif address == SCATTER_FNPC_CALL:
            # Find_Nearby_Passable_Cell runs natively: entered past its
            # `SUB ESP, 0x1DC`, where the refinery_dock observer answers it.
            args = [read32(sp + 4 * index) for index in range(13)]
            events.append(['nearby_passable_cell', list(struct.unpack('<hh', u.mem_read(args[1], 4))),
                           *[struct.unpack('<i', dwords(arg))[0] for arg in args[2:12]]])
            u.mem_write(sp - 4, dwords(SCATTER_FNPC_RETURN))
            u.reg_write(UC_X86_REG_ESP, sp - 4 - 0x1DC)
            u.reg_write(UC_X86_REG_EIP, FNPC + 6)
        elif address == SCATTER_FNPC_RETURN:
            out = u.reg_read(UC_X86_REG_EAX)
            events.append(['nearby_passable_cell_found', list(struct.unpack('<hh', u.mem_read(out, 4)))])

    u.hook_add(UC_HOOK_CODE, hook)


def infantry_state(u, read32, slot):
    return dict(type=crew_name(read32(slot + 0x6C0)), owner=read32(slot + 0x21C) == HOUSE,
                limbo=u.mem_read(slot + 0x81, 1)[0], coord=coord(u, slot + 0x9C),
                mission=signed(u, slot + 0xAC), queued=signed(u, slot + 0xB4),
                nav=cell_xy(read32(slot + 0x5A4)), ctor_word=read32(slot + 0x3C8) & 0xFFFF,
                nominal=u.mem_read(slot + 0x6D9, 1)[0])


def crew(case):
    """BuildingClass::Sell's stage 1 on the refinery (module doc)."""
    u, call, read32, events = crew_fixture(case)
    observe_crew(u, read32, events, case)
    building = sm.YAREFN
    before = [read32(SCENARIO + 0x21C), read32(SCENARIO + 0x220)]
    count = bc.invoke(u, bc.SURVIVOR_COUNT, building)
    # Stage 1 runs every observer hook per instruction through several native
    # FNPC searches: a wall-clock limit well past the default 10 s.
    u.mem_write(bc.SP, dwords(RET_MAGIC))
    u.reg_write(UC_X86_REG_ECX, building)
    u.reg_write(UC_X86_REG_ESP, bc.SP)
    run_checked(u, SELL, RET_MAGIC, count=50_000_000, timeout_us=600_000_000)
    delay = u.reg_read(UC_X86_REG_EAX)
    for event in events:
        # The refinery_dock observer names objects it does not know by address.
        if event[0] in ('queue', 'commence') and isinstance(event[1], str) and event[1].startswith('0x'):
            event[1] = slot_name(int(event[1], 16))
    constructed = [event[1] for event in events if event[0] == 'construct']
    passengers = [f'passenger{index}' for index in range(len(case.get('passengers', [])))]
    infantry = {name: infantry_state(u, read32, slot_address(int(name[4:]) if name.startswith('crew')
                                                             else CREW_SLOTS + int(name[9:])))
                for name in constructed + passengers}
    after = [read32(SCENARIO + 0x21C), read32(SCENARIO + 0x220)]
    next_random = bc.invoke(u, RANDOM, SCENARIO + 0x218)
    return dict(input=case, survivors=count, delay=delay, events=events, infantry=infantry,
                building=dict(status=signed(u, building + 0xBC), done=u.mem_read(building + 0x6DD, 1)[0],
                              passengers=signed(u, building + 0x114)),
                random_indices=dict(before=before, after=after), next_random=next_random,
                scenario_init=signed(u, 0xA8E7AC))


def crew_cases():
    allied = dict(side=0, cost=2000)
    return [
        # Refund 1000 / 500 = 2 survivors; the Allied crew, 25% Engineer roll
        # only for a yard.
        dict(allied, name='c_allied_two'),
        dict(allied, name='c_allied_two_seed_7', seed=7),
        # Clamp: 200 / 500 -> 1; 5000 / 500 -> 5.
        dict(allied, name='c_allied_clamp_low', cost=400),
        dict(allied, name='c_allied_clamp_high', cost=12000),
        # Soviet (divisor 250) and Yuri (750); an unknown side owes none.
        dict(name='c_soviet_four', side=1, cost=2000),
        dict(name='c_yuri_one', side=2, cost=2000),
        dict(name='c_other_side', side=3, cost=2000),
        # A computer house: the full cost refund (no RefundPercent).
        dict(allied, name='c_computer_full_refund', human=False, player=False),
        # Captured: divisor doubled, no Engineer roll.
        dict(allied, name='c_captured', cost=4000, captured=True),
        # NoSurvivor and an uncrewed type owe none.
        dict(allied, name='c_no_survivor', no_survivor=True),
        dict(allied, name='c_uncrewed', crewed=False),
        # An armed building: GetCrew's 15% Technician roll per survivor.
        dict(allied, name='c_armed', armed=True, cost=5000),
        # A country without a Side: Technician before the roll.
        dict(allied, name='c_sideless_country', country_side=-1),
        # A Construction Yard: the Engineer roll can win, once; later picks
        # re-roll while they give an Engineer.
        dict(allied, name='c_yard_seed_1', yard=True, cost=5000),
        dict(allied, name='c_yard_seed_2', yard=True, cost=5000, seed=2),
        dict(allied, name='c_yard_seed_3', yard=True, cost=5000, seed=3),
        dict(allied, name='c_yard_seed_4', yard=True, cost=5000, seed=4),
        dict(allied, name='c_yard_seed_5', yard=True, cost=5000, seed=5),
        dict(allied, name='c_yard_armed', yard=True, cost=5000, armed=True, seed=6),
        # A 3x3Refinery foundation: the 8-cell list.
        dict(allied, name='c_refinery_foundation', foundation=9, cost=5000),
        # Not the local player: no sounds; selected: re-selected after.
        dict(allied, name='c_other_player', player=False),
        dict(allied, name='c_selected_packup', selected=True, packup_sound=9),
        # A Bio Reactor's infantry leave before the crew.
        dict(name='c_absorbed', side=2, cost=2000, passengers=['E1', 'INIT']),
        dict(name='c_absorbed_computer', side=2, cost=2000, passengers=['E1'], human=False, player=False),
        dict(name='c_absorbed_no_survivor', side=2, cost=2000, passengers=['E1'], no_survivor=True),
        # A full Bio Reactor: five passengers over the four-cell list; the
        # fifth reads the list's terminator and places in MapClass's
        # off-map cell.
        dict(name='c_absorbed_five', side=2, cost=2000, passengers=['E1', 'E1', 'E1', 'E1', 'INIT']),
    ]


# --- refund --------------------------------------------------------------


def refund(case):
    u, call, read32, events = crew_fixture(dict(case, name=case['name']))
    value = bc.invoke(u, REFUND, sm.YAREFN)
    return dict(input=case, refund=struct.unpack('<i', dwords(value))[0])


def refund_cases():
    rows = []
    for cost in (0, 1, 3, 399, 401, 2000, 2501, 5000):
        for human, mode in ((True, 1), (False, 1), (True, 0), (False, 0)):
            rows.append(dict(name=f'r_{cost}_{"human" if human else "computer"}_mode{mode}', cost=cost,
                             human=human, game_mode=mode))
    # House+0x1ED counts as human only in a campaign (0x50B730).
    for mode in (0, 1):
        rows.append(dict(name=f'r_2501_player_control_mode{mode}', cost=2501, human=False, player_control=True,
                         game_mode=mode))
    return rows


# --- ai_sale -------------------------------------------------------------


def ai_sale(case):
    """BuildingClass::UpdateRepairAndPower (module doc) from its entry to the
    repair tick or the auto-repair start: the Scenario draws, Sell_Back's
    controls and where it stopped."""
    u, call, read32, events = sm.make_fixture(dict(
        name=case['name'], manager_state=0, nodes=[], ore=[], seed=case.get('seed', 1), human=False,
        game_mode=case['game_mode']))
    building, kind = sm.YAREFN, sm.YTYPE
    u.mem_write(building + 0x6C, dwords(case['health']))
    u.mem_write(building + 0xAC, dwords(MISSION[case['mission']]))
    u.mem_write(building + 0xB4, dwords(MISSION[case.get('queued', 'none')]))
    u.mem_write(building + 0x3D1, bytes([case['attacked']]))
    u.mem_write(building + 0x6DC, bytes([case['ai_sellable']]))
    u.mem_write(building + 0x34, dwords(AI_SALE_TAG if case['tag'] else 0))
    u.mem_write(building + 0x6E8, b'\x00')
    # Strength, ClickRepairable=, Repairable=, UndeploysInto= (a unit type
    # pointer), the Foundation index (0 is 1x1) and Factory=.
    u.mem_write(kind + 0xA0, dwords(case['strength']))
    u.mem_write(kind + 0x157A, bytes([case['click_repairable']]))
    u.mem_write(kind + 0xCCC, bytes([case['repairable']]))
    u.mem_write(kind + 0x408, dwords(sm.STYPE if case['undeploys'] else 0))
    u.mem_write(kind + 0xEF0, dwords(case['foundation']))
    u.mem_write(kind + 0xEB8, dwords(7 if case['yard'] else -1))
    # The owner: its money interface (the HouseClass constructor's vtable,
    # whose +0x18 is Available_Money 0x4F6990), Balance, the authored IQ,
    # TechLevel, CurrentIQ and the auto-repair byte.
    u.mem_write(HOUSE + 0x24, dwords(HOUSE_MONEY_VTABLE))
    u.mem_write(HOUSE + 0x30C, dwords(case['balance']))
    u.mem_write(HOUSE + 0x1D0, dwords(case['authored_iq'], case['tech_level']))
    u.mem_write(HOUSE + 0x24C, dwords(case['current_iq']))
    u.mem_write(HOUSE + 0x245, b'\x00')
    # [IQ] RepairSell/SellBack, [AI] CreditReserve and [AudioVisual]
    # ConditionRed (retail .25).
    u.mem_write(RULES + 0x1444, dwords(case['repair_sell']))
    u.mem_write(RULES + 0x145C, dwords(case['sell_back']))
    u.mem_write(RULES + 0x1758, dwords(case['credit_reserve']))
    u.mem_write(RULES + 0x1708, struct.pack('<d', 0.25))
    draws, sell_back = [], []

    def hook(_u, address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if address == RANDOM_RANGED:
            draws.append([read32(sp + 4), read32(sp + 8)])
        elif address == SALE_ROLL_RETURN:
            draws[-1].append(u.reg_read(UC_X86_REG_EAX))
        elif address == SELL_BACK:
            sell_back.append(struct.unpack('<i', dwords(read32(sp + 4)))[0])
            ret(u, read32, 4, 1)

    u.hook_add(UC_HOOK_CODE, hook)
    before = [read32(SCENARIO + 0x21C), read32(SCENARIO + 0x220)]
    u.mem_write(bc.SP, dwords(RET_MAGIC))
    u.reg_write(UC_X86_REG_ECX, building)
    u.reg_write(UC_X86_REG_ESP, bc.SP)
    run_checked(u, UPDATE_REPAIR_AND_POWER, tuple(AI_SALE_ENDS), count=2_000_000)
    return dict(input=case, draws=draws, sell_back=sell_back, end=AI_SALE_ENDS[u.reg_read(UC_X86_REG_EIP)],
                random_indices=dict(before=before, after=[read32(SCENARIO + 0x21C), read32(SCENARIO + 0x220)]))


AI_SALE_BASE = dict(game_mode=1, current_iq=2, authored_iq=2, repair_sell=1, sell_back=2, credit_reserve=100,
                    balance=0, tech_level=51, strength=1000, health=200, mission='guard', attacked=True,
                    ai_sellable=False, click_repairable=True, repairable=True, undeploys=False, foundation=3,
                    yard=False, tag=False)


def ai_sale_cases():
    base = AI_SALE_BASE
    return [
        # A broke computer house's building below ConditionRed, hit by an enemy.
        dict(base, name='s_sold'),
        dict(base, name='s_sold_seed_7', seed=7),
        # CurrentIQ (+0x24C) below RepairSell (0x450653).
        dict(base, name='s_repair_sell_iq', current_iq=0),
        # Get_Mission (vt+0x184: current, else queued) on Construction or Selling.
        dict(base, name='s_constructing', mission='construction'),
        dict(base, name='s_selling', mission='selling'),
        dict(base, name='s_selling_queued', mission='none', queued='selling'),
        # Can_Repair (vt+0x94): full Strength, ClickRepairable=, Repairable=,
        # a 1x1 UndeploysInto= type (a 2x2 one repairs).
        dict(base, name='s_full_strength', health=1000),
        dict(base, name='s_no_click_repair', click_repairable=False),
        dict(base, name='s_unrepairable', repairable=False),
        dict(base, name='s_one_cell_undeploy', undeploys=True, foundation=0),
        dict(base, name='s_two_cell_undeploy', undeploys=True, foundation=3),
        # Money at CreditReserve takes the auto-repair start instead (0x4506B2).
        dict(base, name='s_money_at_reserve', balance=100),
        dict(base, name='s_money_below_reserve', balance=99),
        # A campaign building needs its AI sale byte (+0x6DC, 0x450781).
        dict(base, name='s_campaign_no_byte', game_mode=0),
        dict(base, name='s_campaign_byte', game_mode=0, ai_sellable=True),
        dict(base, name='s_not_attacked', attacked=False),
        # The authored IQ (+0x1D0, unsigned) against SellBack: a skirmish
        # house's zero, and -1 reading as 0xFFFFFFFF.
        dict(base, name='s_skirmish_authored_iq', authored_iq=0, current_iq=5),
        dict(base, name='s_negative_authored_iq', authored_iq=-1),
        # The roll comes before the tag, the yard and the health checks.
        dict(base, name='s_tech_level_zero', tech_level=0),
        dict(base, name='s_tagged', tag=True),
        dict(base, name='s_yard', yard=True),
        dict(base, name='s_at_red', health=250),
        dict(base, name='s_below_red', health=249),
        dict(base, name='s_above_red', health=600),
    ]


def ai_sale_rows():
    rows = [ai_sale(case) for case in ai_sale_cases()]
    # TechLevel at and just above the first row's roll (unsigned `JAE`).
    roll = rows[0]['draws'][0][2]
    rows += [ai_sale(dict(AI_SALE_BASE, name=name, tech_level=level))
             for name, level in (('s_tech_level_at_roll', roll), ('s_tech_level_above_roll', roll + 1))]
    return rows


def generate():
    return {'source': 'unicorn/gamemd.exe',
            'route': [route(case) for case in route_cases()],
            'crew': [crew(case) for case in crew_cases()],
            'refund': [refund(case) for case in refund_cases()],
            'ai_sale': ai_sale_rows()}


def main(argv=None):
    finish_vectors(
        generate, Path(__file__).with_suffix('.json'),
        provenance=lambda: provenance(
            scope='BuildingClass::Sell_Back 0x447110 and a plain (non-UndeploysInto) sale through '
                  'BuildingClass::Update\'s construction pieces to the stage-2 completion 0x449CA7; '
                  'BuildingClass::Sell stage 1 (0x44A2EE..0x44A8DE) with How_Many_Survivors 0x451330, '
                  'Crew_Type 0x44EB10, TechnoClass::GetCrew 0x707D20, the InfantryClass constructor draw, '
                  'the occupy-list cell pick, PlaceInfantryInCell 0x481180, InfantryClass::Unlimbo 0x51DFF0, '
                  'Scatter 0x51D0D0 with its setter and first Walk Process, and the absorbed passengers; '
                  'the sale refund 0x70ADA0; the computer\'s low-credit sale in BuildingClass::'
                  'UpdateRepairAndPower 0x450630 up to the repair tick 0x450813 or the auto-repair start 0x4506B2',
            entry_points={'sell_back': SELL_BACK, 'sell': SELL, 'survivor_count': bc.SURVIVOR_COUNT,
                          'refund': REFUND, 'occupy_init': OCCUPY_INIT,
                          'update_repair_and_power': UPDATE_REPAIR_AND_POWER},
            assumptions=['route rows: building_construction\'s route fixture (the slave_manager refinery, '
                         'Building vtables) with UndeploysInto clear; Rules SellSound/GenericClick and the '
                         'type\'s PackupSound supplied as sound indices',
                         'crew rows: the slave_manager fixture refinery (2x2 at NW (12,12); foundation 9 rows '
                         'use the 3x3Refinery list) in Sell status 1 with Selling current; RefundPercent .5 and '
                         'cost multipliers 1.0 (repair_refund\'s fixture); retail survivor divisors 500/250/750; '
                         'crew types E1/E2/INIT/CTECH/ENGINEER (InfantryType vtable, MovementZone Infantry, '
                         'ENGINEER Engineer=yes); multiplayer game mode, PlayerPtr the owner unless '
                         '`player` is false; Scenario RNG seeded through the original seeder; the building\'s '
                         'occupy-list cells listed (+0xE4) and marked 0x80 as AddContent 0x47E8A0 -> vt+0xF0 '
                         '0x453D60 leaves them; prepared Walks hold the owner\'s COM reference (+0x14 = 1); the '
                         'step offsets 0x89F6D8 (0x49F3A0) and the Infantry startup initialisers 0x517840..'
                         '0x5179B0 (level height 0xA8F240, bridge threshold 0xA8F234 = 416) run natively',
                         'ai_sale rows: the slave_manager fixture refinery owned by a computer House (Balance '
                         'House+0x30C, empty ore storage, IncomeMult 1.0) whose money interface House+0x24 holds the '
                         'constructor\'s vtable 0x7EA834; ConditionRed .25; the Scenario RNG seeded through the '
                         'original seeder'],
            substitutions=['route rows: the broadcast 0x65ACE0, the survivor count 0x451330 (0) and the occupy '
                           'list 0x5F5B90 (empty) answered; IsHumanPlayer 0x50B6F0 answered from `player`; '
                           'VocClass::PlayAtPos 0x750920 and PlayAt 0x7509E0 observed and answered',
                           'crew rows: operator new at 0x44A635 hands out prepared InfantryClass objects (original '
                           'vtables, a constructed Walk); the InfantryClass constructor 0x517A50 fills the type and '
                           'owner and runs the TechnoClass constructor\'s Scenario draw through a trampoline '
                           '(0x6F3249..0x6F3266 in effect); the building\'s IsArmed 0x458DB0 answered from `armed`; '
                           'the broadcast 0x65ACE0, FootClass::Unlimbo 0x4D7170 (writes the coordinate and the '
                           'limbo/on-map bytes), FootClass::Find_Path 0x4D3920 (the one-step route from the '
                           'crewman\'s cell to its neighbouring Scatter cell written to Foot+5E0, AL = 1, as '
                           'track_fresh_response answers it; the AStar core does not run), Limbo 0x51DF10, '
                           'PlayAnim 0x51D6F0, the scalar delete 0x523350, the kill credit 0x702D40, '
                           'VocClass::PlayAt 0x7509E0 and Select 0x6FBFA0 answered; Unlimbo 0x51DFF0, '
                           'PlaceInfantryInCell, Scatter 0x51D0D0, the Infantry setter 0x51AA40, Walk MoveTo and '
                           'the first Walk Process 0x75AEC0 (Infantry Can_Enter_Cell, FindSubCellDest 0x75C240, '
                           'the raw leaves 0x5217C0/0x521850) native, and Scatter\'s Find_Nearby_Passable_Cell '
                           '0x56DC20 (called at 0x51D41D) runs natively past the refinery_dock observer\'s answer; '
                           'the harvest_field and refinery_dock observers otherwise',
                           'ai_sale rows: Sell_Back 0x447110 observed and answered; everything else to the stop '
                           'native']),
        argv=argv)


if __name__ == '__main__':
    main()
