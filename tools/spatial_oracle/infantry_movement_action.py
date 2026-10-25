"""Bounded original Infantry movement/idle action acceptance and timer writes.

Executable caller comparison: supplied valid Infantry/type/sequence state, ordinary
ground gates, no transport/remap-to-water, no full action sequencer delivery.
No original callable is replaced. All 42 requested sequence records have a
supplied count of 6 unless a row explicitly tests an absent requested sequence.
"""
from pathlib import Path
import argparse
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.native_oracle import load_image, run_checked, STACK_BASE, STACK_SIZE, SCRATCH, RET_MAGIC, finish_vectors, provenance

ACTOR, TYPE, SEQUENCES, LOCO = [SCRATCH + n * 0x4000 for n in range(4)]
HOUSE, SCENARIO = SCRATCH + 0x10000, SCRATCH + 0x12000
FEAR_SPANS = {'fear': (0x5200B0, 0x1AC), 'do_action': (0x51D6F0, 0x400),
              'house_control': (0x50B730, 0x23), 'walk_is_moving': (0x75AB30, 10),
              'is_armed': (0x701120, 0x1A)}
DEFAULT_PAYLOAD_SHA256 = '2f40b3be270bdce0070d59ac8efb6c02ab89a6ba68ad82d8ba93dada141cab27'


def query(row, *, fear=False, capture_original=False):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(SCRATCH, 0x18000 if fear else 0x10000)
    uc.mem_map(RET_MAGIC, 0x1000)
    uc.reg_write(UC_X86_REG_FPCW, 0x0E7F)

    def write32(address, *values):
        uc.mem_write(address, struct.pack('<' + 'I' * len(values), *(v & 0xffffffff for v in values)))

    def read32(address):
        return struct.unpack('<i', uc.mem_read(address, 4))[0]

    def call(address, owner, args):
        sp = STACK_BASE + STACK_SIZE - 0x1000
        write32(sp, RET_MAGIC, *args)
        uc.reg_write(UC_X86_REG_ESP, sp)
        uc.reg_write(UC_X86_REG_ECX, owner)
        run_checked(uc, address, RET_MAGIC, count=100000 if fear else 10000, required_addresses=[address])
        assert uc.reg_read(UC_X86_REG_ESP) == sp + 4 * (len(args) + 1)

    # Original vtable and function slots; supplied object/type state.
    write32(ACTOR, 0x7EB058)
    write32(ACTOR + 0x6C0, TYPE)
    write32(TYPE + 0xE3C, SEQUENCES)
    for action in range(42):
        write32(SEQUENCES + action * 36 + 4, 6)
    requested = row.get('request', 3)
    if row.get('absent') and requested >= 0:
        write32(SEQUENCES + requested * 36 + 4, 0)
    write32(ACTOR + 0x6C4, row.get('current', -1))
    write32(ACTOR + 0x6C, 100)  # nonzero gate bypasses conditional +500 callback
    uc.mem_write(ACTOR + 0x90, b'\x01')
    uc.mem_write(ACTOR + 0x8D, bytes([int(row.get('object_is_falling_down', False))]))
    uc.mem_write(ACTOR + 0x6DB, bytes([int(row.get('prone', False))]))
    write32(ACTOR + 0x6D4, row.get('fear', 0))
    write32(ACTOR + 0xAC, 5)
    write32(ACTOR + 0xB4, -1)
    write32(ACTOR + 0x100, 17, 0, 91, 92)
    write32(ACTOR + 0xF8, 7)
    write32(0xA8ED84, 100)
    if fear:
        write32(ACTOR + 0x21C, HOUSE)
        write32(ACTOR + 0x2FC, row.get('ammo', -1))
        write32(TYPE + 0x684, row.get('type_ammo', -1))
        # The original IsArmed reader compares this slot's pointer only. No
        # WeaponType reader/constructor or weapon gameplay is claimed.
        write32(TYPE + 0x898, TYPE + 0x1000 if row.get('armed') else 0)
        uc.mem_write(TYPE + 0xEBC, bytes([row.get('fearless', False)]))
        uc.mem_write(TYPE + 0xEBD, bytes([row.get('crawls', True)]))
        uc.mem_write(TYPE + 0xEBF, bytes([row.get('fraidycat', False)]))
        uc.mem_write(HOUSE + 0x1EC, bytes([row.get('human', False)]))
        uc.mem_write(HOUSE + 0x1ED, bytes([row.get('player_control', False)]))
        write32(0xA8B238, int(row.get('game_mode_nonzero', True)))
        write32(ACTOR + 0x5A4, TYPE if row.get('nav') else 0)
        uc.mem_write(ACTOR + 0xFC, b'\x01')
        write32(ACTOR + 0x110, 3)
        write32(0xA8B230, SCENARIO)
        rngs = {'main': 0x886B88, 'scenario': SCENARIO + 0x218, 'mapgen': 0xABE890}
        for stream in rngs.values():
            call(0x65C6D0, stream, [31])
        before_rng = {key: bytes(uc.mem_read(ptr, 0x3F4)).hex() for key, ptr in rngs.items()}
    # Type+5B4=0; actor+2DC=0 and +74=0. Original +54 executes and returns false.
    call(0x75AA90, LOCO, [])
    write32(LOCO + 0xC, ACTOR)
    write32(ACTOR + 0x674, LOCO + 4)
    if row.get('motion'):
        write32(LOCO + 0x28, 2880, 2624, 0)
        uc.reg_write(UC_X86_REG_ECX, LOCO)
        run_checked(uc, 0x75AEC0, 0x75BD29, count=100, required_addresses=[0x75AEC0, 0x75BD25])
    if fear:
        # Actual Walk constructor supplies false. This selected mode declares
        # the independently retained +34 moving byte as a prior-state input;
        # original ILocomotion+10 reads it, never the +36 motion byte.
        uc.mem_write(LOCO + 0x34, bytes([row.get('moving', False)]))
        original = {key: bytes(uc.mem_read(a, length)) for key, (a, length) in FEAR_SPANS.items()}
        tables = {'infantry': (0x7EB058, 0x600), 'walk': (0x7F69F8, 0xB0)}
        original_tables = {key: bytes(uc.mem_read(a, length)) for key, (a, length) in tables.items()}

        def state():
            return dict(fear=read32(ACTOR + 0x6D4), doing=read32(ACTOR + 0x6C4),
                        prone=uc.mem_read(ACTOR + 0x6DB, 1)[0], ammo=read32(ACTOR + 0x2FC),
                        stage=read32(ACTOR + 0xF8), changed=uc.mem_read(ACTOR + 0xFC, 1)[0],
                        timer_start=read32(ACTOR + 0x100), timer_duration=read32(ACTOR + 0x108),
                        rate=read32(ACTOR + 0x10C), increment=read32(ACTOR + 0x110),
                        moving=uc.mem_read(LOCO + 0x34, 1)[0])

        start = state()
        events, returns = [], {}
        names = {0x5200B0: 'fear', 0x50B730: 'house_control', 0x75AB30: 'walk_is_moving',
                 0x51D6F0: 'do_action', 0x701120: 'is_armed', 0x51D0D0: 'scatter',
                 0x520105: 'ammo_refill', 0x51D9D2: 'doing_write',
                 0x65C780: 'rng_next', 0x65C7E0: 'rng_range'}

        def observe(_u, address, _size, _data):
            for event in returns.pop(address, []):
                event['returned_al'] = uc.reg_read(UC_X86_REG_EAX) & 255
            if address not in names:
                return
            sp = uc.reg_read(UC_X86_REG_ESP)
            event = dict(kind=names[address], address=hex(address), state=state())
            if address in (0x51D6F0, 0x51D0D0):
                event['args'] = [read32(sp + 4 + 4 * i) for i in range(3)]
            if address in (0x65C780, 0x65C7E0):
                event['stream'] = next((key for key, ptr in rngs.items()
                                        if ptr == uc.reg_read(UC_X86_REG_ECX)), 'unrecognized')
            if address in (0x50B730, 0x75AB30, 0x51D6F0, 0x701120):
                returns.setdefault(read32(sp), []).append(event)
            events.append(event)

        uc.hook_add(UC_HOOK_CODE, observe)
        fields = {ACTOR + offset: name for offset, name in (
            (0x6D4, 'fear'), (0x6C4, 'doing'), (0x6DB, 'prone'), (0x2FC, 'ammo'),
            (0xF8, 'stage'), (0xFC, 'changed'), (0x100, 'timer_start'),
            (0x108, 'timer_duration'), (0x10C, 'rate'), (0x110, 'increment'))}

        def record_write(_u, _access, address, size, value, _data):
            if address in fields:
                events.append(dict(kind='field_write', field=fields[address], size=size,
                                   value=value, pc=hex(uc.reg_read(UC_X86_REG_EIP))))

        uc.hook_add(UC_HOOK_MEM_WRITE, record_write)
        if row.get('scatter_boundary'):
            sp = STACK_BASE + STACK_SIZE - 0x1000
            write32(sp, RET_MAGIC)
            uc.reg_write(UC_X86_REG_ESP, sp)
            uc.reg_write(UC_X86_REG_ECX, ACTOR)
            run_checked(uc, 0x5200B0, 0x51D0D0, count=10000,
                        required_addresses=[0x5200B0, 0x520254])
            # run_checked stops before the entry hook: read actual pushed args.
            sp = uc.reg_read(UC_X86_REG_ESP)
            events.append(dict(kind='scatter_boundary', address='0x51d0d0', state=state(),
                               args=[read32(sp + 4 + i * 4) for i in range(3)]))
        else:
            call(0x5200B0, ACTOR, [])
        assert original == {key: bytes(uc.mem_read(a, length)) for key, (a, length) in FEAR_SPANS.items()}
        assert original_tables == {key: bytes(uc.mem_read(a, length)) for key, (a, length) in tables.items()}
        result = dict(input=row, before=start, after=state(), events=events,
                      rng_before=before_rng,
                      rng_after={key: bytes(uc.mem_read(ptr, 0x3F4)).hex() for key, ptr in rngs.items()},
                      returned=not row.get('scatter_boundary', False),
                      original_code_and_vtables_unchanged=True)
        if capture_original:
            result['original_receipt'] = {
                'text': {key: dict(address=hex(FEAR_SPANS[key][0]), length=len(raw),
                                   sha256=hashlib.sha256(raw).hexdigest(), bytes_hex=raw.hex())
                         for key, raw in original.items()},
                'vtables': {key: dict(address=hex(tables[key][0]), length=len(raw),
                                      sha256=hashlib.sha256(raw).hexdigest(), bytes_hex=raw.hex())
                            for key, raw in original_tables.items()}}
        return result
    events = []
    observed = {0x521161, 0x75CB20, 0x51D6F0, 0x51D91D, 0x51D934, 0x51D9D2, 0x51DA34, 0x51DA8C, 0x4DE620, 0x5F6B90}
    uc.hook_add(UC_HOOK_CODE, lambda _u, address, _size, _data: events.append(hex(address)) if address in observed else None)
    if row.get('consumer'):
        # Whole 520F40 with Guard/NavCom=NULL; supplied gates skip its earlier
        # destination-recovery work. Real Walk +A8 and Infantry +558 execute.
        call(0x520F40, ACTOR, [])
        accepted = None  # caller has no promised return-value contract
    else:
        call(0x51D6F0, ACTOR, [requested, int(row.get('force', False)), 0])
        accepted = uc.reg_read(UC_X86_REG_EAX) & 0xff
    return {'input': row, 'accepted': accepted, 'events': events,
            'doing': read32(ACTOR + 0x6C4), 'frame': read32(ACTOR + 0xF8),
            'timer_start': read32(ACTOR + 0x100), 'timer_duration': read32(ACTOR + 0x108),
            'timer_repeat': read32(ACTOR + 0x10C), 'prone': uc.mem_read(ACTOR + 0x6DB, 1)[0],
            'motion': uc.mem_read(LOCO + 0x36, 1)[0]}


def generate():
    rows = [{'request': request, 'current': current} for request in (0, 2, 3, 6) for current in range(-1, 42)]
    rows += [{'request': 3, 'current': 31, 'force': True},
             {'request': -1}, {'request': 3, 'absent': True},
             {'request': 3, 'current': 33, 'object_is_falling_down': True},
             {'request': 3, 'fear': 199}, {'request': 3, 'fear': 200}]
    rows += [{'consumer': True, 'motion': motion, 'prone': prone, 'current': current}
             for motion in (False, True) for prone in (False, True) for current in (-1, 3, 6, 17, 31)]

    return [query(row) for row in rows]


def generate_fear():
    rows = [dict(name='zero_prone', fear=0, prone=True, current=0),
            dict(name='down_threshold49', fear=49, current=0),
            dict(name='down_threshold50', fear=50, current=0),
            dict(name='down_threshold51', fear=51, current=0),
            dict(name='up_threshold51', fear=51, prone=True, current=0),
            dict(name='up_threshold50', fear=50, prone=True, current=0),
            dict(name='up_positive2', fear=2, prone=True, current=0),
            dict(name='up_positive1', fear=1, prone=True, current=0),
            dict(name='fearless_down', fear=50, fearless=True, current=0),
            dict(name='fearless_up', fear=49, fearless=True, prone=True, current=0),
            dict(name='fearless_up_positive1', fear=1, fearless=True, prone=True, current=0),
            dict(name='fearless_zero_prone', fear=0, fearless=True, prone=True, current=0),
            dict(name='up_same_refusal', fear=50, prone=True, current=7),
            dict(name='up_undeploy_refusal', fear=50, prone=True, current=31),
            dict(name='down_undeploy_refusal', fear=51, current=31),
            dict(name='up_absent_refusal', fear=50, prone=True, current=0, request=7, absent=True),
            dict(name='down_absent_refusal', fear=51, current=0, request=5, absent=True),
            dict(name='down_crawls_refusal', fear=51, current=0, crawls=False),
            dict(name='up_without_crawls', fear=50, prone=True, current=0, crawls=False),
            dict(name='human_nav', fear=51, current=0, human=True, nav=True),
            dict(name='human_moving', fear=51, current=0, human=True, moving=True),
            dict(name='human_stationary', fear=51, current=0, human=True),
            dict(name='ai_nav_moving', fear=51, current=0, nav=True, moving=True),
            dict(name='player_control_campaign', fear=51, current=0, player_control=True,
                 game_mode_nonzero=False, nav=True),
            dict(name='player_control_skirmish', fear=51, current=0, player_control=True, nav=True),
            dict(name='human_up_nav_moving', fear=50, prone=True, current=0,
                 human=True, nav=True, moving=True),
            dict(name='fraidycat_threshold', fear=51, current=0, fraidycat=True),
            dict(name='fraidycat_nav', fear=52, current=0, fraidycat=True, nav=True),
            dict(name='fraidycat_moving', fear=52, current=0, fraidycat=True, moving=True),
            dict(name='fraidycat_falling', fear=52, current=0, fraidycat=True, object_is_falling_down=True),
            dict(name='fraidycat_scatter_boundary', fear=52, current=0, fraidycat=True, scatter_boundary=True),
            dict(name='zero_ammo_unarmed', fear=1, prone=True, current=0, ammo=0),
            dict(name='zero_ammo_armed_refill', fear=1, prone=True, current=0,
                 ammo=0, armed=True, type_ammo=4),
            dict(name='zero_ammo_armed_unlimited', fear=1, prone=True, current=0,
                 ammo=0, armed=True, type_ammo=-1)]
    rows += [dict(name=f'{name}_doing{doing}', fear=fear, current=doing,
                  prone=prone, fraidycat=fraidycat)
             for doing in range(27, 31)
             for name, fear, prone, fraidycat in
             [('down', 51, False, False), ('up', 50, True, False), ('fraidycat', 52, False, True)]]
    results = [query(row, fear=True, capture_original=i == 0) for i, row in enumerate(rows)]
    receipt = results[0].pop('original_receipt')
    assert len(results) == 46
    return dict(schema_version=1,
        default_payload_sha256=DEFAULT_PAYLOAD_SHA256, original_receipt=receipt, rows=results)


def fear_provenance():
    return provenance(
        scope='46 original Infantry5200B0 controls, real51D6F0 acceptance/refusal, actual house/motion/IsArmed readers, stage and three full RNG states. Fraidycat admission stops before original Scatter; no complete InfantryAI/world/finite-ammo parity claim.',
        assumptions=[
            'Selected --fear mode reuses the existing movement-action VM. Original198 default rows retain canonical SHA256 ' + DEFAULT_PAYLOAD_SHA256 + '.',
            'Supplied Infantry/type/House prior state, original Infantry7EB058 and original Walk7F69F8 tables. Original Walk75AA90 constructor executes. Infantry/type/House construction and retail rule/sequence loading are excluded; all42 sequence counts supplied6 except explicit absent action. Ordinary E1 tests separately use the production RULES/ART readers.',
            'Fear, Doing, prone, Human1EC/PlayerControl1ED, Session mode, falling, NavCom and Fearless/Crawls/Fraidycat are supplied. NavCom nonnull is a comparison-only sentinel. Retained actual Walk+34 moving byte is supplied independently of motion+36; original COM75AB30 queries it.',
            'Actor Health100, Alivetrue, no transport/highflight/Jumpjet/water remap. Positive Ammo-1 controls isolate the ordinary unlimited-ammo caller. Three additional zero-ammo controls execute actual IsArmed701120/GetWeapon70E1A0/70E140/7177C0 on supplied rookie/type weapon-slot presence; the nonnull slot pointer is compared only, no weapon/type reader or whole ammo lifecycle is claimed.',
            'Frame100, old stage7/changed1/start17/duration91/rate92/increment3. Original class action force0/randomStart0. Timer+104 stack auxiliary is not authoritative or compared; refusal retains stage/timer/prone. The old independent motion+36 is not substituted for IsMoving.',
            'All three native generators are seeded31 by original65C6D0. Full0x3F4 before/after states and actual RNG entry events are retained. These bounded requests draw no RNG; no whole match scheduling or absolute stage advancement is claimed.',
            'One strict Fraidycat fear51-after-decay stationary/null-Nav control stops before51D0D0, retaining the original pushed NullCoordA8F200/forced1/noKidding0. Its Scatter body, RNG, destination/Process/bridge effects and enclosing5200B0 return are excluded. Other Fraidycat controls execute full5200B0 and prove its gates.'
        ], substitutions=[], entry_points={'fear': 0x5200B0, 'ai_call': 0x51BF0B,
            'do_action': 0x51D6F0, 'house_control': 0x50B730, 'walk_is_moving': 0x75AB30,
            'is_armed': 0x701120, 'scatter_boundary': 0x51D0D0, 'rng_seed': 0x65C6D0})


if __name__ == "__main__":
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--fear', action='store_true')
    selected, remaining = parser.parse_known_args()
    if selected.fear:
        finish_vectors(generate_fear, Path(__file__).with_name('infantry_fear_action.json'),
                       provenance=fear_provenance, argv=remaining,
                       source_paths={'infantry_movement_action.py': Path(__file__)})
        raise SystemExit(0)
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=provenance(
        scope="Original Infantry movement/idle DoAction acceptance, timer/frame writes and ordinary Walk movement-flag consumer; no full action sequencer or Rust parity claim.",
        assumptions=[
            "Supplied valid Infantry object, original vtable7EB058, own Type+E3C sequence records; all42 frame counts supplied6 unless an absent requested sequence is explicit. Infantry/type constructors and retail sequence loading are not executed.",
            "Actor+6C is supplied100, alive+90=true, actor+2DC=0, actor+74=false, type MovementZone+5B4=0 and type+D94=false. Transport/carry, water remap, highflight, zero-health+500 and specialized Jumpjet branches are outside these rows.",
            "Frame100; old timer start17,duration91,repeat92 and old image frame7. Timer+104 is the native ignored stack-derived middle slot and is not assigned a logical meaning or compared.",
            "172 direct requests0/2/3/6 cross currentDoing-1..41. Six contrasts cover forced interruption, request-1, absent requested sequence, falling Paradrop refusal, and fear199/200 Panic remap.",
            "20 whole520F40 consumers use Guard mission5 and NavComNULL, skipping earlier destination recovery. Motion=true executes original Walk75AEC0 entry/head gate through75BD25; constructor supplies initial false. Numeric movement and head retirement are not executed.",
            "Force and randomStartFrame are false except the explicit forced row. No sequence advancement, randomized start, normalized-action delay, swimming sound, or complete Doing lifecycle claim.",
            "The observed events identify original call sites. The whole520F40 return value has no claimed contract and is excluded from acceptance output."
        ],
        substitutions=[],
        entry_points={"do_action":0x51D6F0,"movement_action_consumer":0x520F40,"walk_constructor":0x75AA90,"walk_motion_producer":0x75AEC0}
    ))
