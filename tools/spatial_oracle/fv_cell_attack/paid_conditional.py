"""Original FV continuation under declared identity/options and outside RNG calls.

The compact input contains no Rust motion, timer, projectile or bridge outcomes.
Original Paid/Mission owners construct the FV and execute every selected-object
decision. Outside producers supply only their recorded raw-Next call schedule.
"""
from pathlib import Path
import hashlib
import json

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESI
from tools.native_oracle import RET_MAGIC, provenance
from tools.spatial_oracle.fv_cell_attack.publication import finish_vectors
from tools.spatial_oracle.building_body_rules import dwords
from tools.spatial_oracle.fv_cell_attack import paid_world as owner, move_sound
from tools.spatial_oracle.anim_bouncer_launch import constructor_state
from tools.rules_oracle.bridge_anim_inputs import Reader

HERE = Path(__file__).resolve().parent
INPUT = HERE / 'paid_conditional_v26.input.json'
OUTPUT = HERE / 'paid_conditional_v26.json.gz'
sha = lambda raw: hashlib.sha256(raw).hexdigest()


class Conditional(owner.Paid):
    def __init__(self, world, boundary):
        self.boundary = boundary
        self.identity_input = None
        self.seed_input = None
        self.measured_logic_frame = None
        self.words = []
        super().__init__(world)
        self.u.hook_add(UC_HOOK_CODE, self.raw_word)

    def observe(self, u, pc, n, d):
        if pc == 0x7353C0:
            assert self.identity_input is None
            p = self.m.read32(0xA8B230) + 0x214
            cursor = self.boundary['scenario_cursor_before_constructor']
            self.identity_input = dict(pc=hex(pc), prior_cursor=self.m.read32(p), supplied_cursor=cursor)
            u.mem_write(p, dwords(cursor))
        super().observe(u, pc, n, d)

    def setup(self):
        self.inputs['move_sound_install'] = move_sound.install(self)
        invoke = self.m.invoke
        def seeded_invoke(fn, this, args=()):
            if fn == 0x7353C0:
                assert self.seed_input is None
                seed = self.boundary.get('scenario_seed', 31)
                assert isinstance(seed, int) and 0 <= seed <= 0xFFFFFFFF
                pointer = self.resident.rngs['scenario']
                before = bytes(self.u.mem_read(pointer, 1012))
                invoke(0x65C6D0, pointer, (seed,))
                after = bytes(self.u.mem_read(pointer, 1012))
                if seed == 31:
                    assert before == after, 'Default constructor seed changed existing native input'
                self.seed_input = dict(entry='0x65C6D0', before_entry='0x7353C0',
                    scenario_seed=seed, prior_sha256=sha(before), supplied_sha256=sha(after))
            return invoke(fn, this, args)
        self.m.invoke = seeded_invoke
        try:
            super().setup()
        finally:
            self.m.invoke = invoke
        self.inputs['move_sound_binding'] = move_sound.bound(self)
        assert self.m.read32(0xA8EB60) == self.boundary['game_speed']
        assert self.m.read32(self.src + 0x10) == self.boundary['source_native_id']
        assert self.frame == self.boundary['command_frame'] == 1
        current = self.full_state()
        hashes = {k:sha(bytes.fromhex(v)) for k,v in current['rng'].items()}
        assert hashes == self.boundary['command_rng_sha256'], (hashes, self.boundary)

    def full_state(self):
        result = super().full_state()
        result['options'] = dict(game_speed=self.m.read32(0xA8EB60))
        result['anims'] = []
        for index in range(self.m.read32(0xA8E9B8)):
            p = self.m.read32(self.m.read32(0xA8E9AC) + index * 4)
            typ = self.m.read32(p + 0xC8)
            name = self.m.string(typ + 0x24)
            config = Reader.result(self.m, name, typ, 0)
            config.pop('art_body_read')
            result['anims'].append(dict(type_name=name, config=config,
                raw=bytes(self.u.mem_read(p, 0x1C8)).hex(),
                state=constructor_state(self.u, p)))
        return result

    def raw_word(self, u, pc, _n, _d):
        if self.measured_logic_frame is None or pc not in (0x65C79D, 0x65C84B):
            return
        pointer = u.reg_read(UC_X86_REG_ECX if pc == 0x65C79D else UC_X86_REG_EDX)
        stream = next(k for k,p in self.resident.rngs.items() if p == pointer)
        self.words.append(dict(logic_frame=self.measured_logic_frame, phase=self.phase,
            pc=hex(pc), stream=stream,
            before_indices=[self.m.read32(pointer + 4), self.m.read32(pointer + 8)],
            value=u.reg_read(UC_X86_REG_ESI)))

    def execute_followup(self, command):
        """Original Target/Event constructors and full Event6 receiver; no outcome writes."""
        assert command == dict(frame=self.frame, kind='stop'), command
        before = self.full_state()
        event_start, write_start, word_start = len(self.events), len(self.memwrites), len(self.words)
        self.phase = 'followup_event'
        token = self.m.alloc(8)
        self.m.invoke(0x6E6AB0, token, (self.src,))
        native_token = bytes(self.u.mem_read(token, 5))
        event = self.m.alloc(0x70)
        house = self.m.read32(0xA83D4C)
        args = (self.m.read32(house + 0x30), 6,
                int.from_bytes(native_token[:4], 'little'), native_token[4])
        self.m.invoke(0x4C65E0, event, args)
        event_bytes = bytes(self.u.mem_read(event, 0x6F)).hex()
        self.m.invoke(0x4C6CB0, event)
        return dict(input=command, target_constructor='0x6e6ab0',
            event_constructor='0x4c65e0', event_execute='0x4c6cb0',
            target_token=native_token.hex(), constructor_args=list(args), event_bytes=event_bytes,
            before=before, after=self.full_state(), events=self.events[event_start:],
            writes=self.memwrites[write_start:], raw_words=self.words[word_start:])

    def supply(self, rows, phase):
        self.phase = phase
        for row in rows:
            pointer = self.resident.rngs[row['stream']]
            before = [self.m.read32(pointer + 4), self.m.read32(pointer + 8)]
            assert before == row['before_indices'], ('external-input-indices', self.frame, row, before)
            actual = self.m.invoke(0x65C780, pointer)
            request = self.pending.pop(RET_MAGIC)
            request['returned_eax'] = actual
            assert actual == row['value'], ('external-original-Next-value', self.frame, row, actual)


def run(world, supplied, on_state=None):
    heaps = owner.lists_owner.HEAP, owner.reader_owner.HEAP
    owner.lists_owner.HEAP = owner.reader_owner.HEAP = 0x28000000
    q = None
    try:
        q = Conditional(world, supplied['boundary'])
        q.setup()
        states = [q.full_state()]
        if on_state:
            on_state(q, states[-1], 0)
        frames, followup_events = [], []
        commands = supplied.get('followup_commands', [])
        assert all(set(c) == {'frame', 'kind'} and c['kind'] == 'stop' for c in commands)
        assert len({c['frame'] for c in commands}) == len(commands)
        assert all(c['frame'] in {f['logic_frame'] for f in supplied['frames']} for c in commands)
        for frame in supplied['frames']:
            number = frame['logic_frame']
            assert q.frame == number
            q.measured_logic_frame = number
            start, event_start = len(q.words), len(q.events)
            for command in commands:
                if command['frame'] == number:
                    followup_events.append(q.execute_followup(command))
            q.supply(frame['prefix'], 'supplied_external_prefix')
            q.tick()
            q.supply(frame['suffix'], 'supplied_external_suffix')
            states.append(q.full_state())
            if on_state:
                on_state(q, states[-1], number)
            frames.append(dict(logic_frame=number, observed=q.words[start:],
                requests=[e for e in q.events[event_start:] if e['kind'] == 'rng']))
            print('Native conditional FV', world['stage'], 'Logic', number,
                  'Bullets', states[-1]['bullet_count'], 'Anims', states[-1]['anim_count'], flush=True)
        assert not q.pending, q.pending
        code_hash = sha(bytes(q.u.mem_read(0x401000, 0x3E0000)))
        assert code_hash == q.resident.code_hash
        result = dict(stage=supplied['stage'], identity_input=q.identity_input,
            boundary=supplied['boundary'], text_sha256=code_hash,
            inputs=q.inputs, navigation_setup=q.navigation_setup,
            lifecycle=q.lifecycle, states=states, rng_passes=frames,
            events=q.events, timeline=q.timeline, shots=q.shots, impacts=q.impacts,
            all_selected_effects_drained=states[-1]['bullet_count'] == states[-1]['anim_count'] == states[-1]['deferred_count'] == 0)
        if commands:
            result['followup_commands'] = commands
            result['followup_events'] = followup_events
        if 'physical_stage' in supplied:
            result['physical_stage'] = world['stage']
        if 'scenario_seed' in supplied['boundary']:
            result['seed_input'] = q.seed_input
        return result
    finally:
        if q:
            q.close()
        owner.lists_owner.HEAP, owner.reader_owner.HEAP = heaps


def generate():
    inputs = json.loads(INPUT.read_bytes())
    worlds = owner.native_worlds()
    by_stage = {world['stage']:world for world in worlds}
    supplied = inputs['cases']
    assert len({case['stage'] for case in supplied}) == len(supplied)
    physical = lambda case: case.get('physical_stage', case['stage'])
    assert all(physical(case) in by_stage for case in supplied)
    # Preserve the established four-case order before supplementary cases.
    names = [world['stage'] for world in worlds]
    order = lambda case: names.index(case['stage']) if case['stage'] in names else len(names)
    cases = [run(by_stage[physical(case)], case) for case in sorted(supplied, key=order)]
    return dict(schema=1, input_sha256=sha(INPUT.read_bytes()),
                cases=cases)


def metadata():
    result = provenance(scope=__doc__, entry_points={
        'drive_crt_dispatcher':0x7CBED3, 'drive_height_initializer':0x4AF400,
        'scenario_constructor_seed':0x65C6D0, 'unit_ctor':0x7353C0, 'unit_unlimbo':0x737BA0, 'event':0x4C6CB0,
        'live_logic':0x55B5FF, 'outside_raw_next':0x65C780,
        'raw_word':0x65C79D, 'ranged_word':0x65C84B, 'anim_ai':0x423AC0},
        assumptions=[
            'The input declares the Scenario identity cursor before actual FV construction, original GameOptions speed3, declared Scenario seed (default31) executed by original65C6D0 immediately before the actual FV ctor, command frame1, constructor RNG-boundary hashes and outside raw-Next prefix/suffix receipts. It contains no expected selected-object outcomes. Boundary hashes are checked, never imported.',
            'Original constructor/placement/Event, Mission/Approach/Drive, MoveSound/FireAt, Bullet flight/impact/bridge damage, Anim AI and live-vector removal/append/deferred cleanup execute unchanged through the supplied bounded frame schedule. Full native states, raw calls/ranged requests/results, native IDs, velocity bytes, Anim runtime/config and ordered callbacks remain output.',
            'The physical four-stage map is freshly produced by the existing owner and matches its frozen cells/navigation/hierarchies before selected execution. Compact production input provenance is preserved.'],
        substitutions=[
            'Whole native Scenario startup/population and other-world AI are not executed. Their recorded Next calls run original65C780 as explicit outside-world inputs. This is selected-owner native evidence conditional on incoming identity/options/RNG scheduling, not whole-world parity.',
            'Shared Paid OS/assets/physical-world boundaries and MoveSound sound-device boundary remain. No selected RNG return, constructed object ID, movement, timer, projectile, damage or cleanup result is supplied.'])
    result.update(input_sha256=sha(INPUT.read_bytes()))
    frozen = json.loads((HERE / 'promotion.json').read_bytes())['results']['paid_conditional_v26.json']
    result.update(frozen_source_sha256=frozen['frozen_source_sha256'],
                  supersedes='paid_conditional_drive_crt.json.gz',
                  publication_changes=['Original selected execution uses the new sealed bridge-fv-current-v26 outside-call boundary. All six original declarations and horizons remain; prior conditional inputs and native packets are preserved. Drive CRT initialization remains enabled.', 'Copied lexical INI text is represented by SHA256 and checkout references become relative. Historical Rust input receipts stay sealed in the compact input.'])
    return result


if __name__ == '__main__':
    finish_vectors(generate, OUTPUT, provenance=metadata)
