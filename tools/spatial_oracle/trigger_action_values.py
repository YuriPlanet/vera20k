"""Original TAction constructor/read, references and selected action dispatch.

Pins +90 materialization independently of ActionID and the four +34..40
parameters. The first 61 numeric rows are retained unchanged. Additional rows
identify their bounded reference, input or EVA observation with ``case_kind``.

``--click-shroud`` executes the reachable ProcessClickCoords floor/shroud
suffix and Tactical pick on declared flat/elevated Cell knowledge. Screen-to-Cell
conversion and the registered screen list are explicit inputs.

The separate ``--damage-waypoint`` mode records Action63 and the bounded
ALL01 reactor damage consumer without regenerating the legacy corpus. Set
``VERA20K_TRIGGER_DAMAGE_INPUTS`` to extracted RULESMD.INI, ARTMD.INI,
SOUNDMD.INI and ALL01UMD.MAP (optional LANGRULE.INI), or pass ``--inputs``.
Retail asset files stay external; native readers own their scalar values.

``--super-charge`` independently records original Action129 materialization,
dispatch, percentage arithmetic and bounded Grant/Suspend/AI histories. It uses
the same external input directory, supplied by ``VERA20K_TRIGGER_SUPER_INPUTS``
or ``--inputs``, without regenerating either earlier corpus. Retained replay
commands use the environment variable so they contain no machine-local path.
An optional ``VERA20K_TRIGGER_SUPER_RECEIPTS`` external file receives detailed
ABI observations on ``--write``; canonical vectors and Rust tests omit them.

``--reveal-zone`` reuses the area/knowledge execution owner for original
Action18 with declared small LocalSize/raw-zone priors. It generates only
trigger_reveal_zone; no retail topology or aircraft drawing is certified.

``--forced-map`` reuses that cell/edge owner for four original4A9890
frontier visits. Counter/gap/cache/notification outputs are independently
generated; it does not regenerate any earlier corpus or certify whole-map
Tag reentry, enabled FogOfWar or rendered pixels.

``--flash-team`` independently records original Action104's TeamType reader,
dispatch and full TeamType/Team flash loops with declared member-list priors.
The existing team_creation corpus owns signed countdown arithmetic; neither
this mode nor those scalar controls certify rendered campaign output.

``--reshroud`` independently executes original Map577C90, Cell487690 and
allocated iterator578290 on explicit sparse map/House priors. Sight/gap
release/reentry and presentation calls are recorded sinks, not invented world
callbacks. No earlier corpus is regenerated.

``--scenario-timer`` executes original Actions23..27/103 and Event14 against
the same small Scenario timer. The reader, timer arithmetic and predicate run
unchanged; only the tactical redraw is a recorded sink. Earlier corpora are
not regenerated.

``--chrono-house`` bounds original Action128/House50D6D0 traversal with the
original initialized CellSpread table. Virtual CanEnter/Warp and infantry
subcell choices are explicit boundaries; original Foot refusal has a control.

``--all-change-house`` bounds Action36's original target selection and mutable
two-pass Techno traversal. ChangeOwner and powered-building refresh are
recorded callback boundaries, with explicit list-mutation controls.

``--conceal-area`` executes Action101, Map567F70 and the complete578100 sweep
on a small allocated diamond. Native projection, radius read and Cell/cache
effects run; only the existing no-dirty-screen world prior excludes rendering.

``--all-to-hunt`` independently records Action6/House501400 and the shared
garrison-release procedure on explicit world priors. Actual ALL06 records,
source-aware Scatter/RNG and stated placement/Team/lifecycle seams remain
separate from whole-campaign or general Hunt parity.

``--music-theme`` executes Action20 Read/Execute and the shared Theme queue,
with declared catalog/stream priors and recorded device transports.

``--crate`` independently records108 placement, solo Money/49 callback
boundaries,49/50 predicates and bounded Logic50 histories. World geometry,
actor/House and callback seams are explicit; no full campaign is replayed.

``--wakeup`` executes Actions81..83 and their real effective-Mission/Tag/
deferred-Queue receivers on declared ordered object lists.

``--ai-activation`` executes Actions74/75 and the ordinary House team-timer
block with the real selector, eligibility and seeded RNG in the same image.
Actual ALL05 records bind the stop route; Team creation remains an observed
boundary, and the positive path has explicit minimal skirmish priors.

``--chrono-reinforcement`` executes107 through the existing physical Foot
constructor/placement owner and original Teleport COM factory. ALL07 records
bind the selected Unit/MCV definitions; waypoint geometry is a declared crop.

``--superweapon-state`` independently records132/135/136 and the bounded
ALL03 NPC NukeSpecial sequence, real recharge/Reset and preferred-cell
consumers. Physical MD bytes come from the configured original installation.
Detailed ABI receipts may be retained externally with
``VERA20K_TRIGGER_SUPER_STATE_RECEIPTS`` when writing the new corpus.

``--superweapon-reset`` uses the same physical Type reader and Super VM for
133/134 dispatch, current-timer preservation/restart and animation-notification
release. Its optional ``VERA20K_TRIGGER_SUPER_RESET_RECEIPTS`` output remains
external; canonical controls exclude ABI, heap addresses and clock padding.

``--tech-level`` executes106's exact global type lookup, physical counted
retail/scalar readers and the existing reset owner. A bounded ALL05 paid ORCA
history follows both authored106 actions through House/Strip and local receive.
It reuses the existing Factory fixture with explicit campaign layers; terrain,
played-world and OS/service priors do not certify full campaign startup.

``--nuke-strike`` reads selected physical inputs from VERA20K_TRIGGER_NUKE_INPUTS
(or --inputs): six HighExpR records, isolated95 launch controls and a full native
Bullet constructor/Fire/descent history to the NUKE impact entry.

``--do-explosion`` always reads fresh physical retail inputs. Its two map
branches share only an in-memory snapshot from that invocation; serialized
native-state caches are unsupported.
"""
from pathlib import Path
import hashlib
import tools.native_oracle as native
import json
import struct
from functools import lru_cache
from types import SimpleNamespace

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDX,
                               UC_X86_REG_EIP, UC_X86_REG_ESP,
                               UC_X86_REG_EDI, UC_X86_REG_ESI)
from tools.native_oracle import (SCRATCH, RET_MAGIC, STACK_BASE, STACK_SIZE,
    finish_vectors, provenance, run_checked)
from tools.spatial_oracle.trigger_type_flags import FlagsFixture
from tools.spatial_oracle.infantry_deploy_rules import HEAP, PTD
from tools.spatial_oracle.building_body_rules import INI, TYPE, SP, RULES
from tools.spatial_oracle.map_queries import dwords
from tools.input_oracle.fast_scroll import return_from_sink
from tools.rules_oracle.bridge_child_sound import Sound, sections, playing_handle
from tools.rules_oracle.bridge_anim_inputs import Reader
from tools.spatial_oracle.anim_bouncer_launch import constructor_state
from tools.spatial_oracle.bridge_constructor import OriginalBridgeConstructor
from tools.sidebar_oracle.stock import mix, mix_hash
from tools.native_oracle import configured_gamemd
from tools.native_slope import slope_matrices
from tools.input_oracle.campaign_start import install_csf, CampaignReader
from tools.spatial_oracle.infantry_deploy_action import Fixture as AtomicTransportOwner
from tools.spatial_oracle.reveal_area import (reveal_area_row, in_diamond, ridge,
    CELLS as REVEAL_CELLS, CELL_STRIDE as REVEAL_CELL_STRIDE, WINDOW as REVEAL_WINDOW,
    STANDARD_Z_MULTIPLIER_BITS)
from tools.spatial_oracle.jumpjet_entry_discovery import (Entry as DiscoveryEntry,
    HOUSE as DISCOVERY_PLAYER, OTHER as DISCOVERY_OTHER, initialize_discovery_prefix)
from tools.spatial_oracle.walk_head_occupation import (CURRENT as DISCOVERY_CURRENT,
    MAP as DISCOVERY_MAP, OWNER as DISCOVERY_OBJECT, OUTPUT as DISCOVERY_OUTPUT,
    SCENARIO as DISCOVERY_SCENARIO, TABLE as DISCOVERY_TABLE,
    TYPE as DISCOVERY_TYPE, VTABLE as DISCOVERY_VTABLE)
from tools.spatial_oracle.map_queries import packed

ACTION, BUFFER, SCENARIO = HEAP+0x1000, HEAP+0x2000, HEAP+0x20000
REFERENCE_REGISTRIES = {'team':0xA8ECA0, 'trigger':0x8B4178, 'tag':0xB0E780}
PRE_CURSOR_CONTROL_ROWS = 212
PRE_CURSOR_CONTROL_SHA256 = '05e87e0576b9da7935486aa6ac3ea9e74d58fc3d8c16780e0f660d4d8bb1298a'
PRE_ANIM_CONTROL_ROWS = 214
PRE_ANIM_CONTROL_SHA256 = 'a86bea00768c2170934a96b143fa407e854f5cb3da60a2bfeba9a5998aec8f03'
PRE_REVEAL_CONTROL_ROWS = 219
PRE_REVEAL_CONTROL_SHA256 = '6b82bb4d6071f29e3af421fe67fab3b69b9672f333360a2cf6ea624944ab961b'
PRE_DISCOVERY_CONTROL_ROWS = 220
PRE_DISCOVERY_CONTROL_SHA256 = 'f91481cdca8d3af67571ff29ddd49139b5c7e4e5449600c5639664b499eee26e'
PRE_NULL_PLAYER_PLACEMENT_ROWS = 8
PRE_NULL_PLAYER_DISCOVERY_SHA256 = '342e2d4accc1384c740edc11ec881f7db97f7b1f32f1ba22f7c883892d571a9f'
REFERENCE_NAMES = {
    'team':('0BD6841C', '0B4FF03C', 'LongTeamReference0123456789ABCDE'),
    'trigger':('0DD8E18C', '0B76DF3C', 'LongTriggerReference0123456789AB'),
    'tag':('0B76DE5C', '0DD8D6DC', 'LongTagReference0123456789ABCDEF'),
}
SOURCE_PATHS = {name:Path(__file__).parents[2]/path for name,path in {
    'producer':'tools/spatial_oracle/trigger_action_values.py',
    'native_runner':'tools/native_oracle.py',
    'trigger_type_transport':'tools/spatial_oracle/trigger_type_flags.py',
    'vector_transport':'tools/spatial_oracle/infantry_deploy_rules.py',
    'ini_transport':'tools/spatial_oracle/building_body_rules.py',
    'map_queries':'tools/spatial_oracle/map_queries.py',
    'sink_return':'tools/input_oracle/fast_scroll.py',
    'ordered_ini_transport':'tools/rules_oracle/bridge_child_sound.py',
    'ini_index_transport':'tools/rules_oracle/bridge_anim_inputs.py',
    'anim_constructor_observation':'tools/spatial_oracle/anim_bouncer_launch.py',
    'object_registry_transport':'tools/spatial_oracle/bridge_constructor.py',
    'mix_transport':'tools/sidebar_oracle/stock.py',
    'slope_initializer':'tools/native_slope.py',
    'csf_transport':'tools/input_oracle/campaign_start.py',
    'physical_csf':'tools/storage_oracle/keyboard_bindings.py',
    'atomic_imports':'tools/spatial_oracle/infantry_deploy_action.py',
    'area_geometry':'tools/spatial_oracle/reveal_area.py',
    'area_geometry_transport':'tools/ai_base_building_oracle.py',
    'stock_definition_reader':'tools/spatial_oracle/tag_lifecycle.py',
    'stock_definition_transport':'tools/spatial_oracle/team_creation.py',
    'discovery_transport':'tools/spatial_oracle/jumpjet_entry_discovery.py',
    'discovery_position_transport':'tools/spatial_oracle/jumpjet_coordinates.py',
    'discovery_cell_transport':'tools/spatial_oracle/walk_head_occupation.py',
    'discovery_air_transport':'tools/spatial_oracle/fly_landing_phase.py',
    'retail_map_pack':'tools/spatial_oracle/shrapnel_repair/map_facts.py',
    'building_foundation_reader':'tools/spatial_oracle/engineer_bridge_cursor_caller.py',
}.items()}


class ActionFixture(FlagsFixture):
    """Existing native reader transport plus spare counted registry storage."""

    def __init__(self):
        super().__init__()
        u = self.u
        for n, base in enumerate((0xB0E658,0xB0F670,0xB0F658,
                                  *REFERENCE_REGISTRIES.values())):
            u.mem_write(base+4,dwords(HEAP+0x8000+n*0x100,64,0,0,64))
        u.mem_write(0xA8B230,dwords(SCENARIO))
        u.mem_write(PTD,bytes(0x74))
        self.cursor = HEAP+0x40000
        # Reader.alloc shares this VM through ordered_ini; VectorFixture maps
        # exactly this arena. Keep its configurable bound on composed readers.
        self.arena_size = 0x100000
        self.allocations, self.calls = [], []
        self.hook = u.hook_add(UC_HOOK_CODE,self.observe)
        self.ranges = [(0x6DD000,0x6DD17B),(0x6DD5B0,0x6DD87D),
                       (0x6DD8B0,0x6E45D0),(0x410800,0x4109B0),
                       (0x6F06E0,0x6F0900),(0x6F1920,0x6F19A3),
                       (0x726C80,0x726DFB),(0x727AA0,0x727B23)]
        self.ranges += [(0x6E5B60,0x6E5C90),(0x6E6310,0x6E6393),
                        (0x752210,0x753630),(0x4072C0,0x4074A0),
                        (0x407F40,0x407F74),(0x684240,0x6842F0),
                        (0x4A8BF0,0x4A8D31),(0x4AC660,0x4AC6F6),
                        (0x4AC8C0,0x4AC957),(0x4AEAD0,0x4AEB04),
                        (0x6938C0,0x6938E7),(0x6DA160,0x6DA17D),
                        (0x54F720,0x54F789),(0x5D4D50,0x5D4E70),
                        (0x53E770,0x53EB41),(0x7B89F0,0x7B89F4),
                        (0x7B9750,0x7B9A55),(0x7BA330,0x7BA334),
                        (0x5BDA40,0x5BDDF0)]
        self.ranges += [(0x421EA0,0x422723),(0x423AC0,0x424B90),
                        (0x424CE0,0x424EFF),(0x4255B0,0x425629),
                        (0x427530,0x4293C0),(0x665650,0x665F70),
                        (0x6691E0,0x66B85F),(0x6728B0,0x672970)]
        self.ranges += [(0x668BF0,0x668FA3),(0x725C70,0x725D8F)]
        self.ranges += [(0x47B150,0x47BB5B),(0x6DCD70,0x6DCF70),
                        (0x565730,0x565803),(0x578080,0x5780F9),
                        (0x734E60,0x735090),(0x69A310,0x69A338)]
        self.ranges += [(0x67181C,0x67183C),(0x47BBF0,0x47BDA8),
                        (0x487630,0x4876F0),(0x5678E0,0x567D92),
                        (0x653830,0x65384C),(0x4A9CA0,0x4A9DC9),
                        (0x586360,0x586497),(0x561710,0x561811)]
        self.ranges += [(0x6D1830,0x6D1858),(0x6D18C0,0x6D18D3),
                        (0x6D1BB0,0x6D1BE4),(0x6D8700,0x6D8ADB)]
        self.original = [bytes(u.mem_read(a,b-a)) for a,b in self.ranges]

    def integer(self, address):
        return struct.unpack('<i',self.u.mem_read(address,4))[0]

    def integer_reg(self, register):
        return struct.unpack('<i',dwords(self.u.reg_read(register)))[0]

    def read32(self, address):
        return struct.unpack('<I',self.u.mem_read(address,4))[0]

    def string(self, address):
        return bytes(self.u.mem_read(address,512)).split(b'\0',1)[0].decode('latin1')

    def allocate(self, size):
        pointer = self.cursor
        self.cursor += (max(size,1)+15)&~15
        assert self.cursor < HEAP+0x100000
        self.u.mem_write(pointer,b'\xA5'*max(size,1))
        return pointer

    def observe(self,u,pc,_size,_data):
        if pc in (0x6F1920,0x727AA0,0x6F06E0,0x726C80,0x6E6310,0x6E5B60):
            self.calls.append(f'{pc:08X}')
        if pc == 0x7C8E17:
            size = self.read32(u.reg_read(UC_X86_REG_ESP)+4)
            self.allocations.append(size)
            return_from_sink(u,0,self.allocate(size))
        elif pc == 0x7D140B:
            return_from_sink(u,0,PTD)
        elif pc == 0x7C8B3D:
            return_from_sink(u,0,0)

    def read_action(self, raw, *, ini_capacity=None, address=ACTION, buffer=BUFFER):
        u = self.u
        u.mem_write(address,b'\xA5'*0x94)
        self.call(0x6DD000,[],ecx=address)
        counted = '1,'+raw
        if ini_capacity is None:
            u.mem_write(buffer,counted.encode('ascii')+b'\0')
        else:
            # Same native ReadString512 contract as the TriggerType action
            # caller727516..72752E, with supplied cache rather than file IO.
            self.ini(HEAP+0x7000,counted)
            self.call(0x528A10,[TYPE+0x1F8,HEAP+0x7000,0x889F64,buffer,ini_capacity],ecx=INI)
        read = self.string(buffer)
        self.call(0x7C9CC2,[buffer,0x817F70],cdecl=True)
        self.call(0x6DD5B0,[],ecx=address)
        return dict(raw=raw,kind=self.integer(address+0x2C),
                    value=self.integer(address+0x90),
                    args=[self.integer(address+n) for n in (0x34,0x38,0x3C,0x40)],
                    waypoint=self.integer(address+0x44),read=read)

    def check_code(self):
        assert self.original == [bytes(self.u.mem_read(a,b-a)) for a,b in self.ranges]

    def rng_hashes(self):
        proxy=SimpleNamespace(u=self.u,scenario=SCENARIO)
        return {key:hashlib.sha256(bytes.fromhex(raw)).hexdigest()
                for key,raw in CampaignReader.rng_state(proxy).items()}

    def definitions(self, kind):
        base = REFERENCE_REGISTRIES[kind]
        return [self.read32(self.read32(base+4)+n*4)
                for n in range(self.integer(base+16))]

    def initialize_references(self, kind):
        fn = {'team':0x6F1920,'trigger':0x727AA0,'tag':0x6E6310}[kind]
        for name in REFERENCE_NAMES[kind]:
            self.u.mem_write(BUFFER,name.encode('ascii')+b'\0')
            assert self.call(fn,[],ecx=BUFFER)
        assert len(self.definitions(kind)) == len(REFERENCE_NAMES[kind])

    def ordered_ini(self, values):
        # Share Sound/Reader's original source-order link/cache transport;
        # do not duplicate section indexing or any native value reader.
        proxy = Sound.__new__(Sound)
        proxy.__dict__ = self.__dict__
        Sound.make_ini(proxy,values)
        assert self.cursor < HEAP+0x100000


def physical_eva():
    master = (configured_gamemd().parent/'ra2md.mix').read_bytes()
    local = mix(master)[mix_hash('localmd.mix')]
    raw = mix(local)[mix_hash('EVAMD.INI')]
    return sections(raw),dict(archive='ra2md.mix -> localmd.mix -> EVAMD.INI',
                             master_sha256=hashlib.sha256(master).hexdigest(),
                             local_sha256=hashlib.sha256(local).hexdigest(),
                             bytes=len(raw),sha256=hashlib.sha256(raw).hexdigest())


def initialize_eva(f, physical):
    u = f.u
    # Original CRT scanner registration and EVA registry/list constructors.
    # Preallocated capacity excludes vector-growth allocator behavior.
    f.call(0x7C8F5E,[])
    # Stop at the CRT exit-callback registration; it is outside the selected
    # original registry initialization prefix, not an answered gameplay call.
    sp = u.reg_read(UC_X86_REG_ESP)
    run_checked(u,0x752210,0x752246)
    assert u.reg_read(UC_X86_REG_ESP)==sp-4
    u.reg_write(UC_X86_REG_ESP,sp)
    u.mem_write(0xB1D4A4,dwords(f.allocate(4096),1024,1,0,10))
    f.ordered_ini(physical)
    u.mem_write(SP,dwords(RET_MAGIC))
    u.reg_write(UC_X86_REG_ESP,SP)
    u.reg_write(UC_X86_REG_ECX,INI)
    run_checked(u,0x753000,RET_MAGIC,count=100_000_000,timeout_us=60_000_000)
    assert u.reg_read(UC_X86_REG_ESP)==SP+4
    stream = f.allocate(0xA8)
    # Supplied initialized stream shell, no backend/file/channel. The
    # original queued-action route is paused before PlayNext's device checks.
    u.mem_write(stream,bytes(0xA8))
    u.mem_write(0xB1D4CC,dwords(stream))
    # VoiceSystem_Init752290 prefix initializes every list, the pending slot,
    # suspend depths and cleanup guardB1D4BC. Stop before stream setup, whose
    # channel/file/backend contract is outside this paused queue witness.
    sp = u.reg_read(UC_X86_REG_ESP)
    run_checked(u,0x752290,0x7522EC)
    assert u.reg_read(UC_X86_REG_ESP)==sp
    u.mem_write(0xA8ED64,b'\1')
    u.mem_write(0xB1D4C0,dwords(0))
    u.mem_write(0xB1D4C4,dwords(0))
    return stream


def eva_snapshot(f):
    u = f.u
    def entry(pointer):
        return None if pointer==0 else dict(name=f.string(pointer),
            type=f.integer(pointer+0x4C),priority=f.integer(pointer+0x48),
            state=f.integer(pointer+0x50))
    def node(pointer):
        return dict(entry=entry(f.read32(pointer+0xC)),
                    priority=f.integer(pointer+0x14),type=f.integer(pointer+0x18),
                    sequence=f.integer(pointer+0x1C))
    def fifo(head):
        output = []
        pointer = f.read32(head)
        while pointer and pointer!=head:
            output.append(node(pointer))
            pointer = f.read32(pointer)
            assert len(output)<=64
        return output
    pending = f.read32(0xB1D4B8)
    return dict(current=entry(f.read32(0xB1D4C4)),
                pending=None if pending==0 else node(pending),
                interrupts=fifo(0xB1D3C8),critical=fifo(0xB1D3F0),
                queues={str(n):fifo(0xB1D450+n*12) for n in range(4)},
                sequence=f.integer(0xB1D4C0),suspend=f.integer(0xB1D3D8),
                side=f.integer(0xB1D4C8),
                pause_flag=u.mem_read(0xA8ED64,1)[0],
                wait=list(struct.unpack('<2I',u.mem_read(0xB1D4D0,8))))


def execute_reference(raw, *, ini_capacity=None):
    f = ActionFixture()
    for kind in REFERENCE_REGISTRIES:
        f.initialize_references(kind)
    f.allocations.clear(); f.calls.clear()
    if ini_capacity is not None:
        f.u.mem_write(HEAP+0x7000,b'TEST_ACTION\0')
        f.u.mem_write(HEAP+0x71F8,b'TEST\0')
    output = f.read_action(raw,ini_capacity=ini_capacity)
    references = {}
    for kind,offset in (('team',0x30),('trigger',0x50),('tag',0x4C)):
        pointer = f.read32(ACTION+offset)
        definitions = f.definitions(kind)
        references[kind] = None if pointer==0 else dict(
            registry_index=definitions.index(pointer),id=f.string(pointer+0x24))
    returned = None
    if output['kind'] in (22,53,54):
        # Live instance fanout/timers have their existing tag_lifecycle owner.
        returned = f.call(0x6DD8B0,[0,0,0,0],ecx=ACTION)&255
    output.update(case_kind='reference_read',references=references,returned=returned,
                  allocations=f.allocations,calls=f.calls,
                  registry_ids={kind:[f.string(p+0x24) for p in f.definitions(kind)]
                                for kind in REFERENCE_REGISTRIES})
    if ini_capacity is not None: output['ini_capacity'] = ini_capacity
    f.check_code()
    return output


def reference_cases():
    rows = []
    for kind,typ,name in ((7,1,'0BD6841C'),(4,5,'0BD6841C'),(53,2,'0DD8E18C'),
                          (54,2,'0DD8E18C'),(22,2,'0DD8E18C'),(19,3,'0B76DE5C')):
        operands = [name,name.lower(),'-1',' -1junk','-1tail','4294967295',
                    '0','00','1',' 1','2','0 ','000','none','<none>',
                    ' '+name,name+' ', 'none ', 'AbsentDefinition',
                    'LongDefinitionReference01234567890123456789']
        rows += [execute_reference(f'{kind},{typ},{operand},41,42,43,44,PQ')
                 for operand in operands]
    rows += [execute_reference('53,2,,0,41,42,43,44,PQ'),
             execute_reference('53,2,LongTriggerReference0123,41,42,43,44,PQ'),
             execute_reference('7,1,LongTeamReference0123456,41,42,43,44,PQ')]
    rows += [execute_reference(raw,ini_capacity=512) for raw in (
        '53,2,0DD8E18C,41,42,43,44,PQ',
        '53,2,0DD8E18C,41,42,43,44,PQ'+'z'*550)]
    f = ActionFixture()
    f.u.mem_write(ACTION,b'\xA5'*0x94)
    f.call(0x6DD000,[],ecx=ACTION)
    rows.append(dict(case_kind='reference_constructor',kind=f.integer(ACTION+0x2C),
        value=f.integer(ACTION+0x90),waypoint=f.integer(ACTION+0x44),
        references={kind:None if f.read32(ACTION+offset)==0 else f.read32(ACTION+offset)
                    for kind,offset in (('team',0x30),('trigger',0x50),('tag',0x4C))}))
    f.check_code()
    # Missing/empty Actions values end before any TAction operand read. Pin
    # real ReadString's empty default independently of malformed atoi(NULL).
    for raw in (None,''):
        f = ActionFixture()
        f.u.mem_write(HEAP+0x7000,b'TEST_ACTION\0')
        f.ini(HEAP+0x7000,raw)
        length=f.call(0x528A10,[TYPE+0x1F8,HEAP+0x7000,0x889F64,BUFFER,512],ecx=INI)
        rows.append(dict(case_kind='reference_ini_read',raw=raw,ini_capacity=512,
                         length=length,read=f.string(BUFFER)))
        f.check_code()
    return rows


def eva_cases():
    f = ActionFixture()
    physical,_identity = physical_eva()
    initialize_eva(f,physical)
    u = f.u
    observed = []
    def observe(_u,pc,_size,_data):
        if pc==0x752480:
            observed.append(dict(call='QueueVoice',index=f.integer_reg(UC_X86_REG_ECX),
                type_override=f.integer_reg(UC_X86_REG_EDX),
                priority_override=f.integer(u.reg_read(UC_X86_REG_ESP)+4)))
        elif pc in (0x753250,0x752590,0x752760,0x752370,0x407F40):
            observed.append(dict(call=f'{pc:08X}'))
    from unicorn.x86_const import UC_X86_REG_EDX
    hook = u.hook_add(UC_HOOK_CODE,observe)
    names = ('EVA_EstablishBattlefieldControl','Mis_xS1_ZofiaAlcatrazIntro',
             'EVA_BattlefieldControlOnline','EVA_UnitReady')
    rows = []
    raws = [f'21,6,{name},0,0,0,0,A' for name in names]
    raws += [f'21,6,{name},41,42,43,44,PQ' for name in (
        'eva_establishbattlefieldcontrol','EVA_EstablishBattlefieldControl ',
        ' EVA_EstablishBattlefieldControl','-1','15','none','<none>',
        'AbsentEVA','0','Mis_xS1_ZofiaAlcatrazIntro',
        'mis_xs1_zofiaalcatrazintro')]
    for raw in raws:
        f.call(0x752370,[])
        observed.clear()
        output = f.read_action(raw)
        value = output['value']
        p = f.read32(f.read32(0xB1D4A4)+value*4) if 0<=value<f.integer(0xB1D4B0) else 0
        entry = None
        if p:
            columns = {}
            for side in (0,1,2):
                f.call(0x7534E0,[],ecx=side)
                columns[str(side)] = f.string(f.call(0x753380,[],ecx=p))
            entry = dict(name=f.string(p),type=f.integer(p+0x4C),
                         priority=f.integer(p+0x48),columns=columns)
            f.call(0x7534E0,[],ecx=1)
        output.update(case_kind='eva_read_dispatch',registry_count=f.integer(0xB1D4B0),
                      entry=entry,before=eva_snapshot(f))
        output['returned'] = f.call(0x6DD8B0,[0,0,0,0],ecx=ACTION)&255
        output.update(after=eva_snapshot(f),calls=list(observed))
        rows.append(output)
    # Retained ordered requests show original duplicate, priority, interrupt
    # clearing and cleanup effects. The supplied pause prevents device IO.
    f.call(0x752370,[])
    history = []
    for label,raw,suspend,current in (
        ('lock_cue','21,6,EVA_EstablishBattlefieldControl,0,0,0,0,A',0,None),
        ('duplicate_lock_cue','21,6,EVA_EstablishBattlefieldControl,0,0,0,0,A',0,None),
        ('intro_cue','21,6,Mis_xS1_ZofiaAlcatrazIntro,0,0,0,0,A',0,None),
        ('unlock_cue','21,6,EVA_BattlefieldControlOnline,0,0,0,0,A',0,None),
        ('same_as_current','21,6,EVA_BattlefieldControlOnline,0,0,0,0,A',0,'EVA_BattlefieldControlOnline'),
        ('interrupt_current','21,6,Mis_xS1_ZofiaAlcatrazIntro,0,0,0,0,A',0,None),
        ('suspended_cue','21,6,EVA_EstablishBattlefieldControl,0,0,0,0,A',1,None),
    ):
        u.mem_write(0xB1D3D8,dwords(suspend))
        if current:
            u.mem_write(BUFFER,current.encode('ascii')+b'\0')
            value=f.call(0x753250,[],ecx=BUFFER)
            u.mem_write(0xB1D4C4,dwords(f.read32(f.read32(0xB1D4A4)+value*4)))
        observed.clear()
        output = f.read_action(raw)
        before = eva_snapshot(f)
        returned = f.call(0x6DD8B0,[0,0,0,0],ecx=ACTION)&255
        history.append(dict(label=label,raw=raw,before=before,after=eva_snapshot(f),
                            returned=returned,calls=list(observed)))
    observed.clear()
    f.call(0x752370,[])
    history.append(dict(label='original_queue_cleanup',after=eva_snapshot(f),calls=list(observed)))
    rows.append(dict(case_kind='eva_history',history=history))
    u.hook_del(hook)
    f.check_code()
    return rows


def execute_input(case):
    f = ActionFixture()
    u = f.u
    display = 0x87F7E8
    mouse,keyboard,tactical = (HEAP+n for n in (0x30000,0x31000,0x32000))
    # Supplied empty construction placement, initialized MouseClass/WWMouse
    # vtables and Tactical band rectangle. No drawing surface or mouse image
    # is attached; all mode/capture/cursor-depth/queue instructions execute.
    # Original MouseClass ctor5BDA40 installs7E1964 and zeroes these fields.
    # Its RevertCursor slot50 is ret0, unlike the ScrollClass base's ret8.
    u.mem_write(display,dwords(0x7E1964))
    u.mem_write(display+0x555C,b'\0')
    u.mem_write(display+0x5560,dwords(0,0,0))
    # Original SetCursor5BDC80 takes the initialized-but-no-SHP early exit;
    # the cursor resource/presentation path is an explicit fixture boundary.
    u.mem_write(0xABF2DD,b'\1')
    u.mem_write(0xABF294,dwords(0))
    u.mem_write(mouse,dwords(0x7F7B2C))
    u.mem_write(mouse+0xC,dwords(case.get('show_level',0)))
    u.mem_write(mouse+0x10,b'\0')
    u.mem_write(0x887640,dwords(mouse))
    u.mem_write(0x87F770,dwords(keyboard))
    u.mem_write(keyboard+0x314,dwords(3,9))
    u.mem_write(0x887324,dwords(tactical))
    u.mem_write(tactical+0xD90,dwords(12,23,34,45))
    u.mem_write(display+0x11CF,b'\1\1')
    u.mem_write(display+0x555A,b'\1')
    u.mem_write(display+0x117C,dwords(0))
    u.mem_write(display+0x11B0,bytes(case.get('modes',[1,0,1,1,1])))
    u.mem_write(SCENARIO+0x35A2,bytes([case.get('scenario_locked',0)]))
    u.mem_write(0xA8ED9C,bytes([case.get('mirror_locked',0)]))
    u.mem_write(0x88098C,dwords(7,8,9))
    u.mem_write(0x88099A,b'\1\1\1')
    u.mem_write(0xB07784,dwords(71))
    # Empty Windows messages and single-thread cursor mutex are the only OS
    # answers. Native GetShowLevel/HideCursor/ShowCursor stay original.
    u.mem_write(0xB73550,dwords(0x1234))
    u.mem_write(0xAA0444,dwords(0))
    imports = {name:SCRATCH+0xD900+n*0x20 for n,name in enumerate(
        ('wait','release_mutex','get_capture','release_capture','show_cursor','peek_message'))}
    for slot,name in ((0x7E11DC,'wait'),(0x7E1198,'release_mutex'),
                      (0x7E13B0,'get_capture'),(0x7E13F8,'release_capture'),
                      (0x7E13DC,'show_cursor'),(0x7E1484,'peek_message')):
        u.mem_write(slot,dwords(imports[name]))
    capture = 0x1234 if case.get('captured',True) else 0
    os_level = case.get('os_show_level',case.get('show_level',0))
    observed = []
    watch = {0x4A8BF0:'clear_placement',0x4AC8C0:'clear_repair_mode',
        0x4AC660:'clear_sell_mode',0x684240:'lock',0x684290:'unlock',
        0x6938C0:'cancel_mouse_drag',0x4AEAD0:'cancel_band_box',
        0x6DA160:'clear_band_rect',0x7B89F0:'get_show_level',
        0x7B9930:'hide_cursor',0x7B9750:'show_cursor',0x54F720:'clear_keyboard',
        0x5D4D50:'process_messages'}
    def observe(_u,pc,_size,_data):
        nonlocal capture,os_level
        if pc in watch:
            observed.append(dict(call=watch[pc]))
        for name,sink in imports.items():
            if pc!=sink: continue
            sp=u.reg_read(UC_X86_REG_ESP)
            if name=='wait':
                args=[f.read32(sp+n) for n in (4,8)]
                assert args[1]==10000
                observed.append(dict(os=name,args=args,result=0))
                return_from_sink(u,8,0)
            elif name=='release_mutex':
                observed.append(dict(os=name,result=1))
                return_from_sink(u,4,1)
            elif name=='get_capture':
                observed.append(dict(os=name,result=capture))
                return_from_sink(u,0,capture)
            elif name=='release_capture':
                observed.append(dict(os=name,prior=capture,result=1))
                capture=0
                return_from_sink(u,0,1)
            elif name=='show_cursor':
                show=f.read32(sp+4)
                assert show in (0,1)
                os_level += 1 if show else -1
                observed.append(dict(os=name,show=show,result=os_level))
                return_from_sink(u,4,os_level)
            else:
                assert name=='peek_message'
                observed.append(dict(os=name,result=0))
                return_from_sink(u,20,0)
            return
    hook=u.hook_add(UC_HOOK_CODE,observe)
    def snapshot():
        return dict(scenario_locked=u.mem_read(SCENARIO+0x35A2,1)[0],
            mirror_locked=u.mem_read(0xA8ED9C,1)[0],
            captured=u.mem_read(display+0x555A,1)[0],os_capture=capture,
            show_level=f.integer(mouse+0xC),os_show_level=os_level,
            band_rect=[f.integer(tactical+0xD90+n*4) for n in range(4)],
            band_flags=list(u.mem_read(display+0x11CF,2)),
            modes=list(u.mem_read(display+0x11B0,5)),
            action_state=[f.integer(0x88098C+n*4) for n in range(3)],
            action_flags=list(u.mem_read(0x88099A,3)),
            keyboard=[f.integer(keyboard+0x314+n*4) for n in range(2)],
            command_latch=f.integer(0xB07784),
            band_update=u.mem_read(0xA8ED9D,1)[0])
    history=[]
    for kind in case['actions']:
        raw=f'{kind},0,0,0,0,0,0,A'
        f.read_action(raw)
        before=snapshot()
        observed.clear()
        returned=f.call(0x6DD8B0,[0,0,0,0],ecx=ACTION)&255
        history.append(dict(kind=kind,returned=returned,before=before,after=snapshot(),
                            calls=list(observed)))
    for level in case.get('show_levels',[]):
        # These supplied scalar controls execute one complete original Show
        # call. MAX/MIN full Unlock would need billions of loop iterations;
        # no shortened/replaced Unlock is represented by these rows.
        u.mem_write(mouse+0xC,dwords(level))
        before=snapshot()
        observed.clear()
        f.call(0x7B9750,[],ecx=mouse)
        history.append(dict(kind='show_cursor',before=before,after=snapshot(),
                            calls=list(observed)))
    u.hook_del(hook)
    f.check_code()
    return dict(case_kind='cursor_show_history' if 'show_levels' in case else 'input_history',
                input=case,history=history)


def input_cases():
    return [execute_input(case) for case in (
        dict(name='lock_repeat_unlock_repeat',actions=[46,46,47,47]),
        dict(name='unlock_multiple_hidden_depth',scenario_locked=1,mirror_locked=1,
             show_level=-3,actions=[47,47]),
        dict(name='unlocked_nonzero_mouse_depth',show_level=-2,actions=[47]),
        dict(name='scenario_authority_over_mirror',scenario_locked=0,mirror_locked=1,
             show_level=-2,actions=[47]),
        dict(name='sell_mode_uncaptured',modes=[0,1,1,1,1],captured=False,actions=[46,47]),
        dict(name='already_hidden_lock',show_level=-2,actions=[46,47]),
        dict(name='noncanonical_lock_byte',scenario_locked=2,mirror_locked=1,
             show_level=-2,actions=[47]),
        dict(name='unlock_positive_depth',scenario_locked=1,mirror_locked=1,
             show_level=1,os_show_level=0,actions=[47,47]),
    )]


def cursor_cases():
    return [execute_input(dict(name='cursor_show_signed_boundaries',actions=[],
        show_levels=[1,2147483647,-2147483648,-1],os_show_level=0))]


def initialize_waypoint_map(f):
    """Prepared sparse map; original startup constants and slope matrices."""
    from tools.spatial_oracle.map_queries import TABLE,EMPTY_TABLE
    f.table=TABLE
    f.cell=f.allocate(0x200)
    f.u.mem_write(f.cell,bytes(0x200))
    f.u.mem_write(TABLE,EMPTY_TABLE)
    f.u.mem_write(0x87F7E8+0x13C,dwords(TABLE,0x40000))
    f.u.mem_write(0x87F924,dwords(TABLE,0x40000))
    f.u.mem_write(0xABDC50,bytes(0x200))
    f.u.mem_write(0x822D80,struct.pack('<H',0x0E7F))
    for start in (0x8129FC,0x814EA8):
        for i in range(14):
            f.call(f.read32(start+i*4),[])
    matrices=slope_matrices()
    for i,row in enumerate(matrices):
        f.u.mem_write(0xB45188+48*i,dwords(*row))
    f.waypoint_pending={}
    f.waypoint_events=[]
    f.waypoint_hook=f.u.hook_add(UC_HOOK_CODE,
        lambda u,pc,n,d: observe_waypoint(f,u,pc))
    return dict(ground_level_leptons=f.integer(0x89E7C0),
                bridge_deck_leptons=f.integer(0xB0E6D4),
                slope_matrices_sha256=hashlib.sha256(
                    b''.join(dwords(*row) for row in matrices)).hexdigest())


def observe_waypoint(f,u,pc):
    if pc in f.waypoint_pending:
        event=f.waypoint_pending.pop(pc)
        if event['call']=='ground':
            event['result']=f.integer_reg(UC_X86_REG_EAX)
        else:
            cell=u.reg_read(UC_X86_REG_EAX)
            event.update(dummy=cell==0xABDC50,
                returned_cell=list(struct.unpack('<hh',u.mem_read(cell+0x24,4))),
                flags=f.read32(cell+0x140))
    sp=u.reg_read(UC_X86_REG_ESP)
    if pc in (0x578080,0x5657A0):
        event=dict(call='ground' if pc==0x578080 else 'packed_lookup',
            coord=list(struct.unpack('<3i' if pc==0x578080 else '<hh',
                u.mem_read(f.read32(sp+4),12 if pc==0x578080 else 4))))
        f.waypoint_events.append(event)
        f.waypoint_pending[f.read32(sp)]=event
    elif pc in (0x6D2420,0x6D6070):
        event=dict(call='focus' if pc==0x6D2420 else 'set_view',
            coord=list(struct.unpack('<3i',u.mem_read(f.read32(sp+4),12))))
        if pc==0x6D2420:event['speed']=f.integer(sp+8)
        f.waypoint_events.append(event)
        return_from_sink(u,8 if pc==0x6D2420 else 4,0)


def install_waypoint_cell(f,case):
    x,y=case['cell']
    f.u.mem_write(f.table,bytes(0x40000*4))
    target=0xABDC50 if case.get('missing') else f.cell
    if target==f.cell:
        f.u.mem_write(f.table+(y*512+x)*4,dwords(f.cell))
    f.u.mem_write(target+0x24,struct.pack('<hh',x,y))
    f.u.mem_write(target+0x11B,bytes((case.get('level',0)&255,case.get('slope',0))))
    f.u.mem_write(target+0x140,dwords(case.get('flags',0)))
    f.u.mem_write(target+0x44,dwords(-1))


def waypoint_cases():
    f=ActionFixture();constants=initialize_waypoint_map(f)
    cases=[dict(name='flat',cell=[10,10]),
           dict(name='slope_1_level_2',cell=[10,10],level=2,slope=1),
           dict(name='slope_4_level_negative',cell=[10,10],level=-1,slope=4),
           dict(name='slope_15_level_127',cell=[11,10],level=127,slope=15),
           dict(name='slope_20',cell=[10,10],slope=20),
           dict(name='bridge_100',cell=[10,10],level=2,slope=1,flags=0x100),
           dict(name='bridge_400',cell=[10,10],level=2,slope=1,flags=0x400),
           dict(name='both_bridge_flags',cell=[10,10],flags=0x500),
           dict(name='missing_positive',cell=[12,11],missing=True,level=-7,slope=1),
           dict(name='negative_dummy_bridge',cell=[-1,-2],missing=True,
                level=-7,slope=1,flags=0x400)]
    rows=[]
    for case in cases:
        install_waypoint_cell(f,case)
        for kind in (48,112):
            read=f.read_action(f'{kind},0,4,0,0,0,0,BH')
            f.u.mem_write(SCENARIO+0x632+read['waypoint']*4,
                          struct.pack('<hh',*case['cell']))
            f.waypoint_events.clear()
            returned=f.call(0x6DD8B0,[0,0,0,0],ecx=ACTION)&255
            rows.append(dict(input=case,read=read,returned=returned,
                             calls=f.waypoint_events.copy()))
    f.check_code()
    return [dict(case_kind='waypoint_coord',constants=constants,rows=rows)]


@lru_cache(maxsize=1)
def physical_animation_inputs():
    root = configured_gamemd().parent
    expansion = mix((root/'expandmd01.mix').read_bytes())
    yr = mix((root/'ra2md.mix').read_bytes())
    local = mix(yr[mix_hash('localmd.mix')])
    ra2 = mix((root/'ra2.mix').read_bytes())
    conquer = mix(ra2[mix_hash('conquer.mix')])
    rules = expansion[mix_hash('RULESMD.INI')]
    art = local[mix_hash('ARTMD.INI')]
    identity = [dict(file=name,archive=archive,bytes=len(raw),
                     sha256=hashlib.sha256(raw).hexdigest()) for name,archive,raw in (
        ('RULESMD.INI','expandmd01.mix',rules),
        ('ARTMD.INI','ra2md.mix -> localmd.mix',art))]
    return sections(rules),sections(art),conquer,identity


def initialize_animation_catalog(f, physical, *, cold=True):
    """Original cold AV/read master or post-reset Process prefix.

    Only physical AudioVisual AnimType-reference keys are included. Other
    families and later ART-reference discovery are outside this prefix.
    """
    u = f.u
    u.mem_write(0x8B4150,dwords(0x7EB6D4,f.allocate(4096),1024,1,0,10))
    u.mem_write(0x889F64,b'\0')
    rules = f.allocate(0x2000)
    u.mem_write(0x8871E0,dwords(rules))
    f.call(0x7C8F5E,[])
    f.call(0x665650,[],ecx=rules)
    keys = ('DropPodPuff','VeinAttack','Dig','AtmosphereEntry','TreeFire',
            'OnFire','Smoke','SmallFire','LargeFire')
    f.ordered_ini({'AudioVisual':{k:v for k,v in physical['AudioVisual'].items()
                                 if k in keys},
                   'Animations':physical['Animations']})
    def names():
        items = f.read32(0x8B4154)
        return [f.string(f.read32(items+n*4)+0x24)
                for n in range(f.integer(0x8B4160))]
    if cold:
        f.call(0x6691E0,[INI],ecx=rules)
    prefix = names()
    u.mem_write(SP,dwords(RET_MAGIC,INI))
    u.reg_write(UC_X86_REG_ESP,SP)
    u.reg_write(UC_X86_REG_ECX,rules)
    run_checked(u,0x6728B0 if cold else 0x668BF0,
                RET_MAGIC if cold else 0x668E9D,
                count=80_000_000,timeout_us=60_000_000)
    if cold:
        assert u.reg_read(UC_X86_REG_ESP)==SP+8
    else:
        # Stop after the real Process call668E98 completes ReadAnimations,
        # before later body/ART discovery. Its unfinished stack is discarded.
        assert u.reg_read(UC_X86_REG_ESP)==SP-0x3C
        u.reg_write(UC_X86_REG_ESP,SP)
    return dict(phase='cold' if cold else 'post_reset_process',
                audio_visual_prefix=prefix,names=names())


def initialize_animation_sounds(f, selected_names=('Explosion06', 'ExplosionShard'), *, sample_ids=None):
    local = mix(mix((configured_gamemd().parent/'ra2md.mix').read_bytes())[
        mix_hash('localmd.mix')])
    raw = local[mix_hash('SOUNDMD.INI')]
    physical = sections(raw)
    selected = {name:physical[name] for name in ('Defaults', *selected_names)}
    selected['SoundList'] = {k:v for k,v in physical['SoundList'].items()
                             if v in selected_names}
    proxy = Sound.__new__(Sound)
    proxy.__dict__ = f.__dict__
    proxy.samples = []
    # The proxy shares the fixture dictionary; every initialization resets
    # this optional IO boundary, including custom-to-default reinitialization.
    proxy.sample_ids = sample_ids
    f.u.mem_write(0x87E2A0,dwords(1))
    system = f.allocate(0x100)
    f.u.mem_write(system,bytes(0x100))
    f.u.mem_write(0x87E294,dwords(system))
    f.call(0x4072C0,[],ecx=0x87E250)
    f.u.mem_write(0xB1D378,dwords(0x7EB6D4,f.allocate(64),16,1,0,10))
    f.ordered_ini(selected)
    hook = f.u.hook_add(UC_HOOK_CODE,
        lambda u,pc,n,d: Sound.hook(proxy,u,pc,n,d) if pc==0x4015C0 else None)
    f.call(0x7510D0,[],ecx=INI)
    f.u.hook_del(hook)
    return dict(file='SOUNDMD.INI',archive='ra2md.mix -> localmd.mix',
                bytes=len(raw),sha256=hashlib.sha256(raw).hexdigest(),selected=selected)


class AnimationFixture(ActionFixture):
    """Compose original action/ART/Anim owners with explicit transport seams.

    Historical animation controls supply Object::Mark admission. Joined
    gameplay controls opt into its original body; IO/OS/display seams remain.
    """
    def __init__(self, *, original_object_mark=False):
        super().__init__()
        # Historical animation-only controls retain their explicit Mark seam.
        # Joined gameplay histories opt into the original admission/state body.
        self.original_object_mark = original_object_mark
        self.rules_input,self.art_input,self.conquer,self.identity=physical_animation_inputs()
        self.catalog=initialize_animation_catalog(self,self.rules_input,cold=False)
        self.sound_identity=initialize_animation_sounds(self)
        self.assets,self.asset_ptr,self.asset_loaded={},{},[]
        self.reader=Reader.__new__(Reader)
        self.reader.__dict__=self.__dict__
        self.events=[]
        self.draw_count=0
        self.pending_draws={}
        self.map_constants=initialize_waypoint_map(self)
        OriginalBridgeConstructor.empty_registries(self.u,self.allocate,capacity=64)
        self.u.mem_write(0xA8E9A8,dwords(0x7EB6D4,self.allocate(256),64,1,0,10))
        self.u.mem_write(0xA8E9A0,b'\1')
        self.u.mem_write(0xA8E7AC,dwords(0))
        self.u.mem_write(0xA8ED84,dwords(1000))
        self.u.mem_write(0xA8EB60,dwords(4))
        rules=self.read32(0x8871E0)
        self.u.mem_write(rules+0x147C,dwords(0))
        self.u.mem_write(rules+0x7B4,dwords(0))
        self.u.mem_map(0,0x1000)
        self.u.mem_write(0,dwords(0xFFFFFFFF))
        self.pointer_probe=RET_MAGIC+0x100
        self.u.mem_write(0x7E115C,dwords(self.pointer_probe))
        self.anim_hook=self.u.hook_add(UC_HOOK_CODE,self.observe_animation)

    def anim_snapshot(self,anim):
        out=constructor_state(self.u,anim)
        # Non-bouncing position/velocity bytes are uninitialized native
        # storage, not an observable gameplay contract to copy into Rust.
        out.pop('bounce');out.pop('bounce_hex');out.pop('marked')
        out['no_effects']=bool(self.u.mem_read(anim+0x198,1)[0])
        out['type']=self.string(self.read32(anim+0xC8)+0x24)
        out['pending_delete_count']=self.integer(0xB0F6A8)
        out['anim_count']=self.integer(0xA8E9B8)
        return out

    def observe_animation(self,u,pc,_size,_data):
        sp=u.reg_read(UC_X86_REG_ESP)
        receiver=u.reg_read(UC_X86_REG_ECX)
        if pc in self.pending_draws:
            self.pending_draws.pop(pc)['result']=self.integer_reg(UC_X86_REG_EAX)
        if pc in (0x65C788,0x65C837):
            self.draw_count+=1
        if pc in (0x65C780,0x65C7E0):
            event=dict(call='next' if pc==0x65C780 else 'ranged',
                site=f'{self.read32(sp)-5:08X}')
            if pc==0x65C7E0:
                event.update(min=self.integer(sp+4),max=self.integer(sp+8))
            self.events.append(event)
            self.pending_draws[self.read32(sp)]=event
        elif pc==0x5B40B0:
            requested=self.string(receiver)
            self.assets[requested.upper()]=self.conquer.get(mix_hash(requested))
            Reader.hook(self.reader,u,pc,_size,_data)
        elif pc==0x5F5850 and not self.original_object_mark:
            return_from_sink(u,4,1)
        elif pc==0x4A9720:
            self.events.append(dict(call='display_submit'))
            return_from_sink(u,4,1)
        elif pc==self.pointer_probe:
            u.mem_read(self.read32(sp+4),self.read32(sp+8))
            return_from_sink(u,8,0)
        elif pc in (0x424CE0,0x424F00,0x4255B0):
            self.events.append(dict(call={0x424CE0:'start',0x424F00:'middle',
                0x4255B0:'uninit'}[pc],no_effects=bool(u.mem_read(receiver+0x198,1)[0])))
        elif pc in (0x405D40,0x406060):
            self.events.append(dict(call='stop_handle' if pc==0x405D40 else 'release_handle',
                handle_present=self.read32(receiver)!=0))
        elif pc==0x7509E0:
            index=receiver
            entry=self.read32(self.read32(0xB1D37C)+index*4)
            native_entry=self.read32(entry)
            name=self.string(native_entry+0x6C)
            self.events.append(dict(call='play_sound',sound=name,index=index,
                coord=list(struct.unpack('<3i',u.mem_read(u.reg_read(UC_X86_REG_EDX),12)))))
            playing_handle(self.reader,native_entry,handle=self.read32(sp+4))
            # Sound playback admission/channel/device is outside this
            # Anim producer witness. Original Start/Stop/Release calls run.
            return_from_sink(u,4,0)
        elif pc==0x7258D0:
            self.events.append(dict(call='detach_all'))

    def read_anim_art(self,index,keys):
        pointer=self.read32(self.read32(0x8B4154)+index*4)
        name=self.catalog['names'][index]
        self.ordered_ini({name:keys})
        mark=len(self.asset_loaded)
        admitted=self.call(0x427D00,[INI],ecx=pointer)
        out=self.reader.result(name,pointer,admitted)
        out.update(z_adjust=self.integer(pointer+0x348),
            stop_sound_index=self.integer(pointer+0x2FC),
            asset_loads=self.asset_loaded[mark:],physical_art_keys=keys)
        return pointer,out


def animation_cases():
    cold=ActionFixture()
    cold_catalog=initialize_animation_catalog(cold,physical_animation_inputs()[0])
    cold.check_code()
    f=AnimationFixture();u=f.u
    names=f.catalog['names'];index=483;name=names[index]
    cases=[dict(name='retail_BH',waypoint='BH',cell=[10,10],flags=0),
           dict(name='retail_BI_bridge',waypoint='BI',cell=[11,10],flags=0x100),
           dict(name='retained_no_effects_next',waypoint='BH',cell=[10,10],flags=0,
                art_overrides={'Report':'Explosion06','StopSound':'ExplosionShard',
                               'Next':names[index-1]},
                next_art_overrides={'Report':'Explosion06','StopSound':'ExplosionShard'})]
    histories=[]
    for case in cases:
        u.mem_write(0xA8ED84,dwords(1000))
        u.mem_write(SCENARIO+0x214,dwords(1000))
        f.call(0x65C6D0,[0],ecx=SCENARIO+0x218)
        f.draw_count=0
        rng_before=bytes(u.mem_read(SCENARIO+0x218,0x3F4)).hex()
        keys=f.art_input[name] | case.get('art_overrides',{})
        pointer,art=f.read_anim_art(index,keys)
        next_art=None
        if case.get('next_art_overrides'):
            next_name=names[index-1]
            _,next_art=f.read_anim_art(index-1,f.art_input[next_name] | case['next_art_overrides'])
        install_waypoint_cell(f,case)
        x,y=case['cell']
        read=f.read_action(f"41,0,{index},0,0,0,0,{case['waypoint']}")
        u.mem_write(SCENARIO+0x632+read['waypoint']*4,struct.pack('<hh',x,y))
        f.events.clear();f.waypoint_events.clear()
        returned=f.call(0x6DD8B0,[0,0,0,0],ecx=ACTION)&255
        anim=f.read32(f.read32(0xA8E9AC))
        initial=f.anim_snapshot(anim)
        constructor_calls=f.events.copy();history=[]
        for frame in range(1000,1100):
            u.mem_write(0xA8ED84,dwords(frame));f.events.clear()
            f.call(0x423AC0,[],ecx=anim)
            history.append(dict(frame=frame,state=f.anim_snapshot(anim),calls=f.events.copy()))
            if not u.mem_read(anim+0x90,1)[0]:break
        else:raise AssertionError('bounded selected animation did not expire')
        rng_after=bytes(u.mem_read(SCENARIO+0x218,0x3F4)).hex()
        f.events.clear();f.call(0x725C70,[])
        cleanup=dict(pending_delete_count=f.integer(0xB0F6A8),
                     anim_count=f.integer(0xA8E9B8),calls=f.events.copy())
        histories.append(dict(input=case,read=read,art=art,next_art=next_art,
            returned=returned,initial=initial,constructor_calls=constructor_calls,
            history=history,cleanup=cleanup,rng_before=rng_before,rng_after=rng_after,
            raw_draw_count=f.draw_count,coordinate_calls=f.waypoint_events.copy()))
    # The zero fallback and explicit override exercise the sole constructor,
    # independently of the caller's late NoEffects store.
    u.mem_write(0xA8ED84,dwords(1000));f.read_anim_art(index,f.art_input[name])
    constructor_controls=[]
    coord=f.allocate(12);u.mem_write(coord,dwords(2688,2688,0))
    for z_adjust in (0,-30,11,-2147483648):
        anim=f.allocate(0x1C8);f.events.clear()
        f.call(0x421EA0,[pointer,coord,0,1,0x600,z_adjust,0],ecx=anim)
        constructor_controls.append(dict(input_z_adjust=z_adjust,state=f.anim_snapshot(anim),
                                          calls=f.events.copy()))
        f.call(0x4255B0,[],ecx=anim);f.call(0x725C70,[])
    f.check_code()
    return [dict(case_kind='anim_catalog',catalog=cold_catalog),
            dict(case_kind='anim_catalog',catalog=f.catalog,retail=f.identity),
            dict(case_kind='anim_history',histories=histories,
                 constructor_controls=constructor_controls,sound=f.sound_identity,
                 map_constants=f.map_constants)]


def message_cases():
    f=ActionFixture();u=f.u
    u.mem_write(0x822D80,struct.pack('<H',0x0E7F))
    f.call(0x7C8F5E,[])
    rules=f.allocate(0x2000)
    u.mem_write(0x8871E0,dwords(rules))
    f.call(0x665650,[],ecx=rules)
    constructor_delay=bytes(u.mem_read(rules+0x14C0,8)).hex()
    proxy=Reader.__new__(Reader);proxy.__dict__=f.__dict__
    csf=install_csf(proxy,['DESC:SOV1'])
    atomic=SimpleNamespace(u=u,imports=[f.allocate(16),f.allocate(16)],read=f.integer)
    u.mem_write(0x7E11C8,dwords(*atomic.imports))
    events=[]
    def utf16(pointer):
        words=[]
        while value:=bytes(u.mem_read(pointer+len(words)*2,2)):
            if value==b'\0\0':break
            words.append(value)
            assert len(words)<2048
        return b''.join(words).decode('utf-16-le')
    def observe(u,pc,n,d):
        sp=u.reg_read(UC_X86_REG_ESP)
        if pc in atomic.imports:
            AtomicTransportOwner.observe(atomic,u,pc,n,d)
        elif pc==0x734E60:
            events.append(dict(call='csf_lookup',label=f.string(u.reg_read(UC_X86_REG_ECX))))
        elif pc==0x5D3BA0:
            args=list(struct.unpack('<7i',u.mem_read(sp+4,28)))
            events.append(dict(call='message_add',receiver=f'{u.reg_read(UC_X86_REG_ECX):08X}',
                x=args[0],y=args[1],text=utf16(args[2]),color=args[3],
                style=args[4],duration=args[5],silent=args[6]))
            return_from_sink(u,28,0)
        elif pc==0x4068E0:
            # Diagnostic logging only; missing-CSF construction and CRT
            # formatting execute unchanged before this external sink.
            return_from_sink(u,0,0)
    hook=u.hook_add(UC_HOOK_CODE,observe)
    histories=[]
    delay_inputs=(None,'0.6','0','-0.6','1.25','2386093','-2386093')
    for delay in delay_inputs:
        f.call(0x665650,[],ecx=rules)
        # The absent key exercises the actual retained constructor default.
        f.ordered_ini({'AudioVisual':{} if delay is None else {'MessageDelay':delay}})
        f.call(0x6691E0,[INI],ecx=rules)
        bits=bytes(u.mem_read(rules+0x14C0,8)).hex()
        for side in (0,1,2):
            u.mem_write(SCENARIO+0x34B8,dwords(side))
            read=f.read_action('11,4,DESC:SOV1,0,0,0,0,A')
            label=f.string(ACTION+0x6D)
            events.clear()
            returned=f.call(0x6DD8B0,[0,0,0,0],ecx=ACTION)&255
            histories.append(dict(input_delay=delay,delay_f64_le_hex=bits,side=side,
                read=read,label=label,returned=returned,calls=events.copy()))
    label_controls=[]
    for label in ('  x  ','  x ','\t x\t ','-1',' -1tail','none',
                  'abcdefghijklmnopqrstuvwxyz0123456789',' ', ' desc:sov1 '):
        read=f.read_action(f'11,4,{label},0,0,0,0,A')
        materialized=f.string(ACTION+0x6D)
        events.clear()
        returned=f.call(0x6DD8B0,[0,0,0,0],ecx=ACTION)&255
        label_controls.append(dict(input=label,read=read,label=materialized,
            returned=returned,calls=events.copy()))
    # Constructor-only missing definition: Execute checks the empty inline
    # label. A missing strtok operand feeding atoi(NULL) is intentionally out
    # of bounds, rather than shortened or replaced by a fixture default.
    u.mem_write(ACTION,bytes(0x94));f.call(0x6DD000,[],ecx=ACTION)
    u.mem_write(ACTION+0x2C,dwords(11));events.clear()
    returned=f.call(0x6DD8B0,[0,0,0,0],ecx=ACTION)&255
    label_controls.append(dict(input=None,label=f.string(ACTION+0x6D),
                               returned=returned,calls=events.copy()))
    u.hook_del(hook);f.check_code()
    return [dict(case_kind='localized_message',constructor_delay_f64_le_hex=constructor_delay,
                 csf=csf,histories=histories,label_controls=label_controls)]


def reveal_radius_reader(physical_rules, physical_map):
    """Original retained General scalar kernel; cache/file seams stay explicit."""
    f=ActionFixture();u=f.u
    u.reg_write(UC_X86_REG_ESI,RULES)
    u.reg_write(UC_X86_REG_ESP,SP)
    run_checked(u,0x665EDE,0x665EE8)
    constructor=f.integer(RULES+0x62C)
    for stream,seed in ((SCENARIO+0x218,31),(0x886B88,31),(0xABE890,31)):
        f.call(0x65C6D0,[seed],ecx=stream)
    rng_before=f.rng_hashes();rng_calls=[]
    physical_empty='[General]\nRevealTriggerRadius=\n'
    physical_blank='[General]\nRevealTriggerRadius= \t \n'
    layers=[('retail_rulesmd',{'General':physical_rules.get('General',{})},None),
        ('retail_map',{'General':physical_map.get('General',{})},None),
        ('missing_section',{},None),('missing_key',{'General':{}},None),
        ('physical_empty_lexical_cache',sections(physical_empty.encode()),physical_empty),
        ('physical_blank_lexical_cache',sections(physical_blank.encode()),physical_blank),
        ('wrong_key_case',{'General':{'revealtriggerradius':'2'}},None),
        ('wrong_section_case',{'general':{'RevealTriggerRadius':'2'}},None),
        ('stored_empty_cache',{'General':{'RevealTriggerRadius':''}},None),
        ('restore_nonzero_prior',{'General':{'RevealTriggerRadius':'9'}},None),
        ('malformed_with_nonzero_prior',{'General':{'RevealTriggerRadius':'junk'}},None),
        ('missing_after_malformed',{},None),
        ('signed_suffix',{'General':{'RevealTriggerRadius':' -3tail'}},None),
        ('missing_retains_signed',{'General':{}},None),
        ('hex_signed',{'General':{'RevealTriggerRadius':'$FFFFFFFD'}},None),
        ('zero',{'General':{'RevealTriggerRadius':'0'}},None),
        ('zero_x_atoi',{'General':{'RevealTriggerRadius':'0x10'}},None),
        ('large_positive',{'General':{'RevealTriggerRadius':'99'}},None),
        ('integer_wrap',{'General':{'RevealTriggerRadius':'4294967297'}},None),
        ('ordered_layer_one',{'General':{'RevealTriggerRadius':'7junk'}},None),
        ('ordered_absent_layer',{},None),
        ('ordered_final_override',{'General':{'RevealTriggerRadius':'11'}},None)]
    reads=[]
    def observe(_u,pc,_size,_data):
        if pc==0x5276D0:
            sp=u.reg_read(UC_X86_REG_ESP)
            reads.append(dict(section=f.string(f.read32(sp+4)),
                key=f.string(f.read32(sp+8)),default=f.integer(sp+12)))
        elif pc in (0x65C780,0x65C7E0):rng_calls.append(f'{pc:08X}')
    hook=u.hook_add(UC_HOOK_CODE,observe)
    rows=[]
    for name,values,text in layers:
        f.ordered_ini(values)
        prior=f.integer(RULES+0x62C);reads.clear()
        for reg,value in ((UC_X86_REG_ESI,RULES),(UC_X86_REG_EDI,INI),
                          (UC_X86_REG_ESP,SP)):
            u.reg_write(reg,value)
        run_checked(u,0x67181C,0x67183C,required_addresses=(0x5276D0,))
        rows.append(dict(name=name,sections=values,physical_text=text,prior=prior,
            value=f.integer(RULES+0x62C),reads=reads.copy()))
    u.hook_del(hook);f.check_code()
    return dict(key='RevealTriggerRadius',section='General',constructor=constructor,
                rows=rows,rng_before=rng_before,rng_after=f.rng_hashes(),rng_calls=rng_calls,
                layer_scope='Declared sequential Rules/map and synthetic cache passes; full Process chronology and physical INI loader excluded')


@lru_cache(maxsize=1)
def retail_definition_fixture():
    """One retained native fixed-AI/map Type read transaction for this producer."""
    from tools.spatial_oracle.tag_lifecycle import StockPrefix
    s=StockPrefix(1);m,u=s.m,s.u
    for fixed,physical in ((True,s.physical_ai),(False,s.physical_map)):
        chosen={'TeamTypes':physical['TeamTypes']}
        chosen.update({name:physical[name] for name in physical['TeamTypes'].values()
                       if name in physical})
        m.ordered_ini(chosen);s.list_call(0x6F19B0,0x6F1A8E,fixed=fixed)
    m.ordered_ini({name:s.physical_map[name] for name in
                   ('Triggers','Events','Actions','Tags','CellTags')})
    s.list_call(0x7275D0,0x727684);s.list_call(0x6E5ED0,0x6E5FBE)
    return s


def retail_reveal_definitions():
    """Use the existing original stock Type reader, not a second definition parser."""
    s=retail_definition_fixture();m,u=s.m,s.u
    rows=[];waypoint_indices=set();action16_count=0
    for pointer in s.vector(0x8B4178):
        row=s.selected_trigger(s.type_name(pointer))
        action16_count+=sum(a['kind']==16 for a in row['actions'])
        if not (row['events']==[dict(kind=47,value=0)] and
                any(a['kind']==17 for a in row['actions'])):
            continue
        node=m.read32(pointer+0xB0)
        for action in row['actions']:
            action['waypoint_44']=struct.unpack('<i',u.mem_read(node+0x44,4))[0]
            if action['kind']==17:waypoint_indices.add(action['value'])
            node=m.read32(node+0x28)
        rows.append(row)
    identities={row['id'] for row in rows}
    tags=[tag for tag in s.definitions() if tag['trigger'] in identities]
    waypoints={};m.ordered_ini({'Waypoints':s.physical_map['Waypoints']})
    for index in sorted(waypoint_indices):
        u.mem_write(s.sp+0x10,str(index).encode()+b'\0')
        for reg,value in ((UC_X86_REG_EBX,s.ini),(UC_X86_REG_ESI,index),
                          (UC_X86_REG_EDI,s.scenario+0x632+index*4)):
            u.reg_write(reg,value)
        m.region(0x68BDE7,0x68BE35,required=(0x5276D0,))
        waypoints[str(index)]=list(struct.unpack('<hh',
            u.mem_read(s.scenario+0x632+index*4,4)))
    m.check_code()
    return dict(sources=s.sources,triggers=rows,tags=tags,waypoints=waypoints,
        action17_count=sum(a['kind']==17 for r in rows for a in r['actions']),
        action16_count_all_definitions=action16_count,
        map_size_lexical=s.physical_map['Map'],
        flags_scope='Native ctor and INI9C..A0 flags; live classification, difficulty admission and opening poll not executed by this row'),s.physical_map


def execute_trigger_reveal(case):
    """Full TAction17/selected18 through real area/Cell knowledge; no leaf sink.

    Geometry storage reuses reveal_area's diamond/window contract. Full native
    Cell construction supplies counters/cache/vtables; explicit level/flags,
    minimal Houses and Map dirty-screen admission are supplied world priors.
    """
    f=ActionFixture();u=f.u;constants=initialize_waypoint_map(f)
    u.hook_del(f.waypoint_hook)
    # Active CRT entries814E58/814E68/814EA0 produce the height-to-screen
    # multiplier before area5678E0 calls AdjustForZ6D20E0. The earlier
    # prototype omitted this prior and is retained only in rejected scratch.
    for entry in (0x6D1830,0x6D18C0,0x6D1BB0,
                  0x49F2F0,0x561710,0x5617A0,0x5617C0,0x5617E0,
                  0x561910,0x5638D0):
        f.call(entry,[])
    assert struct.unpack('<Q',u.mem_read(0xB0CD48,8))[0]==STANDARD_Z_MULTIPLIER_BITS
    constants.update(trigger_level_leptons_B0E6E0=f.integer(0xB0E6E0),
        area_level_leptons_ABDE88=f.integer(0xABDE88),
        adjust_for_z_multiplier_hex=bytes(u.mem_read(0xB0CD48,8)).hex(),
        projection_initializers=['006D1830','006D18C0','006D1BB0'],
        cell_diagonal_hex=bytes(u.mem_read(0xB0CD78,8)).hex(),
        rad60_hex=bytes(u.mem_read(0xB0CD88,8)).hex(),
        native_neighbor_lut_sha256=hashlib.sha256(u.mem_read(0x7F4194,256)).hexdigest())
    u.mem_map(REVEAL_CELLS,0x200000)
    size=(58,140);center=(68,91)
    u.mem_write(0x87F7E8+0xF4,dwords(*size))
    # Original6DA7D0 takes its no-dirty-screen path. Native shroud/cache and
    # empty resident-object notification bodies still run, without rendering.
    u.mem_write(0x87F7E8+0xC,dwords(1))
    u.mem_write(0x8871E0,dwords(RULES))
    u.mem_write(RULES+0x62C,dwords(case.get('radius',9)))
    u.mem_write(RULES+0x17EE,b'\1');u.mem_write(RULES+0x17E7,b'\1')
    u.mem_write(0xA8B238,dwords(0));u.mem_write(0xA8B23C,dwords(0))
    u.mem_write(SCENARIO,dwords(0));u.mem_write(0xA8ED84,dwords(0))
    houses={name:f.allocate(0x300) for name in ('player','other')}
    for name,pointer in houses.items():u.mem_write(pointer,bytes(0x300))
    u.mem_write(0xA83D4C,dwords(houses['player']))
    u.mem_write(houses['player']+0x240,bytes([case.get('latch',0)]))
    u.mem_write(houses['other']+0x240,bytes([case.get('other_latch',0)]))
    f.call(0x47BBF0,[],ecx=0xABDC50)
    cells={}
    for y in range(center[1]-REVEAL_WINDOW,center[1]+REVEAL_WINDOW+1):
        for x in range(center[0]-REVEAL_WINDOW,center[0]+REVEAL_WINDOW+1):
            if not in_diamond(size,x,y):continue
            pointer=REVEAL_CELLS+len(cells)*REVEAL_CELL_STRIDE
            u.mem_write(pointer,bytes(REVEAL_CELL_STRIDE))
            f.call(0x47BBF0,[],ecx=pointer)
            u.mem_write(pointer+0x24,struct.pack('<hh',x,y))
            u.mem_write(f.table+(y*512+x)*4,dwords(pointer))
            cells[x,y]=pointer
    source=tuple(case['cell'])
    pointer=0xABDC50 if case.get('missing') else cells[source]
    u.mem_write(pointer+0x24,struct.pack('<hh',*source))
    u.mem_write(pointer+0x11B,bytes([case.get('level',0)&255]))
    u.mem_write(pointer+0x140,dwords(case.get('flags',0)))
    read=f.read_action(case.get('action_raw',
        f"17,0,59,0,0,0,0,{case.get('token','BH')}"))
    u.mem_write(SCENARIO+0x632+read['value']*4,struct.pack('<hh',*source))
    if read['waypoint']!=read['value']:
        u.mem_write(SCENARIO+0x632+read['waypoint']*4,struct.pack('<hh',48,76))
    zone_prior=case.get('zone_prior')
    if zone_prior is not None:
        # Only source topology/bounds/pointer priors are supplied. The actual
        # zone query, local iterator, membership and area bodies execute.
        local=zone_prior['local_size']
        u.mem_write(0x87F7E8+0xFC,dwords(*local))
        stride=sum(size)+1
        plane=f.allocate(stride*stride*4)
        backing=bytearray(stride*stride*4)
        for (x,y) in cells:
            struct.pack_into('<H',backing,(y*stride+x)*4+2,
                2 if x<=zone_prior['cluster_split_x'] else 3)
        u.mem_write(plane,bytes(backing))
        raw=zone_prior['raw_row']
        row=f.allocate(len(raw)*2)
        u.mem_write(row,struct.pack(f'<{len(raw)}H',*raw))
        u.mem_write(0x87F7E8+0x68,dwords(plane,stride*stride))
        u.mem_write(0x87F7E8+0x1C,dwords(row))
        if removed:=zone_prior.get('removed_cell'):
            x,y=removed
            u.mem_write(f.table+(y*512+x)*4,dwords(0))
    for stream in (SCENARIO+0x218,0x886B88,0xABE890):
        f.call(0x65C6D0,[case['rng_seed']],ecx=stream)
    def state(cell):
        # A selected null-member18 control reads the native current pointer,
        # not the inert old host allocation; IsShrouded's guard remains below.
        p=(f.read32(f.table+(cell[1]*512+cell[0])*4) or 0xABDC50) if zone_prior is not None else cells[cell]
        alt=f.read32(p+0x12C)
        return dict(cell=list(cell),counter=f.integer(p+0x130),gap=f.integer(p+0x134),
            alt=alt,flags=f.read32(p+0x140),
            cache=list(struct.unpack('<bb',u.mem_read(p+0x120,2))),
            shrouded=not bool(alt&8))
    initial=state(next(iter(cells)))
    calls=[];passes=[];rng_calls=[];current_pass=None;pending={};touched=set()
    zone_events=[];zone_pending={}
    def observe(_u,pc,_size,_data):
        nonlocal current_pass
        sp=u.reg_read(UC_X86_REG_ESP)
        if zone_prior is not None:
            key=(pc,sp)
            if key in zone_pending:
                kind,event=zone_pending.pop(key)
                result=u.reg_read(UC_X86_REG_EAX)
                if kind=='next':
                    event['cell']=None if not result else list(struct.unpack('<hh',u.mem_read(result+0x24,4)))
                    event['dummy']=result==0xABDC50
                else:
                    event['zone']=result
            if pc in (0x578710,0x56D230):
                kind='next' if pc==0x578710 else 'zone'
                event=dict(kind=kind)
                if kind=='zone':
                    event.update(cell=list(struct.unpack('<hh',u.mem_read(f.read32(sp+4),4))),
                        row=f.read32(sp+8),check_bridge=f.read32(sp+12))
                zone_events.append(event)
                zone_pending[(f.read32(sp),sp+(4 if kind=='next' else 16))]=(kind,event)
        if pc in pending:
            completed=pending.pop(pc)
            completed['states']=[state(c) for c in sorted(touched)]
            current_pass=None
        if pc==0x68BCC0:
            calls.append(dict(call='waypoint',index=f.integer(sp+8)))
        elif pc==0x5657A0 and current_pass is None:
            calls.append(dict(call='packed_lookup',cell=list(struct.unpack('<hh',
                u.mem_read(f.read32(sp+4),4)))))
        elif pc==0x5678E0:
            args=[f.read32(sp+4+n*4) for n in range(8)]
            event=dict(call='reveal_area',coord=list(struct.unpack('<iii',
                u.mem_read(args[0],12))),radius=args[1],
                house=next(name for name,p in houses.items() if p==args[2]),
                outline=args[3],unreveal=args[4],arg6=args[5],
                by_height=args[6],final=args[7])
            calls.append(event)
            current_pass=dict(event,walk=[])
            passes.append(current_pass);pending[f.read32(sp)]=current_pass
        elif pc==0x653830:
            cell=tuple(struct.unpack('<hh',u.mem_read(f.read32(sp+4),4)))
            assert current_pass is not None and cell in cells,cell
            current_pass['walk'].append(cell);touched.add(cell)
        elif pc in (0x65C780,0x65C7E0):
            rng_calls.append(dict(pc=f'{pc:08X}',stream=u.reg_read(UC_X86_REG_ECX)))
    hook=u.hook_add(UC_HOOK_CODE,observe)
    before=f.rng_hashes();allocations=len(f.allocations)
    cursor=f.read32(SCENARIO+0x214);frame=f.read32(0xA8ED84)
    returned=f.call(0x6DD8B0,[houses[case.get('trigger_house','other')],0,0,0],ecx=ACTION)&255
    assert not pending and not zone_pending
    after=f.rng_hashes();u.hook_del(hook)
    final=[state(c) for c in sorted(touched)]
    world=f.allocate(12)
    # Execute the actual IsShrouded consumer at ordinary worldZ0 after the
    # complete action. Intermediate shrouded fields project its direct +12C8
    # test from the real cells; they do not run a reentrant native query.
    for cell_state in [initial,*final]:
        x,y=cell_state['cell']
        u.mem_write(world,struct.pack('<iii',x*256+128,y*256+128,0))
        observed=bool(f.call(0x586360,[world])&255)
        if cell_state is not initial:assert observed==cell_state['shrouded']
        cell_state['shrouded_query_al']=observed
    for phase in passes:
        phase['walk']=' '.join(f'{x},{y}' for x,y in phase['walk'])
    f.check_code()
    result=dict(name=case['name'],input=case,read=read,returned_al=returned,
        constants=constants,size=list(size),allocated_cells=len(cells),
        initial_cell=initial,calls=calls,passes=passes,final_cells=final,
        rng_before=before,rng_after=after,rng_calls=rng_calls,
        execution_allocations=len(f.allocations)-allocations,
        native_id_before=cursor,native_id_after=f.read32(SCENARIO+0x214),
        frame_before=frame,frame_after=f.read32(0xA8ED84),
        latch_after=u.mem_read(houses['player']+0x240,1)[0])
    if zone_prior is not None:
        result['zone_control']=dict(size=list(size),local_size=list(local),
            zone_row=list(raw),events=zone_events)
    return result


def compact_reveal_zone_control(row):
    """Keep only declared inputs and original outputs consumed by Rust."""
    input=row['input']
    prior=input.get('zone_prior',dict(local_size=[15,50,4,2],
        cluster_split_x=68,raw_row=[0,1,2,3]))
    fields=('cell','counter','gap','alt','flags')
    result=dict(name=row['name'],input={key:input[key] for key in
        ('cell','level','rng_seed','latch') if key in input},
        read=row['read'],returned_al=row['returned_al'],
        size=row['size'],local_size=prior['local_size'],
        cluster_split_x=prior['cluster_split_x'],raw_row=prior['raw_row'],
        removed_cell=prior.get('removed_cell'),
        allocated_cells=row['allocated_cells'],
        next=[event for event in row['zone_control']['events'] if event['kind']=='next'],
        final_cells=[{key:cell[key] for key in fields} for cell in row['final_cells']],
        rng_before=row['rng_before'],rng_after=row['rng_after'],
        rng_calls=row['rng_calls'],frame_before=row['frame_before'],frame_after=row['frame_after'],
        native_id_before=row['native_id_before'],native_id_after=row['native_id_after'],
        latch_after=row['latch_after'])
    return result


def reveal_zone_generate():
    cases=[dict(name='flat',cell=[68,91],level=0),
        dict(name='positive_odd_level5',cell=[68,91],level=5),
        dict(name='negative_level3',cell=[68,91],level=-3),
        dict(name='whole_map_latch_skip',cell=[68,91],level=0,latch=1),
        dict(name='positive_odd_member_level5',cell=[70,95],level=5),
        dict(name='flat_missing_member',cell=[68,91],level=0,removed_cell=[68,91])]
    rows=[]
    for case in cases:
        removed=case.pop('removed_cell',None)
        zone=dict(local_size=[15,50,4,2],cluster_split_x=68,raw_row=[0,1,2,3])
        if removed is not None:zone['removed_cell']=removed
        native=execute_trigger_reveal(dict(case,rng_seed=31,
            action_raw='18,0,495,0,0,0,0,NA',zone_prior=zone))
        rows.append(compact_reveal_zone_control(native))
    return dict(schema_version=1,fixture=dict(size=[58,140],
        allocated_window=[44,67,92,115],
        allocation_admission='original Size diamond',
        cell_constructor=0x47BBF0,dummy_constructor=0x47BBF0,
        cell_initial=dict(counter=1,gap=0,alt=0,flags=0,cache=[-2,-2])),
        controls=rows)


def reveal_zone_metadata():
    return provenance(
        scope='Original Execute18/RevealZone, shared local iterator, Crusher raw-zone query and actual empty-cell area/knowledge consumers on declared synthetic priors.',
        assumptions=[
            'Reuses execute_trigger_reveal and the existing ActionFixture/native CRT/Cell constructors/area transport; no gameplay instruction or returned answer is replaced. LocalSize15,50,4,2, Size58,140, raw cluster split68 and row0,1,2,3 are supplied topology priors, not the retail ALL01 connectivity producer.',
            'Full original constructor/read/Execute18,6E11C0,578850/578710,578460 mode1,56D230 row1/checkBridge0 and5678E0/Cell knowledge run. A missing native member pointer returns shared Dummy and later members continue. The observer reads the current pointer/Dummy and retains the original IsShrouded consistency guard.',
            'Radius2, signed half/quarter source projection, ordered local members, counters/ground knowledge, whole-map latch skip and unchanged RNG/frame/native-ID outputs are pinned. Detailed area walks/ABI receipts remain external. Full House construction, occupied-cell discoveries, nested Tag actions, altered PlayerPtr/cursor/bounds between passes, raw whole-Map save and raster/aircraft output are outside these controls.',
            'No dirty-screen/display submission; ordinary empty-resident notifications still execute. Detailed intermediate states/ABI receipts are external research data; Rust consumes compact declared inputs and original walks/final values. This selected mode leaves legacy, Action63 and Action129 corpus payloads unchanged.',
            'Initial compact vectors were imported from previously executed scratch compositions of this same owner. This import is not a claim that the selected generator has been re-executed or checked in the current turn.'],
        substitutions=[
            'Inherited ActionFixture allocation/Windows/CRT/INI transports are reused. Source LocalSize, retained raw topology rows, minimal House storage and one optional null member pointer are explicit world priors; original gameplay bodies and their answers are not substituted.'],
        entry_points=dict(action_execute=0x6DD8B0,reveal_zone=0x6E11C0,
            local_reset=0x578850,local_next=0x578710,membership=0x578460,
            zone=0x56D230,reveal_area=0x5678E0,cell_knowledge=0x4A9CA0))


@lru_cache(maxsize=1)
def reveal_cases():
    definitions,physical_map=retail_reveal_definitions()
    physical_rules=physical_animation_inputs()[0]
    cases=[dict(name='stock_BH_flat',cell=[68,91],level=0),
        dict(name='stock_BH_level4',cell=[68,91],level=4),
        dict(name='odd_level3',cell=[68,91],level=3),
        dict(name='negative_level3',cell=[68,91],level=-3),
        dict(name='bridge100',cell=[68,91],level=4,flags=0x100),
        dict(name='bridge400',cell=[68,91],level=4,flags=0x400),
        dict(name='both_bridge_flags',cell=[68,91],level=4,flags=0x500),
        dict(name='missing_signed_dummy',cell=[-1,-2],level=-7,flags=0x400,missing=True),
        dict(name='signed_word_subtraction_wrap',cell=[-32768,-32767],level=4,missing=True),
        dict(name='literal59_trailing_BB',cell=[68,91],level=4,token='BB'),
        dict(name='player_latch_skip_other_house',cell=[68,91],level=4,latch=1),
        dict(name='other_latch_does_not_skip_player',cell=[68,91],level=0,other_latch=1),
        dict(name='same_player_trigger_house',cell=[68,91],level=0,trigger_house='player'),
        dict(name='radius_zero',cell=[68,91],level=0,radius=0),
        dict(name='radius_eleven',cell=[68,91],level=0,radius=11),
        dict(name='radius_large_positive',cell=[68,91],level=0,radius=99)]
    controls=[execute_trigger_reveal(dict(case,rng_seed=31)) for case in cases]
    los=[]
    for level in (-1,-128,4,127):
        row=reveal_area_row(center=(68,91),radius=9,by_height=True,size=(58,140),
                           levels=ridge(67,70,88,94,level))
        los.append(dict(name=f'signed_los_level_{level}',**row))
    return [dict(case_kind='trigger_reveal',constants=controls[0]['constants'],
        radius_reader=reveal_radius_reader(physical_rules,physical_map),
        retail_definitions=definitions,controls=controls,los_controls=los)]


def click_shroud_generate():
    """Actual692419 suffix, including native floor, shroud and object pick.

    The production caller69325E has already resolved a click Cell when this
    block starts. Supply that frame, not a synthetic replacement for IsShrouded.
    Original692419..692605 balances its own saved-register frame and returns.
    """
    cases = [
        dict(name='flat_open', level=0, mapped=[[20,20]]),
        dict(name='flat_closed', level=0, mapped=[]),
        dict(name='even_projected_only', level=4, mapped=[[18,18]]),
        dict(name='even_raw_only', level=4, mapped=[[20,20]]),
        dict(name='even_both', level=4, mapped=[[18,18],[20,20]]),
        dict(name='even_closed', level=4, mapped=[]),
        dict(name='odd_first_only', level=3, mapped=[[18,18]]),
        dict(name='odd_second_only', level=3, mapped=[[19,19]]),
        dict(name='odd_raw_only', level=3, mapped=[[20,20]]),
        dict(name='odd_closed', level=3, mapped=[]),
        dict(name='bridge100_flat_open', level=0, flags=0x100, mapped=[[20,20]]),
        dict(name='bridge100_even_projected_only', level=4, flags=0x100, mapped=[[18,18]]),
    ]
    rows=[]
    for case in cases:
        f=DiscoveryFixture();u=f.uc;f.capture=False
        click=(20,20)
        for y in range(17,22):
            for x in range(17,22):
                pointer=f.add_cell((x,y),level=case['level'] if (x,y)==click else 0)
                if [x,y] in case['mapped']:
                    u.mem_write(pointer+0x12C,dwords(f.read32(pointer+0x12C)|8))
        u.mem_write(f.cells[click]+0x140,dwords(case.get('flags',0)))
        tactical,screen,cell_ptr,object_out,shroud_out,fog_out=(
            SCRATCH+offset for offset in (0x72000,0x74000,0x74020,0x74030,0x74040,0x74050))
        # An ordinary foreign, alive, non-limbo/non-cloaked Infantry shell.
        # Discovery remains constructor-produced zero; it is not set by hand
        # to admit the object. No concrete allocation/Unlimbo is represented.
        actor=DISCOVERY_OBJECT
        assert list(u.mem_read(actor+0x41A,3))==[0,0,0]
        u.mem_write(actor+0x90,b'\1');u.mem_write(actor+0x81,b'\0')
        u.mem_write(actor+0x220,dwords(0))
        u.mem_write(actor+0x9C,dwords(click[0]*256+128,click[1]*256+128,
            case['level']*f.constants['level_leptons']))
        u.mem_write(DISCOVERY_TYPE+0xC9A,b'\0')
        u.mem_write(tactical,bytes(0x1000));u.mem_write(tactical+0xDB0,dwords(1))
        u.mem_write(0x887324,dwords(tactical))
        u.mem_write(0xB0CEC8,dwords(actor,100,100))
        u.mem_write(screen,dwords(100,100));u.mem_write(cell_ptr,packed(*click))
        u.mem_write(object_out,dwords(0));u.mem_write(shroud_out,b'\0');u.mem_write(fog_out,b'\0')
        u.mem_write(0xB73550,dwords(1))
        # Original ProcessClickCoords fog branch is disabled for ordinary YR.
        u.mem_write(0xA8B230,dwords(DISCOVERY_SCENARIO))
        u.mem_write(DISCOVERY_SCENARIO,dwords(0))
        sp=STACK_BASE+STACK_SIZE-0x2000
        u.mem_write(sp,bytes(0x100))
        # 0x18-byte locals and four saved registers precede the return address.
        u.mem_write(sp+0x28,dwords(RET_MAGIC))
        u.mem_write(sp+0x38,dwords(object_out))
        u.mem_write(sp+0x3C,dwords(fog_out));u.mem_write(sp+0x40,dwords(shroud_out))
        for register,value in ((UC_X86_REG_ESP,sp),(UC_X86_REG_ESI,cell_ptr),
                               (UC_X86_REG_EDI,fog_out),(UC_X86_REG_EBP,screen)):
            u.reg_write(register,value)
        ranges=((0x692419,0x692606),(0x6DA380,0x6DA553),
                (0x586360,0x586495),(0x578080,0x5780D8))
        original=[bytes(u.mem_read(a,b-a)) for a,b in ranges]
        trace=[];query=[];pick_calls=[]
        def observe(_u,pc,size,data):
            if pc in (0x578080,0x586360):
                current_sp=u.reg_read(UC_X86_REG_ESP)
                point=list(struct.unpack('<iii',u.mem_read(f.read32(current_sp+4),12)))
                query.append(dict(pc=f'{pc:08X}',point=point))
            if pc==0x6DA380:pick_calls.append(f'{pc:08X}')
            if pc in (0x69244C,0x692480,0x6924FC,0x692515,0x692522,0x6925F0):
                trace.append(f'{pc:08X}')
        hook=u.hook_add(UC_HOOK_CODE,observe)
        before=f.rng_hashes();discovery_before=list(u.mem_read(actor+0x41A,3))
        run_checked(u,0x692419,RET_MAGIC,count=10000,
                    required_addresses=(0x578080,0x586360,0x6924FC))
        u.hook_del(hook)
        assert u.reg_read(UC_X86_REG_ESP)==sp+0x44
        assert original==[bytes(u.mem_read(a,b-a)) for a,b in ranges]
        f.check_code()
        rows.append(dict(name=case['name'],input=dict(click_cell=list(click),
            level=case['level'],flags=case.get('flags',0),mapped_cells=case['mapped'],
            target_cell=list(click),target_screen=[100,100],cursor_screen=[100,100],
            discovered_by_current_house=False),
            constants=f.constants,query=query,trace=trace,
            floor_z=next(q['point'][2] for q in query if q['pc']=='00586360'),
            target_raw_mapped=bool(f.read32(f.cells[click]+0x12C)&8),
            returned_al=u.reg_read(UC_X86_REG_EAX)&255,
            shrouded=bool(u.mem_read(shroud_out,1)[0]),
            fogged=bool(u.mem_read(fog_out,1)[0]),pick_calls=len(pick_calls),
            picked_target=f.read32(object_out)==actor,
            discovery_before=discovery_before,
            discovery_after=list(u.mem_read(actor+0x41A,3)),
            rng_before=before,rng_after=f.rng_hashes(),rng_calls=f.rng_calls))
    return dict(controls=rows)


def click_shroud_metadata():
    return provenance(
        scope='Original Display692419..692605 click Cell floor/shroud suffix and Tactical6DA380 pick; supplied screen-to-Cell and registration priors, no whole UI/attack mission parity.',
        assumptions=[
            'Reuse DiscoveryFixture native Cell47BBF0/sparse512-stride table, original projection/Cell startup and constructor discovery prefixes. Click Cell20,20, explicit signed level0/3/4, optional structural bridge flag100 and mapped-ground bit8 inputs on a flat25-cell neighborhood; ordinary YR fog-mode off, shroud enabled. These are controlled knowledge histories, not a retail scenario load.',
            'Scroll69325E calls full ProcessClickCoords692300; its known reachable suffix692419 constructs click Cell center XY, obtains actual Map578080 floor height at69244C, writes that Z at692461, and calls unchanged586360 at692480. Starting at that suffix supplies the upstream Cell conversion and saved-register frame, without skipping any shroud/pick branch.',
            'Native586360 projects level/2 in both axes; odd levels test the native481810 neighbor when the first mapped test fails. Original floor arithmetic, packed map lookup, shroud tests and high-level rejection branches execute. ProcessClickCoords does not add structural bridge height after Map578080; flag100 controls retain actual queryZ. No Python calculation supplies the query or boolean golden.',
            'One declared Tactical registration at screen100,100 references the existing foreign Infantry shell, supplied alive/non-limbo/non-cloaked and ordinary type Invisible=false. Constructor-produced+41A/+41B/+41C are all zero before/after. Actual Pick6DA380, its casts and post-pick ProcessClickCoords cloak/type checks execute. This demonstrates manual pick admission independent of discovered+41B; object allocation/Unlimbo, screen registration production, rendered pixels and later DetermineAction/Attack are excluded.',
            'The active left-release route69325E->692300,693271->692610 and69328B->4AB9B0 passes the same object to action and release. That ordering is established by static instructions, not a whole mouse-message execution here.',
            'Three seeded native RNG states and draw observations remain unchanged. This suffix has no timer write, detach or allocation producer.'],
        substitutions=[
            'Only inherited DiscoveryFixture constructor/platform transport boundaries. No Display suffix, floor height, native shroud, TacticalPick or post-pick gameplay instruction is substituted.'],
        entry_points=dict(click_process=0x692300,suffix=0x692419,floor_call=0x69244C,
            shroud_call=0x692480,shroud=0x586360,pick_call=0x692515,pick=0x6DA380,
            mouse_process_call=0x69325E,mouse_action_call=0x693271,mouse_release_call=0x69328B))


def initialize_discovery_geometry(call, read32, u):
    """Shared original startup group; caller supplies its existing VM transport."""
    entries = [0x6D1830,0x6D18C0,0x6D1BB0,0x49F2F0,
               0x561710,0x5617A0,0x5617C0,0x5617E0,0x561910,0x5638D0]
    for start in (0x8129FC,0x814EA8):
        entries.extend(read32(start+i*4) for i in range(14))
    for entry in entries:call(entry,0,[])
    raw = bytes(u.mem_read(0xB0CD48,8))
    assert struct.unpack('<Q',raw)[0]==STANDARD_Z_MULTIPLIER_BITS
    return dict(entries=[f'{entry:08X}' for entry in entries],b0cd48=raw.hex())


class DiscoveryFixture(DiscoveryEntry):
    """Reuse Entry's constructor prefixes, original Infantry vtable and transport.

    Original visibility replaces Entry's declared visibility answer seam. Cells,
    actor fields and minimal Houses are supplied; allocation/Unlimbo/retail Fill
    are not represented. All selected map/cache/query/discovery bodies run.
    """

    def __init__(self, *, mode=0, foreign=True):
        self.capture=False
        self.rng_calls=[]
        super().__init__(dict(mode=mode,foreign_constructor=foreign,move=False))
        self.events=[];self.lookups=[];self.pending={};self.cells={};self.actors={DISCOVERY_OBJECT:'object'}
        self.area_walks=[];self.current_area=None;self.trigger_auto_area=False
        u=self.uc
        u.mem_map(SCRATCH+0x60000,0x20000)
        self.cell_cursor=SCRATCH+0x68000;self.actor_cursor=SCRATCH+0x60000
        initialize_discovery_geometry(self.call,self.read32,u)
        self.constants=dict(adjust_for_z_multiplier_hex=bytes(u.mem_read(0xB0CD48,8)).hex(),
            cell_diagonal_hex=bytes(u.mem_read(0xB0CD78,8)).hex(),
            rad60_hex=bytes(u.mem_read(0xB0CD88,8)).hex(),
            level_leptons=self.read32(0xABDE88),
            trigger_level_leptons_B0E6E0=self.read32(0xB0E6E0),
            bridge_deck_leptons_B0E6D4=self.read32(0xB0E6D4),
            native_neighbor_lut_sha256=hashlib.sha256(u.mem_read(0x7F4194,256)).hexdigest())
        u.mem_write(DISCOVERY_MAP+0xC,dwords(1))
        u.mem_write(DISCOVERY_SCENARIO,dwords(0))
        u.mem_write(0xA8ED84,dwords(0));u.mem_write(0xA8B23C,dwords(0))
        self.call(0x47BBF0,0xABDC50,[])
        self.add_cell((9,10),pointer=DISCOVERY_CURRENT)
        self.add_cell((10,10),pointer=SCRATCH)
        self.initial_cell=self.cell_state((9,10))
        self.initial_topology=self.topology_input()
        for stream in (DISCOVERY_SCENARIO+0x218,0x886B88,0xABE890):
            self.call(0x65C6D0,stream,[31])
        self.rng_calls=[]
        self.ranges=[(0x47BBF0,0x47BDA8),(0x47E8A0,0x47EA87),
            (0x586360,0x586497),(0x5865E0,0x5866BA),(0x47C3D0,0x47C4C8),
            (0x6F4960,0x6F4A3D),(0x4A9CA0,0x4A9DC9),(0x6D8700,0x6D8ADA),
            (0x6565A0,0x65674E),(0x6562D0,0x6563A2),
            (0x5678E0,0x567D90),(0x487630,0x4876F0),
            (0x44D5D0,0x44D69E),(0x445F80,0x446FCD),(0x6E0FE0,0x6E1172),
            (0x5657A0,0x5657E9),(0x440E5C,0x440E7C),(0x50B730,0x50B753),
            (0x65AE30,0x65AE55),(0x5B3040,0x5B3052),
            (0x4F5746,0x4F574D),(0x4F5789,0x4F578F),(0x4F57DB,0x4F57E4)]
        self.original=[bytes(u.mem_read(a,b-a)) for a,b in self.ranges]
        self.stage='ready';self.capture=True

    def actor_name(self, pointer):
        return self.actors.get(pointer, None if pointer==0 else f'{pointer:08X}')

    def complete_return(self, pc):
        if pc not in self.pending:return
        event=self.pending[pc].pop()
        result=self.uc.reg_read(UC_X86_REG_EAX)
        if event['pc']=='005657A0':
            event.update(returned='dummy' if result==0xABDC50 else 'real',
                returned_cell=list(struct.unpack('<hh',self.uc.mem_read(result+0x24,4))))
        else:event['returned']=self.actor_name(result) if event['pc']=='0047C3D0' else result&255
        if not self.pending[pc]:del self.pending[pc]

    def call(self, entry, this, args):
        super().call(entry,this,args)
        if self.capture:self.complete_return(RET_MAGIC)

    def block(self, begin, end, registers, required=None):
        super().block(begin,end,registers,required)
        if self.capture:self.complete_return(end)

    def observe(self, u, pc, size, data):
        if pc in (0x65C780,0x65C7E0):
            self.rng_calls.append(dict(stage=getattr(self,'stage','constructor'),
                pc=f'{pc:08X}',stream=u.reg_read(UC_X86_REG_ECX)))
        if getattr(self,'fixture_ready',False) and pc in (0x586360,0x5865E0):
            return  # Execute the original visibility body, without patching it.
        if self.capture:
            sp=u.reg_read(UC_X86_REG_ESP)
            if self.current_area is not None and pc==self.current_area.get('_return'):
                del self.current_area['_return']
                self.current_area['walk']=' '.join(f'{x},{y}' for x,y in self.current_area['walk'])
                self.current_area=None
            self.complete_return(pc)
            event=None
            if pc==0x5657A0:
                lookup=dict(stage=self.stage,pc=f'{pc:08X}',
                    cell=list(struct.unpack('<hh',u.mem_read(self.read32(sp+4),4))))
                self.lookups.append(lookup)
                self.pending.setdefault(self.read32(sp),[]).append(lookup)
            if pc in (0x4A9CA0,0x6D8700,0x5865F0,0x6565A0,0x47C3D0,
                      0x6F4960,0x6F497E,0x65C780,0x65C7E0,0x44D5D0,0x445F80):
                event=dict(stage=self.stage,pc=f'{pc:08X}')
                if pc in (0x4A9CA0,0x6D8700,0x6565A0):
                    event['cell']=list(struct.unpack('<hh',u.mem_read(self.read32(sp+4),4)))
                if pc==0x6D8700:event['channel']=self.read32(sp+8)&255
                elif pc==0x5865F0:
                    p=self.read32(sp+4)
                    event.update(cell=list(struct.unpack('<hh',u.mem_read(p+0x24,4))),
                        viewer_current=self.read32(sp+8)==DISCOVERY_PLAYER,
                        fresh=self.read32(sp+12)&255)
                elif pc==0x47C3D0:
                    p=u.reg_read(UC_X86_REG_ECX)
                    event.update(cell=list(struct.unpack('<hh',u.mem_read(p+0x24,4))),
                        point=list(struct.unpack('<ii',u.mem_read(self.read32(sp+4),8))),
                        upper=self.read32(sp+8)&255,exclude=self.actor_name(self.read32(sp+12)))
                elif pc in (0x6F4960,0x6F497E,0x44D5D0):
                    event.update(actor=self.actor_name(u.reg_read(UC_X86_REG_ECX)),
                        viewer_current=self.read32(sp+4)==DISCOVERY_PLAYER)
                elif pc==0x445F80:
                    event.update(actor=self.actor_name(u.reg_read(UC_X86_REG_ECX)),
                        captured=self.read32(sp+4)&255)
                elif pc in (0x65C780,0x65C7E0):
                    event['stream']=u.reg_read(UC_X86_REG_ECX)
                if pc in (0x4A9CA0,0x6D8700,0x47C3D0,0x6F4960,0x44D5D0):
                    self.pending.setdefault(self.read32(sp),[]).append(event)
            elif pc==0x586643:
                p=u.reg_read(UC_X86_REG_ESI)
                event=dict(stage=self.stage,pc=f'{pc:08X}',
                    cell=list(struct.unpack('<hh',u.mem_read(p+0x24,4))),
                    level=struct.unpack('<b',u.mem_read(p+0x11B,1))[0],
                    rung=u.reg_read(UC_X86_REG_EDI),dummy=p==0xABDC50)
            elif pc==0x653830 and self.current_area is not None:
                self.current_area['walk'].append(list(struct.unpack('<hh',u.mem_read(self.read32(sp+4),4))))
            elif pc==0x5678E0 and self.trigger_auto_area:
                args=[self.read32(sp+4+n*4) for n in range(8)]
                phase=dict(stage=self.stage,coord=list(struct.unpack('<iii',u.mem_read(args[0],12))),
                    radius=args[1],final=args[7],walk=[],_return=self.read32(sp))
                self.area_walks.append(phase);self.current_area=phase
            if event is not None:self.events.append(event)
        return super().observe(u,pc,size,data)

    def add_cell(self, cell, *, level=0, pointer=None):
        if pointer is None:pointer=self.cell_cursor;self.cell_cursor+=0x200
        self.uc.mem_write(pointer,bytes(0x200));self.call(0x47BBF0,pointer,[])
        self.uc.mem_write(pointer+0x24,packed(*cell))
        self.uc.mem_write(pointer+0x11B,bytes([level&255]))
        linear=cell[1]*512+cell[0]
        assert 0<=linear<0x40000
        self.uc.mem_write(DISCOVERY_TABLE+linear*4,dwords(pointer))
        self.cells[cell]=pointer
        return pointer

    def add_actor(self, name, *, cell=(9,10), offset=(128,128), upper=False):
        p=self.actor_cursor;self.actor_cursor+=0x1000
        u=self.uc;u.mem_write(p,bytes(0x1000));self.actors[p]=name
        u.mem_write(p,dwords(DISCOVERY_VTABLE))
        u.mem_write(p+0x21C,dwords(DISCOVERY_OTHER));u.mem_write(p+0x6C0,dwords(DISCOVERY_TYPE))
        u.mem_write(p+0x9C,dwords(cell[0]*256+offset[0],cell[1]*256+offset[1],0))
        initialize_discovery_prefix(u,self.block,owner=p)
        self.block(0x6F322F,0x6F323A,{UC_X86_REG_ESI:p,
            UC_X86_REG_EAX:u.mem_read(p+0x14,1)[0]})
        self.call(0x47E8A0,self.cells[cell],[p,int(upper)])
        return p

    def cell_state(self, cell):
        return self.pointer_state(self.cells[cell])

    def pointer_state(self, p, *, include_dirty=False):
        result = dict(cell=list(struct.unpack('<hh',self.uc.mem_read(p+0x24,4))),
            level=struct.unpack('<b',self.uc.mem_read(p+0x11B,1))[0],
            counter=struct.unpack('<i',self.uc.mem_read(p+0x130,4))[0],
            gap=struct.unpack('<i',self.uc.mem_read(p+0x134,4))[0],
            flags=self.read32(p+0x140),alt=self.read32(p+0x12C),
            cache=list(struct.unpack('<bb',self.uc.mem_read(p+0x120,2))))
        if include_dirty:result['dirty'] = self.read32(p+0x138)
        return result

    def topology_input(self):
        return dict(allocated_cells=[self.cell_state(c) for c in sorted(self.cells)],
            dummy_prior=self.pointer_state(0xABDC50))

    def rng_hashes(self):
        proxy=SimpleNamespace(u=self.uc,scenario=DISCOVERY_SCENARIO)
        return {name:hashlib.sha256(bytes.fromhex(value)).hexdigest()
            for name,value in CampaignReader.rng_state(proxy).items()}

    def check_code(self):
        assert self.original==[bytes(self.uc.mem_read(a,b-a)) for a,b in self.ranges]
        assert not self.pending,self.pending

    def reveal_cell(self, cell, final=0, viewer=None):
        self.uc.mem_write(DISCOVERY_OUTPUT,packed(*cell))
        self.call(0x4A9CA0,DISCOVERY_MAP,
            [DISCOVERY_OUTPUT,DISCOVERY_PLAYER if viewer is None else viewer,final])
        return self.uc.reg_read(UC_X86_REG_EAX)&255

    def area(self, *, center, radius, final):
        self.uc.mem_write(DISCOVERY_OUTPUT,dwords(center[0]*256+128,center[1]*256+128,0))
        phase=dict(stage=self.stage,center=list(center),radius=radius,final=final,walk=[])
        self.area_walks.append(phase);self.current_area=phase
        # Reuse the existing ABI/runner; Entry's 30k single-leaf limit is too
        # small for a whole area visit. No native gameplay body is replaced.
        sp=STACK_BASE+STACK_SIZE-0x1000
        self.uc.mem_write(sp,dwords(RET_MAGIC,DISCOVERY_OUTPUT,radius,DISCOVERY_PLAYER,0,0,0,1,final))
        self.uc.reg_write(UC_X86_REG_ESP,sp);self.uc.reg_write(UC_X86_REG_ECX,DISCOVERY_MAP)
        run_checked(self.uc,0x5678E0,RET_MAGIC,count=8_000_000,required_addresses=[0x5678E0])
        assert self.uc.reg_read(UC_X86_REG_ESP)==sp+36
        self.complete_return(RET_MAGIC)
        self.current_area=None
        phase['walk']=' '.join(f'{x},{y}' for x,y in phase['walk'])

    def trigger_reveal(self, index, cell):
        action=SCRATCH+0x67000
        self.uc.mem_write(action,bytes(0x100));self.uc.mem_write(action+0x90,dwords(index))
        self.uc.mem_write(DISCOVERY_SCENARIO+0x632+index*4,packed(*cell))
        sp=STACK_BASE+STACK_SIZE-0x1000
        self.uc.mem_write(sp,dwords(RET_MAGIC,DISCOVERY_OTHER,0,0,0))
        self.uc.reg_write(UC_X86_REG_ESP,sp);self.uc.reg_write(UC_X86_REG_ECX,action)
        run_checked(self.uc,0x6E0FE0,RET_MAGIC,count=12_000_000,
            required_addresses=[0x6E0FE0,0x5678E0])
        assert self.uc.reg_read(UC_X86_REG_ESP)==sp+20 and self.current_area is None


def discovery_placement(case):
    f=DiscoveryFixture(mode=case['mode'],foreign=case['foreign']);u=f.uc
    f.block(0x6F322F,0x6F323A,{UC_X86_REG_ESI:DISCOVERY_OBJECT,
        UC_X86_REG_EAX:u.mem_read(DISCOVERY_OBJECT+0x14,1)[0]})
    f.stage='foot_owner_entry'
    f.block(0x4D7221,0x4D7235,{UC_X86_REG_ESI:DISCOVERY_OBJECT},[0x4D722F,0x6F4960])
    after_entry=f.history()
    if case.get('open_before_put'):
        f.stage='empty_cell_reveal';f.reveal_cell((9,10))
    before=f.rng_hashes();f.stage='cell_put'
    f.call(0x47E8A0,DISCOVERY_CURRENT,[DISCOVERY_OBJECT,0]);after_put=f.history()
    f.stage='nonempty_reveal';returned=f.reveal_cell((9,10));after_reveal=f.history()
    f.stage='repeat_reveal';repeated=f.reveal_cell((9,10));f.check_code()
    return dict(input=dict(case,rng_seed=31,topology=f.initial_topology),initial_cell=f.initial_cell,
        after_entry=after_entry,after_put=after_put,after_reveal=after_reveal,
        returned=returned,repeated=repeated,trace=f.events,cell=f.cell_state((9,10)),
        rng_before=before,rng_after=f.rng_hashes(),rng_calls=f.rng_calls)


def forced_map_generate():
    f=DiscoveryFixture();u=f.uc
    for y in range(9,12):
        for x in range(8,11):
            if (x,y) not in f.cells:f.add_cell((x,y))
    # Supplied frontier: eight initialized neighbors except the last corner;
    # an unrevealed center retains nonzero counters and pending20.
    for cell,p in f.cells.items():
        if cell not in ((9,10),(10,11)):
            u.mem_write(p+0x12C,dwords(8));u.mem_write(p+0x140,dwords(2))
    p=f.cells[(9,10)]
    u.mem_write(p+0x130,dwords(7));u.mem_write(p+0x134,dwords(3))
    u.mem_write(p+0x140,dwords(0x20))
    f.ranges.append((0x4A9890,0x4A9C99))
    f.original.append(bytes(u.mem_read(0x4A9890,0x409)))
    topology=f.topology_input();rows=[]
    for name,cell in (('first_frontier',(9,10)),('last_corner',(10,11)),
                      ('center_cache_change',(9,10)),('center_unchanged',(9,10))):
        f.stage=name;before=f.rng_hashes();event_start=len(f.events)
        u.mem_write(DISCOVERY_OUTPUT,packed(*cell))
        f.call(0x4A9890,DISCOVERY_MAP,[DISCOVERY_OUTPUT,DISCOVERY_PLAYER])
        rows.append(dict(name=name,cell=list(cell),returned_al=u.reg_read(UC_X86_REG_EAX)&255,
            cells=[f.cell_state(c) for c in sorted(f.cells)],
            dummy=f.pointer_state(0xABDC50),
            notifications=[dict(cell=e['cell'],fresh=e['fresh']) for e in f.events[event_start:]
                           if e['pc']=='005865F0'],
            rng_before=before,rng_after=f.rng_hashes(),rng_calls=list(f.rng_calls)))
    f.check_code()
    return dict(schema_version=1,topology=topology,controls=rows)


def forced_map_metadata():
    return provenance(scope='Original Display4A9890, actual6D8700 edge reads and empty-resident5865F0 notifications on a supplied frontier.',
        assumptions=[
            'Reuse DiscoveryFixture original Cell constructors, original neighbor LUT and existing ABI/runner. Declared cell allocation, prior counters/cache/flags, empty residents, frame0 and current minimal House are inputs, not a whole scenario load.',
            'Four visits execute the original leaf unmodified and retain real/dummy cell bytes, changed/fresh notifications and three RNG-stream hashes. Ground/fog channels and unchanged counters are compared in Rust. Map dirty-screen byte1 selects the original6DA7D0 early exit.',
            'Scenario FogOfWar is supplied clear. ALL01 separately has an executed original SpecialFlags reader receipt in tools/input_oracle/campaign_start.json::basic_start; this scalar control does not read retail layers itself. Enabled FogOfWar486BF0/fogged-object lifecycle, Cell140bit40 producers, occupied-cell discovery, nested Tags, PlayerPtr mutation, full Execute16 traversal and rendered pixels are outside coverage.',
            'This selected mode leaves legacy, Action63, Action129 and Action18 corpus payloads unchanged. Detailed instruction traces are external research data; canonical vectors contain only comparison inputs and outputs.'],
        substitutions=['Inherited DiscoveryFixture/Entry allocation, platform/CRT and minimal world transports; no4A9890,6D8700,5865F0 or gameplay return answer is replaced.'],
        entry_points=dict(forced_cell=0x4A9890,edge=0x6D8700,notification=0x5865F0,cell_constructor=0x47BBF0))


def discovery_nearest(case):
    f=DiscoveryFixture();before=f.rng_hashes();f.stage='ordered_cell_put'
    for actor in case['actors']:f.add_actor(actor['name'],offset=actor['offset'],upper=actor.get('upper',False))
    topology=f.topology_input()
    f.stage='notify';f.call(0x5865F0,DISCOVERY_MAP,[DISCOVERY_CURRENT,DISCOVERY_PLAYER,1])
    f.check_code()
    return dict(input=dict(case,rng_seed=31,topology=topology),trace=f.events,
        actors=[dict(name=name,discovery=list(f.uc.mem_read(p+0x41A,3)))
                for p,name in f.actors.items() if p!=DISCOVERY_OBJECT],
        rng_before=before,rng_after=f.rng_hashes(),rng_calls=f.rng_calls)


def discovery_diagonal(case):
    f=DiscoveryFixture();before=f.rng_hashes();f.stage='diagonal_setup'
    source=tuple(case.get('source',[9,10]));pointers=[]
    for i,level in enumerate(case['levels']):
        cell=tuple(((value+i+32768)&65535)-32768 for value in source)
        linear=cell[1]*512+cell[0]
        if not 0<=linear<0x40000:continue
        p=f.add_cell(cell,level=level);pointers.append(p)
        f.add_actor(f'object{i}',cell=cell)
    topology=f.topology_input()
    f.stage='diagonal_notify';f.call(0x5865F0,DISCOVERY_MAP,[pointers[0],DISCOVERY_PLAYER,case.get('fresh',1)])
    f.check_code()
    return dict(input=dict(case,rng_seed=31,topology=topology),trace=f.events,
        actors=[dict(name=name,discovery=list(f.uc.mem_read(p+0x41A,3)))
                for p,name in f.actors.items() if p!=DISCOVERY_OBJECT],
        rng_before=before,rng_after=f.rng_hashes(),rng_calls=f.rng_calls)


def discovery_cache():
    f=DiscoveryFixture();u=f.uc;f.capture=False
    neighbors=[(-1,-1,0x40),(0,-1,0x80),(1,-1,1),(-1,0,0x20),
               (1,0,2),(-1,1,0x10),(0,1,8),(1,1,4)]
    for dx,dy,_ in neighbors:f.add_cell((9+dx,10+dy))
    topology=f.topology_input()
    before=f.rng_hashes();mask_outputs=[];gates=[]
    for channel in (0,1):
        field,bit=(0x12C,8) if channel==0 else (0x140,2)
        u.mem_write(DISCOVERY_CURRENT+field,dwords(bit));outputs=[]
        for mask in range(256):
            for dx,dy,mask_bit in neighbors:
                u.mem_write(f.cells[9+dx,10+dy]+field,dwords(0 if mask&mask_bit else bit))
            u.mem_write(DISCOVERY_OUTPUT,packed(9,10))
            f.call(0x6D8700,0,[DISCOVERY_OUTPUT,channel])
            outputs.append(struct.unpack('<i',dwords(u.reg_read(UC_X86_REG_EAX)))[0])
        mask_outputs.append(outputs)
        for value in ((0,0x10,8,0x18) if channel==0 else (0,1,2,3)):
            u.mem_write(DISCOVERY_CURRENT+field,dwords(value))
            f.call(0x6D8700,0,[DISCOVERY_OUTPUT,channel])
            gates.append(dict(channel=channel,center=value,neighbor_mask=255,
                returned=struct.unpack('<i',dwords(u.reg_read(UC_X86_REG_EAX)))[0]))
    f.capture=True;f.stage='history_setup'
    for cell,pointer in list(f.cells.items()):f.add_cell(cell,pointer=pointer)
    histories=[]
    for stage,cell,final in [('first',(9,10),0),('neighbor',(10,10),0),
        ('stale_cache',(9,10),0),('repeat',(9,10),0),('leave',(9,10),1)]:
        f.stage=stage;start=len(f.events);returned=f.reveal_cell(cell,final)
        histories.append(dict(stage=stage,cell=list(cell),final=final,returned=returned,
            trace=f.events[start:],states=[f.cell_state(c) for c in sorted(f.cells)]))
    f.check_code()
    return dict(rng_seed=31,topology=topology,initial_cell=f.initial_cell,neighbor_masks=[list(v) for v in neighbors],
        channel_mask_outputs=mask_outputs,center_gates=gates,histories=histories,
        constants=f.constants,rng_before=before,rng_after=f.rng_hashes(),rng_calls=f.rng_calls)


def discovery_area_history():
    f=DiscoveryFixture();u=f.uc;f.capture=False
    u.mem_map(REVEAL_CELLS,0x200000);f.cell_cursor=REVEAL_CELLS
    u.mem_write(DISCOVERY_TABLE,bytes(0x100000));f.cells={}
    size=(58,140);center=(68,91)
    u.mem_write(DISCOVERY_MAP+0xF4,dwords(*size))
    for y in range(center[1]-12,center[1]+13):
        for x in range(center[0]-12,center[0]+13):
            if in_diamond(size,x,y):f.add_cell((x,y))
    for name,cell,offset in [('near',(68,91),(16,16)),('far',(68,91),(192,192)),
                            ('overlap',(69,91),(64,64)),('diagonal',(70,93),(128,128))]:
        f.add_actor(name,cell=cell,offset=offset)
    topology=f.topology_input()
    before=f.rng_hashes();f.capture=True;histories=[]
    for stage,cell,final in [('first',(68,91),0),('first_leave',(68,91),1),
        ('overlap',(69,91),0),('overlap_leave',(69,91),1),('repeat',(68,91),0)]:
        f.stage=stage;start=len(f.events);f.area(center=cell,radius=2,final=final)
        histories.append(dict(stage=stage,trace=f.events[start:],
            states=[f.cell_state(c) for c in sorted(f.cells) if c[0]>=65 and c[0]<=72 and c[1]>=88 and c[1]<=95]))
    f.check_code()
    return dict(input=dict(size=list(size),radius=2,rng_seed=31,level=0,topology=topology),
        passes=f.area_walks,histories=histories,
        actors=[dict(name=name,discovery=list(u.mem_read(p+0x41A,3)))
                for p,name in f.actors.items() if p!=DISCOVERY_OBJECT],
        rng_before=before,rng_after=f.rng_hashes(),rng_calls=f.rng_calls)


def discovery_dummy_history():
    """Real MapCell on absent in-diamond slots, sharing native global Dummy."""
    f=DiscoveryFixture();u=f.uc;before=f.rng_hashes()
    topology=f.topology_input();histories=[]
    for stage,cell,final in [('first_absent',(20,21),0),('second_absent',(21,22),0),
        ('repeat_first_absent',(20,21),0),('leave_second_absent',(21,22),1)]:
        assert in_diamond((40,40),*cell) and cell not in f.cells
        f.stage=stage;start=len(f.events);lookup_start=len(f.lookups)
        returned=f.reveal_cell(cell,final)
        histories.append(dict(stage=stage,cell=list(cell),final=final,returned=returned,
            trace=f.events[start:],lookups=f.lookups[lookup_start:],
            dummy=f.pointer_state(0xABDC50),states=[f.cell_state(c) for c in sorted(f.cells)]))
    f.check_code()
    return dict(input=dict(size=[40,40],rng_seed=31,topology=topology),histories=histories,
        constants=f.constants,rng_before=before,rng_after=f.rng_hashes(),rng_calls=f.rng_calls)


def discovery_viewer_control():
    """MapCell forwards its passed House; ordinary area admission is separate."""
    controls=[]
    for stage,viewer in [('passed_foreign_owner',DISCOVERY_OTHER),('passed_current_player',DISCOVERY_PLAYER)]:
        f=DiscoveryFixture();u=f.uc;p=f.add_actor('foreign_viewer_resident')
        topology=f.topology_input();before=f.rng_hashes()
        f.stage=stage;start=len(f.events);lookup_start=len(f.lookups)
        returned=f.reveal_cell((9,10),viewer=viewer)
        f.check_code()
        controls.append(dict(input=dict(size=[40,40],rng_seed=31,topology=topology),
            stage=stage,viewer_current=viewer==DISCOVERY_PLAYER,
            returned=returned,discovery=list(u.mem_read(p+0x41A,3)),trace=f.events[start:],
            lookups=f.lookups[lookup_start:],cell=f.cell_state((9,10)),
            rng_before=before,rng_after=f.rng_hashes(),rng_calls=f.rng_calls))
    return controls


def discovery_house_constructor_priors():
    """Full existing House constructor, with actual Country/Rules prerequisites."""
    m=CampaignReader();m.invoke(0x6832C0,m.scenario);m.seed_streams(31)
    rules=m.alloc(0x2000);m.u.mem_write(0x8871E0,dwords(rules));m.invoke(0x665650,rules)
    country=m.alloc(0x300);m.invoke(0x5113F0,country,(m.cstring('Control'),))
    house=m.alloc(0x17000);rng_calls=[]
    def observe(u,pc,size,data):
        if pc in (0x65C780,0x65C7E0):
            rng_calls.append(dict(pc=f'{pc:08X}',
                stream='scenario' if u.reg_read(UC_X86_REG_ECX)==m.scenario+0x218 else 'other'))
    hook=m.u.hook_add(UC_HOOK_CODE,observe)
    before=m.rng_state();m.native_events.clear();m.invoke(0x4F54A0,house,(country,))
    m.u.hook_del(hook);m.check_code()
    return dict(input=dict(rng_seed=31,country_ctor_name='Control',rules_constructor=True),
        constructor_pc='004F54A0',
        defaults=dict(type_df8_copy=struct.unpack('<i',m.u.mem_read(house+0x26C,4))[0],
            byte246=m.u.mem_read(house+0x246,1)[0],byte1fc=m.u.mem_read(house+0x1FC,1)[0]),
        attack_timer={key:struct.unpack('<i',m.u.mem_read(house+offset,4))[0]
            for key,offset in [('start',0x55F0),('remaining',0x55F4),('duration',0x55F8),('initial_delay',0x55FC)]},
        team_timer={key:struct.unpack('<i',m.u.mem_read(house+offset,4))[0]
            for key,offset in [('start',0x5798),('remaining',0x579C),('duration',0x57A0)]},
        native_events=m.native_events,rng_calls=rng_calls,
        rng_before={k:hashlib.sha256(bytes.fromhex(v)).hexdigest() for k,v in before.items()},
        rng_after={k:hashlib.sha256(bytes.fromhex(v)).hexdigest() for k,v in m.rng_state().items()})


def discovery_building_placement(case):
    """Original owner-entry admission, Radio test and Grand_Opening fast return."""
    f=DiscoveryFixture(mode=case.get('mode',0));u=f.uc;p=DISCOVERY_OBJECT
    u.mem_write(p,dwords(0x7E3EBC));u.mem_write(p+0x520,dwords(DISCOVERY_TYPE))
    u.mem_write(p+0x6E4,b'\1');u.mem_write(DISCOVERY_TYPE+0xDF8,dwords(73))
    u.mem_write(p+0xAC,dwords(case.get('mission',0)))
    u.mem_write(p+0xB4,dwords(case.get('queued',-1)))
    u.mem_write(DISCOVERY_OTHER+0x1EC,bytes([case.get('human',0),case.get('player_control',0)]))
    # Actual constructor writer kernels, rather than image-zero House defaults.
    f.block(0x4F5746,0x4F574D,{UC_X86_REG_EBP:DISCOVERY_OTHER})
    f.block(0x4F5789,0x4F578F,{UC_X86_REG_EBP:DISCOVERY_OTHER,UC_X86_REG_EBX:0})
    f.block(0x4F57DB,0x4F57E4,{UC_X86_REG_EBP:DISCOVERY_OTHER})
    radio=SCRATCH+0x67600
    contacts=[0 if not v else DISCOVERY_PLAYER for v in case.get('contacts',[])]
    u.mem_write(radio,dwords(*contacts) if contacts else bytes(4))
    u.mem_write(p+0xE4,dwords(radio,len(contacts)))
    if case.get('player_null',False):
        #6F496E compares the nonnull owner-viewer with PlayerPtr directly;
        # NULL PlayerPtr selects ordinary other-house discovery, not refusal.
        u.mem_write(0xA83D4C,dwords(0))
    u.mem_write(0xA8E7AC,dwords(2));before=f.rng_hashes();f.events=[];f.lookups=[]
    f.stage=case['name']
    f.block(0x440E5C,0x440E7C,{UC_X86_REG_ESI:p},[0x50B730])
    f.check_code()
    return dict(input=dict(**case,rng_seed=31,scenario_init=2,actually_placed=1,
        topology=f.initial_topology,type_df8=73),discovery=list(u.mem_read(p+0x41A,3)),
        owner=dict(type_df8_copy=struct.unpack('<i',u.mem_read(DISCOVERY_OTHER+0x26C,4))[0],
            byte246=u.mem_read(DISCOVERY_OTHER+0x246,1)[0],
            byte1fc=u.mem_read(DISCOVERY_OTHER+0x1FC,1)[0]),
        trace=f.events,rng_before=before,rng_after=f.rng_hashes(),rng_calls=f.rng_calls)


def discovery_building_override():
    """Actual AI Unlimbo owner-discovery corridor and current-player override.

    A supplied already-opened6E4 prior selects Grand_Opening's real fast return.
    This pins override/house effects, not a second whole first-opening oracle.
    """
    f=DiscoveryFixture();u=f.uc;p=DISCOVERY_OBJECT
    # The vtable is the original BuildingClass table used by slave_manager.
    assert f.read32(0x7E3EBC+0x198)==0x44D5D0
    u.mem_write(p,dwords(0x7E3EBC));u.mem_write(p+0x520,dwords(DISCOVERY_TYPE))
    u.mem_write(p+0x6E4,b'\1');u.mem_write(DISCOVERY_TYPE+0xDF8,dwords(73))
    f.block(0x4F5746,0x4F574D,{UC_X86_REG_EBP:DISCOVERY_OTHER})
    f.block(0x4F5789,0x4F578F,{UC_X86_REG_EBP:DISCOVERY_OTHER,UC_X86_REG_EBX:0})
    u.mem_write(DISCOVERY_OTHER+0x26C,dwords(99))
    before=f.rng_hashes();histories=[]
    for stage,init,viewer in [('ai_unlimbo_owner',2,DISCOVERY_OTHER),
        ('player_reveal',0,DISCOVERY_PLAYER),('repeat_player_reveal',0,DISCOVERY_PLAYER)]:
        f.stage=stage;start=len(f.events);u.mem_write(0xA8E7AC,dwords(init))
        if stage=='ai_unlimbo_owner':
            f.block(0x440E5C,0x440E7C,{UC_X86_REG_ESI:p},[0x50B730,0x44D5D0])
        else:f.call(0x44D5D0,p,[viewer])
        histories.append(dict(stage=stage,scenario_init=init,viewer_current=viewer==DISCOVERY_PLAYER,
            returned=u.reg_read(UC_X86_REG_EAX)&255,discovery=list(u.mem_read(p+0x41A,3)),
            owner=dict(type_df8_copy=f.read32(DISCOVERY_OTHER+0x26C),
                byte246=u.mem_read(DISCOVERY_OTHER+0x246,1)[0],
                byte1fc=u.mem_read(DISCOVERY_OTHER+0x1FC,1)[0]),
            actually_placed=u.mem_read(p+0x6E4,1)[0],trace=f.events[start:]))
    f.check_code()
    return dict(input=dict(rng_seed=31,topology=f.initial_topology,owner_human=False,owner_player_control=False,
        no_radio=True,mission=0,queued=-1,actually_placed=1,type_df8=73,
        initial_house26c=99,initial_house246=0,initial_house1fc=1),
        histories=histories,rng_before=before,rng_after=f.rng_hashes(),rng_calls=f.rng_calls)


def discovery_retail_footprint():
    """Actual BH/BI body walks with decoded retail height bytes, empty lists.

    Authored structures/foundations are explicit leads, not an emulated whole
    native object loader or a claim about nearest resident publication.
    """
    from tools.input_oracle.campaign_start import assets_root,house_case
    from tools.spatial_oracle.shrapnel_repair.map_facts import decode_cells
    raw=(assets_root()/'sov01umd.map').read_bytes();parsed,terrain=decode_cells(raw)
    physical=sections(raw);_,art,_,art_identity=physical_animation_inputs()
    houses=house_case('sov01umd.map',1)
    f=DiscoveryFixture();u=f.uc;f.capture=False
    u.mem_map(REVEAL_CELLS,0x200000);f.cell_cursor=REVEAL_CELLS
    u.mem_write(DISCOVERY_TABLE,bytes(0x100000));f.cells={}
    size=(58,140);center=(68,91)
    u.mem_write(DISCOVERY_MAP+0xF4,dwords(*size))
    for y in range(center[1]-REVEAL_WINDOW,center[1]+REVEAL_WINDOW+1):
        for x in range(center[0]-REVEAL_WINDOW,center[0]+REVEAL_WINDOW+1):
            if in_diamond(size,x,y):f.add_cell((x,y),level=terrain.get((x,y),{}).get('level',0))
    topology=f.topology_input()
    # The same radius reader and waypoint materialization are already executed
    # by220. This input uses their observed9,59,60 fields, not VERA results.
    rules=f.read32(0x8871E0)
    u.mem_write(rules+0x62C,dwords(9));u.mem_write(rules+0x17EE,b'\1')
    u.mem_write(rules+0x17E7,b'\1')
    before=f.rng_hashes();f.capture=True;f.trigger_auto_area=True
    for index,cell in [(59,(68,91)),(60,(72,95))]:
        f.stage=f'waypoint{index}';f.trigger_reveal(index,cell)
    f.check_code()
    admitted={tuple(e['cell']) for e in f.events if e['pc']=='0047C3D0'}
    leads=[]
    for key,value,line in parsed.get('Structures',[]):
        tokens=value[:127].split(',');x,y=int(tokens[3]),int(tokens[4]);name=tokens[1]
        if (x,y) not in admitted:continue
        leads.append(dict(key=key,type=name,owner=tokens[0],cell=[x,y],tag=tokens[6],
            foundation_lexical=art.get(name,{}).get('Foundation'),
            source_line=line,level=terrain.get((x,y),{}).get('level')))
    return dict(input=dict(size=list(size),radius=9,rng_seed=31,waypoints=[59,60],topology=topology),
        sources=dict(map=dict(bytes=len(raw),sha256=hashlib.sha256(raw).hexdigest()),art=art_identity[1]),
        constants=f.constants,passes=f.area_walks,
        notify_cells=[e for e in f.events if e['pc']=='00586643'],
        admitted_cells=[list(c) for c in sorted(admitted)],structure_anchor_leads=leads,
        structure_owner_counts={owner:sum(value.split(',')[0]==owner for value in physical['Structures'].values())
            for owner in sorted({value.split(',')[0] for value in physical['Structures'].values()})},
        native_houses=[{k:h[k] for k in ('name','human','player_control')} for h in houses['final_houses']],
        current_house=houses['current_house_name'],rng_before=before,rng_after=f.rng_hashes(),rng_calls=f.rng_calls)


@lru_cache(maxsize=None)
def discovery_structure_foundation(type_id):
    """Original retained Image/ART Foundation read and occupied-list producer.

    Reuse the existing BuildingType reader transport. The only Object shell
    supplies its real Building vtable/type pointer; whole map Fill is excluded.
    """
    from tools.spatial_oracle.engineer_bridge_cursor_caller import (
        HutTypeReader, READER_INI, READER_SP)
    from tools.input_oracle.campaign_start import assets_root
    m=HutTypeReader();typ=m.construct(type_id)
    rules,art,_,identity=physical_animation_inputs()
    map_sections=sections((assets_root()/'sov01umd.map').read_bytes())
    image_reads=[];rng_calls=[]
    def observe(u,pc,size,data):
        if pc in (0x65C780,0x65C7E0):rng_calls.append(f'{pc:08X}')
    hook=m.u.hook_add(UC_HOOK_CODE,observe)
    langrule=configured_gamemd().parent/'LANGRULE.INI'
    layers=[('RULESMD.INI',rules.get(type_id,{}))]
    if langrule.exists():layers.append(('LANGRULE.INI',sections(langrule.read_bytes()).get(type_id,{})))
    layers.append(('sov01umd.map',map_sections.get(type_id,{})))
    # Absent Image retains the preceding bytes through the original reader.
    for layer,values in layers:
        m.make_ini({type_id:values});m.phase='image_read';m.trace=[]
        before=m.string(typ+0x1F8)
        m.u.mem_write(READER_SP,dwords(RET_MAGIC))
        m.u.mem_write(READER_SP+0x1B4,dwords(READER_INI))
        for register,value in ((UC_X86_REG_ESP,READER_SP),(UC_X86_REG_EBX,typ)):
            m.u.reg_write(register,value)
        run_checked(m.u,0x5F92F9,0x5F9340,count=200000,required_addresses=(0x528A10,))
        image_reads.append(dict(layer=layer,operand=values.get('Image'),
            before=before,after=m.string(typ+0x1F8)))
    image=m.string(typ+0x1F8)
    art_sections={name:art[name] for name in (image,type_id) if name in art}
    m.make_ini(art_sections);m.phase='foundation_read';m.trace=[]
    m.u.mem_write(READER_SP,dwords(RET_MAGIC))
    for register,value in ((UC_X86_REG_ESP,READER_SP),(UC_X86_REG_EBP,typ),
        (UC_X86_REG_EBX,typ+0x24),(UC_X86_REG_EDI,typ+0x1F8),
        (UC_X86_REG_ESI,READER_INI)):
        m.u.reg_write(register,value)
    run_checked(m.u,0x461225,0x46125D,count=200000,required_addresses=(0x474DA0,0x528A10))
    reads=list(m.trace);index=m.read32(typ+0xEF0)
    assert not any(e['kind']=='fixture_seam' for e in reads)
    m.phase='foundation_table_init';m.invoke(0x45B1C0,0)
    m.u.reg_write(UC_X86_REG_ESP,READER_SP);m.u.reg_write(UC_X86_REG_EBP,typ)
    run_checked(m.u,0x46152C,0x461547,count=100)
    building=m.alloc(0x1000)
    m.u.mem_write(building,dwords(0x7E3EBC));m.u.mem_write(building+0x520,dwords(typ))
    # Execute each original lazy guard/sentinel write, stopping immediately
    # before the unrelated CRT exit-registration call. Subsequent complete
    # GetOccupyList calls then use the actually initialized guards.
    for begin,end,obj in ((0x5F5B90,0x5F5BC7,building),(0x45EC20,0x45EC57,typ)):
        m.u.reg_write(UC_X86_REG_ESP,READER_SP);m.u.reg_write(UC_X86_REG_ECX,obj)
        run_checked(m.u,begin,end,count=100)
    pointer=m.invoke(0x5F5B90,building,(0,));offsets=[]
    for ordinal in range(30):
        cell=list(struct.unpack('<hh',m.u.mem_read(pointer+ordinal*4,4)))
        if cell==[32767,32767]:break
        offsets.append(cell)
    else:raise AssertionError('Original Foundation list has no terminator')
    m.u.hook_del(hook);m.unchanged()
    return dict(input=dict(type_id=type_id,image_layers=image_reads,
        langrule_absent=not langrule.exists(),
        art_sections={name:{'Foundation':entry.get('Foundation')} for name,entry in art_sections.items()}),
        sources=identity,image=image,foundation_index=index,
        dimensions=[m.invoke(0x45EC90,typ),m.invoke(0x45ECA0,typ,(0,))],
        foundation_reads=reads,occupied_offsets=offsets,rng_calls=rng_calls)


def discovery_startup_footprint():
    """Compact real55-action/Notify projection, without resident substitutions."""
    from tools.input_oracle.campaign_start import assets_root
    from tools.spatial_oracle.shrapnel_repair.map_facts import decode_cells
    raw=(assets_root()/'sov01umd.map').read_bytes();physical,terrain=decode_cells(raw)
    definitions=reveal_cases()[0]['retail_definitions']
    f=DiscoveryFixture();u=f.uc;f.capture=False
    u.mem_map(REVEAL_CELLS,0x1000000);f.cell_cursor=REVEAL_CELLS
    u.mem_write(DISCOVERY_TABLE,bytes(0x100000));f.cells={};size=(58,140)
    u.mem_write(DISCOVERY_MAP+0xF4,dwords(*size))
    for y in range(sum(size)+1):
        for x in range(sum(size)+1):
            if in_diamond(size,x,y):f.add_cell((x,y),level=terrain.get((x,y),{}).get('level',0))
    compact_topology=dict(allocated_cells=[[x,y,f.cell_state((x,y))['level']] for x,y in sorted(f.cells)],
        cell_constructor_prior={k:v for k,v in f.initial_cell.items() if k not in ('cell','level')},
        dummy_prior=f.pointer_state(0xABDC50))
    rules=f.read32(0x8871E0);u.mem_write(rules+0x62C,dwords(9))
    u.mem_write(rules+0x17EE,b'\1');u.mem_write(rules+0x17E7,b'\1')
    passes=[];admitted=set();current=None
    def observe(u,pc,size,data):
        nonlocal current
        if pc==0x5678E0:
            sp=u.reg_read(UC_X86_REG_ESP)
            current=dict(source=len(controls)-1,
                coord=list(struct.unpack('<iii',u.mem_read(f.read32(sp+4),12))),
                radius=f.read32(sp+8),final=f.read32(sp+32),walk=[])
            passes.append(current)
        elif pc==0x653830:
            sp=u.reg_read(UC_X86_REG_ESP)
            current['walk'].append(struct.unpack('<hh',u.mem_read(f.read32(sp+4),4)))
        elif pc==0x47C3D0:
            admitted.add(struct.unpack('<hh',u.mem_read(u.reg_read(UC_X86_REG_ECX)+0x24,4)))
    hook=u.hook_add(UC_HOOK_CODE,observe);before=f.rng_hashes();controls=[]
    for trigger in definitions['triggers']:
        for ordinal,action in enumerate(trigger['actions']):
            if action['kind']!=17:continue
            index=action['value'];cell=definitions['waypoints'][str(index)]
            control=dict(trigger=trigger['id'],action_ordinal=ordinal,value=index,
                waypoint_44=action['waypoint_44'],cell=cell)
            controls.append(control);f.stage=f'startup{len(controls)-1}'
            f.trigger_reveal(index,cell);control['returned']=u.reg_read(UC_X86_REG_EAX)&255
    u.hook_del(hook);assert len(controls)==55 and len(passes)==110
    for phase in passes:phase['walk']=' '.join(f'{x},{y}' for x,y in phase['walk'])
    f.check_code()
    # Query the same retained original Type read transaction. Linked Trigger
    # fields are followed without assuming lexical tag order or live polling.
    s=retail_definition_fixture();tags=s.definitions();tag_by_id={t['id']:t for t in tags}
    event4_tags={}
    for tag in tags:
        identity=tag['trigger'];chain=[]
        while identity is not None:
            trigger=s.selected_trigger(identity);chain.append(dict(id=identity,events=trigger['events']))
            identity=trigger['linked'];assert len(chain)<=s.m.read32(0x8B4188)
        if any(e['kind']==4 for trigger in chain for e in trigger['events']):
            event4_tags[tag['id']]=dict(tag=tag,trigger_chain=chain)
    event4_leads=[];admitted_structures=[]
    for section,column in [('Structures',6),('Units',7),('Aircraft',7),('Infantry',8)]:
        for key,value,line in physical.get(section,[]):
            tokens=value[:127].split(',');tag=tokens[column]
            cell=(int(tokens[3]),int(tokens[4]))
            lead=dict(section=section,key=key,type=tokens[1],owner=tokens[0],
                cell=list(cell),tag=tag,source_line=line)
            if tag in event4_tags:
                offsets=(discovery_structure_foundation(tokens[1])['occupied_offsets']
                    if section=='Structures' else [[0,0]])
                lead['resident_cells']=[[(cell[0]+dx+32768)%65536-32768,
                    (cell[1]+dy+32768)%65536-32768] for dx,dy in offsets]
                lead['query_intersection']=[c for c in lead['resident_cells'] if tuple(c) in admitted]
                if section=='Structures':lead['native_foundation']=discovery_structure_foundation(tokens[1])
                event4_leads.append(lead)
            if section=='Structures' and cell in admitted:
                binding=tag_by_id.get(tag)
                lead['native_tag']=binding
                lead['native_trigger']=None if binding is None else s.selected_trigger(binding['trigger'])
                admitted_structures.append(lead)
    s.m.check_code()
    return dict(input=dict(size=list(size),radius=9,rng_seed=31,topology=compact_topology,
        source_order='Native TriggerType registry order, then linked action order; not a live Tag poll-order claim'),
        sources=s.sources,constants=f.constants,controls=controls,passes=passes,
        admitted_cells=[list(c) for c in sorted(admitted)],
        event4_tag_definitions=list(event4_tags.values()),event4_entity_literal_leads=event4_leads,
        event4_lead_intersection=[lead for lead in event4_leads if lead['query_intersection']],
        admitted_structure_anchor_leads=admitted_structures,
        rng_before=before,rng_after=f.rng_hashes(),rng_calls=f.rng_calls)


@lru_cache(maxsize=1)
def discovery_cases():
    placements=[dict(name='campaign_hidden_foreign',mode=0,foreign=True),
        dict(name='campaign_open_foreign',mode=0,foreign=True,open_before_put=True),
        dict(name='noncampaign_hidden_foreign',mode=5,foreign=True),
        dict(name='campaign_hidden_current_owner',mode=0,foreign=False)]
    nearest=[dict(name='closest_before_new_head',actors=[dict(name='near',offset=[16,16]),dict(name='far',offset=[192,192])]),
        dict(name='equal_distance_later_put_first',actors=[dict(name='first',offset=[64,64]),dict(name='last',offset=[64,64])]),
        dict(name='equal_distance_reversed_put',actors=[dict(name='last',offset=[64,64]),dict(name='first',offset=[64,64])]),
        dict(name='upper_list_excluded',actors=[dict(name='ground',offset=[192,192]),dict(name='upper',offset=[0,0],upper=True)])]
    diagonals=[dict(name='signed_lower_bounds',levels=[-1,1,3,5,7,9,11]),
        dict(name='upper_bounds',levels=[1,3,5,7,9,11,13]),
        dict(name='below_bounds',levels=[-2,0,2,4,6,8,10]),
        dict(name='above_bounds',levels=[2,4,6,8,10,12,14]),
        dict(name='repeat_without_fresh_radar',levels=[-1,1,3,5,7,9,11],fresh=0),
        dict(name='signed_word_wrap_dummy',source=[32766,0],levels=[-1,1,3,5,7,9,11])]
    building_placement=[dict(name='campaign_ai_owner'),
        dict(name='campaign_player_control_owner',player_control=1),
        dict(name='noncampaign_player_control_only_owner',mode=5,player_control=1),
        dict(name='campaign_human_owner',human=1),
        dict(name='ai_current_construction',mission=18),
        dict(name='ai_queued_construction',queued=18),
        dict(name='ai_null_radio_slots',contacts=[0,0]),
        dict(name='ai_one_nonnull_radio_contact',contacts=[0,1]),
        dict(name='ai_owner_null_player_ptr',player_null=True)]
    return [dict(case_kind='trigger_reveal_discovery',rng_seed=31,
        cell_put_controls=[discovery_placement(c) for c in placements],
        nearest_controls=[discovery_nearest(c) for c in nearest],
        diagonal_controls=[discovery_diagonal(c) for c in diagonals],
        cache_controls=discovery_cache(),area_histories=[discovery_area_history()],
        dummy_controls=discovery_dummy_history(),viewer_controls=discovery_viewer_control(),
        house_constructor=discovery_house_constructor_priors(),
        building_placement_controls=[discovery_building_placement(c) for c in building_placement],
        building_override=discovery_building_override(),retail_footprint=discovery_retail_footprint(),
        full_startup_footprint=discovery_startup_footprint())]


def execute(raw):
    f = ActionFixture()
    u = f.u
    kind=f.read_action(raw)['kind']
    initial_globals=[0,7,49] if kind in (29,57) else []
    initial_locals=[0,7,49,99] if kind in (29,57) else []
    for index in initial_globals:
        u.mem_write(SCENARIO+0x1CB0+index*41,b'\1')
    for index in initial_locals:
        u.mem_write(SCENARIO+0x24B2+index*41,b'\1')
    returned=f.call(0x6DD8B0,[0,0,0,0],ecx=ACTION)&255
    f.check_code()
    def integer(offset): return struct.unpack('<i',u.mem_read(ACTION+offset,4))[0]
    return dict(raw=raw,kind=kind,value=integer(0x90),initial_globals=initial_globals,initial_locals=initial_locals,
                args=[integer(n) for n in (0x34,0x38,0x3C,0x40)],waypoint=integer(0x44),
                returned=returned,globals=[n for n in range(50) if u.mem_read(SCENARIO+0x1CB0+n*41,1)[0]],
                locals=[n for n in range(100) if u.mem_read(SCENARIO+0x24B2+n*41,1)[0]],
                changed=u.mem_read(SCENARIO+0x34AA,1)[0])


def generate():
    rows=[f'{kind},0,{value},41,42,43,44,Z' for kind in (28,29,56,57)
          for value in ('0','7','49','50','99','100','-1','4294967303','  +7tail','junk')]
    rows += [f'28,{typ},{value},41,42,43,44,{last}' for typ,value,last in
             [(11,'7','8'),(9,'text','7'),(9,'text','-1'),(5,'-1','7'),
              (5,'-1',''),(9,'text',''),(12,'7','8'),(-1,'7','8'),
              (1,'-1','Z'),(2,'-1','Z'),(3,'-1','Z'),(4,'text','Z'),(10,'text','7')]]
    rows += ['28,0,7,41,42,43,44', '28,0,7,41,42,43,44,',
             '28,,0,,7,,41,42,43,44,Z', '28,0, ,41,42,43,44,Z',
             '28,0,\t-2,41,42,43,44,Z', '28,0,\v7,41,42,43,44,Z',
             '28,0,2147483648,41,42,43,44,Z','28,0,-9223372036854775808,41,42,43,44,Z']
    output=([execute(raw) for raw in rows]+reference_cases()+eva_cases()+input_cases()+cursor_cases())
    prior=json.dumps(output[:PRE_CURSOR_CONTROL_ROWS],sort_keys=True,separators=(',',':'))
    assert hashlib.sha256(prior.encode()).hexdigest()==PRE_CURSOR_CONTROL_SHA256
    prior=json.dumps(output[:PRE_ANIM_CONTROL_ROWS],sort_keys=True,separators=(',',':'))
    assert hashlib.sha256(prior.encode()).hexdigest()==PRE_ANIM_CONTROL_SHA256
    output+=animation_cases()+waypoint_cases()+message_cases()
    prior=json.dumps(output[:PRE_REVEAL_CONTROL_ROWS],sort_keys=True,separators=(',',':'))
    assert hashlib.sha256(prior.encode()).hexdigest()==PRE_REVEAL_CONTROL_SHA256
    output+=reveal_cases()
    prior=json.dumps(output[:PRE_DISCOVERY_CONTROL_ROWS],sort_keys=True,separators=(',',':'))
    assert hashlib.sha256(prior.encode()).hexdigest()==PRE_DISCOVERY_CONTROL_SHA256
    discovery=discovery_cases()
    legacy=dict(discovery[0])
    legacy['building_placement_controls']=legacy['building_placement_controls'][:PRE_NULL_PLAYER_PLACEMENT_ROWS]
    prior=json.dumps(legacy,sort_keys=True,separators=(',',':'))
    assert hashlib.sha256(prior.encode()).hexdigest()==PRE_NULL_PLAYER_DISCOVERY_SHA256
    return output+discovery


def metadata():
    f = ActionFixture()
    _,eva_identity = physical_eva()
    out = provenance(
        entry_points={'constructor':0x6DD000,'read':0x6DD5B0,'execute':0x6DD8B0,
            'read_string':0x528A10,'strtok':0x7C9CC2,'atoi':0x7C9BFD,
            'set_global':0x689670,'set_local':0x689910,
            'team_name':0x6F1920,'trigger_name':0x727AA0,'tag_name':0x6E6310,
            'eva_registry_init':0x752210,'eva_read':0x753000,
            'eva_voice_init':0x752290,'eva_name':0x753250,
            'eva_set_side':0x7534E0,'eva_filename':0x753380,
            'eva_queue':0x752480,'eva_insert':0x752590,
            'eva_play_next':0x752760,'eva_clear':0x752370,'stream_stop':0x407F40,
            'lock_input':0x684240,'unlock_input':0x684290,
            'cancel_mouse_drag':0x6938C0,'cancel_band_box':0x4AEAD0,
            'clear_band_rect':0x6DA160,'clear_keyboard':0x54F720,
            'process_messages':0x5D4D50,'mouse_hide':0x7B9930,'mouse_show':0x7B9750,
            'anim_action':0x6E2290,'anim_constructor':0x421EA0,'anim_ai':0x423AC0,
            'anim_start':0x424CE0,'anim_uninit':0x4255B0,'anim_art_read':0x427D00,
            'rules_constructor':0x665650,'rules_audio_visual':0x6691E0,
            'rules_animation_list':0x6728B0,'rules_process':0x668BF0,
            'delete_cleanup':0x725C70,'waypoint':0x68BCC0,'packed_cell':0x5657A0,
            'ground_height':0x578080,'slope_height':0x47B3A0,
            'message_color':0x69A310,'csf_lookup':0x734E60,'message_add':0x5D3BA0,
            'trigger_reveal':0x6E0FE0,'reveal_radius_ctor_store':0x665EDE,
            'reveal_radius_reader':0x67181C,'read_int':0x5276D0,
            'reveal_area':0x5678E0,'area_cell':0x653830,'map_cell':0x4A9CA0,
            'cell_constructor':0x47BBF0,'cell_reveal':0x487630,
            'cell_leave':0x487690,'is_shrouded':0x586360,
            'area_offsets':0x561910,'area_los_offsets':0x5638D0,
            'adjust_for_z':0x6D20E0,'projection_diagonal_init':0x6D1830,
            'projection_angle_init':0x6D18C0,'projection_z_multiplier_init':0x6D1BB0,
            'projection_z_multiplier_store':0x6D1BDD,'neighbor_cache':0x6D8700,
            'cell_put':0x47E8A0,'cell_notify':0x5865F0,'nearest_cell_object':0x47C3D0,
            'techno_discovered':0x6F4960,'building_discovered':0x44D5D0,
            'ai_building_unlimbo_owner_discovery':0x440E5C,
            'discovery_house_constructor':0x4F54A0,'house_human_query':0x50B730,
            'radio_has_contact':0x65AE30,'mission_query':0x5B3040,
            'building_type_constructor':0x45DD90,'type_image_reader':0x5F92F9,
            'art_foundation_reader':0x461225,'foundation_list_init':0x45B1C0,
            'foundation_list_bind':0x46152C,'object_foundation_list':0x5F5B90,
            'type_foundation_list':0x45EC20,
            'radar_cell_dirty':0x6565A0,'radar_pixel_dirty':0x6562D0},
        assumptions=[
            'The first61 legacy numeric rows retain their schema and values. Additive rows are selected by case_kind before typed deserialization.',
            'Original full TAction constructor, CRT strtok/atoi, Read and selected Execute dispatch bodies run unchanged. Supplied counted action strings and Scenario storage isolate the reader and selected consumers.',
            'Native TeamType/TriggerType/TagType constructors publish ordered registries through their original find-or-construct helpers. Name matching, ID truncation and short-index lookup are original instructions, not fixture logic.',
            'Reference dispatch22/53/54 has an empty live instance registry. Live force/enable/disable fanout, timer writes and detach ownership remain with existing tag_lifecycle evidence.',
            'ReadString512 uses the existing cached single-entry transport. Capacity and missing/empty defaults execute natively; physical map-file line admission and malformed absent operands reaching atoi(NULL) are excluded.',
            'Full original Vox Read753000 consumes all470 EVAMD DialogList entries in physical source order through shared prepared INI indexes. Physical MIX/INI preparation uses existing tools; original INI file-loading IO is outside the prepared-cache seam.',
            'Original registry initialization752210..752246 stops before CRT exit registration; original VoiceSystem Init752290..7522EC stops before channel/file setup and sets the real cleanup guard. Original queue admission, duplicate search, insertion, unlinking, entry-state writes and cleanup execute.',
            'EVA PlayNext takes its original pause_flagA8ED64 early exit. A supplied zeroed stream shell has no device channel, file or hardware; current-entry control is supplied for same-current and interrupt-current branches. No speech timing, audible playback or whole-mission opening claim follows.',
            'Input actions46/47 execute original placement/mode clearing, mouse gesture cancellation, capture release, cursor show-level loops, keyboard queue clearing and Scenario lock writes. Empty placement foundation, Tactical band rectangle and MouseClass/WWMouse initialized fields/vtables are supplied.',
            'Original MouseClass cursor methods take their initialized-but-no-SHP path. WWMouse captured-surface flag is false, keyboard record/replay selector is zero and the Windows message queue is empty. Sprite drawing, nonempty placement removal, replay IO and device event translation are excluded.',
            'Appended positive ShowLevel1 runs full original action47 Unlock to completion, including the Show positive clamp and repeat-unlock prior gate. The signed-boundary history executes one full original Show7B9750 for1/i32MAX/i32MIN/-1; MAX/MIN full Unlock would require billions of iterations and is excluded, without replacing or shortening the loop.',
            'Legacy EVA/input controls claim no gameplay RNG or timer cadence. EVA wait-dword clearing is observed for supplied-current interruption, without native end-time or hardware playback measurements.',
            'Anim catalog controls execute physical RULESMD AV/master cold chronology and the post-reset Process668BF0 prefix through668E9D, after its ReadAnimations call. Other family lists are absent from this prefix; later ART-reference append discoveries are not a full-registry claim. The first607 post-reset names are established by original instructions; cold startup includes an earlier XXXX discovery.',
            'Anim histories execute full original TAction41, Anim constructor/Start/AI/Next/UnInit and725C70 cleanup against original retail vtables. Physical ART and SHP bytes establish selected type483, frame bounds and rate. Two selected stock BH/BI histories and one synthetic Next+sound control cover retained NoEffects, late-store order, CdTimer/loops, zero gameplay RNG draws and detach/retirement. Mark and display submission are sinks; whole GPU output and broad AnimType effects are outside these histories.',
            'Ground578080, Cell47B3A0, packed5657A0 and waypoint68BCC0 execute unchanged. Ground/bridge constants come from the original Cell/TAction CRT initializer tables; all21 slope matrices come from the shared unhooked original startup helper. Flat, slopes1/4/15/20, signed levels,100/400 bridge flags and supplied Dummy state are bounded; Focus/SetView are observed coordinate sinks.',
            'Action11 uses original Read inline31-byte label handling, AudioVisual MessageDelay binary64 read, color LUT69A310, multiplication/ftol and CSF734E60 against prepared retail DESC:SOV1. MessageList insertion is an observed sink; native on-screen layout, expiration/pause cadence and CSF archive IO are not executed here. Missing tokenizer operands reaching atoi(NULL) are excluded; an empty constructor label and space/null-reference controls cover the valid no-message arm.',
            'Additive trigger_reveal retains all219 preceding canonical payloads. Original Execute17 calls6E0FE0 and full5678E0/653830/4A9CA0/487630/487690 run without reveal sinks. Two original area return boundaries retain actual ordered walks and intermediate/final Cell state. Full Cell47BBF0 initializes counters/cache/vtables; supplied source level/flags distinguish signed division, word wrapping, bridge100/400 short-circuit and literal+90 versus trailing+44.',
            'Reveal geometry reuses reveal_area.py Size58x140 diamond/window contract and original561910/5638D0 offset initialization. Map561710/5617A0/5617C0/5617E0 and existing Cell/TAction CRT tables derive104/416. Original projection6D1830/6D18C0/6D1BB0 execute in active CRT pointer814E58/814E68/814EA0 order; store6D1BDD yields native binary64 bits3FC25E5374344960 atB0CD48. The earlier zero-B0CD48 prototype is rejected and preserved only in scratch. All cells in the bounded source window execute their real constructor; other map cells are absent. Supplied flat levels plus a single source level are synthetic controls, not complete SOV terrain.',
            'Current PlayerPtrA83D4C and a distinct triggerHouse first Execute argument have minimal supplied House storage. Both latch directions and same-player control execute original House240 gate. No full House constructor, legal null PlayerPtr, alliance transport or original client player-selection chronology is claimed.',
            'Native dirty-screen Map+0C=1 selects6DA7D0 early exit. Empty resident lists, native frame0, GameMode0, null network session and Scenario FogOfWar clear are supplied. Real Tactical6D8700 reads the original static256-byte neighbor LUT7F4194; real5865F0 empty-object notifications execute. Boundary cache/flags and center cache/flags are distinct actual cell outputs. No full Tactical screen construction, rendered pixels, complete cache/render parity, fogged object records or long-lived sight-source admission is certified.',
            'Intermediate shrouded booleans project original direct Cell+12C bit8 reads; final shrouded_query_al executes actual586360 at ordinary worldZ0. Every input declares rng_seed31 and executes the original scenario/main/mapgen RandomClass Seed65C6D0. rng_before/rng_after are SHA256 hashes of original-layout state bytes, not importable state hex;65C780/65C7E0 entry order is observed separately. Zero draws, unchanged native ID cursor/frame and no native allocation are bounded to these action visits, not constructors or whole opening.',
            'RevealTriggerRadius reads exact General key83B83C, ctor665EDE stores5, retained read67181C passes Rules+62C to5276D0 and stores671836. Declared sequential physical RULESMD/map caches and synthetic missing/malformed/case/hex/wrap overrides execute that unchanged kernel. Cached empty is distinct from lexical physical empty omission. Full Rules Process chronology, optional LANGRULE/mode layers, archive IO and native physical INI loading remain outside this scalar comparison.',
            'Existing StockPrefix owns full original fixed AIMD/map TeamType lists, TriggerType/TagType readers and selected waypoint ReadInt fragments. Their observed four Event47(0) definitions contain55 action17 records and no action16 exists in all SOV01 definitions. These are actual native type fields/linked action order; difficulty classification, live Tag construction and whole opening polling stay with tag_lifecycle rather than physical Tag ordering assumptions.',
            'Signed LOS contrasts call unchanged reveal_area_row and execute567CB5 movsx byte11B against signed viewer level plus3. Its MapCell/Unshroud/House/fog-border sinks retain that existing geometry-only scope; the separate flat combined controls execute real knowledge leaves. Negative-radius action execution is excluded: native indexes before the count table and existing unsigned-radius production ownership requires a separate defined-domain audit. Signed negative reader outputs are retained without fabricated leaf goldens.',
            'Additive trigger_reveal_discovery retains all220 accepted payloads, including16 action17 controls and the219 seal. Existing Entry owns native constructor/InitManagers discovery prefixes, original Infantry vtable and runtime transport; its visibility answer seam is bypassed so full586360/5865E0 execute. Real47BBF0 produces counter1/gap0/cache-2/-2 and zeroed initialized low flags. Every sparse allocated cell, signed level, flags/caches and Dummy prior is explicit in each applicable input topology.',
            'Four campaign/noncampaign CellPUT histories execute original47E8A0 and4A9CA0 through real cache/Notify/nearest/TechnoDiscovered bodies. Owner-entry is original4D7221..4D7235. Full object allocation/Unlimbo/retail Fill is excluded. Foreign objects have NULL Tags; current-owner supplied nonnull Tag is suppressed by actual produced41A/41B. Later nonempty Event4 action chains are not executed or claimed.',
            'Nearest controls use original CellPUT prepend order and original47C3D0 strict-distance selection, including equal-distance reversed insertion and excluded upper-list members. Seven signed diagonal levels execute586643 movsx, both inclusive bounds, fresh0 and signed-i16 wrapping into actual Dummy lookups. Supplied actors use genuine Techno discovery prefixes and native identity writer6F322F..6F323A; whole concrete constructors and transport physics are excluded.',
            'Cache controls execute all256 masks in both real6D8700 channels and original center fallback gates; synthetic channel fields are declared inputs, not inferred render state. Fresh/cache histories and five overlapping radius2 area visits execute real immediate cell mutation/cache/notification order, retaining actual intermediate counters/flags/caches and callback traces. Only the visited cell cache is refreshed; no inferred neighbor-wide cache update is applied.',
            'MapCell performs its own5657A0 lookup, then each6D8700 channel performs another center lookup followed by eight neighbors only when eligible. Four direct absent-in-diamond controls retain one real globalDummyABDC50 counter/cache/flags object: two distinct missing coordinates share its history. The independent packed input remains fixed while missing neighbor queries stampDummy+24; Notify reads that late-mutated cached Cell pointer. lookup records retain exact requests and real/Dummy stamped returns. These synthetic holes establish global Dummy behavior, not whole retail Map allocation or per-house-plane equivalence.',
            'MapCell4A9D88 forwards its passed House to5865F0/vt198 rather than reading PlayerPtr again. Two identical sparse-resident controls contrast foreign-owner/current-player callbacks. Ordinary567AA9..567B18 first resolves the supplied owner to PlayerPtr through its original own/spy/allied admission and skips the area otherwise; those alternate admission branches are read-only instruction evidence and are not executed by the direct MapCell contrasts.',
            '6565A0 fresh notifications execute their actual Map121C-null surface early exit. Original6565A0/6562D0 identify scaled radar pixel dirty-bit/vector work; this is a presentation-surface substitution, not a FogOfWar=no gate or a fogged-object cleanup claim. Frame0, Map0C=1 dirty-screen bypass, GameMode0, null network, Scenario FogOfWar clear and minimal Houses remain explicit world priors.',
            'Building override control executes full44D5D0, actual50B730/65AE30 and original AI Unlimbo440E5C..440E7C owner-discovery corridor. A supplied already-opened6E4=1 selects the original445F80(captured0) fast return; this does not claim whole first Grand_Opening, whose existing construction owner remains separate. Synthetic TypeDF8=73 and priorHouse26C=99 expose exact copy/246/1FC writes, ScenarioInit retention, current-player discovery and duplicate suppression. No animation/first-placement opening behavior is fabricated.',
            'Eight Building placement controls execute campaign1EC/1ED and noncampaign1EC-only human admission, current versus queuedConstruction18 and null versus nonnull Radio slots. The owner-entry callback precedes this corridor440E7C return. Building44D5ED gates both26C/246 on ScenarioInit0;1FC is always set after successful shared discovery. Full House4F54A0 executes through CampaignReader with actual Scenario/Country/Rules constructors and seed31, establishing26C=-1,246=0,1FC=1. Its one Scenario ranged450..1800 constructor call returns789 and its timer writes are explicit; that constructor is outside the zero-draw map-visit claim. Placement fixtures initialize these fields through original constructor writer kernels, while their other minimal House/Building fields remain supplied.',
            'The ninth ai_owner_null_player_ptr placement control supplies PlayerPtrA83D4C=NULL with the same nonnull AI owner/viewer, ScenarioInit2, no Radio, currentmission0/queued-1 and already-placed6E4=1. Original6F496E compares viewer to PlayerPtr without a null guard, takes the ordinary other-house branch and admits Building44D5D0/Grand445F80 fast return. This establishes the shared headless owner-discovery boundary only; it does not establish legal full scenario startup, a null discovery viewer or nullable Action17. All prior221 aggregate outputs are sealed after projecting out only this appended placement control.',
            'Retail BH/BI footprint runs original6E0FE0 and both complete area passes with real decoded SOV01 height bytes and allocated constructor cells in the declared58x140 diamond/window. Both original Cell/TAction CRT groups execute before the caller, deriving104/416 and nativeB0CD48. Source waypoints59/60 and retained radius9 are native-read priors established by220, not VERA inputs. Supplied zero slope/bridge state and empty resident lists exclude native TMP/object Fill. Original House ctor/read/Basic.Player selection establishes onlyRussians human/player-controlled; admitted GTGCAN/neutral structure anchors and lexical ART foundations are clearly labeled resident leads rather than nearest-object publication proof. Earlier unsealed zero-TAction-height footprint outputs are rejected and scratch-only.',
            'Full startup footprint executes all55 native-read Event47/action17 values and retained+44 fields through original6E0FE0 and110 complete area passes. Compact topology supplies constructor cells throughout the Size58x140 diamond, each with decoded retail height or explicit zero for absent terrain records, zero slope/bridge state and empty resident lists. Actual653830 walks and actual47C3D0 admission union are retained without millions of duplicate lookup records. Source order is native TriggerType registry order followed by linked actions, not an additional claim about live Tag poll order. Shared retained StockPrefix readers provide all linked Event4 TagType/TriggerType identities; physical Techno line/Tag coordinates remain lexical object-publication leads. Every authored Event4 resident cell is compared: non-Structures use their one authored cell; CATHOSP uses all24 actually produced Foundation cells. Empty intersection excludes these supplied initial residents from this initialized footprint, not moving actors or whole native Fill.',
            'CATHOSP Foundation evidence reuses HutTypeReader: actual45DD90 constructs the Type, original5F92F9..5F9340 retains Image across physical RULESMD/optional loose LANGRULE/map prepared cache passes, and original461225..46125D reads fixed ART Image then TypeID Foundation with original defaults/parser. Selected LANGRULE is physically absent. Actual45B1C0 initializes occupied lists and46152C..461547 binds+DFC; complete Object5F5B90/Type45EC20 produce the ordered24 offsets and7FFF sentinel. Native lazy guard/sentinel writers run until their unrelated CRT exit-registration calls, then the initialized guards are retained. The Object shell supplies the real Building vtable and constructed Type pointer; full Object constructor/Fill publication and destructor/exit registration are excluded. Measured Image/ART/list phases trace no RNG entries.',
            'All discovery/control map fixtures seed the actual scenario/main/mapgen RandomClass with31 after initialization; hashes cover original-layout state bytes and entry traces observe65C780/65C7E0. Zero draws/no timers/no detach calls are bounded to selected NULL-Tag map/query/discovery visits. Separate whole native House setup in the retail lead projection is not included in this zero-draw claim. Scenario teardown, resident Building first-opening effects and later discovered Tag actions are excluded.',
        ],
        substitutions=[
            'Inherited CRT/heap transport supplies GetLastError, SetLastError, TlsGetValue, HeapAlloc/HeapFree and critical-section imports. Original new7C8E17 receives bump storage; deletion retains storage. Original CRT accessor7D140B returns the supplied per-thread block. Gameplay instructions and real retail vtables are never patched.',
            'Input OS imports only: WaitForSingleObject returns0, ReleaseMutex returns1, GetCapture/ReleaseCapture use the supplied window handle, ShowCursor updates a supplied OS depth and PeekMessage returns0. The native higher-level mouse/input bodies remain original.',
            'Anim archive IO returns unchanged physical SHP bytes through the existing Reader owner. Mark5F5850/display submission4A9720 observe rather than render. PlaySound7509E0 records the original resolved sound/coordinate and prepares a valid tagged handle through the existing sound fixture; original StopAndClear/Release execute, without channel admission, audio RNG, hardware or audible output claims. Cleanup uses supplied empty Windows SEH and a checked IsBadReadPtr import over mapped memory.',
            'MessageList5D3BA0 records all seven original arguments, including signed duration/color/style/text. Missing-CSF formatting runs original CRT; diagnostic logging4068E0 is an external sink. Locale reference-count InterlockedIncrement/Decrement imports use the shared atomic transport; no original gameplay bytes or vtables are patched.',
        ],
        scope='Selected original TAction numeric/reference materialization, stock EVA/input transitions, post-reset Anim41 histories, waypoint48/112, action11 message arguments, action17 retained-radius/coordinate-to-real-Cell reveal and immediate sparse-resident/cache/discovery controls. Fixed original PE only; bounded world/OS/audio/presentation seams, no whole-campaign or hardware output.')
    out['command'] = 'python -m tools.spatial_oracle.trigger_action_values --check'
    out['retail_eva'] = eva_identity
    out['retail_animation_inputs'] = physical_animation_inputs()[3]
    out['token_layout'] = {'token1':'ActionID -> +2C',
        'token2':'ParameterType metadata dispatch; never the definition operand',
        'token3':'Actual operand consumed by Read6DD5B0, including actions22/53/54',
        'tokens4_to7':'Four numeric parameters -> +34/+38/+3C/+40',
        'token8':'Waypoint -> +44, except types5/9 write atoi -> +90 and type11 -> +48'}
    out['reference_fields'] = {'1_and5':'TeamType +30, registryA8ECA0',
        '2':'TriggerType +50, registry8B4178','3':'TagType +4C, registryB0E780',
        '6':'Vox name lookup753250 -> numeric+90',
        '4':'31-byte inline CSF label+6D, explicitNUL+8C'}
    out['reference_selection'] = 'Original atoi(operand)==-1 null check precedes strlen<3 index selection. Otherwise names remain untrimmed and helpers match case-insensitively or construct. ID storage is23 bytes plus NUL.'
    out['legacy_numeric'] = dict(rows=61,
        canonical_payload_sha256='ac1ab9e82b219e30325f9c0ee7e110d02baaecc949fa07be5cead697be629767')
    out['legacy_pre_cursor_controls'] = dict(rows=PRE_CURSOR_CONTROL_ROWS,
        canonical_payload_sha256=PRE_CURSOR_CONTROL_SHA256)
    out['legacy_pre_anim_controls'] = dict(rows=PRE_ANIM_CONTROL_ROWS,
        canonical_payload_sha256=PRE_ANIM_CONTROL_SHA256)
    out['legacy_pre_reveal_controls'] = dict(rows=PRE_REVEAL_CONTROL_ROWS,
        canonical_payload_sha256=PRE_REVEAL_CONTROL_SHA256)
    out['legacy_pre_discovery_controls'] = dict(rows=PRE_DISCOVERY_CONTROL_ROWS,
        canonical_payload_sha256=PRE_DISCOVERY_CONTROL_SHA256)
    out['legacy_pre_null_player_controls'] = dict(placement_rows=PRE_NULL_PLAYER_PLACEMENT_ROWS,
        canonical_discovery_aggregate_sha256=PRE_NULL_PLAYER_DISCOVERY_SHA256)
    out['retail_reveal_inputs'] = reveal_cases()[0]['retail_definitions']['sources']
    out['reveal_callers'] = {'execute17':'6DE34E..6DE36E calls6E0FE0',
        'spring_house':'7265F0..726605 resolves TriggerType owner through502D30 and passes first Execute argument',
        'radius_read_order':['SpotlightRadius83B850','RevealTriggerRadius83B83C',
                             'ChargeToDrainRatio83B828','WallBuildSpeedCoefficient83B80C'],
        'scalar_reader_file_scope':'General kernel is called per prepared INI pass; complete loader chronology excluded',
        'cleanup':'Action17 has no detach/timer/allocation producer; Cell knowledge persists after final1. Periodic sweep/full mission cleanup excluded.'}
    out['original_slices'] = [dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',
        sha256=hashlib.sha256(code).hexdigest()) for (a,b),code in zip(f.ranges,f.original)]
    discovery=DiscoveryFixture()
    out['discovery_original_slices'] = [dict(start=f'{a:08X}',end_exclusive=f'{b:08X}',
        sha256=hashlib.sha256(code).hexdigest()) for (a,b),code in zip(discovery.ranges,discovery.original)]
    return out


def damage_waypoint_dispatch_case(case):
    """Whole Action63; area, bridge and presentation are observed boundaries."""
    f = ActionFixture()
    u = f.u
    constants = initialize_waypoint_map(f)
    # Scenario's registered startup bodies initialize GetWaypointLocation's
    # bridge-height global separately from CellClass's deck-height global.
    scenario_initializers = list(struct.unpack('<15I', u.mem_read(0x8148A8, 60)))
    assert scenario_initializers[0] == 0x683070 and scenario_initializers[-1] == 0x683280
    for entry in scenario_initializers:
        f.call(entry, [])
    constants['scenario_initializers'] = scenario_initializers
    constants['waypoint_bridge_height'] = f.integer(0xB054BC)
    f.call(0x47BBF0, [], ecx=f.cell)
    f.call(0x47BBF0, [], ecx=0xABDC50)
    install_waypoint_cell(f, case)
    assert f.read32(f.read32(f.cell) + 0x48) == 0x486840
    read = f.read_action('63,0,364,0,0,0,0,A')
    u.mem_write(SCENARIO + 0x632 + 364 * 4, struct.pack('<hh', *case['cell']))
    u.mem_write(SCENARIO + 0x632, struct.pack('<hh', 20, 20))
    warhead = f.allocate(0x200)
    u.mem_write(0x8871E0, dwords(RULES))
    u.mem_write(RULES + 0xFA8, dwords(warhead))
    calls = []
    bridge_attempts = 0

    def observe(_u, pc, _size, _data):
        nonlocal bridge_attempts
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc == 0x489280:
            args = [f.read32(sp + 4 * i) for i in range(1, 5)]
            assert args[1] == warhead
            args[1] = 'C4Warhead'
            calls.append(dict(kind='area', coord=list(struct.unpack('<3i',
                u.mem_read(u.reg_read(UC_X86_REG_ECX), 12))),
                damage=f.integer_reg(UC_X86_REG_EDX), args=args))
            return_from_sink(u, 16, 0)
        elif pc == 0x587180:
            answer = bridge_attempts == case.get('bridge_success_at', -1)
            calls.append(dict(kind='bridge', cell=list(struct.unpack('<hh',
                u.mem_read(f.read32(sp + 4), 4))), result=answer))
            bridge_attempts += 1
            return_from_sink(u, 4, int(answer))
        elif pc == 0x6D2140:
            calls.append(dict(kind='project', coord=list(struct.unpack('<3i',
                u.mem_read(f.read32(sp + 4), 12)))))
            u.mem_write(f.read32(sp + 8), dwords(1000, 2000))
            return_from_sink(u, 8, f.read32(sp + 8))
        elif pc == 0x6D2790:
            calls.append(dict(kind='dirty', rectangle=list(struct.unpack('<4i',
                u.mem_read(sp + 4, 16))), flag=f.read32(sp + 20)))
            return_from_sink(u, 20, 0)

    hook = u.hook_add(UC_HOOK_CODE, observe)
    before = f.rng_hashes()
    try:
        returned = f.call(0x6DD8B0, [0, 0, 0, 0], ecx=ACTION) & 255
    finally:
        u.hook_del(hook)
    f.check_code()
    return dict(input=case, read=read, constants=constants, returned=returned,
                calls=calls, rng_before=before, rng_after=f.rng_hashes())


def damage_waypoint_native_inputs(root):
    """Compose existing physical-byte, ordered INI and native scalar owners."""
    from tools.rules_oracle.bridge_landing_inputs import RULES as rule_blocks
    required = ('RULESMD.INI', 'ARTMD.INI', 'SOUNDMD.INI', 'ALL01UMD.MAP')
    for name in required:
        if not (root / name).is_file():
            raise ValueError(f'Missing --damage-waypoint input {name}; set VERA20K_TRIGGER_DAMAGE_INPUTS')
    m = Sound.__new__(Sound)
    m.samples, m.calls = [], []
    sound = sections((root / 'SOUNDMD.INI').read_bytes())
    selected = {name: sound[name] for name in ('Defaults', 'Dummy')}
    selected['SoundList'] = {key: name for key, name in sound['SoundList'].items() if name == 'Dummy'}
    Reader.__init__(m, root, selected)
    u, read = m.u, m.read32
    # The native lazy foundation-list getter registers its exit cleanup.
    # Reuse the same platform seam as the joined receiver below.
    u.hook_add(UC_HOOK_CODE, lambda _u, pc, _n, _d: m.ret(0) if pc == 0x7C978A else None)
    m.selected = selected
    u.mem_write(0x87E2A0, dwords(1))
    u.mem_write(0x87E294, dwords(m.alloc(0x100)))
    m.invoke(0x4072C0, 0x87E250)
    u.mem_write(0xB1D378, dwords(0x7EB6D4, m.alloc(64), 16, 1, 0, 10))
    m.invoke(0x7510D0, INI)
    m.rules = m.alloc(0x5000)
    u.mem_write(0x8871E0, dwords(m.rules))
    m.invoke(0x665650, m.rules)
    m.invoke(0x4E7CF0, 0)
    scene = m.alloc(0x7000)
    u.mem_write(0xA8B230, dwords(scene))
    u.mem_write(scene + 0x1258, dwords(0))
    # Reuse the native ParticleSystem/Particle registries' prepared-capacity
    # boundary from electric_bolt.SparkBirth; the constructors/readers below
    # still materialize and resolve their actual selected references.
    for registry in (0x8874C0, 0xA83C98, 0xA83D68, 0xA83D98):
        u.mem_write(registry, dwords(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
    m.country = m.alloc(0x400)
    m.invoke(0x5113F0, m.country, (m.cstring('Confederation'),))
    raw_art = (root / 'ARTMD.INI').read_bytes()
    art = sections(raw_art)
    m.make_ini({name: art[name] for name in
        ('NANRCT', 'NANRCT_A', 'NANRCT_AD', 'NANRCT_B', 'NANRCT_BD') if name in art})
    art_cache = bytes(u.mem_read(INI, 0x40))
    typ = m.alloc(0x2000)
    m.invoke(0x45DD90, typ, (m.cstring('NANRCT'),))
    m.types = {'NANRCT': typ}
    warhead = m.invoke(0x75E3B0, m.cstring('Super'))
    layers = []
    damage_system_layers = []
    for name in ('RULESMD.INI', 'LANGRULE.INI', 'ALL01UMD.MAP'):
        path = root / name
        if not path.is_file():
            assert name == 'LANGRULE.INI'
            layers.append(dict(file=name, absent=True))
            continue
        raw = path.read_bytes()
        physical = sections(raw)
        retained = {name: physical[name] for name in
            ('General', 'AudioVisual', 'CombatDamage', 'NANRCT', 'Super', 'Confederation') if name in physical}
        m.make_ini(retained)
        rules_ini = m.alloc(0x40)
        u.mem_write(rules_ini, bytes(u.mem_read(INI, 0x40)))
        u.mem_write(INI, art_cache)
        building_read = m.invoke(0x45FE50, typ, (rules_ini,)) & 255
        warhead_read = m.invoke(0x75D3A0, warhead, (rules_ini,)) & 255
        system_list = read(typ + 0x77C)
        systems = [read(system_list + i * 4) for i in range(read(typ + 0x788))]
        selected_systems = {m.string(p + 0x24): p for p in systems}
        for name in selected_systems:
            if name in physical:
                retained[name] = physical[name]
        m.make_ini(retained)
        u.mem_write(rules_ini, bytes(u.mem_read(INI, 0x40)))
        u.mem_write(INI, art_cache)
        system_reads = [(p, m.invoke(0x6442D0, p, (rules_ini,)) & 255) for p in systems]
        particles = {}
        for p in systems:
            particle_index = struct.unpack('<i', u.mem_read(p + 0x294, 4))[0]
            if particle_index >= 0:
                assert particle_index < read(0xA83DA8)
                particle = read(read(0xA83D9C) + particle_index * 4)
                particle_name = m.string(particle + 0x24)
                particles[particle_name] = particle
                if particle_name in physical:
                    retained[particle_name] = physical[particle_name]
        m.make_ini(retained)
        u.mem_write(rules_ini, bytes(u.mem_read(INI, 0x40)))
        u.mem_write(INI, art_cache)
        particle_reads = {name: m.invoke(0x644F50, p, (rules_ini,)) & 255
                          for name, p in particles.items()}
        damage_system_layers.append(dict(file=path.name,
            systems=[dict(name=m.string(p + 0x24), native_read=answer,
                behaves_like=struct.unpack('<i', u.mem_read(p + 0x2B4, 4))[0],
                holds_what=m.string(read(read(0xA83D9C) + read(p + 0x294) * 4) + 0x24)
                    if read(p + 0x294) != 0xFFFFFFFF else None,
                spawns=bool(u.mem_read(p + 0x298, 1)[0]),
                spawn_frames=read(p + 0x29C), lifetime=read(p + 0x2B8),
                particle_cap=read(p + 0x2A4), spawn_radius=read(p + 0x2A8),
                slowdown_bits=bytes(u.mem_read(p + 0x2A0, 4)).hex(),
                spawn_cutoff_bits=bytes(u.mem_read(p + 0x2AC, 4)).hex())
                for p, answer in system_reads],
            particles=[dict(name=name, native_read=particle_reads[name],
                behaves_like=struct.unpack('<i', u.mem_read(p + 0x314, 4))[0])
                for name, p in particles.items()]))
        for section, begin, end, _delta in rule_blocks.values():
            if section not in retained:
                continue
            u.mem_write(SP, bytes(0x300))
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_ESI, m.rules)
            u.reg_write(UC_X86_REG_EDI, rules_ini)
            run_checked(u, begin, end, count=2_000_000)
        layers.append(dict(file=path.name, sha256=hashlib.sha256(raw).hexdigest(),
            native_building_read=building_read, native_warhead_read=warhead_read,
            strength=read(typ + 0xA0), armor=read(typ + 0x9C),
            power_output=read(typ + 0xEE0), power_drain=read(typ + 0xEE4),
            foundation=read(typ + 0xEF0), c4_warhead=m.string(read(m.rules + 0xFA8) + 0x24),
            condition_yellow_bits=bytes(u.mem_read(m.rules + 0x1700, 8)).hex(),
            super_cellspread_bits=bytes(u.mem_read(warhead + 0x124, 4)).hex(),
            super_verses_bits=[f'{bits:016x}' for bits in struct.unpack('<11Q', u.mem_read(warhead + 0xA0, 88))]))
    assert read(m.rules + 0xFA8) == warhead
    m.invoke(0x45B1C0, 0)
    dimensions = [m.invoke(0x45EC90, typ), m.invoke(0x45ECA0, typ)]
    foundation_list = m.invoke(0x45EC20, typ, (0,))
    offsets = []
    for index in range(64):
        pair = list(struct.unpack('<hh', u.mem_read(foundation_list + index * 4, 4)))
        if pair == [0x7FFF, 0x7FFF]:
            break
        offsets.append(pair)
    else:
        raise AssertionError('Native foundation list has no sentinel within64 pairs')
    m.warhead = warhead
    m.damage_waypoint_inputs = dict(artmd=dict(file='ARTMD.INI', bytes=len(raw_art),
        sha256=hashlib.sha256(raw_art).hexdigest()), layers=layers,
        foundation_dimensions=dimensions, foundation_offsets=offsets,
        damage_particle_system_layers=damage_system_layers)
    return m


def damage_waypoint_receiver_case(inputs, *, area):
    """Real receiver chain on declared translated flat admitted-world priors."""
    from unicorn import UC_HOOK_MEM_WRITE, UC_HOOK_MEM_INVALID
    from tools.spatial_oracle import building_construction as building
    from tools.spatial_oracle.refinery_dock import BLD, HOUSE, BLD_ITEMS, cell, place_building
    from tools.spatial_oracle.object_health import Fixture as HealthFixture
    from tools.spatial_oracle.unit_source_scatter import SCENARIO as receiver_scenario
    f = building.JoinedFixture(inputs, seed=31, arena_size=0x400000)
    u, read = f.u, f.read32
    for registry in (0xA83D68, 0xA83D98):
        u.mem_write(registry, bytes(inputs.u.mem_read(registry, 24)))
    # Existing SparkBirth's empty runtime registry capacity boundary. The
    # actual constructor registers each original-produced Smoke instance.
    for registry in (0xA80208, 0xB0F720):
        u.mem_write(registry, dwords(0x7EB6D4, f.allocate(4096), 1024, 1, 0, 10))
    u.mem_write(BLD, bytes(0x800))
    building.invoke(u, 0x43B740, BLD, inputs.types['NANRCT'], HOUSE)
    ctor_health, flags = read(BLD + 0x6C), read(BLD + 0x14)
    building.invoke(u, 0x45B1C0, 0)
    authored = HealthFixture().map_health('building', 197, read(inputs.types['NANRCT'] + 0xA0))
    initial = authored['output']['actual']
    place_building(u, BLD, (10, 10), constructor_abstract_flags=flags)
    u.mem_write(BLD + 0x520, dwords(inputs.types['NANRCT']))
    for offset in (0x6C, 0x70, 0x544):
        u.mem_write(BLD + offset, dwords(initial))
    for offset, value in ((0x90, 1), (0x74, 1), (0x81, 0), (0x98, 1), (0x6E4, 1)):
        u.mem_write(BLD + offset, bytes([value]))
    u.mem_write(BLD + 0xE0, dwords(0x7E180C, BLD_ITEMS, 1, 1))
    u.mem_write(BLD_ITEMS, dwords(0))
    for y in range(32):
        for x in range(32):
            u.mem_write(cell(x, y) + 0xE4, dwords(BLD if 10 <= x < 14 and 10 <= y < 14 else 0))
    u.mem_write(HOUSE + 0x34, dwords(inputs.country))
    for offset in (0x188, 0x1A8):
        u.mem_write(HOUSE + offset, struct.pack('<d', 1.0))
    u.mem_write(HOUSE + 0x264, dwords(0))
    buildings = f.allocate(16)
    u.mem_write(buildings, dwords(BLD))
    u.mem_write(HOUSE + 0x68, dwords(0x7E4488, buildings, 4, 1, 1, 10))
    u.mem_write(HOUSE + 0x1EC, b'\0\0')
    u.mem_write(0xA8B238, dwords(0))
    u.mem_write(0xA8E9A0, b'\1')
    u.mem_write(0x8464AC, b'\0')
    u.mem_write(0xA8ED84, dwords(15))
    building.invoke(u, 0x561910, 0)
    events, writes, trail = [], [], []
    kinds = {0x489280: 'area_damage', 0x442230: 'building_receive',
        0x701900: 'techno_receive', 0x5F5390: 'object_receive', 0x44E7B0: 'power_output',
        0x65C780: 'rng_next', 0x65C7E0: 'rng_ranged', 0x62DC50: 'particle_system_constructor'}

    def observe(_u, pc, _size, _data):
        trail.append(pc)
        trail[:] = trail[-35:]
        if pc == 0x7C978A:
            f.ret()  # Existing joined receiver owner's CRT exit-registration seam.
            return
        if pc != RET_MAGIC and not any(a <= pc < a + n for a, n in f.code_spans):
            raise AssertionError(dict(non_original_pc=f'{pc:08X}', trail=[f'{a:08X}' for a in trail]))
        if pc not in kinds:
            return
        sp = u.reg_read(UC_X86_REG_ESP)
        event = dict(kind=kinds[pc], pc=f'{pc:08X}')
        if pc in (0x442230, 0x701900, 0x5F5390):
            event.update(damage=struct.unpack('<i', u.mem_read(read(sp + 4), 4))[0],
                distance=read(sp + 8), source_null=read(sp + 16) == 0,
                attacking_house_null=read(sp + 28) == 0)
        elif pc in (0x65C780, 0x65C7E0):
            event.update(caller=f'{read(sp):08X}',
                stream='scenario' if u.reg_read(UC_X86_REG_ECX) == receiver_scenario + 0x218
                else 'main' if u.reg_read(UC_X86_REG_ECX) == 0x886B88 else 'other')
            if pc == 0x65C7E0:
                event['args'] = [read(sp + 4), read(sp + 8)]
        elif pc == 0x62DC50:
            event.update(type_name=bytes(u.mem_read(read(sp + 4) + 0x24, 25))
                .split(b'\0', 1)[0].decode('latin1'),
                coordinate=list(struct.unpack('<3i', u.mem_read(read(sp + 8), 12))))
        events.append(event)

    def write(_u, _access, address, size, value, _data):
        if BLD <= address < BLD + 0x800:
            target, base = 'building', BLD
        elif HOUSE <= address < HOUSE + 0x6000:
            target, base = 'house', HOUSE
        else:
            return
        if target == 'building' and address - base == 0x178:
            return  # Timer's unused middle word is not a gameplay golden.
        writes.append(dict(pc=f'{u.reg_read(UC_X86_REG_EIP):08X}', target=target,
            offset=f'{address - base:X}', size=size, value=value & ((1 << (size * 8)) - 1)))

    def invalid(_u, _access, address, _size, _value, _data):
        raise AssertionError(dict(invalid_pc=f'{u.reg_read(UC_X86_REG_EIP):08X}',
            address=f'{address:08X}', trail=[f'{a:08X}' for a in trail]))

    u.hook_add(UC_HOOK_CODE, observe)
    u.hook_add(UC_HOOK_MEM_WRITE, write)
    u.hook_add(UC_HOOK_MEM_INVALID, invalid)
    damage, coordinate = f.allocate(4), f.allocate(16)

    def power_prefix():
        u.mem_write(SP, dwords(RET_MAGIC))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ECX, HOUSE)
        run_checked(u, 0x4F8440, 0x4F84F1, count=2_000_000)

    def power_state():
        return dict(sampled_health=read(BLD + 0x544), house_power=read(HOUSE + 0x53A4),
                    house_dirty=list(u.mem_read(HOUSE + 0x5778, 2)))

    def rng_states():
        return {name: bytes(u.mem_read(pointer, 0x3F4)).hex()
            for name, pointer in (('scenario', receiver_scenario + 0x218),
                                  ('main', 0x886B88), ('mapgen', 0xABE890))}

    def rng_hashes():
        return {name: hashlib.sha256(bytes.fromhex(state)).hexdigest()
                for name, state in rng_states().items()}

    def smoke_state():
        pointer = read(BLD + 0x310)
        if not pointer:
            return None
        return dict(type_name=bytes(u.mem_read(read(pointer + 0xAC) + 0x24, 25))
            .split(b'\0', 1)[0].decode('latin1'),
            native_unique_id=read(pointer + 0x10),
            coordinate=list(struct.unpack('<3i', u.mem_read(pointer + 0x9C, 12))),
            owner_is_building=read(pointer + 0xE0) == BLD,
            registered_count=read(0xA80218),
            spawn_timer_bits=bytes(u.mem_read(pointer + 0xE8, 4)).hex(),
            lifetime=read(pointer + 0xEC))

    u.mem_write(HOUSE + 0x5778, b'\1')
    power_prefix()
    initial_power = read(HOUSE + 0x53A4)
    before = rng_hashes()
    raw_before = rng_states()
    advances_before = len(f.advances)
    rows = []
    for visit, (dx, dy) in enumerate(((0, 0), (85, 85), (-85, 85), (85, -85), (-85, -85)), 1):
        u.mem_write(damage, dwords(100))
        mark = len(events)
        if area:
            u.mem_write(coordinate, dwords(10 * 256 + 128 + dx, 10 * 256 + 128 + dy, 0))
            u.mem_write(SP, dwords(RET_MAGIC, 0, inputs.warhead, 1, 0))
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_ECX, coordinate)
            u.reg_write(UC_X86_REG_EDX, 100)
            run_checked(u, 0x489280, RET_MAGIC, count=2_000_000)
            answer = u.reg_read(UC_X86_REG_EAX)
        else:
            answer = building.invoke(u, 0x442230, BLD, damage, 0, inputs.warhead, 0, 0, 0, 0)
        output = building.invoke(u, 0x44E7B0, BLD)
        rows.append(dict(visit=visit, offset=[dx, dy], native_return=answer,
            health=read(BLD + 0x6C), estimated=read(BLD + 0x70),
            damaged_mode=u.mem_read(BLD + 0x6E6, 1)[0], sampled_health=read(BLD + 0x544),
            power_output=output, observed_techno_timer=dict(start=read(BLD + 0x174),
                duration=read(BLD + 0x17C)), damage_smoke=smoke_state(), calls=events[mark:]))
    before_sample = power_state()
    u.mem_write(SP, dwords(RET_MAGIC))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ESI, BLD)
    run_checked(u, 0x440042, 0x440072, count=2_000_000)
    after_sample = power_state()
    power_prefix()
    after_power = dict(power_state(), house_drain=read(HOUSE + 0x53A8))
    assert f.code_unchanged()
    return dict(route='area' if area else 'direct', frame=15, ctor_health=ctor_health,
        reactor_native_unique_id=read(BLD + 0x10),
        authored_health_native=authored, rows=rows, rng_before=before, rng_after=rng_hashes(),
        rng_state_before=raw_before, rng_state_after=rng_states(),
        rng_calls=[event for event in events if event['kind'].startswith('rng_')], writes=writes,
        rng_advances=[dict(frame=entry['frame'], raw=entry['raw'],
            masked=entry.get('masked'), range=entry.get('range'), rejected=entry.get('rejected'),
            stream='scenario' if entry['this'] == receiver_scenario + 0x218
            else 'main' if entry['this'] == 0x886B88 else 'other')
            for entry in f.advances[advances_before:]],
        power_consumer=dict(initial_power=initial_power, before_sample=before_sample,
                            after_sample=after_sample, after_power=after_power))


def damage_waypoint_generate(root):
    cases = [dict(name='flat_all_bridge_attempts_fail', cell=[10, 10]),
        dict(name='flat_bridge_first_succeeds', cell=[10, 10], bridge_success_at=0),
        dict(name='flat_bridge_last_succeeds', cell=[10, 10], bridge_success_at=3),
        dict(name='slope_level2', cell=[10, 10], level=2, slope=1),
        dict(name='bridge_100', cell=[10, 10], level=2, flags=0x100),
        dict(name='unset_waypoint', cell=[0, 0])]
    inputs = damage_waypoint_native_inputs(root)
    return dict(schema_version=1,
        dispatch_controls=[damage_waypoint_dispatch_case(case) for case in cases],
        retail_inputs=inputs.damage_waypoint_inputs,
        receiver_controls=[damage_waypoint_receiver_case(inputs, area=area) for area in (False, True)])


def damage_waypoint_metadata():
    return provenance(
        scope='Action63 original dispatch/waypoint geometry; independent real retail NANRCT AreaDamage/Building/Techno/Object receiver, health sampler and retained House power controls.',
        assumptions=[
            'The separate --damage-waypoint mode executes only this corpus and leaves all legacy TAction outputs untouched. External selected physical RULESMD/optional LANGRULE/ALL01 MAP strings pass through existing ordered INI caches, original Rules constructor/scalar blocks, full BuildingType and Warhead readers; fixed ARTMD is not layered. Whole INI/MIX loader, full Rules::Process, scenario-named campaign INI and country/difficulty admission are excluded.',
            'Six dispatch controls execute original constructor/read/Execute63/6E0490. Original Cell47BBF0/GetCoords486840 and registered Cell initialization produce ground104 and Cell bridge deck416. The separate15-entry Scenario initializer table8148A8 executes original683070..683280 in order and initializes GetWaypointLocation bridge heightB054BC to416; a ground level2/bridge100 source therefore has centreZ624. Slope matrices reuse the existing original startup owner. Waypoint364 comes from value+90, not the trailing+44 slot. Downstream AreaDamage, bridge outcomes and projection/redraw are explicit observed sinks in these controls.',
            'The independent receiver controls execute whole original Building43B740, authored197/256 arithmetic44FB67 through existing object_health, original AreaDamage489280 and real442230/701900/5F5390. Original selected561910 initializes the CellSpread table. No damage return, health subtraction, defense kernel or Building power computation is supplied. The referenced DamageParticleSystems are constructed by the original BuildingType reader, then full original ParticleSystemType6442D0 and ParticleType644F50 readers consume their selected physical sections in each applicable layer. Native master-registry indices are fixture-local; comparisons resolve actual selected type names rather than claiming full retail registry chronology.',
            'Declared already-admitted flat32x32 map/one AI Confederation House and translated4x4 Building anchor10,10 stand in for complete map/House/Unlimbo admission. Five Area calls use the separately executed Action63 centres and100/C4/null source/null attacking House ABI; direct calls isolate the same receivers with distance0. House firepower/defense factors1, absent Supers/factories/active slots and constructor-only country are explicit priors, not complete ALL01-world evidence.',
            'After receiver HP writes, original Building sampler440042..440072 rewrites sampledHP and House dirty bytes; original House4F8440..4F84F1 includes retained power508C30 and original Building output44E7B0. Radar and SuperWeapon continuation, demand from the separate YAPPET, active Building/Anim AI, full timer cadence and rendered output are excluded.',
            'Original45B1C0/45EC90/45ECA0/45EC20 produce the foundation dimensions and ordered offsets, with7FFF sentinel, independently of VERA input. Frame15 controls record observed Techno timer+174(start) and+17C(duration21) writes on each hit; existing shrapnel_damage/occupants.md identifies this as RadarCombatFlash, not an attack/rearm timer. Its native+178 padding write is excluded from gameplay goldens. The timer/read/render continuation is not implemented or certified by this corpus. No detach is reached in the nonfatal chain. Hit3 crosses Yellow and original7029DC draws once from Scenario range0..1, selecting SmallGreySSys from the reverse-ordered Smoke list. Full original ParticleSystem62DC50 constructs and publishes the one retained attached system; later hits retain it. Main and MapGen remain unchanged. Raw original-layout states and draw/advance observations record the exact comparison boundary. Native Building43B740 already consumed one Scenario constructor draw before rng_state_before; a bare Rust entity must import that incoming state rather than compare independently seeded priors.'],
        substitutions=[
            'Reuse ActionFixture/Reader/Sound/JoinedFixture allocator, ordered cache, CRT TLS and sparse Cell transports. No original gameplay bytes or retail vtables are changed. CRT exit registration7C978A uses the existing joined receiver platform seam; no callback body is substituted.',
            'Receiver audio is disabled at its original8464AC gate; no playback or NonCritical audio RNG claim. Initial animations are absent and image/archive IO uses existing Reader transport. Native selected ParticleSystem/Particle type registries are copied with their heap-backed original references into JoinedFixture. Empty runtime system registries reuse SparkBirth prepared capacity; real constructor registration, Unlimbo and live Logic publication run unchanged. Particle image binding, ParticleSystem/Particle AI visits, future smoke spawning/rendering and retirement are outside the reached constructor/receiver boundary. This corpus validates receiver damage-mode and attached Smoke fields, not art-slot rendering or whole campaign parity.',
            'Dispatch-only Area/bridge/projection sinks are distinct from the real Area/receiver controls. Their composition is bounded evidence and does not certify all terrain, elevation, bridge, collateral, fatal lifecycle or campaign scheduler routes.'],
        entry_points=dict(action_constructor=0x6DD000, action_read=0x6DD5B0,
            action_execute=0x6DD8B0, damage_waypoint=0x6E0490, area_damage=0x489280,
            building_receiver=0x442230, techno_receiver=0x701900, object_receiver=0x5F5390,
            building_type_ctor=0x45DD90, building_type_read=0x45FE50,
            warhead_factory=0x75E3B0, warhead_read=0x75D3A0, c4_reader=0x66C304,
            building_constructor=0x43B740, authored_building_health=0x44FB67,
            building_health_sample=0x440042, house_prefix=0x4F8440,
            retained_power=0x508C30, building_power_output=0x44E7B0,
            observed_techno_timer_store=0x701FBD, cell_spread_startup=0x561910,
            particle_system_type_read=0x6442D0, particle_type_read=0x644F50,
            damage_smoke_selection=0x7029DC, particle_system_constructor=0x62DC50,
            foundation_init=0x45B1C0, foundation_width=0x45EC90,
            foundation_height=0x45ECA0, foundation_offsets=0x45EC20))


def super_charge_native_inputs(root):
    """Reuse the existing physical/INI transport, then run the Super reader."""
    m = damage_waypoint_native_inputs(root)
    u, read = m.u, m.read32
    raw_rules = (root / 'RULESMD.INI').read_bytes()
    rules = sections(raw_rules)
    u.mem_write(0xA8E330, dwords(0x7EB6D4, m.alloc(4096), 1024, 1, 0, 10))
    m.make_ini({'SuperWeaponTypes': rules['SuperWeaponTypes']})
    m.invoke(0x6725F0, m.rules, (INI,))
    types = [read(read(0xA8E334) + i * 4) for i in range(read(0xA8E340))]
    names = [m.string(pointer + 0x24) for pointer in types]
    sw = types[names.index('PsychicDominatorSpecial')]
    art = sections((root / 'ARTMD.INI').read_bytes())
    m.make_ini({name: body for name, body in art.items()
                if name == 'YAPPET' or name.startswith('YAPPET_')})
    art_cache = bytes(u.mem_read(INI, 0x40))
    building_type = m.alloc(0x2000)
    m.invoke(0x45DD90, building_type, (m.cstring('YAPPET'),))

    def state():
        return dict(registry_index=read(sw + 0x98),
            kind=struct.unpack('<i', u.mem_read(sw + 0xB4, 4))[0],
            recharge_frames=struct.unpack('<i', u.mem_read(sw + 0xB0, 4))[0],
            powered=bool(u.mem_read(sw + 0xE6, 1)[0]),
            show_timer=bool(u.mem_read(sw + 0xF4, 1)[0]),
            manual_control=bool(u.mem_read(sw + 0xF5, 1)[0]),
            charge_drain=bool(u.mem_read(sw + 0xE5, 1)[0]))

    def building_state():
        return dict(super_weapon_index=struct.unpack('<i', u.mem_read(building_type + 0x16F0, 4))[0],
            secondary_super_weapon_index=struct.unpack('<i', u.mem_read(building_type + 0x16F4, 4))[0],
            power_output=struct.unpack('<i', u.mem_read(building_type + 0xEE0, 4))[0],
            power_drain=struct.unpack('<i', u.mem_read(building_type + 0xEE4, 4))[0])

    layers = [dict(file='constructors', state=state(), building=building_state())]
    constructor_recharge = state()['recharge_frames']
    for filename in ('RULESMD.INI', 'LANGRULE.INI', 'ALL01UMD.INI', 'ALL01UMD.MAP'):
        path = root / filename
        if not path.is_file():
            assert filename in ('LANGRULE.INI', 'ALL01UMD.INI')
            layers.append(dict(file=filename, absent=True))
            continue
        raw = path.read_bytes()
        physical = sections(raw)
        m.make_ini({name: physical[name] for name in ('PsychicDominatorSpecial', 'YAPPET') if name in physical})
        rules_ini = m.alloc(0x40)
        u.mem_write(rules_ini, bytes(u.mem_read(INI, 0x40)))
        u.mem_write(INI, art_cache)
        building_answer = m.invoke(0x45FE50, building_type, (rules_ini,)) & 255
        answer = m.invoke(0x6CEA20, sw, (rules_ini,)) & 255
        layers.append(dict(file=filename, sha256=hashlib.sha256(raw).hexdigest(),
                           native_read=answer, state=state(), native_building_read=building_answer,
                           building=building_state()))
    raw_map = (root / 'ALL01UMD.MAP').read_bytes()
    counted_action = sections(raw_map)['Actions']['0B7C0AEC']
    count, _, raw_action = counted_action.partition(',')
    assert count == '1'
    return SimpleNamespace(type_bytes=bytes(u.mem_read(sw, 0x120)),
        building_type_bytes=bytes(u.mem_read(building_type, 0x2000)),
        state=state(), constructor_recharge=constructor_recharge,
        count=len(names), raw_action=raw_action,
        receipt=dict(super_type_registry=names, layers=layers,
            map_sha256=hashlib.sha256(raw_map).hexdigest(),
            rulesmd=dict(file='RULESMD.INI', sha256=hashlib.sha256(raw_rules).hexdigest()),
            artmd=dict(file='ARTMD.INI', sha256=hashlib.sha256((root / 'ARTMD.INI').read_bytes()).hexdigest()),
            scenario_ini=dict(file='ALL01UMD.INI', selected_input_present=(root / 'ALL01UMD.INI').is_file(),
                absence_scope='Selected external fixture only; independently confirm normal retail asset resolution.'),
            map_action=dict(trigger='0B7C0AEC', counted_raw=counted_action),
            f32_one_percent_bits=bytes(u.mem_read(0x7EAAE0, 4)).hex()))


class SuperChargeFixture(ActionFixture):
    """One native image/state for Action129, Super arithmetic and histories."""

    def __init__(self, inputs, *, house_size=0x6000):
        from tools.native_oracle import NATIVE_FPCW, NativeCallTrace
        from unicorn.x86_const import UC_X86_REG_FPCW
        super().__init__()
        u = self.u
        self.native_fpcw, self.fpcw_register = NATIVE_FPCW, UC_X86_REG_FPCW
        self.house, self.super, self.super_type = (self.allocate(n)
            for n in (house_size, 0x100, 0x200))
        self.items, self.house_type = self.allocate(64), self.allocate(0x400)
        for pointer, size in ((self.house, house_size), (self.super, 0x100),
                              (self.super_type, 0x200), (self.items, 64),
                              (self.house_type, 0x400)):
            u.mem_write(pointer, bytes(size))
        u.mem_write(self.super_type, inputs.type_bytes)
        u.mem_write(self.house + 0x34, dwords(self.house_type))
        u.mem_write(self.house + 0x258, dwords(self.items))
        u.mem_write(self.house + 0x264, dwords(inputs.count))
        u.mem_write(self.items + inputs.state['registry_index'] * 4, dwords(self.super))
        # Original Grant's ShowTimer route performs the actual insertion into a
        # prepared empty vector; the retail ShowTimer flag remains unchanged.
        u.mem_write(0xA83D50, dwords(0x7EB6D4, self.allocate(64), 16, 1, 0, 10))
        self.ranges += [(0x6CAF99, 0x6CB011), (0x6CB4D0, 0x6CB7AA),
                        (0x6CBCA0, 0x6CC078), (0x6CC1E0, 0x6CC278)]
        self.original += [bytes(u.mem_read(a, b-a)) for a, b in self.ranges[-4:]]
        for pointer, seed in ((SCENARIO + 0x218, 7), (0x886B88, 7), (0xABE890, 31)):
            self.invoke(0x65C6D0, pointer, (seed,))
        self.native_calls, self.rng_calls, self.stage = [], [], 'setup'
        self.call_receipts = NativeCallTrace(u, self.read32)
        self.retained_power_low = None
        u.hook_add(UC_HOOK_CODE, self.observe_super)

    def invoke(self, entry, this, args=()):
        self.u.reg_write(self.fpcw_register, self.native_fpcw)
        answer = self.call(entry, list(args), ecx=this)
        if hasattr(self, 'call_receipts'):
            self.call_receipts.returned(RET_MAGIC, self.u.reg_read(UC_X86_REG_ESP))
        return answer

    def observe_super(self, u, pc, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        self.call_receipts.returned(pc, sp)
        if pc == 0x50B0A0:
            self.retained_power_low = False
        elif pc == 0x50B0D5:
            self.retained_power_low = True  # Reached native power-ratio low arm.
        if pc in (0x6CB560, 0x6CB4D0, 0x6CBCA0, 0x6CC1E0, 0x6CC260):
            count = 3 if pc == 0x6CB560 else 0 if pc == 0x6CC260 else 1
            self.call_receipts.entered(pc, sp, (f'{pc:08X}', count, count * 4))
            caller = self.read32(sp)
            row = dict(stage=self.stage, callee=f'{pc:08X}',
                caller='fixture' if caller == RET_MAGIC else f'{caller:08X}')
            if pc in (0x6CB4D0, 0x6CC1E0):
                row['argument'] = self.integer(u.reg_read(UC_X86_REG_ESP) + 4)
            self.native_calls.append(row)
        elif pc in (0x65C780, 0x65C7E0):
            pointer = u.reg_read(UC_X86_REG_ECX)
            self.rng_calls.append(dict(stage=self.stage, callee=f'{pc:08X}',
                stream='scenario' if pointer == SCENARIO + 0x218 else
                       'main' if pointer == 0x886B88 else
                       'mapgen' if pointer == 0xABE890 else 'other'))

    def set_state(self, *, frame=500, recharge, custom_recharge=-1,
                  granted=True, ready=False, held=False, start=123, duration=7000):
        u, s = self.u, self.super
        u.mem_write(0xA8ED84, dwords(frame))
        u.mem_write(s + 0x24, dwords(custom_recharge, self.super_type, self.house))
        u.mem_write(self.super_type + 0xB0, dwords(recharge))
        u.mem_write(s + 0x30, dwords(start, 0, duration))
        u.mem_write(s + 0x50, dwords(-1))
        u.mem_write(s + 0x60, b'\1')
        u.mem_write(s + 0x6D, bytes((int(granted), 0, int(ready), int(held))))
        u.mem_write(s + 0x78, dwords(-1))

    def state(self):
        u, s = self.u, self.super
        return dict(start=self.integer(s + 0x30), duration=self.integer(s + 0x38),
            granted=bool(u.mem_read(s + 0x6D, 1)[0]),
            ready=bool(u.mem_read(s + 0x6F, 1)[0]),
            held=bool(u.mem_read(s + 0x70, 1)[0]),
            custom_recharge=self.integer(s + 0x24))

    def initialize_concrete_super(self, frame):
        u = self.u
        u.mem_write(0xA8ED84, dwords(frame))
        u.mem_write(SP + 0xC, dwords(self.super_type, self.house))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ESI, self.super)
        u.reg_write(self.fpcw_register, self.native_fpcw)
        run_checked(u, 0x6CAF99, 0x6CB011, count=200_000)

    def evidence(self):
        self.check_code()
        return dict(calls=self.native_calls, native_call_receipts=self.call_receipts.calls,
                    rng_after=self.rng_hashes(), rng_calls=self.rng_calls)


def super_charge_dispatch_case(inputs, name, **overrides):
    from tools.native_oracle import NativeExecutionError
    case = dict(owner_present=True, index=inputs.state['registry_index'], count=inputs.count,
        percentage=90, granted=True, start=123, duration=7000, ready=False,
        held=True, frame=500, recharge=inputs.state['recharge_frames'], custom_recharge=-1)
    case.update(overrides)
    f = SuperChargeFixture(inputs)
    f.set_state(**{key: value for key, value in case.items()
                   if key not in ('owner_present', 'index', 'count', 'null_super', 'percentage')})
    f.u.mem_write(f.house + 0x264, dwords(case['count']))
    if case.get('null_super'):
        f.u.mem_write(f.items + case['index'] * 4, dwords(0))
    raw = inputs.raw_action if name == 'retail_action' else f"129,11,{case['index']},0,0,0,0,{case['percentage']}"
    materialized = f.read_action(raw)
    read = dict(raw=materialized['raw'], value=materialized['value'], auxiliary=f.integer(ACTION + 0x48))
    before = f.rng_hashes()
    f.stage = 'execute129'
    try:
        returned = f.invoke(0x6DD8B0, ACTION, (f.house if case['owner_present'] else 0, 0, 0, 0)) & 255
        result = dict(returned=returned)
    except NativeExecutionError:
        # The native index==count and null-slot bugs are observations, not a
        # false-return ABI invented for undefined/out-of-bounds native memory.
        if not case.get('null_super') or f.u.reg_read(UC_X86_REG_EIP) != 0x6E42F7:
            raise
        result = dict(fault=dict(pc='006E42F7', address='0000006D', access='read_unmapped'))
    return dict(name=name, read=read, input=case, output=f.state(), rng_before=before,
                **result, **f.evidence())


def super_charge_setter_case(inputs, name, **case):
    f = SuperChargeFixture(inputs)
    f.set_state(**{key: value for key, value in case.items() if key != 'percentage'})
    before = f.rng_hashes()
    f.stage = 'percentage_setter'
    f.invoke(0x6CC1E0, f.super, (case['percentage'],))
    return dict(name=name, input=case, output=f.state(), rng_before=before, **f.evidence())


def super_charge_sequence(inputs, low_power, grant_frame):
    f = SuperChargeFixture(inputs)
    u = f.u
    f.initialize_concrete_super(grant_frame)
    before = f.rng_hashes()
    rows = []

    def record(stage, returned=None):
        row = dict(stage=stage, frame=f.integer(0xA8ED84), state=f.state(),
            charge_stage=f.integer(f.super + 0x78),
            ready_tick=f.integer(f.super + 0x74),
            granted_timer_count=f.integer(0xA83D60),
            own_timer_registered=f.read32(f.read32(0xA83D54)) == f.super
                if f.integer(0xA83D60) else False)
        if returned is not None:
            row['returned'] = returned
        rows.append(row)

    record('concrete_constructor')
    f.stage = 'grant'
    record(f.stage, f.invoke(0x6CB560, f.super, (0, 0, int(low_power))) & 255)
    f.stage = 'execute129'
    u.mem_write(0xA8ED84, dwords(15))
    f.read_action(inputs.raw_action)
    record(f.stage, f.invoke(0x6DD8B0, ACTION, (f.house, 0, 0, 0)) & 255)
    for stage, frame, entry, args in (
        ('first_hold', 15, 0x6CB4D0, (1,)),
        ('repeat_hold', 16, 0x6CB4D0, (1,)),
        ('held_ai', 16, 0x6CBCA0, (0,)),
        ('release_hold', 100, 0x6CB4D0, (0,)),
        ('released_ai', 100, 0x6CBCA0, (0,)),
        ('elapsed_ai', 1001, 0x6CBCA0, (0,))):
        f.stage = stage
        u.mem_write(0xA8ED84, dwords(frame))
        record(stage, f.invoke(entry, f.super, args) & 255)
    return dict(input_low_power_at_grant=low_power, grant_frame=grant_frame,
                rows=rows, rng_before=before, **f.evidence())


def super_charge_house_refresh(inputs, low_power, power_drop_frame=16):
    """Full original 50AF10 against retained, supplied House power values."""
    f = SuperChargeFixture(inputs)
    u, house = f.u, f.house
    building, building_type = f.allocate(0x800), f.allocate(0x2000)
    u.mem_write(building, bytes(0x800))
    u.mem_write(building_type, inputs.building_type_bytes)
    u.mem_write(building + 0x90, b'\1')
    u.mem_write(building + 0x21C, dwords(house))
    u.mem_write(building + 0x520, dwords(building_type))
    u.mem_write(building + 0x660, b'\1')
    for index in range(inputs.count):
        if index != inputs.state['registry_index']:
            other = f.allocate(0x100)
            u.mem_write(other, bytes(0x100))
            u.mem_write(f.items + index * 4, dwords(other))
    # Original House ctor4F5DD3 assigns this COM identity vtable. Its unchanged
    # QueryIdentity410220 compares the admitted Abstract ID at House+10.
    u.mem_write(house + 4, dwords(0x7EA884))
    u.mem_write(house + 0x10, dwords(42))
    u.mem_write(0xA8E9A0, b'\1')
    u.mem_write(0xA8B263, b'\1')
    u.mem_write(0xA8EB44, dwords(f.allocate(4)))
    u.mem_write(f.read32(0xA8EB44), dwords(building))
    u.mem_write(0xA8EB50, dwords(1))
    u.mem_write(0xA83CBC, dwords(f.items))
    u.mem_write(0xA83CC8, dwords(inputs.count))
    u.mem_write(0xA83D4C, dwords(0))
    added = [(0x50AF10, 0x50B1CB), (0x457630, 0x4576E9),
             (0x410220, 0x41022A), (0x6CC2A0, 0x6CC2AA), (0x6ABC60, 0x6ABCC5)]
    f.ranges.extend(added)
    f.original.extend(bytes(u.mem_read(a, b-a)) for a, b in added)
    f.initialize_concrete_super(0)
    before = f.rng_hashes()
    f.stage = 'grant'
    granted = f.invoke(0x6CB560, f.super, (0, 0, int(low_power))) & 255
    u.mem_write(0xA8ED84, dwords(15))
    f.stage = 'execute129'
    f.read_action(inputs.raw_action)
    executed = f.invoke(0x6DD8B0, ACTION, (house, 0, 0, 0)) & 255
    rows = []
    schedule = ((15, 537), (16, 537), (100, 1537), (101, 537)) if low_power or power_drop_frame == 15 else \
               ((15, 1537), (16, 537), (17, 537), (100, 1537))
    for frame, power in schedule:
        u.mem_write(0xA8ED84, dwords(frame))
        u.mem_write(house + 0x53A4, dwords(power, 1000))
        u.mem_write(house + 0x1FC, b'\0')
        incoming, mark = f.state(), len(f.native_calls)
        f.stage = 'house_refresh'
        f.retained_power_low = None
        f.invoke(0x50AF10, house)
        assert f.retained_power_low is not None
        rows.append(dict(frame=frame, retained_output=power, retained_drain=1000,
            input=incoming, output=f.state(), calls=f.native_calls[mark:],
            low_power_before_call=f.retained_power_low,
            house_sidebar_dirty=bool(u.mem_read(house + 0x1FC, 1)[0])))
    return dict(input_low_power_at_grant=low_power, power_drop_frame=power_drop_frame, grant_return=granted,
        action129_return=executed, rows=rows, rng_before=before, **f.evidence())


def super_charge_generate(root, *, receipt_path=None):
    inputs = super_charge_native_inputs(root)
    dispatch = [('retail_action', {}), ('null_owner', dict(owner_present=False)),
        ('negative_index', dict(index=-1)), ('above_count', dict(index=inputs.count + 1)),
        ('ungranted', dict(granted=False)), ('negative_percentage', dict(percentage=-1)),
        ('zero_percentage', dict(percentage=0)), ('one_percentage', dict(percentage=1)),
        ('ninety_nine_percentage', dict(percentage=99)),
        ('hundred_percentage', dict(percentage=100)), ('above_percentage', dict(percentage=101)),
        ('ready_and_held_preserved', dict(ready=True, held=True))]
    arithmetic = []
    bases = [('constructor', inputs.constructor_recharge, -1),
        ('retail', inputs.state['recharge_frames'], -1),
        ('type_i32_max', 2147483647, -1),
        ('type_i32_min', -2147483648, -1),
        ('custom4500', inputs.state['recharge_frames'], 4500),
        ('custom9000', inputs.state['recharge_frames'], 9000),
        ('custom_zero', inputs.state['recharge_frames'], 0),
        ('custom_negative', inputs.state['recharge_frames'], -2),
        ('custom_i32_max', inputs.state['recharge_frames'], 2147483647),
        ('custom_i32_min', inputs.state['recharge_frames'], -2147483648)]
    for label, recharge, custom in bases:
        for percentage in (-1, 0, 1, 90, 99, 100, 101):
            for ready, held in ((False, False), (False, True), (True, False), (True, True)):
                arithmetic.append(super_charge_setter_case(inputs,
                    f'{label}_p{percentage}_ready{int(ready)}_held{int(held)}',
                    percentage=percentage, granted=True, start=123, duration=7000,
                    ready=ready, held=held, frame=500, recharge=recharge, custom_recharge=custom))
    for ready in (False, True):
        arithmetic.append(super_charge_setter_case(inputs, f'ungranted_ready{int(ready)}',
            percentage=90, granted=False, start=123, duration=7000, ready=ready,
            held=True, frame=500, recharge=inputs.state['recharge_frames'], custom_recharge=-1))
    f = SuperChargeFixture(inputs)
    data = dict(schema_version=1, retail_inputs=inputs.receipt,
        rng_seed_states=CampaignReader.rng_state(SimpleNamespace(u=f.u, scenario=SCENARIO)),
        dispatch_controls=[super_charge_dispatch_case(inputs, name, **case) for name, case in dispatch],
        native_edge_controls=[super_charge_dispatch_case(inputs, 'equal_count_mapped_spare',
                count=inputs.state['registry_index']),
            super_charge_dispatch_case(inputs, 'null_super_slot', null_super=True)],
        setter_controls=arithmetic,
        sequence_controls=[super_charge_sequence(inputs, low, frame)
            for frame in (0, 1) for low in (False, True)],
        house_refresh_controls=[super_charge_house_refresh(inputs, low, frame)
            for low, frame in ((False, 15), (False, 16), (True, 15))])
    if receipt_path is not None:
        from tools.native_oracle import image_sha256
        receipt_path = Path(receipt_path).resolve()
        if receipt_path.is_relative_to(Path(__file__).parents[2].resolve()):
            raise ValueError('Detailed native receipts must remain outside the repository')
        receipt_path.parent.mkdir(parents=True, exist_ok=True)
        receipt_path.write_text(json.dumps(dict(native_sha256=image_sha256(),
            producer='tools.spatial_oracle.trigger_action_values --super-charge',
            vectors=data), indent=2) + '\n', encoding='utf-8')
    for category in ('dispatch_controls', 'native_edge_controls', 'setter_controls',
                     'sequence_controls', 'house_refresh_controls'):
        for case in data[category]:
            del case['native_call_receipts']
    return data


def super_charge_metadata():
    return provenance(
        scope='Independent Action129 native constructor/read/Execute dispatch, Super percentage arithmetic and bounded Grant/Suspend/AI histories.',
        assumptions=[
            'The separate --super-charge mode generates/checks only trigger_super_charge_percent and leaves legacy and Action63 payloads unchanged. Existing physical/ordered-INI and native type-reader transports are reused; original Rules6725F0 registers the actual retail SuperWeaponTypes list, original SuperWeaponType constructor and6CEA20 consume PsychicDominatorSpecial in physical RULESMD, optional LANGRULE, optional ALL01UMD.INI, then ALL01 map order. Constructor recharge4500 and retail recharge9000 are native outputs, not hand goldens. Absent supplemental layers denote the selected extracted fixture, not every retail/mod installation; production normal asset resolution independently binds that absence. Fixed ART is separate and not layered. Whole MIX/INI IO, full Rules::Process and country/difficulty admission are excluded.',
            'Action129 controls execute original6DD000/6DD5B0/6DD8B0 including active switch6DFB06 and6E42D0. Parameter type11 puts stock index7 at+90 and token8 percentage90 at+48; the real ALL01 action supplies the stock case. Null House, negative index, index>count, ungranted and percentage outside0..100 returnAL0. Original index==count uses a supplied mapped spare; a null slot faults. Those two native_edge_controls record native bugs rather than establish safe Rust collection semantics.',
            'Setter6CC1E0 runs unchanged with native x87 PC53 and original_ftol7C5F00. The exact native f32 constant at7EAAE0 has bits3C23D70A; the two multiplies, conversion and signed wrapping subtraction are executed. Custom+24 values exercise the existing native getter choice, including i32 extrema; they are resolved arithmetic controls, not Action128 or custom-charge lifecycle parity. Timer padding+34 is unused stack data and excluded. Ready is only set when remaining==0; held is retained; valid percentages restart the timer even when held.',
            'Each history uses one native image/state: concrete Super initializer6CAF99..6CB011 after the excluded Abstract constructor, full Grant6CB560, real Action129, first/repeated Suspend6CB4D0 and full AI6CBCA0. Physical Type flags, including ShowTimer, remain intact; native Grant inserts the Super into prepared empty timer-vector storage. The history uses supplied one House, frame0/1 grant then15/16/100/1001 visits and explicit low-power argument, not an executed full House power/scheduler or a certified ALL01 firing frame. Release/expiration rows include original charge-stage and ready transitions; local-owner announcement is false. ChargeDrain, full Super/House registration/retirement, animations and rendering remain outside this bounded chain.',
            'Three house_refresh_controls add complete original50AF10 after Grant/Action129 in the same state: normal Grant then first power drop at frame15 or16, and initially held Grant followed by set/repeated hold. Real layered YAPPET BuildingType45FE50 supplies primary/secondary SW indexes and power fields. A supplied already-admitted alive/nonlimbo/online Building and original House identity vtable expose unchanged410220/457630/457690; remaining Supers are explicitly ungranted. Native hold/release decisions read retained1537/537 output and1000 drain. These numbers are retained-power boundary priors from separate reactor evidence, not power recomputation within this control. Successful hold/release writes House+1FC; repeated hold does not. A8E9A0/A8B263 enable gates and a nonlocal, nondefeated House are supplied. Initial grants50B1D0, full House AI, Building online updates, dirty5778/5779 consumption, and cross-object scheduling remain outside the reached50AF10 boundary.',
            'Three RNG streams are seeded through original65C6D0. Raw incoming original-layout states, before/after hashes and observed original Random/RandomRanged entries record the comparison boundary. No RNG draw, detach or anim call occurs in these admitted controls; this is not a whole House AI or future fire/cleanup claim.'],
        substitutions=[
            'Reuse the existing ActionFixture vector/heap, ordered INI and CRT per-thread TLS platform transports. No original gameplay bytes or retail vtables are changed; original Grant, Suspend, SetPercentage, GetRechargeTime and AI are never replaced.',
            'Already-admitted native House/Super pointers and prepared capacity supply registry/storage priors. Original concrete constructor, type materialization, actual Grant timer insertion and selected consumers run within those priors; full Abstract object identity, asset playback and global scenario lifecycle are excluded.',
            'Successful original calls use run_checked and inherited ActionFixture ABI/stack checks. Canonical vectors retain compact caller/callee/order observations and numeric states. Detailed NativeCallTrace stack/register/return receipts are optional external audit data, written by this same generator when requested; they are neither repository artifacts nor Rust-test inputs. The separately identified null-slot fault is not a successful ABI result.'],
        entry_points=dict(action_constructor=0x6DD000, action_read=0x6DD5B0,
            action_execute=0x6DD8B0, execute129=0x6E42D0, registry_read=0x6725F0,
            super_type_read=0x6CEA20, concrete_super_ctor=0x6CAF99,
            building_type_ctor=0x45DD90, building_type_read=0x45FE50,
            grant=0x6CB560, suspend=0x6CB4D0, super_ai=0x6CBCA0,
            house_refresh=0x50AF10, building_primary_super=0x457630,
            building_secondary_super=0x457690, house_identity=0x410220,
            charge_stage=0x6CBEE0, set_percentage=0x6CC1E0, recharge_time=0x6CC260,
            rng_constructor=0x65C6D0))


@lru_cache(maxsize=1)
def superweapon_state_native_inputs():
    """Physical MD layers and original readers for the selected ALL03 path."""
    from tools.input_oracle.campaign_start import prepare_house_rules
    from tools.native_oracle import image_sha256
    directory = configured_gamemd().parent
    names = ('RULESMD.INI', 'LANGRULE.INI', 'ALL03UMD.INI', 'ALL03UMD.MAP')
    payloads, sources = {}, []
    for name in names:
        path = directory / name
        if path.is_file():
            payloads[name] = path.read_bytes()
            sources.append(dict(file=name, source='loose', sha256=hashlib.sha256(payloads[name]).hexdigest()))
    # The existing stock MIX decoder supplies bytes only. These explicit
    # supported-install probes do not replace the production AssetManager.
    for archive in ('langmd.mix', 'language.mix', 'expandmd01.mix', 'mapsmd03.mix'):
        raw = (directory / archive).read_bytes()
        members = mix(raw)
        for name in names:
            data = members.get(mix_hash(name))
            if data is not None and name not in payloads:
                payloads[name] = data
                sources.append(dict(file=name, source=archive,
                    archive_sha256=hashlib.sha256(raw).hexdigest(), sha256=hashlib.sha256(data).hexdigest()))
    if 'RULESMD.INI' not in payloads or 'ALL03UMD.MAP' not in payloads:
        raise ValueError('The selected retail install must provide patched RULESMD.INI and ALL03UMD.MAP')
    master = (directory / 'ra2md.mix').read_bytes()
    local = mix(master)[mix_hash('localmd.mix')]
    raw_art = mix(local)[mix_hash('ARTMD.INI')]
    sources.append(dict(file='ARTMD.INI', source='ra2md.mix -> localmd.mix',
        archive_sha256=hashlib.sha256(master).hexdigest(),
        member_archive_sha256=hashlib.sha256(local).hexdigest(),
        sha256=hashlib.sha256(raw_art).hexdigest()))
    rules, values = sections(payloads['RULESMD.INI']), sections(payloads['ALL03UMD.MAP'])
    # Reader's optional asset root preserves its existing transport owner.
    # Only the MD section dictionaries below enter the native INI reader.
    reader = CampaignReader(Path(__file__).parents[2] / 'ini')
    reader.invoke(0x6832C0, reader.scenario)
    rules_pointer = prepare_house_rules(reader, rules)
    reader.ordered_ini({'SuperWeaponTypes': rules['SuperWeaponTypes']})
    reader.invoke(0x6725F0, rules_pointer, (INI,))
    types = [reader.read32(reader.read32(0xA8E334) + index * 4)
             for index in range(reader.read32(0xA8E340))]
    registry = [reader.string(pointer + 0x24) for pointer in types]
    super_pointer = types[registry.index('NukeSpecial')]
    reader.u.mem_write(0xA83C68, dwords(0x7EB6D4, reader.alloc(4096), 1024, 1, 0, 10))
    provider_type = reader.alloc(0x2000)
    reader.invoke(0x45DD90, provider_type, (reader.cstring('NAMISL'),))
    art = sections(raw_art)
    reader.ordered_ini({key: body for key, body in art.items()
        if key == 'NAMISL' or key.startswith('NAMISL_')})
    art_cache = bytes(reader.u.mem_read(INI, 0x40))

    def super_state():
        return dict(registry_index=reader.read32(super_pointer + 0x98),
            kind=reader.read32(super_pointer + 0xB4),
            recharge_frames=reader.read32(super_pointer + 0xB0),
            powered=bool(reader.u.mem_read(super_pointer + 0xE6, 1)[0]),
            show_timer=bool(reader.u.mem_read(super_pointer + 0xF4, 1)[0]),
            manual_control=bool(reader.u.mem_read(super_pointer + 0xF5, 1)[0]),
            charge_drain=bool(reader.u.mem_read(super_pointer + 0xE5, 1)[0]))

    def provider_state():
        return dict(registry_index=reader.read32(provider_type + 0x98),
            nuke_silo=bool(reader.u.mem_read(provider_type + 0x16BA, 1)[0]),
            super_weapon_index=struct.unpack('<i', reader.u.mem_read(provider_type + 0x16F0, 4))[0],
            secondary_super_weapon_index=struct.unpack('<i', reader.u.mem_read(provider_type + 0x16F4, 4))[0],
            power_output=reader.read32(provider_type + 0xEE0), power_drain=reader.read32(provider_type + 0xEE4))

    layers = [dict(file='constructor', super_state=super_state(), provider_state=provider_state())]
    for name in names:
        if name not in payloads:
            layers.append(dict(file=name, absent=True))
            continue
        layer = sections(payloads[name])
        reader.ordered_ini({key: layer[key] for key in ('NukeSpecial', 'NAMISL') if key in layer})
        rules_ini = reader.alloc(0x40)
        reader.u.mem_write(rules_ini, bytes(reader.u.mem_read(INI, 0x40)))
        reader.u.mem_write(INI, art_cache)
        provider_returned = reader.invoke(0x45FE50, provider_type, (rules_ini,)) & 255
        returned = reader.invoke(0x6CEA20, super_pointer, (rules_ini,)) & 255
        layers.append(dict(file=name, returned=returned, super_state=super_state(),
            provider_returned=provider_returned, provider_state=provider_state()))
    registrations = []
    for name in names:
        if name not in payloads:
            continue
        layer = sections(payloads[name])
        reader.ordered_ini({'Countries': layer.get('Countries', {})})
        reader.u.reg_write(UC_X86_REG_ESI, INI)
        reader.u.reg_write(UC_X86_REG_EDI, rules_pointer)
        reader.region(0x668C8B, 0x668CDB)
        registrations.append(dict(file=name, names=[reader.string(
            reader.read32(reader.read32(0xA83C9C) + index * 4) + 0x24)
            for index in range(reader.read32(0xA83CA8))]))
    houses = []
    for name in ('Americans', 'YuriCountry'):
        reader.ordered_ini({name: values[name]})
        label = reader.cstring(name)
        country = reader.invoke(0x475540, INI, (label, reader.cstring('Country'), 0xFFFFFFFF))
        iq = reader.invoke(0x5276D0, INI, (label, reader.cstring('IQ'), 0))
        control = reader.invoke(0x5295F0, INI, (label, reader.cstring('PlayerControl'), 0)) & 255
        houses.append(dict(name=name, country_index=country, iq=iq, player_control=bool(control), raw=values[name]))
    f = ActionFixture()
    identities = ('0AFF8D8C', '094F5C2C', '094DE72C')
    routes = []
    for identity in identities:
        raw = values['Actions'][identity]
        # The counted loop, not Python's token positions, owns both the
        # materialized fields and physical action order.
        f.ordered_ini({'Actions': {identity: raw}})
        key = f.allocate(len(identity) + 1)
        f.u.mem_write(key, identity.encode('ascii') + b'\0')
        section = f.allocate(8)
        f.u.mem_write(section, b'Actions\0')
        f.call(0x528A10, (section, key, 0x889F64, BUFFER, 512), ecx=INI)
        token = f.call(0x7C9CC2, (BUFFER, 0x817F70), cdecl=True)
        count = f.call(0x7C9BFD, (token,), cdecl=True)
        definition = f.allocate(0xB8)
        f.u.mem_write(definition, bytes(0xB8))
        f.u.reg_write(UC_X86_REG_EAX, count)
        f.u.reg_write(UC_X86_REG_EBX, 0)
        f.u.reg_write(UC_X86_REG_EBP, definition)
        f.u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(f.u, 0x727558, 0x7275A5)
        actions, pointer = [], f.read32(definition + 0xB0)
        while pointer:
            actions.append(dict(kind=f.integer(pointer + 0x2C), value=f.integer(pointer + 0x90),
                waypoint=f.integer(pointer + 0x44), auxiliary=f.integer(pointer + 0x48)))
            pointer = f.read32(pointer + 0x28)
            assert len(actions) <= count
        assert len(actions) == count
        routes.append(dict(id=identity, trigger=values['Triggers'][identity], events=values['Events'][identity],
            actions=raw, native_actions=actions,
            tags={key: body for key, body in values['Tags'].items() if body.endswith(',' + identity)}))
    waypoint_indices = sorted({action['waypoint'] for route in routes
        for action in route['native_actions'] if action['kind'] == 135})
    reader.ordered_ini({'Waypoints': values['Waypoints']})
    waypoints = []
    for index in waypoint_indices:
        # The Windows decimal-key formatter is outside this scalar-reader
        # region. ReadInt and both signed unpacking operations run unchanged.
        reader.u.mem_write(SP + 0x10, str(index).encode('ascii') + b'\0')
        reader.u.reg_write(UC_X86_REG_EBX, INI)
        reader.u.reg_write(UC_X86_REG_ESI, index)
        reader.u.reg_write(UC_X86_REG_EDI, reader.scenario + 0x632 + index * 4)
        reader.region(0x68BDE7, 0x68BE35, required=(0x5276D0,))
        waypoints.append(dict(index=index, raw=values['Waypoints'].get(str(index)),
            cell=list(struct.unpack('<2h', reader.u.mem_read(reader.scenario + 0x632 + index * 4, 4)))))
    # Reuse the original scenario Building reader through its actual first
    # allocation boundary. Its House/type lookups, ReadString128, strtok and
    # atoi own the selected authored provider's identity/strength/cell/tag.
    # Full Building construction, placement and country admission are outside
    # this input extraction; the following plain-name House identities are
    # declared lookup priors for the already separately read map Houses.
    house_items = reader.alloc(8)
    house_pointers = []
    for slot, name in enumerate(('Americans', 'YuriCountry')):
        pointer = reader.alloc(0x16100)
        reader.u.mem_write(pointer, bytes(0x16100))
        reader.u.mem_write(pointer + 0x15FF4, name.encode('ascii') + b'\0')
        reader.u.mem_write(house_items + slot * 4, dwords(pointer))
        house_pointers.append(pointer)
    reader.u.mem_write(0xA8022C, dwords(house_items))
    reader.u.mem_write(0xA80238, dwords(2))
    reader.u.mem_write(0xA8B238, dwords(0))
    reader.ordered_ini({'Basic': values['Basic'],
        'Structures': {'1033236': values['Structures']['1033236']}})
    reader.u.reg_write(UC_X86_REG_ESI, reader.scenario)
    reader.u.reg_write(UC_X86_REG_EDI, INI)
    reader.u.reg_write(UC_X86_REG_EAX, reader.read32(reader.scenario + 0x1C70))
    reader.region(0x68A13D, 0x68A15B, required=(0x5276D0,))
    new_ini_format = reader.read32(0xA8ED7C)
    reader.u.reg_write(UC_X86_REG_ECX, INI)
    reader.region(0x44F820, 0x44FAF8, required=(0x528A10, 0x50C170, 0x45E7B0, 0x7C9BFD))
    sp = reader.u.reg_read(UC_X86_REG_ESP)
    provider_read = dict(new_ini_format=new_ini_format,
        house_index=house_pointers.index(reader.read32(sp + 0x24)),
        type_index=reader.read32(sp + 0x20), authored_strength=reader.read32(sp + 0x18),
        cell=list(struct.unpack('<2h', reader.u.mem_read(sp + 0x14, 4))),
        tag=reader.string(reader.u.reg_read(UC_X86_REG_EBP)),
        read_string_capacity=128, boundary='0044FAF8 before Building allocation')
    reader.check_code(); f.check_code()
    receipt = dict(native_sha256=image_sha256(), sources=sources,
        registry=registry, layers=layers, house_inputs=houses, country_registration=registrations,
        routes=routes, waypoints=waypoints,
        provider=values['Structures']['1033236'], provider_read=provider_read,
        map_provider_overrides=values['NAMISL'])
    return SimpleNamespace(type_bytes=bytes(reader.u.mem_read(super_pointer, 0x120)),
        provider_type_bytes=bytes(reader.u.mem_read(provider_type, 0x2000)),
        state=super_state(), count=len(registry), receipt=receipt,
        waypoints={row['index']: row['cell'] for row in waypoints}, raw_actions=routes[0]['actions'])


def superweapon_state_dispatch_case(inputs, name, raw, *, owner_present=True,
                                    preferred=(17, 18), waypoint_cell=(31, 32),
                                    count=None, null_super=False, mapped_waypoint_spare=False, **state):
    """Original132/135/136 dispatch; malformed native memory stays an edge."""
    from tools.native_oracle import NativeExecutionError
    f = SuperChargeFixture(inputs)
    f.set_state(recharge=inputs.state['recharge_frames'], **state)
    f.u.mem_write(f.house + 0x54F0, struct.pack('<2h', *preferred))
    other_house, other_super = f.allocate(0x6000), f.allocate(0x100)
    f.u.mem_write(other_house, bytes(0x6000))
    f.u.mem_write(other_super, bytes(0x100))
    f.u.mem_write(other_house + 0x54F0, struct.pack('<2h', 45, 46))
    f.u.mem_write(other_super + 0x24, dwords(73, f.super_type, other_house))
    actual_count = inputs.count if count is None else count
    f.u.mem_write(f.house + 0x264, dwords(actual_count))
    read = f.read_action(raw, ini_capacity=512)
    read['auxiliary'] = f.integer(ACTION + 0x48)
    if 0 <= read['waypoint'] < 702 or mapped_waypoint_spare:
        f.u.mem_write(SCENARIO + 0x632 + read['waypoint'] * 4, struct.pack('<2h', *waypoint_cell))
    if null_super:
        f.u.mem_write(f.items + read['value'] * 4, dwords(0))
    before, incoming = f.rng_hashes(), f.state()
    f.stage = f"execute{read['kind']}"
    for start, end in ((0x6CC280, 0x6CC288), (0x50DA00, 0x50DA20), (0x68BCC0, 0x68BCE0)):
        f.ranges.append((start, end))
        f.original.append(bytes(f.u.mem_read(start, end-start)))
    try:
        returned = f.invoke(0x6DD8B0, ACTION, (f.house if owner_present else 0, 0, 0, 0)) & 255
        answer = dict(returned=returned)
    except NativeExecutionError:
        if not null_super or f.u.reg_read(UC_X86_REG_EIP) != 0x6CC284:
            raise
        answer = dict(fault=dict(pc='006CC284', address='00000024', access='write_unmapped'))
    output = dict(f.state(), preferred=list(struct.unpack('<2h', f.u.mem_read(f.house + 0x54F0, 4))))
    output.update(other_preferred=list(struct.unpack('<2h', f.u.mem_read(other_house + 0x54F0, 4))),
        other_custom_recharge=f.invoke(0x6CC260, other_super))
    return dict(name=name, read=read, input=dict(incoming, preferred=list(preferred),
        waypoint_cell=list(waypoint_cell), owner_present=owner_present, count=actual_count,
        frame=f.integer(0xA8ED84), mapped_waypoint_spare=mapped_waypoint_spare,
        other_preferred=[45, 46], other_custom_recharge=73),
        output=output, rng_before=before, **answer, **f.evidence())


def superweapon_state_fixture(inputs, *, fire_boundary=False):
    """Admitted small House/map world; every selected gameplay callee is real."""
    from unicorn.x86_const import UC_X86_REG_FPCW
    f = SuperChargeFixture(inputs, house_size=0x16100)
    u = f.u
    f.state_events, f.fire_boundary = [], fire_boundary
    f.other_house, f.other_house_type = f.allocate(0x16100), f.allocate(0x400)
    u.mem_write(f.other_house, bytes(0x16100))
    u.mem_write(f.other_house_type, bytes(0x400))
    u.mem_write(f.other_house + 0x34, dwords(f.other_house_type))
    for index, house in enumerate((f.other_house, f.house)):
        u.mem_write(house + 0x30, dwords(index))
        u.mem_write(house + 0x54EC, dwords(1))
        u.mem_write(house + 0x5788, dwords(3))
    u.mem_write(f.other_house + 0x54F0, struct.pack('<2h', 45, 46))
    country = {row['name']: row['country_index'] for row in inputs.receipt['house_inputs']}
    u.mem_write(f.house_type + 0xB4, dwords(country['YuriCountry']))
    u.mem_write(f.other_house_type + 0xB4, dwords(country['Americans']))
    u.mem_write(f.house + 0x5600, dwords(0))
    u.mem_write(f.house + 0x184, dwords(1))
    u.mem_write(f.house + 0x254, dwords(0x7EA4E4))
    f.houses = f.allocate(8)
    u.mem_write(f.houses, dwords(f.other_house, f.house))
    u.mem_write(0xA8022C, dwords(f.houses))
    u.mem_write(0xA80238, dwords(2))
    u.mem_write(0xA83D4C, dwords(f.other_house))
    # Scenario campaign mode, nonhuman NPC, no running storm or Dominator.
    u.mem_write(0xA8B238, dwords(0))
    u.mem_write(0xA8E9A0, b'\1')
    f.trigger = f.allocate(0x80)
    u.mem_write(f.trigger, bytes(0x80))
    f.rules = f.allocate(0x2000)
    u.mem_write(f.rules, bytes(0x2000))
    u.mem_write(0x8871E0, dwords(f.rules))
    f.startup = initialize_waypoint_map(f)
    f.call(0x47BBF0, [], ecx=0xABDC50)
    # ObjectClass owns another copy of these header constants. Execute its
    # original ordered thirteen-pointer CRT table; zero-filled LevelHeight
    # would incorrectly make grounded actors high-flying.
    u.reg_write(UC_X86_REG_FPCW, 0x027F)
    for index in range(13):
        f.call(f.read32(0x8141D8 + index * 4), [])
    f.startup['object_level_leptons'] = f.integer(0xAC13C8)
    u.reg_write(UC_X86_REG_FPCW, f.native_fpcw)
    for offset, value in ((0xF4, 40), (0xFC, 2), (0x100, 4), (0x104, 36), (0x108, 36)):
        u.mem_write(0x87F7E8 + offset, dwords(value))
    u.mem_write(0x87F7E8 + 0x124, bytes(16))
    u.mem_write(0x89EA40, bytes(12 * 36))
    u.mem_write(0xB0F5B8, dwords(0x7EB6D4, f.allocate(64), 16, 1, 0, 10))
    f.provider, f.provider_type = f.allocate(0x800), f.allocate(0x2000)
    u.mem_write(f.provider, bytes(0x800))
    u.mem_write(f.provider_type, inputs.provider_type_bytes)
    u.mem_write(f.provider, dwords(0x7E3EBC))
    px, py = inputs.receipt['provider_read']['cell']
    u.mem_write(f.provider + 0x74, b'\1')
    u.mem_write(f.provider + 0x90, b'\1')
    u.mem_write(f.provider + 0x9C, dwords(px * 256 + 128, py * 256 + 128, 0))
    u.mem_write(f.provider + 0x21C, dwords(f.house))
    u.mem_write(f.provider + 0x520, dwords(f.provider_type))
    u.mem_write(f.provider + 0xAC, dwords(5, 0, -1))
    u.mem_write(f.provider + 0x5F8, dwords(-1))
    provider_items = f.allocate(4)
    u.mem_write(provider_items, dwords(f.provider_type))
    u.mem_write(0xA83C6C, dwords(provider_items))
    u.mem_write(0xA83C78, dwords(1))
    buildings, counts = f.allocate(4), f.allocate(4)
    u.mem_write(buildings, dwords(f.provider))
    u.mem_write(counts, dwords(1))
    u.mem_write(f.house + 0x6C, dwords(buildings))
    u.mem_write(f.house + 0x78, dwords(1))
    u.mem_write(f.house + 0x5500, dwords(0x7EB6D4, counts, 1))
    for index, cell in inputs.waypoints.items():
        u.mem_write(SCENARIO + 0x632 + index * 4, struct.pack('<2h', *cell))
        tile = f.allocate(0x200)
        f.call(0x47BBF0, [], ecx=tile)
        u.mem_write(tile + 0x24, struct.pack('<2h', *cell))
        u.mem_write(f.table + (cell[1] * 512 + cell[0]) * 4, dwords(tile))
    added = [(0x4FAE50, 0x4FB0D6), (0x4FD060, 0x4FD104),
        (0x5098F0, 0x50A346), (0x50CBF0, 0x50D3D0),
        (0x6CB920, 0x6CBCA0), (0x6CC260, 0x6CC28A), (0x6CC390, 0x6CE1F0),
        (0x5F36F0, 0x5F38DF), (0x5B3570, 0x5B364B), (0x49FAE0, 0x49FB32)]
    f.ranges.extend(added)
    f.original.extend(bytes(u.mem_read(a, b-a)) for a, b in added)
    calls = {0x4FAE50: 'fire', 0x6CB920: 'click_fire', 0x6CC390: 'launch',
        0x6CE0B0: 'reset', 0x50CBF0: 'best_rally', 0x50D170: 'team_rally',
        0x56DC20: 'nearby_passable', 0x509E00: 'storm_rally',
        0x509CD0: 'ground_rally', 0x509F60: 'genetic_rally', 0x50A150: 'dominator_rally'}

    pending_cells = {}
    def observe(vm, pc, _size, _data):
        sp = vm.reg_read(UC_X86_REG_ESP)
        if pc in pending_cells:
            event = pending_cells.pop(pc)
            event['returned_cell'] = list(struct.unpack('<2h',
                vm.mem_read(vm.reg_read(UC_X86_REG_EAX), 4)))
        if pc in calls:
            row = dict(stage=f.stage, call=calls[pc])
            if pc == 0x4FAE50:
                row.update(index=f.integer(sp + 4), cell=list(struct.unpack('<2h',
                    vm.mem_read(f.read32(sp + 8), 4))))
            elif pc in (0x50CBF0, 0x56DC20):
                pending_cells[f.read32(sp)] = row
                if pc == 0x56DC20:
                    args = [f.integer(sp + 4 + index * 4) for index in range(15)]
                    row.update(input_cell=list(struct.unpack('<2h', vm.mem_read(args[1], 4))),
                        speed_type=args[2], movement_zone=args[3],
                        scalar_arguments=args[2:12] + args[13:],
                        footprint=list(struct.unpack('<2h', vm.mem_read(args[12], 4))))
            f.state_events.append(row)
            if pc == 0x4FAE50 and f.fire_boundary:
                return_from_sink(vm, 8, 1)
        elif pc == 0x65C7E0:
            f.rng_calls[-1]['bounds'] = [f.integer(sp + 4), f.integer(sp + 8)]
        elif pc in (0x750E20, 0x752700):
            # Audio/EVA device transports are outside the selected simulation.
            f.state_events.append(dict(stage=f.stage, call='audio' if pc == 0x750E20 else 'eva'))
            return_from_sink(vm, 4, 0)
    u.hook_add(UC_HOOK_CODE, observe)
    return f


def superweapon_state_sequence(inputs):
    f = superweapon_state_fixture(inputs)
    u = f.u
    f.initialize_concrete_super(0)
    initial = f.state()
    before, rows = f.rng_hashes(), []

    def record(stage, *, raw_action=None, returned=None):
        assert f.read32(f.super + 0x28) == f.super_type, (stage, hex(f.super),
            hex(f.read32(f.super + 0x28)), hex(f.super_type))
        row = dict(stage=stage, frame=f.integer(0xA8ED84), state=f.state(),
            preferred=list(struct.unpack('<2h', u.mem_read(f.house + 0x54F0, 4))),
            other_preferred=list(struct.unpack('<2h', u.mem_read(f.other_house + 0x54F0, 4))),
            enemy_index=f.integer(f.house + 0x5600),
            effective_recharge=f.integer_reg(UC_X86_REG_EAX))
        row['effective_recharge'] = f.invoke(0x6CC260, f.super)
        if row['effective_recharge'] >= 0x80000000:
            row['effective_recharge'] -= 0x100000000
        if raw_action is not None:
            row['raw_action'] = raw_action
        if returned is not None:
            row['returned'] = returned
        if stage == 'npc_ai_fire':
            row.update(fire=[event for event in f.state_events if event['stage'] == stage and event['call'] == 'fire'],
                nuke_target=list(struct.unpack('<2h', u.mem_read(f.house + 0x5784, 4))),
                firing_type=f.integer(f.provider + 0x5F8),
                provider_mission=f.integer(f.provider + 0xAC),
                provider_queued_mission=f.integer(f.provider + 0xB4))
        rows.append(row)

    f.stage = 'grant'
    record(f.stage, returned=f.invoke(0x6CB560, f.super, (0, 0, 0)) & 255)
    u.mem_write(0xA8ED84, dwords(15))
    tokens = inputs.raw_actions.split(',')[1:]
    for offset in range(0, len(tokens), 8):
        raw = ','.join(tokens[offset:offset + 8])
        read = f.read_action(raw, ini_capacity=512)
        f.stage = f"execute{read['kind']}"
        returned = f.invoke(0x6DD8B0, ACTION, (f.house, 0, f.trigger, 0)) & 255
        record(f.stage, raw_action=raw, returned=returned)
    for frame in (2715, 2716):
        f.stage = 'before_elapsed_ai' if frame == 2715 else 'elapsed_ai'
        u.mem_write(0xA8ED84, dwords(frame))
        f.invoke(0x6CBCA0, f.super, (0,))
        record(f.stage)
    # MakeEnemy's real threat-map rebuild invalidates the selected enemy.
    # Whole House target acquisition/cadence is outside this small world;
    # declare the next ready NPC visit's already selected enemy explicitly.
    f.stage = 'enemy_selection_prior'
    u.mem_write(f.house + 0x5600, dwords(0))
    record(f.stage)
    f.stage = 'npc_ai_fire'
    f.invoke(0x5098F0, f.house)
    record(f.stage)
    u.mem_write(0xA8ED84, dwords(3000))
    f.stage = 'reset_running'
    f.invoke(0x6CE0B0, f.super)
    record(f.stage)
    return dict(name='all03_authored_nuke', initial_state=initial, initial_frame=0,
        initial_preferred=[0, 0], initial_other_preferred=[45, 46], startup=f.startup, rows=rows,
        events=f.state_events, rng_before=before, **f.evidence())


def superweapon_state_consumers(inputs, kind):
    """Set -> native consumer -> clear -> same native consumer/history."""
    from tools.spatial_oracle.map_queries import TABLE
    f = superweapon_state_fixture(inputs, fire_boundary=True)
    u = f.u
    # AI selection tests only ready. The intentionally ungranted boundary
    # lets Rust call its ordinary Fire_SW and reject at real ClickFire without
    # launching unrelated SW effects beyond this control's observation seam.
    f.set_state(recharge=inputs.state['recharge_frames'], granted=False, ready=True)
    u.mem_write(f.super_type + 0xB4, dwords(kind))
    u.mem_write(f.house + 0x5788, dwords(2))
    u.mem_write(f.other_house + 0x5788, dwords(1))
    u.mem_write(f.other_house + 0x5490, struct.pack('<2h', 0, 0))
    u.mem_write(0xA9FAB4, dwords(0))
    u.mem_write(0xA9FAC0, dwords(0))
    f.invoke(0x561910, 0)
    f.call(0x47BBF0, [], ecx=0xABDC50)
    objects = []
    # Plain, equally valued enemy buildings exercise a real tie without
    # handwritten WhatAmI, Layer, GetCoords or selector answers. NAMISL's
    # actual layered type has no special Ion-target value classification.
    for cell in ((31, 32), (40, 25)):
        actor, tile = f.allocate(0x800), f.allocate(0x200)
        u.mem_write(actor, bytes(0x800))
        f.call(0x47BBF0, [], ecx=tile)
        u.mem_write(tile + 0x24, struct.pack('<2h', *cell))
        u.mem_write(TABLE + (cell[1] * 512 + cell[0]) * 4, dwords(tile))
        u.mem_write(actor, dwords(0x7E3EBC))
        u.mem_write(actor + 0x74, b'\1')
        u.mem_write(actor + 0x90, b'\1')
        u.mem_write(actor + 0x9C, dwords(cell[0] * 256 + 128, cell[1] * 256 + 128, 0))
        u.mem_write(actor + 0x21C, dwords(f.other_house))
        u.mem_write(actor + 0x520, dwords(f.provider_type))
        objects.append(actor)
    array = f.allocate(8)
    u.mem_write(array, dwords(*objects))
    u.mem_write(0xA8EC7C, dwords(array))
    u.mem_write(0xA8EC88, dwords(len(objects)))
    actor, actor_type, tile = f.allocate(0x800), f.allocate(0x2000), f.allocate(0x200)
    u.mem_write(actor, bytes(0x800))
    u.mem_write(actor_type, bytes(0x2000))
    f.call(0x47BBF0, [], ecx=tile)
    u.mem_write(tile + 0x24, struct.pack('<2h', 31, 32))
    u.mem_write(tile + 0xE4, dwords(actor))
    u.mem_write(TABLE + (32 * 512 + 31) * 4, dwords(tile))
    u.mem_write(actor, dwords(0x7EB058))
    u.mem_write(actor + 0x14, dwords(7))
    u.mem_write(actor + 0x74, b'\1')
    u.mem_write(actor + 0x90, b'\1')
    u.mem_write(actor + 0x9C, dwords(31 * 256 + 128, 32 * 256 + 128, 0))
    u.mem_write(actor + 0x21C, dwords(f.other_house))
    u.mem_write(actor + 0x6C0, dwords(actor_type))
    infantry_items = f.allocate(4)
    u.mem_write(infantry_items, dwords(actor))
    # Preserve real constructor membership for the selected actors. No
    # infantry is registered in the rally/ground fixtures; in the other two
    # fixtures that infantry is present in Techno, Infantry and Foot arrays.
    use_infantry = kind in (9, 7)
    if use_infantry:
        u.mem_write(0xA8EC7C, dwords(infantry_items))
        u.mem_write(0xA8EC88, dwords(1))
    for items, count in ((0xA83DEC, 0xA83DF8), (0x8B3DC4, 0x8B3DD0)):
        u.mem_write(items, dwords(infantry_items))
        u.mem_write(count, dwords(int(use_infantry)))
    initial = f.state()
    before, rows = f.rng_hashes(), []

    def record(stage, returned=None, raw_action=None):
        row = dict(stage=stage, state=f.state(), preferred=list(struct.unpack('<2h',
            u.mem_read(f.house + 0x54F0, 4))),
            other_preferred=list(struct.unpack('<2h', u.mem_read(f.other_house + 0x54F0, 4))),
            events=[event for event in f.state_events if event['stage'] == stage],
            rng_calls=[event for event in f.rng_calls if event['stage'] == stage],
            rng_after=f.rng_hashes())
        if returned is not None:
            row['returned'] = returned
        if raw_action is not None:
            row['raw_action'] = raw_action
        rows.append(row)

    for stage, raw in (('set_preferred', '135,0,364,0,0,0,0,NA'),
                       ('clear_preferred', '136,0,999,0,0,0,0,ZZ')):
        f.stage = stage
        f.read_action(raw, ini_capacity=512)
        record(stage, f.invoke(0x6DD8B0, ACTION, (f.house, 0, f.trigger, 0)) & 255, raw)
        f.stage = 'preferred_consumer' if stage == 'set_preferred' else 'cleared_consumer'
        f.invoke(0x5098F0, f.house)
        record(f.stage)
    return dict(name={0: 'multimissile', 2: 'lightning_storm', 5: 'ground',
                     9: 'genetic_mutator', 7: 'psychic_dominator'}[kind],
        kind=kind, initial_state=initial, frame=500, startup=f.startup,
        input=dict(preferred_type=1, enemy_index=0, difficulty=1,
            buildings=[dict(owner='enemy', type='NAMISL', cell=[31, 32]),
                       dict(owner='enemy', type='NAMISL', cell=[40, 25])] if not use_infantry else [],
            infantry=[dict(owner='enemy', cell=[31, 32], immune_to_psionics=False,
                           balloon=False, iron_curtained=False)] if use_infantry else [],
            enemy_base=[0, 0], initial_other_preferred=[45, 46],
            sparse_flat_cells=[[31, 32], [40, 25], *inputs.waypoints.values()],
            map_word_f8=f.integer(0x87F7E8 + 0xF8),
            map_bounds=[f.integer(0x87F7E8 + offset) for offset in (0x124, 0x128, 0x12C, 0x130)],
            ground_land0_foot_bits=bytes(u.mem_read(0x89EA40, 4)).hex()),
        rows=rows, rng_before=before, **f.evidence())


def superweapon_state_reset(inputs, *, paused=False):
    f = superweapon_state_fixture(inputs)
    u = f.u
    f.set_state(recharge=inputs.state['recharge_frames'], custom_recharge=4500,
        start=-1 if paused else 123, duration=7000, ready=True, held=paused)
    u.mem_write(f.house + 0x54F0, struct.pack('<2h', 31, 32))
    animation = f.allocate(0x200)
    u.mem_write(animation, bytes(0x200))
    u.mem_write(animation + 0x195, b'\3')
    u.mem_write(f.super + 0x68, dwords(animation))
    u.mem_write(f.super + 0x6C, b'\1')
    u.mem_write(f.super + 0x74, dwords(77, 12, 5))
    notification_items = f.read32(0xB0F5BC)
    u.mem_write(notification_items, dwords(f.super))
    u.mem_write(0xB0F5C8, dwords(1))

    def state():
        return dict(f.state(), charge_stage=f.integer(f.super + 0x78),
            ready_tick=f.integer(f.super + 0x74), charge_drain_state=f.integer(f.super + 0x7C),
            one_time=bool(u.mem_read(f.super + 0x6E, 1)[0]),
            animation_present=bool(f.read32(f.super + 0x68)),
            animation_invalid=bool(u.mem_read(f.super + 0x6C, 1)[0]),
            animation_loops=u.mem_read(animation + 0x195, 1)[0],
            notification_count=f.integer(0xB0F5C8),
            preferred=list(struct.unpack('<2h', u.mem_read(f.house + 0x54F0, 4))))
    incoming, before = state(), f.rng_hashes()
    f.stage = 'reset'
    f.invoke(0x6CE0B0, f.super)
    return dict(name='paused' if paused else 'running', frame=500,
        input=incoming, output=state(), rng_before=before, **f.evidence())


def superweapon_state_generate(*, receipt_path=None):
    inputs = superweapon_state_native_inputs()
    controls = [('132_retail_4500', '132,11,0,0,0,0,0,4500', {}),
        ('132_null_owner', '132,11,0,0,0,0,0,4500', dict(owner_present=False)),
        ('132_negative_index', '132,11,-1,0,0,0,0,4500', {}),
        ('132_above_count', f'132,11,{inputs.count + 1},0,0,0,0,4500', {}),
        ('132_ungranted_ready_held', '132,11,0,0,0,0,0,4500', dict(granted=False, ready=True, held=True)),
        ('135_waypoint_independent_of_scalar', '135,0,999,0,0,0,0,NA', {}),
        ('135_null_owner', '135,0,364,0,0,0,0,NA', dict(owner_present=False)),
        ('135_empty_waypoint', '135,0,364,0,0,0,0,NA', dict(waypoint_cell=(0, 0))),
        ('135_one_zero_component', '135,0,364,0,0,0,0,NA', dict(waypoint_cell=(0, 9))),
        ('135_signed_cell', '135,0,364,0,0,0,0,NA', dict(waypoint_cell=(-3, 32767))),
        ('136_clear', '136,0,999,0,0,0,0,ZZ', {}),
        ('136_already_empty', '136,0,999,0,0,0,0,ZZ', dict(preferred=(0, 0))),
        ('136_null_owner', '136,0,999,0,0,0,0,ZZ', dict(owner_present=False))]
    for value in (-1, -2, 0, 2147483647, -2147483648):
        controls.append((f'132_signed_{value}', f'132,11,0,0,0,0,0,{value}', {}))
    f = SuperChargeFixture(inputs)
    data = dict(schema_version=1, retail_inputs=inputs.receipt,
        rng_seed_states=CampaignReader.rng_state(SimpleNamespace(u=f.u, scenario=SCENARIO)),
        dispatch_controls=[superweapon_state_dispatch_case(inputs, name, raw, **options)
            for name, raw, options in controls],
        native_edge_controls=[
            superweapon_state_dispatch_case(inputs, '132_equal_count_mapped_spare',
                '132,11,0,0,0,0,0,4500', count=0),
            superweapon_state_dispatch_case(inputs, '132_null_super_slot',
                '132,11,0,0,0,0,0,4500', null_super=True),
            superweapon_state_dispatch_case(inputs, '135_outside_array_mapped_spare',
                '135,0,364,0,0,0,0,AAA', mapped_waypoint_spare=True)],
        sequence_controls=[superweapon_state_sequence(inputs)],
        reset_controls=[superweapon_state_reset(inputs, paused=paused) for paused in (False, True)],
        consumer_controls=[superweapon_state_consumers(inputs, kind) for kind in (0, 2, 5, 9, 7)])
    if receipt_path is not None:
        path = Path(receipt_path).resolve()
        if path.is_relative_to(Path(__file__).parents[2].resolve()):
            raise ValueError('Detailed native receipts must remain outside the repository')
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(data, indent=2) + '\n', encoding='utf-8')
    for category in ('dispatch_controls', 'native_edge_controls', 'sequence_controls',
                     'reset_controls', 'consumer_controls'):
        for case in data[category]:
            del case['native_call_receipts']
    return data


def superweapon_state_metadata():
    return dict(provenance(
        scope='Selected ALL03 NPC NukeSpecial Action129->135->132->38 state, effective recharge and shared preferred-cell consumers.',
        assumptions=[
            'Physical patched RULESMD from expandmd01, optional LANGRULE/ALL03 campaign INI, and ALL03 map use existing source-order caches and original native readers. Fixed ARTMD comes from ra2md/localmd and is not layered. Native Rules6725F0 registers the twelve actual SuperWeaponTypes; NukeSpecial6CEA20 and NAMISL45DD90/45FE50 execute. Physical file/member/archive SHA256 identities are retained below and in retail_inputs. Full AssetManager/Rules Process, archive IO and the whole campaign are outside this bounded corpus.',
            'Original Action ctor/read, counted TriggerType action loop727558..7275A5 and Execute6DD8B0 own materialization and order. The unique dispatch table base is6DFDEC;132/135/136 reach6E4320/6E43E0/6E4440 and real6CC280/50DA00/50DA10. Action135 uses+44 and ignores scalar+90; empty(0,0) refuses while one zero component and signed cells remain valid. Action136 clears only its House target.132 writes signed custom+24 independently of grant/readiness/hold/current timer. Equal-count/null Super and a declared mapped one-past-waypoint spare are isolated native memory edges, not safe Rust collection semantics.',
            'The original44F820..44FAF8 Structures reader prefix consumes the actual NAMISL record through ReadString128, real House/type-name lookups, strtok and atoi, stopping before allocation/placement. Native68A13D..68A15B reads Basic.NewINIFormat. Two declared plain-name House identities provide its lookup prior; separately executed Country/IQ/PlayerControl readers establish the selected physical House values. Provider cell, strength and Tag, raw Trigger/Event/Tag binding and native counted actions are recorded. Whole Building/House construction and admission are excluded.',
            'The ALL03 sequence declares a previously admitted NPC Yuri House, Americans enemy, one available native-read NAMISL and one concrete Super/Grant at frame0. It executes real129(70),135(wp364),132(4500),38 in file order at frame15, then explicit Super AI visits2715/2716, NPC AI/Fire_SW/ClickFire/Launch and Reset3000. MakeEnemy executes its real threat-map rebuild and invalidates EnemyHouseIndex; enemy_selection_prior explicitly supplies enemy0 for the next ready NPC visit. The current percentage timer and future effective charge both come from original execution; no hand remaining-time golden is used. Whole House power/AI cadence, target acquisition, Building admission and the eventual missile mission/Bullet/damage are excluded; actual Silo QueueMission/Commence, House NukeTarget, ClickFire recharge and far-away defense-alert calls are reached.',
            'Five set/consumer/clear/consumer histories execute real MultiMissile5098F0, Storm509E00, Ground509CD0, Genetic509F60 and Dominator50A150. Preferred nonzero bypasses rally/nearby selection for the first three and suppresses the last two. Clearing restores each actual path; two equally valued native-vtable enemy buildings witness the original rally tie and Scenario RandomRanged in separate rally worlds. Genetic/Dominator instead have one native-vtable infantry consistently registered in Techno, Infantry and Foot, with no overlapping Building. Its immunity/balloon/iron fields are declared false. All consumer Supers are supplied ungranted but ready: original AI checks ready alone; their Fire_SW observation seam therefore can be compared to an ordinary Rust ClickFire refusal without unrelated launches. No WhatAmI, Layer, GetCoords, GetCell, picker, eligibility or RNG answer is replaced.',
            'The sparse map has actual Cell constructors and explicit flat coordinates; missing cells use the original constructed Dummy. Ground fallback supplies empty enemy Base/ComputedBase centers, zero Map+F8 and zero rectangle+124..130, original playfield fields40/2/4/36/36, and the explicitly zero-initialized global Ground speed table. The real56DC20 input/scalars/return and Land0 Foot bits are recorded; its empty-cell return then the caller signed-short+2 execute, rather than an assumed passable flat terrain answer. This is an empty-center boundary control, not ordinary nonempty-base navigation parity. The sequence has zero Rules scalar priors and far-away bases, so reached defense alerts draw no RNG; retail defense-distance scheduling is outside scope.',
            'Reset6CE0B0 controls execute actual animation-loop clearing, notification vector removal, ready/cameo reset and running/paused timer calculation while retaining custom/hold/ready-tick/drain/preferred values. The opaque animation supplies only the written loops byte; full Anim creation/expiration is excluded. No native detach call is reached. Arithmetic runs with native0E7F; Object header constants execute their original ordered8141D8 startup table with CRT027F before gameplay. Three RNG streams are initialized by original65C6D0, with raw incoming layouts, before/after hashes and actual Random/RandomRanged entries/bounds.'],
        substitutions=[
            'Reuse ActionFixture/SuperChargeFixture, CampaignReader optional explicit asset root, existing ordered INI, heap, CRT TLS and sparse map transports. No original gameplay instruction or retail vtable is modified. Only audio/EVA device calls are recorded sinks in the real Nuke route. Consumer-only controls stop at recorded Fire_SW arguments, separately from the real Nuke firing route.',
            'Native-read type scalar layouts are copied into each isolated admitted world; unrelated heap pointers in those layouts are never consumed. Registry/list capacities and already admitted House/Super/Building/Infantry state are declared fixture priors, not constructor or full-world admission parity. Raw optional ABI receipts stay external; code hashes and local validation results are not repository provenance.'],
        entry_points=dict(action_ctor=0x6DD000, action_read=0x6DD5B0, counted_actions=0x727558,
            execute=0x6DD8B0, action132=0x6E4320, action135=0x6E43E0, action136=0x6E4440,
            set_custom=0x6CC280, get_recharge=0x6CC260, set_preferred=0x50DA00,
            clear_preferred=0x50DA10, super_type_read=0x6CEA20, provider_type_read=0x45FE50,
            grant=0x6CB560, set_percentage=0x6CC1E0, super_ai=0x6CBCA0,
            npc_ai=0x5098F0, fire=0x4FAE50, click_fire=0x6CB920, launch=0x6CC390,
            reset=0x6CE0B0, best_rally=0x50CBF0, storm=0x509E00, ground=0x509CD0,
            genetic=0x509F60, dominator=0x50A150, provider_record=0x44F820,
            new_ini_format=0x68A13D, rng_seed=0x65C6D0)),
        retail_sources=superweapon_state_native_inputs().receipt['sources'])


class SuperResetFixture(SuperChargeFixture):
    """Explicit played-state priors; existing image/allocator/reader own execution.

    Execute6DD8B0's133/134 arms call6E4360/6E43A0, then clear6CC290 or
    the shared Reset6CE0B0. No Python implementation of either leaf is used.
    Notification Find525400/Remove45ADD0 execute on a small native vector;
    the animation prior exposes only the Reset-written loops byte+195.
    """

    def __init__(self, inputs, **options):
        super().__init__(inputs)
        u = self.u
        self.notification_items = self.allocate(64)
        u.mem_write(0xB0F5B8, dwords(0x7EB6D4, self.notification_items, 16, 1, 0, 10))
        self.initialize_concrete_super(0)
        self.constructor_state = self.state()
        state = dict(frame=500, recharge=inputs.state['recharge_frames'],
            custom_recharge=4500, granted=True, ready=True, held=False,
            start=123, duration=7000)
        state.update({key: value for key, value in options.items() if key in state})
        self.set_state(**state)
        u.mem_write(self.house + 0x54F0, struct.pack('<2h', 31, 32))
        u.mem_write(self.super + 0x6E, b'\1')
        u.mem_write(self.super + 0x74, dwords(77, 12, 5))
        self.animation = self.allocate(0x200)
        u.mem_write(self.animation, bytes(0x200))
        u.mem_write(self.animation + 0x195, b'\3')
        animation, invalid = options.get('animation', True), options.get('animation_invalid', True)
        u.mem_write(self.super + 0x68, dwords(self.animation if animation else 0))
        u.mem_write(self.super + 0x6C, bytes((invalid,)))
        self.other_house, self.other_super = self.allocate(0x6000), self.allocate(0x100)
        u.mem_write(self.other_house, bytes(0x6000))
        u.mem_write(self.other_super, bytes(0x100))
        u.mem_write(self.other_house + 0x54F0, struct.pack('<2h', 45, 46))
        u.mem_write(self.other_super + 0x24, dwords(73, self.super_type, self.other_house))
        entries = [self.super, self.other_super] if animation or invalid else [self.other_super]
        if options.get('duplicate_notification'):
            entries.insert(0, self.super)
        u.mem_write(self.notification_items, dwords(*entries))
        u.mem_write(0xB0F5C8, dwords(len(entries)))
        for begin, end in ((0x6CC260, 0x6CC2A0), (0x6CE0B0, 0x6CE1A0), (0x45ADD0, 0x45AE01)):
            self.ranges.append((begin, end))
            self.original.append(bytes(u.mem_read(begin, end - begin)))
        self.capture = False
        self.reset_calls, self.reset_writes = [], []
        self.opaque_writes = []
        find = self.read32(self.read32(0xB0F5B8) + 0x10)
        entries = {0x6DD8B0: ('execute', 4),
            0x6E4320: ('action132', 4), 0x6CC280: ('set_override', 1),
            0x6E4360: ('action133', 4), 0x6CC290: ('clear_override', 0),
            0x6E43A0: ('action134', 4), 0x6CE0B0: ('reset', 0),
            0x45ADD0: ('notification_remove', 1), find: ('notification_find', 1)}

        def observe(vm, pc, _size, _data):
            if not self.capture or pc not in entries:
                return
            name, count = entries[pc]
            sp = vm.reg_read(UC_X86_REG_ESP)
            self.call_receipts.entered(pc, sp, (name, count, count * 4))
            caller = self.read32(sp)
            self.reset_calls.append(dict(stage=self.stage, call=name, callee=f'{pc:08X}',
                caller='fixture' if caller == RET_MAGIC else f'{caller:08X}'))

        def wrote(vm, _access, address, size, value, _data):
            if not self.capture:
                return
            if self.super <= address < self.super + 0x100:
                owner, offset = 'selected_super', address - self.super
            elif self.animation <= address < self.animation + 0x200:
                owner, offset = 'animation', address - self.animation
            elif 0xB0F5B8 <= address < 0xB0F5D0:
                owner, offset = 'notification_vector', address - 0xB0F5B8
            elif self.notification_items <= address < self.notification_items + 64:
                owner, offset = 'notification_items', address - self.notification_items
            else:
                return
            raw = (value & ((1 << (8 * size)) - 1)).to_bytes(size, 'little')
            row = dict(pc=f'{vm.reg_read(UC_X86_REG_EIP):08X}', owner=owner,
                offset=f'{offset:02X}', size=size,
                value=self.reference(value) if owner == 'notification_items' else int.from_bytes(raw, 'little', signed=size == 4))
            if owner == 'selected_super' and offset == 0x34:
                self.opaque_writes.append(row)  # Stack padding has no gameplay golden.
            else:
                self.reset_writes.append(row)
        u.hook_add(UC_HOOK_CODE, observe)
        u.hook_add(UC_HOOK_MEM_WRITE, wrote)

    def reference(self, pointer):
        if pointer == self.super:
            return 'selected_super'
        if pointer == self.other_super:
            return 'other_super'
        raise ValueError(f'Unexpected notification identity {pointer:08X}')

    def reset_state(self):
        u, s = self.u, self.super
        effective = self.invoke(0x6CC260, s)
        count = self.integer(0xB0F5C8)
        return dict(self.state(), effective_recharge=struct.unpack('<i', dwords(effective))[0],
            charge_stage=self.integer(s + 0x78), ready_tick=self.integer(s + 0x74),
            charge_drain_state=self.integer(s + 0x7C),
            one_time=bool(u.mem_read(s + 0x6E, 1)[0]),
            animation_present=bool(self.read32(s + 0x68)),
            animation_invalid=bool(u.mem_read(s + 0x6C, 1)[0]),
            animation_loops=u.mem_read(self.animation + 0x195, 1)[0],
            notification_count=count,
            notifications=[self.reference(self.read32(self.notification_items + index * 4)) for index in range(count)],
            preferred=list(struct.unpack('<2h', u.mem_read(self.house + 0x54F0, 4))))

    def dispatch(self, inputs, name, raw, *, owner_present=True):
        read = self.read_action(raw, ini_capacity=512)
        read['auxiliary'] = self.integer(ACTION + 0x48)
        # Native accepts index==Count then dereferences a spare/null pointer.
        # That UB remains a declared residual, never an admitted control.
        assert read['value'] != inputs.count
        incoming, before = self.reset_state(), self.rng_hashes()
        other = bytes(self.u.mem_read(self.other_super, 0x100))
        self.reset_calls, self.reset_writes, self.opaque_writes = [], [], []
        call_start, rng_start = len(self.call_receipts.calls), len(self.rng_calls)
        self.stage, self.capture = name, True
        try:
            returned = self.invoke(0x6DD8B0, ACTION, (self.house if owner_present else 0, 0, 0, 0)) & 255
        finally:
            self.capture = False
        output = self.reset_state()
        self.check_code()
        return dict(name=name, read=read, returned=returned,
            input=dict(incoming, owner_present=owner_present, count=inputs.count,
                frame=self.integer(0xA8ED84), other_preferred=[45, 46], other_custom_recharge=73),
            output=dict(output,
                other_preferred=list(struct.unpack('<2h', self.u.mem_read(self.other_house + 0x54F0, 4))),
                other_custom_recharge=struct.unpack('<i', dwords(self.invoke(0x6CC260, self.other_super)))[0],
                other_super_unchanged=other == bytes(self.u.mem_read(self.other_super, 0x100))),
            calls=self.reset_calls, writes=self.reset_writes,
            rng_before=before, rng_after=self.rng_hashes(), rng_calls=self.rng_calls[rng_start:],
            native_call_receipts=self.call_receipts.calls[call_start:], opaque_timer_writes=self.opaque_writes)


def superweapon_reset_generate(*, receipt_path=None):
    """Fresh physical Type input, then actual133/134 dispatch on declared states."""
    from itertools import product
    inputs = superweapon_state_native_inputs()
    index = inputs.state['registry_index']
    raw = lambda kind: f'{kind},11,{index},0,0,0,0,NA'
    controls = []

    def record(name, kind, **options):
        owner_present = options.pop('owner_present', True)
        action = options.pop('raw_action', raw(kind))
        controls.append(SuperResetFixture(inputs, **options).dispatch(inputs, name, action,
            owner_present=owner_present))

    for kind in (133, 134):
        for granted, ready, held, stopped in product((False, True), repeat=4):
            record(f'{kind}_g{int(granted)}_r{int(ready)}_h{int(held)}_stopped{int(stopped)}', kind,
                granted=granted, ready=ready, held=held, start=-1 if stopped else 123)
    for custom in (-1, 0, -2, 2147483647, -2147483648):
        for stopped in (False, True):
            record(f'134_custom_{custom}_stopped{int(stopped)}', 134,
                custom_recharge=custom, start=-1 if stopped else 123)
        record(f'133_custom_{custom}_preserves_negative_timer', 133,
            custom_recharge=custom, duration=-79, start=-1)
    for frame in (0, -1, 2147483647, -2147483648):
        for stopped in (False, True):
            record(f'134_frame_{frame}_stopped{int(stopped)}', 134,
                frame=frame, custom_recharge=-2, start=-1 if stopped else 123)
    for animation, invalid in ((False, False), (False, True), (True, False)):
        record(f'134_animation{int(animation)}_invalid{int(invalid)}', 134,
            animation=animation, animation_invalid=invalid)
    record('134_duplicate_notifications_declared_prior', 134, duplicate_notification=True)
    for kind in (133, 134):
        record(f'{kind}_null_owner', kind, owner_present=False)
        record(f'{kind}_negative_index', kind, raw_action=f'{kind},11,-1,0,0,0,0,NA')
        record(f'{kind}_above_count', kind, raw_action=f'{kind},11,{inputs.count + 1},0,0,0,0,NA')

    sequences = []
    for stopped in (False, True):
        f = SuperResetFixture(inputs, custom_recharge=-1, start=-1 if stopped else 123, held=not stopped)
        sequence = dict(name='stopped' if stopped else 'running', initial_frame=f.integer(0xA8ED84),
            constructor_state=f.constructor_state, initial_state=f.reset_state(), rng_before=f.rng_hashes(), rows=[])
        for name, action in (('set132', f'132,11,{index},0,0,0,0,4500'),
                ('reset134_custom', raw(134)), ('clear133', raw(133)), ('reset134_type_fallback', raw(134))):
            sequence['rows'].append(f.dispatch(inputs, name, action))
        sequence['rng_after'] = f.rng_hashes()
        sequences.append(sequence)
    f = SuperChargeFixture(inputs)
    data = dict(schema_version=1, retail_inputs=inputs.receipt,
        rng_seed_states=CampaignReader.rng_state(SimpleNamespace(u=f.u, scenario=SCENARIO)),
        dispatch_controls=controls, sequence_controls=sequences)
    cases = controls + [case for sequence in sequences for case in sequence['rows']]
    if receipt_path is not None:
        path = Path(receipt_path).resolve()
        if path.is_relative_to(Path(__file__).parents[2].resolve()):
            raise ValueError('Detailed native receipts must remain outside the repository')
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(data, indent=2) + '\n', encoding='utf-8')
    for case in cases:
        del case['native_call_receipts']
        del case['opaque_timer_writes']
    return data


def superweapon_reset_metadata():
    return dict(provenance(
        scope='Original133/134 Read/Execute, Super clear6CC290 and shared Reset6CE0B0 on declared selected-House/Super priors; two132->134->133->134 histories.',
        assumptions=[
            'Each fresh process calls existing superweapon_state_native_inputs: original SuperWeaponTypes6725F0, NukeSpecial6CEA20 and provider readers run on physical RULESMD, optional LANGRULE/ALL03 campaign INI and ALL03 map; fixed ARTMD is not layered. No serialized machine-local native cache is required. Physical source identities, registry and reader-layer states are retained in retail_inputs. Full Rules/Scenario startup is outside scope.',
            'Original action constructor6DD000, ReadString512/tokenizer/read6DD5B0 and Execute6DD8B0 run. Execute1336DFBD5->6E4360->6CC290 clears custom+24 only;1346DFC02->6E43A0->6CE0B0 reaches the same Reset as spy infiltration. The outer Trigger.Spring7265C0 incoming-House resolver remains an instruction-established caller, not an executed full Trigger/Tag world here.',
            'SuperChargeFixture executes original concrete constructor suffix6CAF99..6CB011 after excluded Abstract/House allocation. The selected native Type layout supplies recharge9000; supplied current timer7000/custom4500 distinguish the owners. One-time, ready-tick77, drain5, charge-stage12, preferred cells and grant/ready/held/timer combinations are explicit played-state priors, not certified whole-world construction or ordinary retail reachability for every combination.',
            'All73 dispatch observations execute original instructions, including signed custom0/-1/-2/i32 limits, independent grant/ready/held/running/stopped combinations and current-frame0/-1/i32 limits. Only custom==-1 falls back to Type. Reset preserves override and the old stopped/running condition; held is independent. Numeric values are native outputs, not a Python timer implementation.',
            'Animation prior supplies loops+195 and a small native notification vector containing the selected and another Super. Original Find525400/Remove45ADD0 and Reset writes execute; the duplicate-notification control is a declared boundary prior. There is no full registered Anim constructor, future Anim.AI expiration, destructor/detach or rendered-disappearance claim. No detach call or RNG draw is reached by these admitted dispatches.',
            'Original65C6D0 initializes three complete RNG streams; raw incoming states, per-case hashes and observed native Random entries bound the comparison. Opaque timer clock-padding+34 and detailed ABI receipts are excluded from canonical gameplay data. Unknown notification identities fail instead of being silently aliased.',
            'Null incoming House, negative index and index>Count execute real false returns. Native133/134 reject only index>Count; index==Count and a null selected pointer can access invalid memory and are deliberately not executed. Safe Rust collection refusal does not claim parity for those unsafe native inputs. No authored133/134 retail campaign occurrence or full Super/House AI/fire chronology is certified.'],
        substitutions=[
            'Reuse the existing ActionFixture/SuperChargeFixture image, allocator, physical Type input reader, ordered lexical INI caches, CRT TLS and OS imports. No selected action, clear, Reset, timer or vector instruction is patched or replaced. State/animation/vector capacity priors are supplied as declared inputs.',
            'Selected native Type scalar bytes cross into isolated VMs; unrelated heap pointers in that layout are not dereferenced. The other Super/House are unchanged sentinels. Canonical controls retain semantic state, native callee order and owner writes; raw ABI/opaque-padding receipts are optional external output only.'],
        entry_points=dict(action_ctor=0x6DD000, action_read=0x6DD5B0, execute=0x6DD8B0,
            action132=0x6E4320, set_override=0x6CC280, action133=0x6E4360, clear_override=0x6CC290,
            action134=0x6E43A0, reset=0x6CE0B0, effective_recharge=0x6CC260,
            notification_find=0x525400, notification_remove=0x45ADD0,
            registry_read=0x6725F0, super_type_read=0x6CEA20, rng_seed=0x65C6D0)),
        retail_sources=superweapon_state_native_inputs().receipt['sources'])


def inline_type_generate():
    """Original constructor/read, independent of the older action corpora."""
    rows = []
    for raw in (
        '125,10,NANRCT,0,0,0,0,NA',
        '125,10,GTGCAN,0,0,0,0,EJ',
        '125,10, TypeNameWithMoreThan24Bytes,0,0,0,0,NZ',
        '125,10,-1tail,0,0,0,0,NA',
        '131,9,GTGCAN,0,0,0,0,90',
        '131,9,-1tail,0,0,0,0,90',
        '125,0,NANRCT,0,0,0,0,NA',
    ):
        fixture = ActionFixture()
        row = fixture.read_action(raw)
        row['inline_type'] = fixture.string(ACTION + 0x54)
        fixture.check_code()
        rows.append(row)
    return rows


def inline_type_metadata():
    return provenance(
        scope='TAction6DD000/6DD5B0 inline Type+54 and independent waypoint/value fields',
        assumptions=['ASCII action chunks include the actual ALL01 NANRCT/GTGCAN parameter-type10 forms; preallocated registry storage and tokenizer transport use ActionFixture'],
        substitutions=['Heap/CRT thread-data allocation uses the existing reader fixture; no action reader, constructor or switch-table byte is replaced'],
        entry_points={'constructor': 0x6DD000, 'read': 0x6DD5B0,
                      'inline_copy': 0x6DD7F6, 'waypoint_decode': 0x763690})


def flash_team_generate():
    """Full original104/6F1F30/6EF5F0; no Flash body is substituted."""
    rows = []
    cases = (
        dict(name='null_type', type_present=False,
             teams=[dict(type='matching', owner=0, members=[0])]),
        dict(name='no_live_teams', type_present=True, teams=[]),
        dict(name='all01_mixed_types_and_houses', type_present=True, teams=[
            dict(type='matching', owner=0, members=[3, 0]),
            dict(type='other', owner=0, members=[1]),
            dict(type='matching', owner=1, members=[2]),
            dict(type='matching', owner=1, members=[])]),
    )
    for case in cases:
        fixture = ActionFixture()
        u = fixture.u
        types = []
        for name in ('088C1A4C', 'OtherFlashTeam'):
            u.mem_write(BUFFER, name.encode('ascii') + b'\0')
            assert fixture.call(0x6F1920, [], ecx=BUFFER)
            types.append(fixture.definitions('team')[-1])
        # Original ALL01 Enter_Tanya operand: param-type5 supplies token8
        # as signed +90; the reference itself lives at +30 independently.
        reference = '088C1A4C' if case['type_present'] else '<none>'
        raw = f'104,5,{reference},0,0,0,0,40'
        read = fixture.read_action(raw)
        assert fixture.read32(ACTION + 0x30) == (types[0] if case['type_present'] else 0)
        initial = [17, -7, 2147483647, -2147483648]
        members = [fixture.allocate(0x800) for _ in initial]
        for pointer, count in zip(members, initial):
            u.mem_write(pointer, bytes(0x800))
            u.mem_write(pointer + 0xF0, dwords(count))
        teams = []
        for definition in case['teams']:
            pointer = fixture.allocate(0xA4)
            u.mem_write(pointer, bytes(0xA4))
            u.mem_write(pointer + 0x24, dwords(types[definition['type'] == 'other']))
            # Declared distinct opaque House identities; neither flash leaf
            # reads a House body or resolves/filter owners.
            u.mem_write(pointer + 0x2C, dwords(HEAP + 0x80000 + definition['owner'] * 0x100))
            chain = definition['members']
            u.mem_write(pointer + 0x54, dwords(members[chain[0]] if chain else 0))
            for position, member in enumerate(chain):
                u.mem_write(members[member] + 0x5D8,
                    dwords(members[chain[position + 1]] if position + 1 < len(chain) else 0))
            teams.append(pointer)
        array = fixture.allocate(max(4, len(teams) * 4))
        u.mem_write(array, dwords(*teams) if teams else dwords(0))
        u.mem_write(0x8B40EC, dwords(array))
        u.mem_write(0x8B40F8, dwords(len(teams)))
        for begin, end in ((0x6F1F30, 0x6F1F67), (0x6EF5F0, 0x6EF60E)):
            fixture.ranges.append((begin, end))
            fixture.original.append(bytes(u.mem_read(begin, end - begin)))
        visits, writes, rng_calls = [], [], []
        def observe(vm, pc, _size, _data):
            if pc == 0x6EF5F0:
                visits.append(teams.index(vm.reg_read(UC_X86_REG_ECX)))
            elif pc == 0x6EF5FB:
                writes.append(members.index(vm.reg_read(UC_X86_REG_EAX)))
            elif pc in (0x65C780, 0x65C7E0):
                rng_calls.append(f'{pc:08X}')
        hook = u.hook_add(UC_HOOK_CODE, observe)
        try:
            returned = fixture.call(0x6DD8B0, [0, 0, 0, 0], ecx=ACTION) & 255
        finally:
            u.hook_del(hook)
        fixture.check_code()
        rows.append(dict(name=case['name'], read=read,
            input=dict(type_present=case['type_present'], teams=case['teams'], initial=initial),
            returned=returned, team_visits=visits, member_writes=writes,
            stored=[fixture.integer(pointer + 0xF0) for pointer in members],
            rng_calls=rng_calls))
    return rows


def flash_team_metadata():
    return provenance(
        scope='Three original TAction104/TeamType6F1F30/Team6EF5F0 controls, including the literal ALL01 Enter_Tanya40-frame operand',
        assumptions=[
            'Original TeamType name factories construct two types; only their reference identities are used. No TeamType INI body or full Team/Techno/House constructors execute.',
            'Live Team array, distinct opaque House pointers and head-first member links are explicit priors. The native loops do not read member health, alive, limbo or disguise fields.',
            'Original Action constructor/reader executes parameter-type5, reference+30 and signed token8 value+90; null reference, empty registry and matching types across Houses are bounded.',
            'The legacy team_creation elite_flash controls own signed countdown4CC770. This mode observes member writes only; no TeamAI, TechnoAI, game-frame schedule, rendering or complete campaign progression executes.'],
        substitutions=[
            'Existing ActionFixture supplies heap/CRT INI-tokenizer transport; no action dispatcher, TeamType flash, Team flash or member write is replaced.'],
        entry_points=dict(action_constructor=0x6DD000, action_read=0x6DD5B0,
            action_execute=0x6DD8B0, execute104=0x6DF562,
            team_type_name_factory=0x6F1920, team_type_flash=0x6F1F30,
            team_flash=0x6EF5F0, member_write=0x6EF5FB))


class ReshroudFixture(ActionFixture):
    """Existing reader/VM transport; declared byte priors, no cell algorithm.

    Reuse DiscoveryFixture's observation format so Rust compares through its
    existing cell-fixture/assertion owner. Only original native instructions
    compute counters, masks, allocated traversal and House branch outputs.
    """

    pointer_state = DiscoveryFixture.pointer_state
    cell_state = DiscoveryFixture.cell_state
    topology_input = DiscoveryFixture.topology_input

    def __init__(self, cells, *, width=3):
        super().__init__()
        self.uc = u = self.u
        self.cells = {}
        self.sinks, self.visits, self.rng_calls = [], [], []
        self.draw_sink = self.allocate(4)
        vtable = self.allocate(0x40)
        u.mem_write(vtable, bytes(0x40))
        u.mem_write(vtable + 0x38, dwords(self.draw_sink))
        u.mem_write(DISCOVERY_MAP, dwords(vtable))
        u.mem_write(DISCOVERY_MAP + 0xF4, dwords(width))
        # Reuse the fixed512 sparse-table layout, not an emulated iterator.
        from tools.spatial_oracle.map_queries import EMPTY_TABLE
        u.mem_write(DISCOVERY_TABLE, EMPTY_TABLE)
        u.mem_write(DISCOVERY_MAP + 0x13C, dwords(DISCOVERY_TABLE, 0x40000))
        u.mem_write(0x87F924, dwords(DISCOVERY_TABLE))
        self.player, self.foreign = self.allocate(0x300), self.allocate(0x300)
        for ordinal, pointer in enumerate((self.player, self.foreign)):
            u.mem_write(pointer, bytes(0x300))
            u.mem_write(pointer + 0x30, dwords(ordinal))
            u.mem_write(pointer + 0x240, bytes((1, 1)))
        houses = self.allocate(8)
        u.mem_write(houses, dwords(self.player, self.foreign))
        u.mem_write(0xA8022C, dwords(houses))
        u.mem_write(0xA83D4C, dwords(self.player))
        for row in cells:
            pointer = self.allocate(0x200)
            self.supply_cell(pointer, row)
            cell = tuple(row['cell'])
            self.cells[cell] = pointer
            u.mem_write(DISCOVERY_TABLE + (cell[1] * 512 + cell[0]) * 4,
                        dwords(pointer))
        self.supply_cell(0xABDC50, dict(cell=[0, 0], level=0, counter=1,
                                      gap=0, alt=0, flags=0, cache=[-2, -2]))
        for begin, end in ((0x577C90, 0x577D8C), (0x578290, 0x578343)):
            self.ranges.append((begin, end))
            self.original.append(bytes(u.mem_read(begin, end - begin)))
        for stream in (SCENARIO + 0x218, 0x886B88, 0xABE890):
            self.call(0x65C6D0, [31], ecx=stream)
        self.reshroud_hook = u.hook_add(UC_HOOK_CODE, self.observe_reshroud)

    def supply_cell(self, pointer, row):
        u = self.u
        u.mem_write(pointer, bytes(0x200))
        u.mem_write(pointer + 0x24, packed(*row['cell']))
        u.mem_write(pointer + 0x11B, bytes((row['level'] & 255,)))
        u.mem_write(pointer + 0x120, bytes(value & 255 for value in row['cache']))
        for offset, key in ((0x12C, 'alt'), (0x130, 'counter'),
                            (0x134, 'gap'), (0x140, 'flags')):
            u.mem_write(pointer + offset, dwords(row[key]))

    def houses_state(self):
        return {name: dict(revealed=self.u.mem_read(pointer + 0x240, 1)[0],
                           clear=self.u.mem_read(pointer + 0x241, 1)[0])
                for name, pointer in (('player', self.player), ('foreign', self.foreign))}

    def observe_reshroud(self, u, pc, _size, _data):
        if pc == 0x487690:
            pointer = u.reg_read(UC_X86_REG_ECX)
            self.visits.append(list(struct.unpack('<hh', u.mem_read(pointer + 0x24, 4))))
        if pc in (0x65C780, 0x65C7E0):
            self.rng_calls.append(f'{pc:08X}')
        sinks = {0x4068E0: ('null_house_debug', 0),
                 0x4ADEE0: ('release', 8), 0x4ADCD0: ('reentry', 8),
                 0x657CE0: ('radar_refresh', 0), self.draw_sink: ('redraw', 4)}
        if pc in sinks:
            name, cleanup = sinks[pc]
            sp = u.reg_read(UC_X86_REG_ESP)
            self.sinks.append(dict(name=name, houses=self.houses_state(),
                args=[self.read32(sp + 4 + i * 4) for i in range(cleanup // 4)],
                cells=[self.cell_state(cell) for cell in sorted(self.cells)]))
            return_from_sink(u, cleanup, 0)


def reshroud_generate():
    # Byte inputs intentionally cover signed wrapping/clamping, the negative
    # clear gate and untouched upper flag/cache bytes. None is a golden answer.
    scalar_inputs = [("unrevealed_sentinel", -1, 0), ("pending_from_sentinel", -1, 3),
                     ("pending_from_zero", 0, 3), ("gap_cap", 3, 1),
                     ("still_negative", -3, 2), ("signed_wrap", 2147483647, 2147483647),
                     ("negative_gap_cap", -1, -2)]
    def input_row(cell, counter, gap):
        return dict(cell=list(cell), level=0, counter=counter, gap=gap,
                    alt=0xAABBCC18, flags=0x12345603, cache=[-12, 93])
    scalar = []
    for name, counter, gap in scalar_inputs:
        prior = input_row((1, 3), counter, gap)
        fixture = ReshroudFixture([prior])
        pointer = fixture.cells[(1, 3)]
        before = fixture.rng_hashes()
        fixture.call(0x487690, [], ecx=pointer)
        after_leave = fixture.pointer_state(pointer)
        # Execute the original post-leave conditional mask, including its
        # negative branch. Do not replace it with a Python counter predicate.
        fixture.u.reg_write(UC_X86_REG_ESI, pointer)
        run_checked(fixture.u, 0x577D26, 0x577D4D, required_addresses=[0x577D26])
        fixture.check_code()
        scalar.append(dict(name=name, input=prior, after_leave=after_leave,
            stored=fixture.pointer_state(pointer), rng_before=before,
            rng_after=fixture.rng_hashes(), rng_calls=fixture.rng_calls))
    cells = [input_row((1, 3), -1, 3), input_row((2, 2), -3, 2),
             input_row((3, 1), 3, 1), input_row((2, 3), 0, 3),
             input_row((3, 2), 2147483647, 2147483647),
             input_row((4, 4), -1, 0)]
    controls = []
    for name, incoming in (('current_house', 'player'), ('null_house', None),
                           ('foreign_house', 'foreign'), ('allocated_hole', 'player')):
        supplied = [row for row in cells if name != 'allocated_hole' or row['cell'] != [2, 2]]
        fixture = ReshroudFixture(supplied)
        topology = fixture.topology_input()
        before, houses = fixture.rng_hashes(), fixture.houses_state()
        pointer = 0 if incoming is None else getattr(fixture, incoming)
        fixture.call(0x577C90, [pointer], ecx=DISCOVERY_MAP)
        fixture.check_code()
        controls.append(dict(name=name, incoming=incoming, size_width=3,
            topology=topology, houses_before=houses, houses=fixture.houses_state(),
            visits=fixture.visits, sinks=fixture.sinks,
            cells=[fixture.cell_state(cell) for cell in sorted(fixture.cells)],
            dummy=fixture.pointer_state(0xABDC50), rng_before=before,
            rng_after=fixture.rng_hashes(), rng_calls=fixture.rng_calls))
    return dict(schema_version=1, scalar_controls=scalar, controls=controls)


def reshroud_metadata():
    return provenance(
        scope='Original Map577C90 House admission, Cell487690 leave and allocated578290 traversal with explicit sparse priors; no native world callback/render parity.',
        assumptions=[
            'Seven signed Cell counter/gap priors execute487690 then577D26..577D4D. Four bulk controls execute full577C90 with current/null/foreign incoming Houses and a sparse-table hole. Native code, not a host formula, supplies every expected counter, mask, visit and House byte.',
            'Map Size width3, canonical House array/index identities, current Player, House240/241 set priors, sparse512-stride Cell pointers and all Cell fields read by the selected bodies are supplied. Cell/House/Map constructors and actual ALL01 map allocation are not executed.',
            'The null input retains Player241; the foreign input clears only its canonical House241 and returns. The local allocated loop exits at its first null pointer and does not visit the supplied later4,4 cell. Subsequent native cursor calls execute through original578290; no host-side scan supplies traversal answers.',
            'Rust compares represented counter/gap, ground bits18, map bits3, pending20 and signed frame bytes. Unrepresented upper raw flags are retained in native outputs but are not a claim of Rust full-Cell parity.',
            'Seed31 initializes the existing three RNG stream transport. The selected bodies have no expected RNG draws; hashes and observed draw entry points are recorded, not used as substitute answers. No timers or detach calls occur in these selected bodies.'],
        substitutions=[
            'Reuse ActionFixture heap/CRT/VM and DiscoveryFixture cell observation format; no577C90,487690,578290 or post-leave instruction is replaced.',
            '4ADEE0 release and4ADCD0 reentry are recorded RET8 sinks with empty world callback behavior. Live sight/gap/RevealToAll, nested Tag/discovery reentry, changes to PlayerPtr during callbacks and whole-scenario ordering are excluded.',
            '4068E0 null-House debug,657CE0 radar refresh and map virtual redraw38(2) are recorded sinks. No audio/draw/device/dirty-region state or rendered pixels is simulated.'],
        entry_points=dict(reshroud=0x577C90, leave=0x487690,
            clear_gate=0x577D26, clear_gate_end=0x577D4D,
            iterator_next=0x578290, release=0x4ADEE0, reentry=0x4ADCD0))


class ScenarioTimerFixture(ActionFixture):
    """Original Scenario timer actions/predicate; UI redraw is excluded."""

    def __init__(self):
        super().__init__()
        u = self.u
        self.events, self.rng_calls = [], []
        self.timer = SCENARIO + 0x11E8
        # Pin irrelevant native temporary/unused timer storage without giving
        # it a gameplay interpretation. Frame, duration and label are native.
        u.mem_write(SP - 0x400, bytes(0x800))
        u.mem_write(SCENARIO, b'\xA5' * 0x5000)
        u.mem_write(0xA8ED84, dwords(123))
        u.reg_write(UC_X86_REG_EBP, SCENARIO)
        u.reg_write(UC_X86_REG_EBX, 0)
        run_checked(u, 0x683302, 0x683319)
        self.constructor_prefix = dict(
            frame=123, start=self.integer(self.timer),
            unused_word=self.integer(self.timer + 4),
            duration=self.integer(self.timer + 8),
            label_first_byte=u.mem_read(SCENARIO + 0x11F8, 1)[0])
        # Execute the Reset683610 timer/label block after declaring the
        # incoming registers and the unrelated+4 local to be zero.
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ESI, 0xFFFFFFFF)
        run_checked(u, 0x6836CC, 0x68375F)
        for stream in (SCENARIO + 0x218, 0x886B88, 0xABE890):
            self.call(0x65C6D0, [31], ecx=stream)
        for i, base in enumerate((0xB0F1A0, 0xB0F658, 0xB0F670)):
            u.mem_write(base + 4, dwords(HEAP + 0x8000 + i * 0x400, 128, 0, 0, 128))
        self.event, self.persist, self.other_timer = (
            self.allocate(size) for size in (0x58, 1, 12))
        self.call(0x71E6A0, [], ecx=self.event)
        u.mem_write(BUFFER, b'1,14,0,0\0')
        self.call(0x7C9CC2, [BUFFER, 0x817F70], cdecl=True)
        self.call(0x71F4E0, [], ecx=self.event)
        self.event_read = dict(raw='1,14,0,0', kind=self.integer(self.event + 0x2C),
                               value=self.integer(self.event + 0x34))
        u.mem_write(self.other_timer, dwords(-1, 0, 999))
        self.csf = install_csf(SimpleNamespace(u=u, alloc=self.allocate), ['DESC:SOV1'])
        for begin, end in ((0x426630, 0x42664C), (0x46B640, 0x46B654),
                           (0x71E6A0, 0x71E7FA), (0x71E940, 0x71F216),
                           (0x71F4E0, 0x71F5B0), (0x6E4790, 0x6E47E7),
                           (0x683302, 0x683319), (0x6836CC, 0x68375F)):
            self.ranges.append((begin, end))
            self.original.append(bytes(u.mem_read(begin, end - begin)))
        self.timer_hook = u.hook_add(UC_HOOK_CODE, self.observe_timer)

    def key(self):
        return bytes(self.u.mem_read(SCENARIO + 0x11F8, 32)).split(b'\0', 1)[0].decode('ascii')

    def reset(self):
        # Reuse one loaded image: native Reset owns the state transition;
        # allocating a separate VM for each scalar history is unnecessary.
        u = self.u
        u.mem_write(SP - 0x400, bytes(0x800))
        u.mem_write(0xA8ED84, dwords(123))
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_EBP, SCENARIO)
        u.reg_write(UC_X86_REG_EBX, 0)
        u.reg_write(UC_X86_REG_ESI, 0xFFFFFFFF)
        run_checked(u, 0x6836CC, 0x68375F)
        self.events, self.rng_calls = [], []

    def title(self):
        pointer = self.read32(SCENARIO + 0x11F4)
        if pointer == 0x887734:
            # This is the original default sentinel pointer, not an asserted
            # file-backed empty-string literal. The VM maps its BSS as zero.
            return dict(kind='default_sentinel', pointer=pointer)
        words = []
        while (word := bytes(self.u.mem_read(pointer + len(words) * 2, 2))) != b'\0\0':
            words.append(word)
            assert len(words) < 2048
        return dict(kind='csf', pointer=pointer, text=b''.join(words).decode('utf-16-le'))

    def observe_timer(self, u, pc, _size, _data):
        if pc in (0x65C780, 0x65C7E0):
            self.rng_calls.append(f'{pc:08X}')
        if pc == 0x734E60:
            self.events.append(dict(call='csf_lookup', key=self.string(u.reg_read(UC_X86_REG_ECX))))
        elif pc == 0x6E47C0 and u.reg_read(UC_X86_REG_ECX) == SCENARIO + 0x11F8:
            source = self.read32(u.reg_read(UC_X86_REG_ESP) + 8)
            self.events.append(dict(call='assign_key', title=self.title(),
                key=bytes(u.mem_read(source, 32)).split(b'\0', 1)[0].decode('ascii')))
        elif pc == 0x6D0F00:
            self.events.append(dict(call='redraw_sink', start=self.integer(self.timer),
                duration=self.integer(self.timer + 8), title=self.title(), key=self.key()))
            return_from_sink(u, 0, 0)

    def snapshot(self):
        remaining = self.call(0x426630, [], ecx=self.timer)
        self.u.mem_write(self.persist, b'\0')
        occurred = self.call(0x71E940, [8, 0, 0, self.other_timer, self.persist, 0],
                             ecx=self.event) & 255
        return dict(frame=self.integer(0xA8ED84), start=self.integer(self.timer),
            unused_word=self.integer(self.timer + 4), duration=self.integer(self.timer + 8),
            remaining=struct.unpack('<i', dwords(remaining))[0],
            event14=occurred, event_persistence=self.u.mem_read(self.persist, 1)[0],
            title=self.title(), key=self.key())

    def step(self, name, frame, action=None, value=0):
        self.u.mem_write(0xA8ED84, dwords(frame))
        self.events = []
        row = dict(name=name, frame=frame)
        if action is not None:
            raw = (f'103,4,{value},0,0,0,0,A' if action == 103 and isinstance(value, str)
                   else f'{action},0,{value},0,0,0,0,A')
            row['read'] = self.read_action(raw)
            row['label_read'] = bytes(self.u.mem_read(ACTION + 0x6D, 32)).split(b'\0', 1)[0].decode('ascii')
            row['action_result'] = self.call(0x6DD8B0, [0, 0, 0, 0], ecx=ACTION) & 255
        row.update(state=self.snapshot(), calls=list(self.events))
        return row


def scenario_timer_generate():
    histories = [
        ('ordinary_expiry', [('start', 123, 27, 120), ('one_frame_left', 1922),
                             ('expired', 1923), ('later', 2000)]),
        ('pause_resume', [('start', 123, 27, 120), ('pause', 423, 24),
                          ('pause_again', 500, 24), ('still_paused', 5000),
                          ('resume', 5000, 23), ('resume_again', 5001, 23),
                          ('one_frame_left', 6499), ('expired', 6500)]),
        ('restart_and_labels', [('title', 123, 103, 'DESC:SOV1'), ('start', 123, 27, 120),
                                ('title_again', 200, 103, 'DESC:SOV1'),
                                ('restart', 423, 27, 2), ('expired', 453),
                                ('clear_title', 453, 103, 0)]),
        ('paused_extend', [('start', 123, 27, 10), ('pause', 153, 24),
                            ('extend', 999, 25, 2), ('expired', 1149)]),
        ('paused_shorten', [('start', 123, 27, 10), ('pause', 153, 24),
                             ('shorten', 999, 26, 2), ('expired', 1089)]),
        ('shorten_to_zero', [('start', 123, 27, 2), ('equal', 123, 26, 2),
                             ('pause_zero', 123, 24), ('resume_zero', 124, 23),
                             ('larger', 124, 26, 3)]),
        ('zero_start', [('start', 123, 27, 0), ('pause', 124, 24)]),
        ('negative_start', [('start', 123, 27, -1)]),
        ('negative_adjustments', [('start', 123, 27, 2), ('shorten_negative', 123, 26, -1),
                                  ('extend_negative', 123, 25, -4)]),
        ('positive_product_limit', [('start', 123, 27, 143165576), ('extend_wrap', 123, 25, 1)]),
        ('product_wrap_negative', [('start', 123, 27, 143165577)]),
        ('i32_max_parameter', [('start', 123, 27, 2147483647)]),
        ('i32_min_parameter', [('start', 123, 27, -2147483648)]),
        ('signed_frame_wrap', [('start', 0x7FFFFFF0, 27, 4), ('wrapped', 0x80000000),
                              ('expired', 0x8000002C)]),
        ('full_frame_wrap', [('start', 0xFFFFFFFA, 27, 2), ('wrapped', 4), ('expired', 24)]),
        ('sentinel_frame', [('start', 0xFFFFFFFF, 27, 1), ('resume', 0, 23)]),
        ('signed_elapsed_boundary', [('start', 0, 27, 1), ('half_cycle', 0x80000000)]),
    ]
    rows = []
    f = ScenarioTimerFixture()
    for name, steps in histories:
        f.reset()
        before = f.rng_hashes()
        initial = f.snapshot()
        rows.append(dict(name=name, initial=initial,
            steps=[f.step(*step) for step in steps],
            rng_before=before, rng_after=f.rng_hashes(), rng_calls=f.rng_calls))
        f.check_code()
    label_read_controls = []
    for raw in ('103,4,DESC:SOV1,0,0,0,0,A',
                '103,4,DESC:' + 'A' * 40 + ',0,0,0,0,A',
                '103,0,0,0,0,0,0,A'):
        label_read_controls.append(dict(read=f.read_action(raw),
            key=bytes(f.u.mem_read(ACTION + 0x6D, 32)).split(b'\0', 1)[0].decode('ascii')))
    f.check_code()
    return dict(schema_version=1, constructor_prefix=f.constructor_prefix,
                event_read=f.event_read, csf=f.csf, histories=rows,
                label_read_controls=label_read_controls)


def scenario_timer_metadata():
    return provenance(
        scope='Original TAction23/24/25/26/27/103 reader/dispatch, Scenario timer arithmetic and Event14 predicate; no UI rendering or mission-level timer scheduler claim.',
        assumptions=[
            'Every action executes its original constructor6DD000, counted read6DD5B0/strtok/atoi, and full dispatch6DD8B0. Event14 executes original constructor71E6A0 and reader71F4E0 once per fixture, then original HasOccurred71E940 with raised8, null House/Object/source, a separate unrelated paused timer and zero persistence byte.',
            'Constructor block683302..683319 executes with EBP=Scenario, EBX=0 and frame123 on A5-poisoned storage. It initializes duration0/start123/label-key first byte0; the unused+4 word remains poison and the title pointer is not initialized by that block. This is selected native body execution, not a full Scenario constructor claim.',
            'Fresh state executes original Reset6836CC..68375F with EBP=Scenario, EBX=0, ESI=-1 and zeroed incoming stack. It pauses zero duration, sets title pointer887734 and copies an empty key. Other ScenarioReset state/lifecycle is excluded. The unused timer+4 word and unused copied label suffixes are not semantic state.',
            'Frame priors, action strings and observation frames are declared inputs. All remaining values, i32 multiplication/addition/subtraction wrapping, signed comparisons, pause/restart and Event14 results come from original instructions, not Python formulas.',
            'Action103 uses the existing physical stock-CSF transport for DESC:SOV1; source SHA/text are pinned. Three reader-only controls include a long key truncated to31 bytes; the deliberately absent long key is not dispatched or assigned an invented translation. Numeric parameter mode supplies its original constructor-empty key for the clear control. Empty comma fields would be skipped by native strtok and are not an empty-key input.',
            'Default title pointer887734 is recorded as a sentinel identity, not a file-backed empty-string assertion: its mapped BSS zero contents are not retail bytes. Native key/string copy and CSF lookup execute. Setter-call observations establish that title assignment precedes key assignment, then redraw.',
            'Three existing RNG streams are initialized with original seed31 transport before histories; unchanged state hashes and observed65C780/65C7E0 entries bound absence of draws. No detach calls occur in the selected bodies.'],
        substitutions=[
            'Reuse ActionFixture VM/PE/heap/CRT and existing install_csf physical translation-record transport. Scenario, UI and CSF startup constructors are not executed; only the explicitly described Scenario constructor/reset blocks and Event/Action constructors are executed.',
            'Tactical redraw6D0F00 is a recorded RET0 sink. Actual sidebar layout, countdown text rendering, window/device updates and scheduling of trigger polls are excluded. No reader, timer arithmetic, dispatch branch or predicate instruction is replaced.'],
        entry_points=dict(action_constructor=0x6DD000, action_read=0x6DD5B0,
            dispatch=0x6DD8B0, resume=0x6DE418, pause=0x6DE457,
            extend=0x6DE4A0, shorten=0x6DE501, start=0x6DE5C9,
            set_label=0x6DE66A, timer_start=0x46B640, timer_remaining=0x426630,
            event_constructor=0x71E6A0, event_reader=0x71F4E0,
            event_predicate=0x71E940, event14=0x71EBE9,
            scenario_ctor_prefix=0x683302, scenario_reset_timer=0x6836CC,
            redraw_sink=0x6D0F00))


class ChronoHouseFixture(ActionFixture):
    """Original128 caller with explicitly admitted virtual leaf inputs."""

    def __init__(self):
        super().__init__()
        from tools.spatial_oracle.reveal_area import CELL_OFFSETS_INIT
        u = self.u
        self.constants = initialize_waypoint_map(self)
        self.call(CELL_OFFSETS_INIT, [], cdecl=True)
        self.offsets = list(struct.iter_unpack('<hh', u.mem_read(0xABD490, 351 * 4)))
        self.house, self.other_house = self.allocate(4), self.allocate(4)
        self.vtable = self.allocate(0x600)
        self.what_sink, self.enter_sink, self.false_sink = (self.allocate(4) for _ in range(3))
        u.mem_write(self.vtable, bytes(0x600))
        for slot, pointer in ((0x2C, self.what_sink), (0x1AC, self.enter_sink),
                              (0x508, 0x4DF7F0), (0x54, self.false_sink),
                              (0x160, self.false_sink), (0x1D4, self.false_sink),
                              (0x1D8, self.false_sink)):
            u.mem_write(self.vtable + slot, dwords(pointer))
        self.pool = [self.allocate(0x300) for _ in range(12)]
        self.foot_array = self.allocate(12 * 4)
        u.mem_write(0x8B3DC4, dwords(self.foot_array))
        # Deliberately poison, then run the real static initializer instead
        # of assigning a guessed invalid Cell prior to this shared global.
        u.mem_write(0xA8EF98, packed(-1, -1))
        self.call(0x4F50A0, [], cdecl=True)
        self.empty_cell = list(struct.unpack('<hh', u.mem_read(0xA8EF98, 4)))
        self.cells, self.cell_indices = {}, {}
        for index, (dx, dy) in enumerate(self.offsets):
            cell = self.allocate(0x200)
            self.call(0x47BBF0, [], ecx=cell)
            xy = 75 + dx, 50 + dy
            u.mem_write(cell + 0x24, packed(*xy))
            u.mem_write(cell + 0x11B, b'\0\0')
            u.mem_write(self.table + (xy[1] * 512 + xy[0]) * 4, dwords(cell))
            self.cells[tuple(xy)], self.cell_indices[cell] = cell, index
        for stream in (SCENARIO + 0x218, 0x886B88, 0xABE890):
            self.call(0x65C6D0, [31], ecx=stream)
        for begin, end in ((0x6E1A40, 0x6E1A67), (0x50D6D0, 0x50D9B1),
                           (0x4DF7F0, 0x4DFA49), (0x68BCC0, 0x68BCD4),
                           (0x5657A0, 0x5657D8), (0x486840, 0x486887),
                           (0x4F50A0, 0x4F50AF)):
            self.ranges.append((begin, end))
            self.original.append(bytes(u.mem_read(begin, end - begin)))
        self.chrono_hook = u.hook_add(UC_HOOK_CODE, self.observe_chrono)

    def observe_chrono(self, u, pc, _size, _data):
        sp, actor = u.reg_read(UC_X86_REG_ESP), u.reg_read(UC_X86_REG_ECX)
        if pc in (0x65C780, 0x65C7E0):
            self.rng_calls.append(f'{pc:08X}')
        if pc == 0x50D72C:
            self.visits.append([self.actor_index[u.reg_read(UC_X86_REG_ESI)],
                                u.reg_read(UC_X86_REG_EAX)])
        elif pc in (0x50D850, 0x50D984):
            # Observe the actual callee return in the original caller,
            # including the refusal whose Foot body was not substituted.
            self.warp_calls[-1]['accepted'] = bool(u.reg_read(UC_X86_REG_EAX) & 255)
        elif pc == self.what_sink:
            return_from_sink(u, 0, self.actors[self.actor_index[actor]]['class'])
        elif pc == self.false_sink:
            return_from_sink(u, 0, 0)
        elif pc == self.enter_sink:
            index = self.actor_index[actor]
            cell = self.read32(sp + 4)
            offset = self.cell_indices[cell]
            self.last_cell[index] = offset
            args = [self.integer(sp + n) for n in (8, 12, 16, 20)]
            assert args == [-1, -1, 0, 1]
            result = self.case.get('blocked_result', 1) if offset in self.blocked else 0
            self.enter_calls.append([index, offset, result])
            return_from_sink(u, 20, result)
        elif pc == 0x481180:
            # A declared identity subcell boundary: the original caller and
            # native Cell486840 already supply its requested coordinate.
            output, incoming = self.read32(sp + 4), self.read32(sp + 8)
            assert [self.read32(sp + n) for n in (12, 16, 20)] == [0, 0, 0]
            raw = bytes(u.mem_read(incoming, 12))
            u.mem_write(output, raw)
            self.subcell_calls.append(list(struct.unpack('<iii', raw)))
            return_from_sink(u, 20, output)
        elif pc == 0x4DF7F0:
            index = self.actor_index[actor]
            row = self.actors[index]
            coord = list(struct.unpack('<iii', u.mem_read(sp + 4, 12)))
            actual = bool(row.get('native_warp_refusal', False))
            accepted = False if actual else row.get('accept', True)
            self.warp_calls.append(dict(actor=index, offset=self.last_cell[index],
                coord=coord,
                boundary='original_27c_refusal' if actual else 'supplied_warp_result'))
            if not actual:
                return_from_sink(u, 12, int(accepted))

    def execute_case(self, case):
        u = self.u
        self.case, self.actors = case, case['actors']
        assert len(self.actors) <= len(self.pool)
        self.blocked = set(case.get('blocked_offsets', []))
        self.actor_index = {pointer: index for index, pointer in enumerate(self.pool[:len(self.actors)])}
        self.visits, self.enter_calls, self.warp_calls = [], [], []
        self.subcell_calls, self.last_cell, self.rng_calls = [], {}, []
        u.mem_write(self.foot_array, dwords(*self.pool[:len(self.actors)]))
        u.mem_write(0x8B3DD0, dwords(len(self.actors)))
        for pointer, row in zip(self.pool, self.actors):
            u.mem_write(pointer, bytes(0x300))
            u.mem_write(pointer, dwords(self.vtable))
            u.mem_write(pointer + 0x21C, dwords(self.other_house if row.get('foreign') else self.house))
            u.mem_write(pointer + 0x11C, dwords(int(row.get('transported', False))))
            u.mem_write(pointer + 0x27C, bytes((int(row.get('native_warp_refusal', False)),)))
        target = case.get('target', [75, 50])
        u.mem_write(SCENARIO + 0x632, packed(*target))
        read = self.read_action('128,0,7,0,0,0,0,A')
        before = self.rng_hashes()
        result = self.call(0x6DD8B0, [self.house, 0, 0, 0], ecx=ACTION) & 255
        self.check_code()
        return dict(input=case, read=read, action_result=result,
            visits=self.visits, can_enter=self.enter_calls, warp=self.warp_calls,
            subcell_identity_calls=self.subcell_calls, rng_before=before,
            rng_after=self.rng_hashes(), rng_calls=self.rng_calls)


def chrono_house_generate():
    f = ChronoHouseFixture()
    cases = [
        dict(name='reverse_mixed_classes', actors=[{'class': 1}, {'class': 15}, {'class': 1}]),
        dict(name='owner_class_and_transporter', actors=[{'class': 1}, {'class': 2},
            {'class': 6}, {'class': 1, 'foreign': True}, {'class': 15, 'transported': True},
            {'class': 1, 'transported': True}, {'class': 15}]),
        dict(name='nonzero_can_enter_skips_shared_offset', actors=[{'class': 15}, {'class': 1}],
             blocked_offsets=[0, 2], blocked_result=2),
        dict(name='supplied_warp_refusal_reuses_offset', actors=[{'class': 1}, {'class': 15, 'accept': False}]),
        dict(name='original_foot_refusal_reuses_offset', actors=[{'class': 1}, {'class': 1, 'native_warp_refusal': True}]),
        dict(name='last_unit_offset_stops_before_next_actor', actors=[{'class': 1}, {'class': 1}],
             blocked_offsets=list(range(350))),
        dict(name='last_infantry_offset_stops_before_next_actor', actors=[{'class': 1}, {'class': 15}],
             blocked_offsets=list(range(350))),
        dict(name='all351_blocked', actors=[{'class': 1}, {'class': 1}], blocked_offsets=list(range(351))),
        dict(name='empty_foot_array', actors=[]),
        dict(name='invalid_waypoint_sentinel', target=f.empty_cell, actors=[{'class': 1}]),
    ]
    return dict(schema_version=1, native_offset_pairs=f.offsets,
                empty_cell=f.empty_cell, map_constants=f.constants,
                rows=[f.execute_case(case) for case in cases])


def chrono_house_metadata():
    return provenance(
        scope='Original Action1286E1A40/House50D6D0 reverse Foot traversal, owner/class/transporter gates and shared351-offset cursor with bounded virtual leaf inputs; not complete teleport or placement parity.',
        assumptions=[
            'Reuse ActionFixture and initialize_waypoint_map original startup/slope transport. Original561910 initializes the CellSpread table; the first351 signed pairs are retained as native outputs. Original47BBF0 constructs the351 flat level0/slope0 Cells on an explicitly supplied sparse512-stride map.',
            'Each row supplies a Foot array in storage order, incoming House identity, RTTI virtual answers, owner pointers, transporter-null/non-null priors and per-offset CanEnter virtual answers. Original dispatcher6DD8B0 invokes6E1A40, original68BCC0 reads waypointA, original50D6D0 computes candidate cells, walks actors backwards and owns the single shared offset cursor.',
            'Original5657A0 and Cell486840 execute for map lookup and ground coordinates. Native selected runtime startup/ground code supplies their flat Z values; no host formula supplies expected candidate coordinates. Infantry481180 identity is an explicit boundary, not a native subcell-position claim.',
            'visits rows are [actor-storage-index,current-offset-index]; can_enter rows are [actor-index,offset-index,supplied-return]. Warp entries retain native caller coordinates and identify supplied return versus original rejection. CanEnter arguments after Cell must be[-1,-1,0,1]. No source array mutation/reentrant callback is supplied.',
            'The original_foot_refusal row really executes Foot4DF7F0 with+27C=1 and the two earlier status virtual answers false; its original return0 leaves the next actor on the same offset. Other Warp returns are explicit supplied leaf results; no COM locomotor, detach, occupation, interpolation, cooldown or WarpIn implementation is certified.',
            'The last-offset and all-blocked controls execute up to the original ABD A08/15E guards, covering exactly candidate indices0..350, then exit before the earlier actor. Original static initializer4F50A0 executes on deliberately poisoned A8EF98 and produces the empty Cell(0,0); the invalid-waypoint control uses that observed sentinel. Null Foot pointers, arbitrary out-of-bounds waypoints and loaded native registry construction are excluded.',
            'Existing three RNG streams start with native seed31; hashes and actual draw entries are recorded. Selected caller bodies contain no timer/detach producer. Hidden effects/RNG behind virtual boundary callbacks are excluded.'],
        substitutions=[
            'Inherited heap/CRT/VM transport; no new mapper or VM. RTTI returns each declared actor class, CanEnterCell vslot1AC returns the declared offset admission, and status vslots54/160/1D4/1D8 return false.',
            'Foot4DF7F0 is an observed RET12 sink returning the declared result except the original+27C-refusal control, which executes the original body. Positive teleport effects, live ownership/list mutations and occupied-cell changes do not occur at this boundary.',
            'Infantry481180 records its request and copies the already native-derived incoming coordinate to its caller output, RET20. Native subcell availability, priority selection and RNG are excluded rather than approximated as verified behavior.'],
        entry_points=dict(dispatch=0x6DD8B0, action128=0x6E1A40, waypoint=0x68BCC0,
            chrono_house=0x50D6D0, cell_spread_init=0x561910,
            empty_cell_init=0x4F50A0,
            cell_constructor=0x47BBF0, cell_lookup=0x5657A0,
            cell_coords=0x486840, infantry_subcell_boundary=0x481180,
            foot_warp=0x4DF7F0))


class AllChangeHouseFixture(ActionFixture):
    """Original36 selection/iteration; supplied owner callbacks are explicit."""

    def __init__(self):
        super().__init__()
        u = self.u
        # Same three-House Country priors as PowerEventControls, and the
        # same Scenario Player@ slot storage as team_creation's native calls.
        self.houses = [self.allocate(0x100) for _ in range(3)]
        self.house_types = [self.allocate(0x100) for _ in range(3)]
        self.house_array, self.live = self.allocate(12), self.allocate(0x48)
        for house, typ, country in zip(self.houses, self.house_types, (0, 7, 0)):
            u.mem_write(house, bytes(0x100))
            u.mem_write(typ, bytes(0x100))
            u.mem_write(house + 0x34, dwords(typ))
            u.mem_write(typ + 0xB8, dwords(country))
        u.mem_write(0xA8022C, dwords(self.house_array))
        u.mem_write(0xA80238, dwords(3))
        self.vtable = self.allocate(0x400)
        self.what_sink, self.change_sink = self.allocate(4), self.allocate(4)
        u.mem_write(self.vtable, bytes(0x400))
        u.mem_write(self.vtable + 0x2C, dwords(self.what_sink))
        u.mem_write(self.vtable + 0x3D4, dwords(self.change_sink))
        self.pool = [self.allocate(0x600) for _ in range(12)]
        self.types = [self.allocate(0x1600) for _ in range(12)]
        self.techno_array = self.allocate(12 * 4)
        for stream in (SCENARIO + 0x218, 0x886B88, 0xABE890):
            self.call(0x65C6D0, [31], ecx=stream)
        for begin, end in ((0x6E0B60, 0x6E0C92), (0x502D30, 0x502D60),
                           (0x510ED0, 0x510FA6), (0x68C030, 0x68C04D),
                           (0x726910, 0x726914)):
            self.ranges.append((begin, end))
            self.original.append(bytes(u.mem_read(begin, end - begin)))
        self.owner_hook = u.hook_add(UC_HOOK_CODE, self.observe_owner)

    def house_index(self, pointer):
        return self.houses.index(pointer) if pointer else None

    def publish(self):
        self.u.mem_write(0xA8EC7C, dwords(self.techno_array))
        self.u.mem_write(0xA8EC88, dwords(len(self.active)))
        self.u.mem_write(self.techno_array, dwords(*(self.pool[i] for i in self.active)))

    def observe_owner(self, u, pc, _size, _data):
        sp, pointer = u.reg_read(UC_X86_REG_ESP), u.reg_read(UC_X86_REG_ECX)
        if pc in (0x65C780, 0x65C7E0):
            self.rng_calls.append(f'{pc:08X}')
        if pc == 0x6E0BAE:
            self.phase = 1
        elif pc == 0x6E0C1D:
            self.phase = 2
        elif pc in (0x6E0BC2, 0x6E0C2F):
            actor = self.actor_index[u.reg_read(UC_X86_REG_ESI)]
            self.visits.append([self.phase, u.reg_read(UC_X86_REG_EDI), actor])
        elif pc in (0x510F60, 0x510ED0, 0x502D30, 0x726910):
            self.selectors.append(dict(entry=f'{pc:08X}', argument=self.integer_reg(UC_X86_REG_ECX)
                if pc != 0x726910 else 'live_trigger'))
        elif pc == self.what_sink:
            return_from_sink(u, 0, self.actors[self.actor_index[pointer]]['class'])
        elif pc == self.change_sink:
            index = self.actor_index[pointer]
            target, announce = self.read32(sp + 4), self.read32(sp + 8)
            assert announce == 0
            before = self.house_index(self.read32(pointer + 0x21C))
            row = self.actors[index]
            if row.get('change_writes_owner', True):
                u.mem_write(pointer + 0x21C, dwords(target))
            effect = row.get('callback_effect')
            if effect == 'remove_self' and index in self.active:
                self.active.remove(index)
            elif effect == 'append' and row['append_actor'] not in self.active:
                self.active.append(row['append_actor'])
            self.publish()
            result = row.get('callback_result', 1)
            self.calls.append(dict(call='change_owner', phase=self.phase, actor=index,
                target=self.house_index(target), announce=announce, before=before,
                after=self.house_index(self.read32(pointer + 0x21C)), supplied_result=result,
                active=list(self.active)))
            return_from_sink(u, 8, result)
        elif pc == 0x4549B0:
            self.calls.append(dict(call='powered_refresh', phase=self.phase,
                actor=self.actor_index[pointer], active=list(self.active)))
            return_from_sink(u, 0, 0)

    def execute_case(self, case):
        u = self.u
        self.actors = case['actors']
        self.actor_index = {p: i for i, p in enumerate(self.pool[:len(self.actors)])}
        self.active = list(case.get('active', range(len(self.actors))))
        self.phase = 0
        self.calls, self.visits, self.selectors, self.rng_calls = [], [], [], []
        order = case.get('house_order', [1, 2, 0])
        u.mem_write(self.house_array, dwords(*(self.houses[i] for i in order)))
        u.mem_write(SCENARIO + 0x1180, dwords(*case.get('slots', [1, 0, -1, -1, -1, -1, -1, -1])))
        u.mem_write(self.live, bytes(0x48))
        trigger_owner = case.get('trigger_owner', 2)
        u.mem_write(self.live + 0x2C, dwords(self.houses[trigger_owner] if trigger_owner is not None else 0))
        for i, row in enumerate(self.actors):
            pointer, typ = self.pool[i], self.types[i]
            u.mem_write(pointer, bytes(0x600))
            u.mem_write(typ, bytes(0x1600))
            u.mem_write(pointer, dwords(self.vtable))
            u.mem_write(pointer + 0x21C, dwords(self.houses[row.get('owner', 0)]))
            u.mem_write(pointer + 0x520, dwords(0 if row.get('null_type') else typ))
            u.mem_write(pointer + 0x90, bytes((int(row.get('alive', True)),)))
            u.mem_write(pointer + 0x81, bytes((int(row.get('limbo', False)),)))
            u.mem_write(typ + 0x1573, bytes((int(row.get('powered', False)), int(row.get('powered_special', False)))))
        self.publish()
        read = self.read_action(f"36,0,{case.get('selector', 7)},0,0,0,0,A")
        before = self.rng_hashes()
        result = self.call(0x6DD8B0, [self.houses[case.get('source_owner', 0)], 0,
            0 if case.get('null_trigger') else self.live, 0], ecx=ACTION) & 255
        self.check_code()
        return dict(input=case, read=read, result=result, visits=self.visits,
            selectors=self.selectors, calls=self.calls, active=list(self.active),
            owners=[self.house_index(self.read32(p + 0x21C)) for p in self.pool[:len(self.actors)]],
            rng_before=before, rng_after=self.rng_hashes(), rng_calls=self.rng_calls)


def all_change_house_generate():
    f = AllChangeHouseFixture()
    ordinary = [{'class': 1}]
    cases = [
        dict(name='ordinary_before_powered_two_passes', actors=[
            {'class': 6, 'powered': True}, {'class': 1}, {'class': 6},
            {'class': 6, 'powered_special': True}, {'class': 1, 'owner': 1},
            {'class': 15, 'alive': False, 'limbo': True}, {'class': 6, 'null_type': True}]),
        dict(name='same_owner_still_calls', selector=0, house_order=[0, 1, 2], actors=[
            {'class': 1}, {'class': 6, 'powered': True}]),
        dict(name='callback_failure_still_refreshes_powered', actors=[
            {'class': 6, 'powered': True, 'callback_result': 0, 'change_writes_owner': False}]),
        dict(name='compacted_first_pass_is_not_a_snapshot', actors=[
            {'class': 1, 'callback_effect': 'remove_self'}, {'class': 1},
            {'class': 1}, {'class': 6, 'powered': True}]),
        dict(name='appended_first_pass_is_visited', active=[0], actors=[
            {'class': 1, 'callback_effect': 'append', 'append_actor': 1}, {'class': 1}]),
        dict(name='appended_powered_second_pass_is_visited', active=[0], actors=[
            {'class': 6, 'powered': True, 'callback_effect': 'append', 'append_actor': 1},
            {'class': 6, 'powered_special': True}]),
        dict(name='country_first_registered_duplicate', selector=0, actors=ordinary),
        dict(name='player_at_0x117b_slot', selector=0x117B, actors=ordinary),
        dict(name='player_at_0x117d_unmapped', selector=0x117D, actors=ordinary),
        dict(name='4507_is_not_0x117b', selector=4507, actors=ordinary),
        dict(name='live_trigger_house_0x2325', selector=0x2325, actors=ordinary),
        dict(name='live_trigger_house_null', selector=0x2325, trigger_owner=None, actors=ordinary),
        dict(name='9001_is_not_0x2325', selector=9001, actors=ordinary),
        dict(name='9005_is_not_0x2325', selector=9005, actors=ordinary),
        dict(name='null_trigger_rejects_even_country', null_trigger=True, actors=ordinary),
        dict(name='minus_one_target', selector=-1, actors=ordinary),
        dict(name='unknown_country', selector=999, actors=ordinary),
        dict(name='no_matching_source', source_owner=2, actors=ordinary),
        dict(name='empty_techno_array', actors=[]),
    ]
    return dict(schema_version=1, house_countries=[0, 7, 0], rows=[f.execute_case(case) for case in cases])


def all_change_house_metadata():
    return provenance(
        scope='Original Action36 AllChangeHouse target selector and two mutable forward Techno passes; callback effects are declared boundaries, not full ChangeOwner/power parity.',
        assumptions=[
            'ActionFixture executes original TAction constructor/read/full dispatch6DD8B0 into6E0B60. Minimal House Country storage follows existing PowerEventControls(0,7,0); Player@ Scenario1180 slot storage follows team_creation. Original510F60/510ED0/68C030/502D30/726910 execute target resolution, with supplied canonical House order, slot map and live Trigger+2C.',
            'Incoming Execute House is source owner0 unless stated; target defaults to Country7, whose first registered matching House is1. The live Trigger pointer is present unless stated. Negative/unknown Country, unmapped Player@, duplicate-Country order, same-owner target,0x2325(8997) live owner and null Trigger controls are bounded by their supplied inputs.9001/9005 are separate unknown-Country negative controls, not aliases for0x2325. Original Player@ range is0x117B..0x1182(4475..4482);4507 is a separate unknown-Country negative control.',
            'Techno storage supplies class RTTI answers, owner pointers, BuildingType nullable pointer and Powered/PoweredSpecial bytes at1573/1574. Actor Alive/limbo bytes are independent priors; the original caller reads neither as admission gates. No complete Techno/Building/House constructors or scenario registry loading run.',
            'visits entries are [pass-number,live-array-index,stable-actor-index]. The original body reloads the mutable Techno base/count and runs ordinary objects before Powered-or-PoweredSpecial Buildings. Callback removal/append controls explicitly mutate the supplied registry; resulting iteration order comes only from original instructions.',
            'ChangeOwner callback writes owner by default and can explicitly return0 without writing, remove its current registry slot or append a supplied actor. The second-pass Building4549B0 call is recorded even when ChangeOwner returns0; same-owner calls are retained. These are caller dispatch/ordering witnesses, not native ChangeOwner results.',
            'Three native RNG streams start with seed31 and retain hash comparisons/draw-entry observations. Selected caller performs no numerical timer or detach operation. RNG, timers, detach and world side effects hidden behind ChangeOwner and Building4549B0 boundaries remain excluded.'],
        substitutions=[
            'Reuse ActionFixture shared PE/VM/heap/CRT transport. RTTI virtual slot2C answers supplied class. ChangeOwner slot3D4 is an explicit RET8 callback boundary with declared state/list effects and result; real tracking, ownership, Team and detach lifecycles are not executed.',
            'Building4549B0 is a recorded RET0 sink; operational power state, animation, sound and nested callbacks are excluded. No target-selector, caller-loop, power-classification or dispatch instruction is replaced.'],
        entry_points=dict(dispatch=0x6DD8B0, action36_dispatch=0x6DE07B,
            all_change_house=0x6E0B60, country_house=0x502D30,
            player_at_test=0x510F60, player_at_resolve=0x510ED0,
            scenario_slot=0x68C030, trigger_house=0x726910,
            building_refresh_boundary=0x4549B0))


class ConcealAreaFixture(ActionFixture):
    """Original101 plus real local conceal and full small-map cache sweep."""

    def __init__(self):
        super().__init__()
        self.uc = u = self.u
        self.constants = initialize_waypoint_map(self)
        u.hook_del(self.waypoint_hook)
        for entry in (0x6D1830, 0x6D18C0, 0x6D1BB0, 0x49F2F0,
                      0x561710, 0x5617A0, 0x5617C0, 0x5617E0):
            self.call(entry, [])
        assert struct.unpack('<Q', u.mem_read(0xB0CD48, 8))[0] == STANDARD_Z_MULTIPLIER_BITS
        self.constants.update(trigger_level_leptons=self.integer(0xB0E6E0),
            area_level_leptons=self.integer(0xABDE88),
            adjust_for_z_multiplier_hex=bytes(u.mem_read(0xB0CD48, 8)).hex())
        self.size = (4, 4)
        u.mem_write(DISCOVERY_MAP + 0xF4, dwords(*self.size))
        # Same original no-dirty-screen branch used by execute_trigger_reveal.
        # No gameplay/Cell/edge/notification method is replaced.
        u.mem_write(DISCOVERY_MAP + 0xC, dwords(1))
        u.mem_write(0x8871E0, dwords(RULES))
        u.mem_write(0x887324, dwords(self.allocate(0x1000)))
        self.player, self.foreign = self.allocate(0x300), self.allocate(0x300)
        for p in (self.player, self.foreign):
            u.mem_write(p, bytes(0x300))
        u.mem_write(0xA83D4C, dwords(self.player))
        self.cells = {}
        for y in range(9):
            for x in range(9):
                if not in_diamond(self.size, x, y):
                    continue
                p = self.allocate(0x200)
                self.call(0x47BBF0, [], ecx=p)
                u.mem_write(p + 0x24, packed(x, y))
                u.mem_write(self.table + (y * 512 + x) * 4, dwords(p))
                self.cells[x, y] = p
        self.call(0x47BBF0, [], ecx=0xABDC50)
        self.constructor_cell = self.state(next(iter(self.cells.values())))
        self.key = self.allocate(32)
        for stream in (SCENARIO + 0x218, 0x886B88, 0xABE890):
            self.call(0x65C6D0, [31], ecx=stream)
        for begin, end in ((0x6E1A70, 0x6E1B7F), (0x567F70, 0x568140),
                           (0x578100, 0x578343), (0x6D20E0, 0x6D211C),
                           (0x47BBF0, 0x47BDA8), (0x6D8700, 0x6D8ADB),
                           (0x5865F0, 0x5866BA)):
            self.ranges.append((begin, end))
            self.original.append(bytes(u.mem_read(begin, end - begin)))
        self.conceal_hook = u.hook_add(UC_HOOK_CODE, self.observe_conceal)

    def state(self, pointer):
        p = DiscoveryFixture.pointer_state(self, pointer)
        return [*p['cell'], p['level'], p['counter'], p['gap'], p['alt'], p['flags'],
                *p['cache'], self.u.mem_read(pointer + 0x138, 1)[0]]

    def states(self):
        return {xy: self.state(p) for xy, p in sorted(self.cells.items())}

    def read_radius(self, values, name):
        self.ordered_ini(values)
        for reg, value in ((UC_X86_REG_ESI, RULES), (UC_X86_REG_EDI, INI),
                           (UC_X86_REG_ESP, SP)):
            self.u.reg_write(reg, value)
        run_checked(self.u, 0x67181C, 0x67183C, required_addresses=(0x5276D0,))
        return dict(layer=name, value=self.integer(RULES + 0x62C))

    def observe_conceal(self, u, pc, _size, _data):
        if not getattr(self, 'capturing_conceal', False):
            return
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc in (0x65C780, 0x65C7E0):
            self.rng_calls.append(f'{pc:08X}')
        if pc == 0x68BCC0:
            self.waypoint_calls.append(self.integer(sp + 8))
        elif pc == 0x567F70:
            self.map_calls.append(dict(coord=list(struct.unpack('<iii',
                u.mem_read(self.read32(sp + 4), 12))), radius=self.integer(sp + 8)))
        elif pc == 0x567FF5:
            self.projected_cells.append(list(struct.unpack('<hh', u.mem_read(sp + 0x14, 4))))
        elif pc == 0x56807B:
            p = u.reg_read(UC_X86_REG_ECX)
            self.probes.append([*struct.unpack('<hh', u.mem_read(p + 0x24, 4)), self.integer(p + 0x130)])
        elif pc == 0x6E1B66:
            self.after_conceal = self.states()
        elif pc == 0x578100:
            self.update_calls += 1
        elif pc in (0x578148, 0x5781E8):
            p = u.reg_read(UC_X86_REG_ESI)
            if p:
                self.update_visits[0 if pc == 0x578148 else 1].append(
                    list(struct.unpack('<hh', u.mem_read(p + 0x24, 4))))
        elif pc in (0x6DA7D0, 0x5865F0):
            p = self.read32(sp + 4)
            self.notifications.append([f'{pc:08X}', *struct.unpack('<hh', u.mem_read(p + 0x24, 4))])

    def execute_case(self, case, layers):
        u = self.u
        self.capturing_conceal = False
        # Execute the existing exact constructor/read scalar owner. These
        # are declared per-layer caches, not an invented radius assignment.
        u.reg_write(UC_X86_REG_ESI, RULES)
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, 0x665EDE, 0x665EE8)
        radius_reads = [self.read_radius(values, name) for name, values in layers]
        if 'radius' in case:
            radius_reads.append(self.read_radius({'General': {'RevealTriggerRadius': str(case['radius'])}}, 'case_override'))
        for xy, p in self.cells.items():
            u.mem_write(p + 0x11B, b'\0\0')
            u.mem_write(p + 0x120, b'\xFE\xFE')
            u.mem_write(p + 0x12C, dwords(0x12340018, 0, 0))
            u.mem_write(p + 0x138, b'\0')
            u.mem_write(p + 0x140, dwords(0))
        for row in case.get('overrides', []):
            p = self.cells[tuple(row['cell'])]
            for field, offset in (('counter', 0x130), ('gap', 0x134), ('alt', 0x12C), ('flags', 0x140)):
                if field in row:
                    u.mem_write(p + offset, dwords(row[field]))
        source = tuple(case.get('cell', (4, 4)))
        p = self.cells[source]
        u.mem_write(p + 0x11B, bytes((case.get('level', 0) & 255,)))
        if 'flags' in case:
            u.mem_write(p + 0x140, dwords(case['flags']))
        u.mem_write(self.player + 0x240, bytes((case.get('latch', 0),)))
        u.mem_write(self.foreign + 0x240, b'\1')
        read = self.read_action('101,0,7,0,0,0,0,A')
        u.mem_write(SCENARIO + 0x632 + 7 * 4, packed(*source))
        u.mem_write(SCENARIO + 0x632, packed(1, 4))
        before = self.states()
        self.after_conceal = None
        self.map_calls, self.projected_cells, self.probes = [], [], []
        self.update_calls, self.update_visits = 0, [[], []]
        self.waypoint_calls, self.notifications, self.rng_calls = [], [], []
        rng_before = self.rng_hashes()
        self.capturing_conceal = True
        result = self.call(0x6DD8B0, [self.foreign, 0, 0, 0], ecx=ACTION) & 255
        self.capturing_conceal = False
        after_conceal = self.after_conceal or before
        after = self.states()
        self.check_code()
        return dict(input=case, read=read, radius_reads=radius_reads, result=result,
            waypoint_calls=self.waypoint_calls, map_calls=self.map_calls,
            projected_cells=self.projected_cells, conceal_probes=self.probes,
            after_conceal_delta=[state for xy, state in after_conceal.items() if state != before[xy]],
            after_update_delta=[state for xy, state in after.items() if state != after_conceal[xy]],
            update_calls=self.update_calls, update_visits=self.update_visits,
            notifications=self.notifications, rng_before=rng_before,
            rng_after=self.rng_hashes(), rng_calls=self.rng_calls)


def conceal_area_generate():
    root = configured_gamemd().parent
    rules_raw = mix((root / 'expandmd01.mix').read_bytes())[mix_hash('RULESMD.INI')]
    map_raw = mix((root / 'mapsmd03.mix').read_bytes())[mix_hash('SOV01UMD.MAP')]
    # This retained scalar kernel reads exactly one key. Reuse its native
    # cache/CRC transport for that key, without initializing hundreds of
    # unrelated General entries and independent native CRC VMs.
    layers = [(name, {'General': {key: value for key, value in sections(raw).get('General', {}).items()
                                if key == 'RevealTriggerRadius'}})
              for name, raw in (('retail_rulesmd', rules_raw), ('retail_sov01', map_raw))]
    f = ConcealAreaFixture()
    cases = [
        dict(name='retail_radius_flat'),
        dict(name='radius2_flat', radius=2),
        dict(name='positive_odd_level3', radius=2, level=3),
        dict(name='negative_odd_level3', radius=2, level=-3),
        dict(name='bridge100', radius=2, level=3, flags=0x100),
        dict(name='bridge400', radius=2, level=3, flags=0x400),
        dict(name='radius_zero', radius=0),
        dict(name='radius_negative1', radius=-1),
        dict(name='counter_and_gap_controls', radius=2, overrides=[
            dict(cell=[3, 4], counter=-1, gap=3), dict(cell=[4, 3], counter=1),
            dict(cell=[5, 4], counter=2), dict(cell=[4, 5], gap=-2)]),
        dict(name='global_pending_outside_local_circle', radius=0,
             overrides=[dict(cell=[1, 4], counter=2, flags=0x12340023)]),
        dict(name='player_latch_skips_conceal_and_update', radius=0, latch=1,
             overrides=[dict(cell=[1, 4], counter=2, flags=0x12340023)]),
        dict(name='signed_max_level', radius=0, level=127),
        dict(name='signed_min_level', radius=0, level=-128),
    ]
    return dict(schema_version=1,
        fields=['x', 'y', 'level', 'counter', 'gap', 'alt', 'flags', 'cache0', 'cache1', 'dirty138'],
        fixture=dict(size=list(f.size), allocated_cells=[list(xy) for xy in sorted(f.cells)],
            constructor_cell=f.constructor_cell,
            default_prior=dict(level=0, counter=0, gap=0, alt=0x12340018, flags=0, cache=[-2, -2], dirty138=0)),
        constants=f.constants,
        physical_inputs=[dict(file=name, bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest())
            for name, raw in (('expandmd01.mix/RULESMD.INI', rules_raw), ('mapsmd03.mix/SOV01UMD.MAP', map_raw))],
        controls=[f.execute_case(case, layers) for case in cases])


def conceal_area_metadata():
    return provenance(
        scope='Original Action101/Map567F70 local conceal and full578100 two-pass sweep on a small complete allocated Size4x4 diamond; no occupied-object or rendered-pixel claim.',
        assumptions=[
            'Reuse ActionFixture, initialize_waypoint_map, DiscoveryFixture Cell observations, native Cell constructor47BBF0 and sparse512-stride table. All28 Size4x4 diamond Cells and shared Dummy execute original constructors. World priors explicitly replace only level/counter/gap/alt/flags/cache/dirty fields for selected histories; this is not retail scenario geometry.',
            'Original TAction constructor/reader/full dispatch1016DE8B7->6E1A70 reads waypoint index from+90(7), independently of trailing tokenA/+44(0). Current Player+240, rather than incoming foreign Trigger House+240, gates both conceal and following update. Foreign latch is1 in all cases; only the declared local latch control skips.',
            'Existing original projection6D1830/6D18C0/6D1BB0 and Cell/TAction/Map constant initializer groups run. Caller half-height,100/400 bridge flags, signed word arithmetic, native AdjustForZ6D20E0, original567F70 radius traversal, counter admission and both alt-bit stores execute without substitutions. No Python formula supplies a golden coordinate, distance, counter or bitmask.',
            'RevealTriggerRadius uses original constructor store665EDE and existing General scalar read67181C..67183C/5276D0 on declared cached physical RULESMD then SOV01 General layers, cropped to the one exact key this body reads; optional case overrides use the same native reader. Physical hashes are pinned. Full INI loader/Process chronology, optional LANGRULE/mode passes and unrelated General keys/index collisions are excluded.',
            'Original578100 and allocated578290 run to completion, including pending20 cleanup,138 dirty writes, both native edge-cache6D8700 recomputation and empty resident5865F0 notifications. The global-pending control lies outside radius0 and demonstrates this sweep is broader than the local circle. CellDelta arrays use the declared fields order and are native post-conceal then post-update differences.',
            'Original6DA7D0 takes its no-dirty-screen branch from supplied Map+C=1; actual dirty-region publication/rendering is excluded. Cells have empty resident lists and no local Tags/units/buildings. Native neighbor/radar/empty-resident bodies execute; occupied knowledge callbacks, nested actions, renderer and full map/session loading remain outside scope.',
            'Three native RNG streams start with seed31; original draw entry observations and full state hashes are retained. The selected path has no timer/detach producer; no gameplay callable is replaced.'],
        substitutions=[
            'Only inherited ActionFixture platform/CRT/heap/INI-cache boundaries; no Action101, Map567F70/578100/578290, projection, Cell writer, edge-cache or notification instruction is replaced.'],
        entry_points=dict(dispatch=0x6DD8B0, execute101=0x6DE8B7, action101=0x6E1A70,
            local_conceal=0x567F70, update=0x578100, allocated_next=0x578290,
            edge_cache=0x6D8700, cell_constructor=0x47BBF0, adjust_for_z=0x6D20E0,
            radius_ctor=0x665EDE, radius_read=0x67181C, resident_notify=0x5865F0))


MUSIC_THEME_KEYS = ('INTRO', 'LOADING', 'CREDITS', 'SCORE', 'BrainFreeze', 'Drok', 'Deceiver', 'PhatAttack')
MUSIC_THEME_INI = '[Themes]\n' + ''.join(f'{i}={key}\n' for i, key in enumerate(MUSIC_THEME_KEYS)) + ''.join(f'[{key}]\nSound={"BrainFre" if key == "BrainFreeze" else key}\n' for key in MUSIC_THEME_KEYS)

def music_theme_case(spec):
    f = ActionFixture()
    # Preserve the original bodies as well as the inherited action reader.
    for begin, end in ((0x720B20, 0x720BA6), (0x721210, 0x721263),
                       (0x407100, 0x4072C0), (0x4080C0, 0x4080EC),
                       (0x720FD0, 0x721006)):
        f.ranges.append((begin, end))
        f.original.append(bytes(f.u.mem_read(begin, end - begin)))
    u = f.u
    theme = 0xA83D10
    u.mem_write(theme, bytes(0x30))
    u.mem_write(theme, dwords(2, spec.get('retained', 2), -2, spec.get('volume', 255)))
    u.mem_write(theme + 0x11, bytes([spec.get('fading', False)]))
    entries = []
    for key in MUSIC_THEME_KEYS:
        pointer = f.allocate(0x2A0)
        u.mem_write(pointer, key.encode('ascii') + b'\0')
        entries.append(pointer)
    vector = f.allocate(4 * len(entries))
    u.mem_write(vector, dwords(*entries))
    u.mem_write(theme + 0x18, dwords(vector))
    u.mem_write(theme + 0x24, dwords(len(entries)))
    stream = f.allocate(0x100) if spec.get('stream', False) else 0
    u.mem_write(theme + 0x2C, dwords(stream))
    timed = 'wall_times' in spec
    now = 0
    sample = None
    if timed:
        assert stream and len(spec['wall_times']) == len(spec['actions'])
        sample = f.allocate(0x100)
        u.mem_write(sample, bytes(0x100))
        u.mem_write(stream, bytes(0x100))
        u.mem_write(stream + 0xC, dwords(1))
        u.mem_write(stream + 0x14, dwords(sample))
    u.mem_write(0xA8EC74, bytes([spec.get('scores', True)]))
    u.mem_write(0xA8ED64, bytes([spec.get('suppressed', False)]))
    for pointer in (SCENARIO + 0x218, 0x886B88, 0xABE890):
        f.call(0x65C6D0, [31], ecx=pointer)
    before = f.rng_hashes()
    calls, queues, draws = [], [], []
    playing = spec.get('playing', bool(stream))
    def observe(_u, pc, _size, _data):
        nonlocal playing
        if pc == 0x720B20:
            queues.append(f.integer(u.reg_read(UC_X86_REG_ESP) + 4))
        elif pc == 0x407000:
            return_from_sink(u, 0, int(spec.get('device', True)))
        elif pc == 0x4068E0:
            return_from_sink(u, 0, 0)
        elif pc == 0x4093B0 and timed:
            u.reg_write(UC_X86_REG_EDX, now >> 32)
            return_from_sink(u, 0, now & 0xFFFFFFFF)
        elif pc == 0x4080D0 and not timed:
            return_from_sink(u, 0, int(spec.get('interpolating', True)))
        elif pc == 0x408070:
            return_from_sink(u, 0, int(playing))
        elif pc == 0x407F40:
            calls.append('stop'); playing = False
            return_from_sink(u, 0, 0)
        elif pc == 0x4080C0:
            calls.append('fade_target_zero')
            if not timed:
                return_from_sink(u, 0, 0)
        elif pc in (0x65C780, 0x65C7E0):
            draws.append(hex(pc))
    u.hook_add(UC_HOOK_CODE, observe)
    if timed:
        # PlayFile407E7D supplies this full-scale initial value. Original
        # Init/SetTarget/Tick run; the clock import is the only time input.
        u.reg_write(UC_X86_REG_EDX, 0x4000)
        f.call(0x407100, [], ecx=sample + 0x10)
    parsed, returned, ramp_history = [], [], []
    for index, raw in enumerate(spec['actions']):
        if timed:
            now = spec['wall_times'][index]
            f.call(0x4071C0, [], ecx=sample + 0x10)
        parsed.append(f.read_action(raw)['value'])
        returned.append(f.call(0x6DD8B0, [0, 0, 0, 0], ecx=ACTION) & 255)
        if timed:
            ramp_history.append(dict(wall_ms=now, current=f.integer(sample + 0x14),
                target=f.integer(sample + 0x18),
                last_ms=struct.unpack('<Q', u.mem_read(sample + 0x28, 8))[0]))
    settled = None
    if timed:
        now = spec['settle_at_ms']
        f.call(0x4071C0, [], ecx=sample + 0x10)
        mark = len(calls)
        query = bool(f.call(0x720FD0, [], ecx=theme) & 255)
        settled = dict(wall_ms=now, playing=query, stopped='stop' in calls[mark:])
    f.check_code()
    after = f.rng_hashes()
    assert before == after and not draws
    result = dict(input=spec, parsed=parsed, returned=returned, queue_requests=queues,
                slots=[f.integer(theme + offset) for offset in (0, 4, 8)],
                fading=bool(u.mem_read(theme + 0x11, 1)[0]), stream_calls=calls,
                rng_draws=draws, rng_unchanged=before == after)
    if timed:
        result.update(ramp_history=ramp_history, settled=settled)
    return result

def music_theme_generate():
    def named(name, **prior):
        return dict(actions=[f'20,8,{name},0,0,0,0,A'], **prior)
    cases = [
        named('Drok'), named('dRoK', stream=True), named('BrainFreeze', stream=True),
        named('BrainFre', stream=True), named(' Drok ', stream=True), named('unknown'),
        dict(actions=['20,0,-3,0,0,0,0,A'], stream=True),
        dict(actions=['20,0,-2,0,0,0,0,A'], stream=True),
        dict(actions=['20,0,1,0,0,0,0,A']),
        named('Drok', scores=False), named('Drok', device=False),
        named('Drok', suppressed=True), named('Drok', retained=5, stream=True),
        named('Drok', volume=0),
        dict(actions=['20,8,Drok,0,0,0,0,A', '20,8,BrainFreeze,0,0,0,0,A',
                      '20,0,-3,0,0,0,0,A'], stream=True),
        dict(actions=['20,11,3,0,0,0,0,29']),
        dict(actions=['20,0,5,0,0,0,0,A', '20,0,4,0,0,0,0,A',
                      '20,0,-3,0,0,0,0,A'], stream=True,
             wall_times=[100, 600, 950], settle_at_ms=1200),
    ]
    return dict(default_prior=dict(slots=[2, 2, -2], volume=255, fading=False,
                                   scores=True, device=True, suppressed=False,
                                   stream=False, interpolating=True),
                theme_ini=MUSIC_THEME_INI, theme_keys=list(MUSIC_THEME_KEYS),
                controls=[music_theme_case(case) for case in cases])


def music_theme_metadata():
    return provenance(
        scope='Seventeen original TAction20 constructor/read/dispatch and Theme queue controls; names, scalar operands, sentinels, ordering, audio gates and one repeated-request fade history. No physical audio claim.',
        assumptions=[
            'The eight declared Theme keys and preexisting slots/stream flags are supplied priors, not a replay of Init_Game or Theme ReadINI. Rust reads the accompanying synthetic INI through its production catalog reader. Native From_Name721210 and the actual CRT case-insensitive compare execute; Sound= stems are not name aliases.',
            'Original6DD000 constructor,6DD5B0 counted action reader and full6DD8B0 Execute run. Parameter8 resolves its untrimmed name into+90; scalar0/11 reads preserve signed values independently of the trailing waypoint/auxiliary token. All requests reach original720B20, including its process/device/startup gates, retained comparison, pending write and fading byte.',
            'The existing ActionFixture supplies CRT thread data/heap/INI transport. No action, name lookup or queue instruction is patched. In the first16 controls, stream playing/interpolating/stop/target-zero are recorded transports, without decoding or output-device callbacks.',
            'The timed history supplies a full-scale sample shell, then executes original VolumeInterp Init407100, Tick4071C0, target-zero4080C0, SetTarget407170 and stream interpolation4080D0. Timestamp4093B0 returns declared milliseconds. Repeated queue requests at100/600/950 finish fading by1200; no call restarts the current ramp. Rust retains its documented linear-fade/1ms residual and compares the post-fade stop, not exact gain samples. This is not a Windows audio-pump cadence or waveform comparison.',
            'Three original RNG constructors seed31 before each control. Read/Execute/Queue leave the complete states unchanged and reach neither original draw entry. This control does not run later Theme AI/NextSong and makes no automatic-playlist RNG claim.',
            'No gameplay timer or detach belongs to this dispatcher leaf. Its ordered queue requests return AL1 even when audio admission rejects them. Numeric sentinels pass unfiltered; missing/unknown names resolve-1. Non-theme named metadata types6/7 on Action20 and malformed/truncated action authoring are outside this ordinary music-parameter chain.',
            'VERA retains type8 names until the process Theme owner receives the ordered effect. Its catalog is immutable after initial selection, so this uses the same lookup without copying a catalog into deterministic simulation. The existing intentional RA2 playlist append policy is unchanged; native controls use only the declared YR-shaped prefix.'
        ],
        substitutions=[
            'Audio device407000, stream playing408070 and stop407F40 return declared/recorded transport results. Interpolation4080D0 and fade target4080C0 are transports only in untimed controls; timed controls execute them unchanged and supply Timestamp4093B0. Diagnostic4068E0 is a no-op. No Theme AI or actual hardware output is included.'
        ],
        entry_points=dict(action_constructor=0x6DD000, action_read=0x6DD5B0,
                          action_execute=0x6DD8B0, action20=0x6DE8E4,
                          theme_from_name=0x721210, theme_queue=0x720B20,
                          interp_init=0x407100, interp_target=0x407170,
                          interp_tick=0x4071C0, still_playing=0x720FD0))




def all_to_hunt_action_controls():
    f = AllChangeHouseFixture()
    u = f.u
    f.houses = [f.allocate(0x17000) for _ in range(3)]
    for house, typ, country in zip(f.houses, f.house_types, (0, 7, 0)):
        u.mem_write(house, bytes(0x17000))
        u.mem_write(house+0x24, dwords(0x7EA834))
        u.mem_write(house+0x34, dwords(typ))
        u.mem_write(typ+0xB8, dwords(country))
    assert f.read32(0x7EA834+0x38) == 0x501400
    for start, end in ((0x6E45E0, 0x6E462B), (0x501400, 0x50153D)):
        f.ranges.append((start, end))
        f.original.append(bytes(u.mem_read(start, end-start)))
    vtable, mission_sink, occupants_sink = f.allocate(0x500), f.allocate(4), f.allocate(4)
    u.mem_write(vtable, bytes(0x500))
    u.mem_write(vtable+0x2C, dwords(f.what_sink))
    u.mem_write(vtable+0x1E8, dwords(mission_sink))
    u.mem_write(vtable+0x408, dwords(occupants_sink))
    f.actors = [dict(owner=0, team=True, foot=True, down=True, limbo=False, **{'class':1}),
        dict(owner=0, team=False, foot=True, down=True, limbo=False, **{'class':1}),
        dict(owner=2, team=False, foot=True, down=True, limbo=False, **{'class':1}),
        dict(owner=1, team=False, foot=True, down=True, limbo=False, **{'class':1}),
        dict(owner=0, team=False, foot=True, down=True, limbo=True, **{'class':1}),
        dict(owner=0, team=False, foot=True, down=False, limbo=False, **{'class':1}),
        dict(owner=0, team=False, foot=False, down=True, limbo=False, **{'class':6})]
    f.actor_index = {pointer:i for i,pointer in enumerate(f.pool[:len(f.actors)])}
    f.active = list(range(len(f.actors)))
    calls, writes, events = [], [], []

    def observe(vm, pc, _size, _data):
        sp = vm.reg_read(UC_X86_REG_ESP)
        if pc == 0x501400:
            calls.append(dict(entry=hex(pc), house=f.house_index(f.read32(sp+4)-0x24)))
        elif pc == mission_sink:
            events.append(dict(call='queue_mission', actor=f.actor_index[vm.reg_read(UC_X86_REG_ECX)],
                               mission=f.integer(sp+4), arg=f.integer(sp+8)))
            return_from_sink(vm, 8, 1)
        elif pc == 0x6EA870:
            events.append(dict(call='team_remove', actor=f.actor_index[f.read32(sp+4)],
                               arg1=f.integer(sp+8), arg2=f.integer(sp+12)))
            return_from_sink(vm, 12, 1)
        elif pc == occupants_sink:
            events.append(dict(call='occupant_count', actor=f.actor_index[vm.reg_read(UC_X86_REG_ECX)], result=0))
            return_from_sink(vm, 0, 0)

    def write(vm, _access, at, size, value, _data):
        for index, pointer in enumerate(f.houses):
            if pointer <= at < pointer+0x17000:
                writes.append(dict(house=index, offset=hex(at-pointer), size=size,
                                   value=value, pc=hex(vm.reg_read(UC_X86_REG_EIP))))

    u.hook_add(UC_HOOK_CODE, observe)
    u.hook_add(UC_HOOK_MEM_WRITE, write)
    view = SimpleNamespace(u=u, scenario=SCENARIO)
    cases = [dict(name='country0_not_incoming_or_retained', selector=0),
        dict(name='country7', selector=7),
        dict(name='duplicate_country_reversed', selector=0, order=(2,1,0)),
        dict(name='retained_house8997', selector=8997),
        dict(name='retained_house_null', selector=8997, trigger_owner=None),
        dict(name='null_trigger_valid_country', selector=0, null_trigger=True),
        dict(name='missing_country999', selector=999),
        dict(name='null_incoming_valid_country', selector=0, incoming=None)]
    rows = []
    for case in cases:
        f.selectors, f.rng_calls = [], []
        order = case.get('order', (0,1,2))
        u.mem_write(f.house_array, dwords(*(f.houses[i] for i in order)))
        u.mem_write(f.live, bytes(0x48))
        owner = case.get('trigger_owner', 2)
        u.mem_write(f.live+0x2C, dwords(f.houses[owner] if owner is not None else 0))
        for i, house in enumerate(f.houses):
            u.mem_write(house+0x249, b'\0')
            u.mem_write(house+0x250, dwords((-7,3,5)[i]))
            u.mem_write(house+0x5634, dwords(41+i,777+i,108+i))
        for pointer, actor in zip(f.pool, f.actors):
            u.mem_write(pointer, bytes(0x600))
            u.mem_write(pointer, dwords(vtable))
            u.mem_write(pointer+0x14, dwords(4 if actor['foot'] else 0))
            u.mem_write(pointer+0x21C, dwords(f.houses[actor['owner']]))
            u.mem_write(pointer+0x74, bytes([actor['down']]))
            u.mem_write(pointer+0x81, bytes([actor['limbo']]))
            u.mem_write(pointer+0x5D4, dwords(0x7EA70000 if actor['team'] else 0))
        f.publish()
        read = f.read_action(f"6,0,{case['selector']},0,0,0,0,A")
        rng_before = CampaignReader.rng_state(view)
        before = [bytes(u.mem_read(p, 0x17000)) for p in f.houses]
        calls.clear(); events.clear(); writes.clear()
        incoming = case.get('incoming', 1)
        result = f.call(0x6DD8B0, [f.houses[incoming] if incoming is not None else 0,
            0, 0 if case.get('null_trigger') else f.live, 0], ecx=ACTION)&255
        after = [bytes(u.mem_read(p, 0x17000)) for p in f.houses]
        rng_after = CampaignReader.rng_state(view)
        assert rng_before == rng_after and not f.rng_calls
        assert all(w['offset']=='0x249' and w['size']==1 and w['value']==1 for w in writes)
        assert all(all(j == 0x249 for j,(a,b) in enumerate(zip(left,right)) if a != b)
                   for left,right in zip(before,after))
        rows.append(dict(input=case, native_read=read, result=result, selector_calls=f.selectors,
            calls=list(calls), events=list(events), writes=list(writes),
            latch_after=[raw[0x249] for raw in after],
            strategy_timers_before=[list(struct.unpack_from('<iii',raw,0x5634)) for raw in before],
            strategy_timers_after=[list(struct.unpack_from('<iii',raw,0x5634)) for raw in after],
            modes_before=[struct.unpack_from('<i',raw,0x250)[0] for raw in before],
            modes_after=[struct.unpack_from('<i',raw,0x250)[0] for raw in after],
            three_full_rng_states_unchanged=True,
            rng_before={k:hashlib.sha256(bytes.fromhex(v)).hexdigest() for k,v in rng_before.items()},
            rng_after={k:hashlib.sha256(bytes.fromhex(v)).hexdigest() for k,v in rng_after.items()}))
    f.check_code()
    return dict(country_priors=[0,7,0],actors=f.actors,controls=rows,code_unchanged=True,
        rng_full_seed31=CampaignReader.rng_state(view),
        limits='Original constructor/record-reader/Execute Action6/6E45E0 and actual7EA834+38/501400 execute. House/list/actor priors are supplied. Team_remove and QueueMission are existing-style recorded leaves, not full Team or Mission execution. The nonfoot Building has explicitly zero occupants; no garrison, permanent-Dominator damage or actual cleanup result is certified.')


def all_to_hunt_garrison_controls():
    # Same VM, original map/CRT/allocator and seed owners as existing controls.
    from tools.native_oracle import image_sha256
    from tools.spatial_oracle.unit_source_scatter import make_source_fixture, CELLS, SCENARIO as scenario
    f = ActionFixture()
    u, _, read = make_source_fixture(dict(seed=31, live_entry=True), uc=f.u)
    alloc = f.allocate
    building, btype, bvt = alloc(0x800), alloc(0x1800), alloc(0x500)
    foots = [alloc(0x800), alloc(0x800)]
    types, locos, houses = [alloc(0x1000) for _ in foots], [alloc(0x200) for _ in foots], [alloc(0x6000) for _ in foots]
    # Supplied reciprocal+2A8 pair; actual Temporal pointers are+274/+278.
    targets, reciprocal2a8_peers = [alloc(0x800) for _ in foots], [alloc(0x400) for _ in foots]
    vector, fvt, rules = alloc(0x100), alloc(0x600), alloc(0x2000)
    u.mem_write(rules,bytes(0x2000))
    u.mem_write(0x8871E0,dwords(rules))
    unlimbo, uninit, can_enter, scatter, do_action, destination = [alloc(4) for _ in range(6)]
    u.mem_write(bvt, bytes(u.mem_read(0x7E3EBC,0x500)))
    u.mem_write(fvt, bytes(u.mem_read(0x7EB058,0x600)))
    # Actual selected class slots. Placement/lifecycle/DoAction are boundaries.
    for slot,pointer in ((0xD8,unlimbo),(0xF8,uninit),(0x558,do_action),(0x480,destination)):
        u.mem_write(fvt+slot,dwords(pointer))
    original_ranges = ((0x457DE0,0x4581E5),(0x4585C0,0x458741),
        (0x51B1F0,0x51B350),(0x6FCDB0,0x6FCF95),(0x5B35E0,0x5B3645),
        (0x51D0D0,0x51D6F0),(0x45EC90,0x45ECE0),(0x447AC0,0x447B14),
        (0x5657A0,0x565803),(0x41BEA0,0x41BEDD),(0x65C6D0,0x65C88D))
    code = [bytes(u.mem_read(a,b-a)) for a,b in original_ranges]
    names = {pointer:index for index,pointer in enumerate(foots)}
    calls, writes, rng_calls, pending_rng, raw_draws = [], [], [], [], []
    row = {}
    returned_entries = []
    watch = {0x51B1F0:'infantry_assign_target',0x6FCDB0:'techno_assign_target',
        0x51D0D0:'scatter_native',0x5B35E0:'queue_native',0x4585C0:'kill_occupants',
        0x4581F0:'occupant_count_native',0x5657A0:'map_lookup_native'}
    phase = 'setup'

    def actor_index(pointer):
        return names.get(pointer)

    def observe(vm,pc,_size,_data):
        if phase != 'run': return
        sp, this = vm.reg_read(UC_X86_REG_ESP), vm.reg_read(UC_X86_REG_ECX)
        index = actor_index(this)
        for returning in list(pending_rng):
            if pc==returning['return_pc'] and sp==returning['return_sp']:
                returning['result']=vm.reg_read(UC_X86_REG_EAX)
                pending_rng.remove(returning)
        if pc in (0x65C780,0x65C7E0):
            entry=dict(pc=hex(pc),this=hex(this),return_pc=read(sp),
                return_sp=sp+(12 if pc==0x65C7E0 else 4),
                args=[read(sp+4),read(sp+8)] if pc == 0x65C7E0 else [])
            rng_calls.append(entry);pending_rng.append(entry)
        if pc in (0x65C79D,0x65C84B):
            # The same actual XOR-word store observed by the existing RNG owner.
            raw_draws.append(vm.reg_read(UC_X86_REG_ESI)&0xFFFFFFFF)
        if pc in watch:
            count = {0x51B1F0:1,0x6FCDB0:1,0x51D0D0:3,0x5B35E0:2,0x4585C0:1,0x5657A0:1}.get(pc,0)
            entry=dict(call=watch[pc],actor=index,args=[read(sp+4+4*i) for i in range(count)])
            if pc == 0x51D0D0:
                entry['source']=list(struct.unpack('<3i',vm.mem_read(read(sp+4),12)))
            if pc == 0x5657A0:
                entry['cell']=list(struct.unpack('<2h',vm.mem_read(read(sp+4),4)))
            calls.append(entry)
        if pc in (can_enter,0x51BF90):
            cell = read(sp+4)
            args=[read(sp+4+4*i) for i in range(5)]
            entry=dict(call='can_enter_native' if pc == 0x51BF90 else 'can_enter_supplied',
                actor=index,cell=list(struct.unpack('<2h',vm.mem_read(cell+0x24,4))),args=args[1:])
            calls.append(entry)
            if pc == can_enter:
                count=sum(item['call']=='can_enter_supplied' for item in calls)
                value=0 if row.get('accept_probe')==count else 7
                entry['supplied_result']=value
                return_from_sink(vm,20,value)
            else:
                returned_entries.append((read(sp),sp+24,entry))
        for returning in list(returned_entries):
            if pc==returning[0] and sp==returning[1]:
                returning[2]['native_result']=vm.reg_read(UC_X86_REG_EAX)
                returned_entries.remove(returning)
        if pc == unlimbo:
            coords=list(struct.unpack('<3i',vm.mem_read(read(sp+4),12)))
            ok=index not in row.get('fail_unlimbo',[])
            calls.append(dict(call='unlimbo_supplied',actor=index,coords=coords,
                facing=read(sp+8),priority=read(0xA8E7AC),supplied_result=ok))
            if ok:
                # Explicit post-placement XYZ prestate, not a claimed full Unlimbo.
                vm.mem_write(this+0x9C,dwords(*coords))
                vm.mem_write(this+0x81,b'\x00')
                vm.mem_write(this+0x74,b'\x01')
            return_from_sink(vm,8,int(ok))
        elif pc == uninit:
            calls.append(dict(call='uninit_recorded',actor=index,priority=read(0xA8E7AC)))
            return_from_sink(vm,0,0)
        elif pc == scatter:
            calls.append(dict(call='scatter_recorded',actor=index,
                source=list(struct.unpack('<3i',vm.mem_read(read(sp+4),12))),
                args=[read(sp+8),read(sp+12)]))
            return_from_sink(vm,12,0)
        elif pc == do_action:
            calls.append(dict(call='do_action_recorded',actor=index,
                args=[read(sp+4+4*i) for i in range(3)]))
            return_from_sink(vm,12,1)
        elif pc == destination:
            cell=read(sp+4)
            calls.append(dict(call='destination_recorded',actor=index,
                cell=list(struct.unpack('<2h',vm.mem_read(cell+0x24,4))),mode=read(sp+8)))
            return_from_sink(vm,8,0)
        elif pc == 0x6EA870:
            calls.append(dict(call='team_remove_recorded',actor=actor_index(read(sp+4)),
                args=[f.integer(sp+8),f.integer(sp+12)]))
            return_from_sink(vm,12,1)
        elif pc == 0x70F6E0:
            calls.append(dict(call='refresh_recorded',this_is_building=this==building,arg=read(sp+4)))
            return_from_sink(vm,4,0)
        elif pc in (0x7258D0,0x70D4A0):
            calls.append(dict(call='detach_native_entry',pc=hex(pc)))

    def write(vm,_access,address,size,value,_data):
        if phase != 'run': return
        if address == 0xA8E7AC:
            writes.append(dict(global_='placement_priority',value=value,pc=hex(vm.reg_read(UC_X86_REG_EIP))))
        for index,pointer in enumerate(foots):
            if pointer<=address<pointer+0x800:
                writes.append(dict(actor=index,offset=hex(address-pointer),size=size,value=value,
                    pc=hex(vm.reg_read(UC_X86_REG_EIP))))

    u.hook_add(UC_HOOK_CODE,observe)
    u.hook_add(UC_HOOK_MEM_WRITE,write)
    view=SimpleNamespace(u=u,scenario=scenario)
    cases=[dict(name='empty_hunt',count=0,hunt=1,inside=0),
        dict(name='outside_first_hunt',hunt=1,inside=0,accept_probe=1),
        dict(name='outside_later_hunt',hunt=1,inside=0,accept_probe=6),
        dict(name='outside_nonhunt',hunt=0,inside=0,accept_probe=1),
        dict(name='distinct_building_and_passenger_owners',hunt=1,inside=0,
             accept_probe=1,building_owner=0,passenger_owners=[1,0]),
        dict(name='no_exit_hunt',hunt=1,inside=0),
        dict(name='inside_fallback_control',hunt=1,inside=1),
        dict(name='outside_failed_last_unlimbo',hunt=1,inside=0,accept_probe=1,fail_unlimbo=[1]),
        dict(name='actual_empty_cell_entry',hunt=1,inside=0,live_entry=True),
        dict(name='actual_blocked_terrain_entry',hunt=1,inside=0,live_entry=True,blocked=True),
        dict(name='actual_entry_outside_playfield',hunt=1,inside=0,live_entry=True,bounds=[16,0,0,1,1]),
        dict(name='nested_placement_scope',hunt=1,inside=0,live_entry=True,priority=7),
        dict(name='actual_scatter_move_then_hunt',hunt=1,inside=0,live_entry=True,live_scatter=True),
        dict(name='actual_nonfraidy_scatter_move_then_hunt',hunt=1,inside=0,live_entry=True,live_scatter=True,fraidycat=False)]
    output=[]
    for row in cases:
        phase='setup'
        for pointer,size in ((building,0x800),(btype,0x1800),(vector,0x100)):
            u.mem_write(pointer,bytes(size))
        u.mem_write(building,dwords(bvt));u.mem_write(building+0x520,dwords(btype))
        u.mem_write(building+0x9C,dwords(2688,2688,0))
        owner=row.get('building_owner')
        u.mem_write(building+0x21C,dwords(houses[owner] if owner is not None else 0))
        u.mem_write(btype+0xEF0,dwords(3)) # Actual image Foundation index3 =2x2.
        count=row.get('count',2)
        u.mem_write(vector,dwords(*foots))
        u.mem_write(building+0x684,dwords(0x7E43C8,vector,2,0x101,count,10))
        u.mem_write(building+0x69C,dwords(7))
        u.mem_write(fvt+0x1AC,dwords(0x51BF90 if row.get('live_entry') else can_enter))
        u.mem_write(fvt+0x174,dwords(0x51D0D0 if row.get('live_scatter') else scatter))
        for y in range(32):
            for x in range(32):
                u.mem_write(CELLS+(y*32+x)*0x200+0xEC,dwords(int(row.get('blocked',False))))
        bounds=row.get('bounds',[16,-16,-16,64,64])
        u.mem_write(0x87F7E8+0xF4,dwords(bounds[0]));u.mem_write(0x87F7E8+0xFC,dwords(*bounds[1:]))
        for i,(foot,typ,loco,house,target,reciprocal2a8_peer) in enumerate(zip(foots,types,locos,houses,targets,reciprocal2a8_peers)):
            for pointer,size in ((foot,0x800),(typ,0x1000),(loco,0x200),(house,0x6000),(target,0x800),(reciprocal2a8_peer,0x400)):
                u.mem_write(pointer,bytes(size))
            u.mem_write(foot,dwords(fvt));u.mem_write(foot+0x14,dwords(5))
            u.mem_write(foot+0x6C0,dwords(typ));u.mem_write(foot+0x6C4,dwords(-1))
            u.mem_write(foot+0x6C,dwords(100));u.mem_write(foot+0x90,b'\x01')
            u.mem_write(foot+0x81,b'\x01')
            u.mem_write(foot+0x21C,dwords(houses[row.get('passenger_owners',[0,1])[i]]))
            u.mem_write(house+0x1EC,bytes([i==0]))
            u.mem_write(house+0x5788,dwords(1));u.mem_write(foot+0x3D5,b'\x01')
            u.mem_write(foot+0xAC,dwords(5,-1,-1));u.mem_write(foot+0xB8,b'\x01')
            u.mem_write(foot+0xC8,dwords(41,777,108));u.mem_write(foot+0x640,dwords(42,778,109))
            u.mem_write(foot+0x690,b'\x01\x01')
            u.mem_write(foot+0x2B4,dwords(target));u.mem_write(foot+0x2A8,dwords(reciprocal2a8_peer))
            u.mem_write(reciprocal2a8_peer+0x2A8,dwords(foot));u.mem_write(foot+0x3B8,dwords(99))
            u.mem_write(foot+0x5D4,dwords(target if i==1 else 0))
            u.mem_write(foot+0x5E0,dwords(9));u.mem_write(foot+0x684,b'\xff')
            u.mem_write(typ+0x67C,dwords(1));u.mem_write(typ+0xDFC,dwords(-1))
            u.mem_write(typ+0xEBF,bytes([row.get('fraidycat',True)]))
            f.call(0x75AA90,[],ecx=loco)
            u.mem_write(loco+0xC,dwords(foot));u.mem_write(foot+0x674,dwords(loco+4))
        u.mem_write(0xA8E9A0,b'\x01');u.mem_write(0xA8B238,dwords(1))
        priority=row.get('priority',0)
        u.mem_write(0xA8ED84,dwords(100));u.mem_write(0xA8E7AC,dwords(priority))
        for pointer in (scenario+0x218,0x886B88,0xABE890): f.call(0x65C6D0,[31],ecx=pointer)
        before=CampaignReader.rng_state(view)
        calls.clear();writes.clear();rng_calls.clear();returned_entries.clear();pending_rng.clear();raw_draws.clear()
        phase='run'
        f.call(0x457DE0,[row['hunt'],row['inside']],ecx=building)
        phase='setup'
        after=CampaignReader.rng_state(view)
        assert not pending_rng and not returned_entries
        assert before['main']==after['main'] and before['mapgen']==after['mapgen']
        assert row.get('live_scatter') or before==after
        assert read(0xA8E7AC)==priority
        assert all(bytes(u.mem_read(a,b-a))==blob for (a,b),blob in zip(original_ranges,code))
        f.check_code()
        final=[]
        for foot,reciprocal2a8_peer in zip(foots,reciprocal2a8_peers):
            final.append(dict(owner=houses.index(read(foot+0x21C)),
                target=read(foot+0x2B4),reciprocal2a8=read(foot+0x2A8),
                reciprocal2a8_backlink=read(reciprocal2a8_peer+0x2A8),path0=f.integer(foot+0x5E0),
                health=f.integer(foot+0x6C),limbo=bool(u.mem_read(foot+0x81,1)[0]),
                cell_marked=bool(u.mem_read(foot+0x74,1)[0]),
                current_mission=f.integer(foot+0xAC),queued_mission=f.integer(foot+0xB4),
                occupy_seeks=list(u.mem_read(foot+0x690,2)),
                mission_timer=list(struct.unpack('<3i',u.mem_read(foot+0xC8,12))),
                movement_timer=list(struct.unpack('<3i',u.mem_read(foot+0x640,12)))))
        output.append(dict(input=row,calls=list(calls),writes=list(writes),occupants_after=read(building+0x694),
            fire_index_after=read(building+0x69C),priority_after=read(0xA8E7AC),foot_after=final,
            three_full_rng_states_unchanged=before==after,
            rng_before={k:hashlib.sha256(bytes.fromhex(v)).hexdigest() for k,v in before.items()},
            rng_after={k:hashlib.sha256(bytes.fromhex(v)).hexdigest() for k,v in after.items()},
            rng_calls=list(rng_calls),raw_draws=list(raw_draws),
            rng_full_before=before if row.get('live_scatter') else None,
            rng_full_after=after if row.get('live_scatter') else None))
    return dict(native_sha256=image_sha256(),
        priors=dict(building_xyz=[2688,2688,0],foundation_index=3,
            native_foundation_dimensions=[read(0x8192B8+12),read(0x819310+12)],
            foot_health=100,foot_current_mission=5,foot_queued_mission=-1,
            human_by_foot=[True,False],team_present_by_foot=[False,True],
            mission_timer=[41,777,108],movement_timer=[42,778,109],
            rules_player_scatter=False,game_mode=1,all_three_rng_seeds=31,
            placement_priority_default=0,nonnull_targets_and_reciprocal2a8_links=True,
            foot_initial_limbo=True,foot_initial_cell_marked=False,
            successful_unlimbo_supplied_xyz_and_limbo_clear_and_cell_marked=True,
            building_owner_default=None,passenger_owners_default=[0,1]),
        controls=output,code_unchanged=True,
        limits=['Original complete457DE0 and4585C0(NULL), actual foundation2x2 table, Building/Cell coords, Map lookup, live dynamic-vector clear/resize, Infantry51B1F0/Techno6FCDB0 NULL setter and Queue5B35E0 execute.',
            'Infantry51B25D..51B26D clears both supplied reciprocal+2A8 fields. These are native observations only: Rust has no corresponding reciprocal2a8 state in this batch, so neither this pair nor Temporal cleanup is claimed implemented. Actual Temporal pointers are+274/+278; see foot_navigation_coordinate.py and src/sim/mission/infantry_target_tests.rs.',
            'Some CanEnter rows supply numeric results; empty/blocked/outside-playfield/nested-scope rows execute real51BF90. Final two rows additionally execute source-aware51D0D0 with actual flags1,1 and native RNG, varying Fraidycat. All use the existing source-map owner and single ActionFixture VM.',
            'Unlimbo return and successful XYZ publication are explicit supplied prestates. Complete placement, world/House publication and locomotor state setup are not certified.',
            'DoAction, TeamRemove, UnInit and threat refresh record argument/order only. Native queue15,0 does not commence Hunt; future Hunt cadence is outside this procedure. SetDestination records selected Cell without executing movement/path/timers. No complete Scatter movement, detach/death/Team lifecycle or whole campaign parity claim.',
            'RNG counts and unchanged timers cover only actual executed instructions across those named supplied seams. Full Unlimbo, DoAction, TeamRemove, UnInit and future Hunt may add draws/timer/detach effects; zero direct detach observations do not prove zero in those excluded dependencies.'])


def all_to_hunt_retail_inputs():
    """Exact retained ALL06 records plus existing original scalar readers.

    Lexical section extraction retains source data, not native gameplay. The
    counted Event/flags/Action6 and Country-registration readers run unchanged.
    Team admission/placement is explicitly a prerequisite, not a supplied result.
    """
    from tools.input_oracle.campaign_start import prepare_house_rules
    from tools.spatial_oracle.tag_lifecycle import PowerEventControls

    install=configured_gamemd().parent
    archive=(install/'mapsmd03.mix').read_bytes()
    raw_map=mix(archive)[mix_hash('ALL06UMD.MAP')]
    rules_path=Path(__file__).parents[2]/'ini'/'rulesmd.ini'
    raw_rules=rules_path.read_bytes()
    values,rule_values=sections(raw_map),sections(raw_rules)
    m=CampaignReader()
    m.invoke(0x6832C0,m.scenario)
    rules_pointer=prepare_house_rules(m,rule_values)
    registrations=[]
    for label,entries in (('RULESMD.INI',rule_values),('ALL06UMD.MAP',values)):
        m.ordered_ini({'Countries':entries.get('Countries',{})})
        m.u.reg_write(UC_X86_REG_ESI,INI);m.u.reg_write(UC_X86_REG_EDI,rules_pointer)
        m.region(0x668C8B,0x668CDB)
        registrations.append(dict(layer=label,entries=[dict(index=i,
            name=m.string(m.read32(m.read32(0xA83C9C)+i*4)+0x24))
            for i in range(m.read32(0xA83CA8))]))
    houses=[]
    for name in values['Houses'].values():
        m.ordered_ini({name:values[name]})
        country=m.invoke(0x475540,INI,(m.cstring(name),m.cstring('Country'),0xFFFFFFFF))
        if country==9:
            houses.append(dict(name=name,raw=values[name],native_country=country))
    flags=FlagsFixture()
    routes={}
    for identity in ('095EB2AC','05C6450C','05C6397C'):
        raw_event=values['Events'][identity]
        routes[identity]=dict(trigger=values['Triggers'][identity],event=raw_event,
            native_event=PowerEventControls.read_event(raw_event),
            native_flags=flags.execute(values['Triggers'][identity],[1,1,1,1,0]),
            actions=values['Actions'][identity],
            tags={key:body for key,body in values['Tags'].items() if body.endswith(','+identity)})
    # Action6 is scalar Country9; original counted reader/constructor are the
    # same owner as the executable controls. Other action lists stay raw here.
    action=ActionFixture()
    native_action=action.read_action('6,0,9,0,0,0,0,A',ini_capacity=512)
    team_id='06C0E3DC';team=values[team_id]
    tf_id,script_id=team['TaskForce'],team['Script']
    selected_names={row['name'] for row in houses}
    m.check_code();action.check_code()
    return dict(map='ALL06UMD.MAP',archive='mapsmd03.mix',
        archive_sha256=hashlib.sha256(archive).hexdigest(),map_sha256=hashlib.sha256(raw_map).hexdigest(),
        rules_file='ini/rulesmd.ini',rules_sha256=hashlib.sha256(raw_rules).hexdigest(),
        basic=values['Basic'],country_registration=registrations,target_houses=houses,
        routes=routes,native_action6_read=native_action,
        target_initial_units={key:body for key,body in values.get('Units',{}).items()
            if body.split(',')[0] in selected_names},
        target_initial_infantry={key:body for key,body in values.get('Infantry',{}).items()
            if body.split(',')[0] in selected_names},
        preferred_ordinary_team=dict(id=team_id,team=team,task_force_id=tf_id,
            task_force=values[tf_id],script_id=script_id,script=values[script_id]),
        limits='Three exact raw elapsed/enable/Hunt records and raw Wave4 Team/TaskForce/Script bind the selected common YTNK route. Original counted Event, flags, Action6 and Country readers execute. Target Country9 has no initial Foot: the real reinforcement/Team owner must admit the selected3YTNK, or production tests must name that precondition explicitly. No native whole Team creation, elapsed Tag scheduling, physical full-map load, world placement or whole campaign outcome is claimed. The other Action53/7 lists and Tag records are exact retail bytes, not native reference-resolution executions in this row.')


def all_to_hunt_generate():
    from tools.native_oracle import image_sha256,NATIVE_FPCW
    return dict(schema_version=1,native_sha256=image_sha256(),native_fpcw=f'0x{NATIVE_FPCW:X}',
        action6=all_to_hunt_action_controls(),garrison=all_to_hunt_garrison_controls(),
        retail=all_to_hunt_retail_inputs())


def all_to_hunt_metadata():
    result=provenance(
        scope='Eight original Action6 selector/state controls, fourteen complete457DE0 garrison procedure controls around explicit dependencies, and retained retail ALL06 Country9 elapsed/enable/Hunt binding. Not a complete campaign, native Team/placement/UnInit lifecycle, general Hunt or Dominator-death parity claim.',
        assumptions=[
            'Reuse the existing ActionFixture/AllChangeHouseFixture VM, allocation/CRT/INI owners. Original Action constructor6DD000, counted read6DD5B0, Execute6DD8B0 and selector6E45E0 execute before actual House IHouse+24/vt38 dispatch501400. Country0/7/0 House order, first duplicate, retained8997/null, null live Trigger, unknown Country and unrelated/null incoming House are supplied priors. A null live Trigger rejects even a valid Country. All three full native seed31 states are saved, not only hashes.',
            '501400 captures the initial global Techno count once, traverses reverse ordinal order and re-reads the current array pointer at each ordinal; owner, cell-marked+74 and not-limbo admission run. The ordinary Foot TeamRemove(-1,0)/QueueMission(Hunt,0) calls are recorded, and only selected House+249 changes. Mode+250 and strategy timers+5634 remain untouched. Permanent Dominator/Insignificant damage is outside the selected ordinary route, not claimed absent.',
            'The existing make_source_fixture on the same ActionFixture VM supplies the small32x32 map and original Walk setup. Garrison rows execute complete457DE0 and no-exit4585C0(NULL), actual image Foundation index3 dimensions2x2, Building/Cell coordinates, map lookup and dynamic-vector clear/resize. Some CanEnter results are supplied; declared empty/blocked/outside-playfield/nested-scope rows run real51BF90. Field/list/House/limbo/type/mission priors are explicit, not native full constructors or entry lifecycle.',
            'Successful Unlimbo result/XYZ/limbo-clear/cell-marked publication is a supplied boundary. Actual Infantry51B1F0 and Techno6FCDB0 NULL target setters clear old Target, supplied reciprocal+2A8 links and path0 with their DoAction child recorded. Infantry51B25D..51B26D clears both supplied+2A8 fields; these are native observations only, with no corresponding Rust reciprocal2a8 state in this batch. Neither this pair nor Temporal cleanup is claimed implemented; actual Temporal pointers are+274/+278. Building centre feeds Scatter(1,1), then TeamRemove(-1,0) if a Team remains, then original QueueMission15,0. The selected owner-transfer control has distinct Building/passenger House priors and records actual per-field writes; SellBuilding only reads the passenger House+21C for its human-seek test. Full Unlimbo remains excluded.',
            'The final two garrison controls execute original source-aware Infantry Scatter with flags1,1, real heading, RandomRanged, rejection/XOR stores, map/CanEnter and QueueMove2,0. SetDestination records the selected Cell without moving it. Full three-stream RNG before/after hex, individual range returns and raw words are retained. Two range calls need three raw words for seed31; Main/MapGen are unchanged. Do not infer total world-chain draws from this bounded subgraph.',
            'Actual Queue15,0 only queues Hunt; the declared Guard current mission and mission/movement timers remain unchanged by executed instructions. Placement priority brackets the successful loop and returns to its input; no-exit does not raise it. DoAction, TeamRemove, UnInit, destination and threat-refresh dependencies are recorded, so their own timer/RNG/detach/lifecycle effects and later Hunt cadence remain outside this execution. Unlimbo failure invokes original caller UnInit before continuing, and no-exit removes all occupant callbacks LIFO before vector cleanup.',
            'Actual mapsmd03.mix ALL06UMD.MAP and retained ini/rulesmd.ini have file hashes. Existing CampaignReader registers physical Rules then map Country entries and reads target Houses Country; counted Event/Trigger flags and scalar Action6 readers execute. Raw three-route Trigger/Tag/Actions and selected Wave4 Team/TaskForce/Script records bind the ordinary path without hand-authored gameplay tuning. Country9 has zero initial Units/Infantry; production must use existing reinforcement/Team admission or explicitly supply that precondition. No original entire wave schedule or full physical INI layering is certified.',
            'CampaignReader requires VERA20K_CAMPAIGN_START_ASSETS to name an existing empty directory; its constructor otherwise preloads that directory. Selected Country prefixes request no assets. RA2_DIR or VERA20K_GAMEMD_EXE selects the supported retail binary and adjacent mapsmd03.mix. Keep native evidence identity separate from prior corpus provenance.'],
        substitutions=[
            'Inherited CRT/heap/TLS/import and cached-INI transports only; original selected dispatch, selector, House loop, garrison branch, target-setter, CanEnter, Scatter/RNG, queue and vector instructions are unmodified.',
            'Ordinary Action6 Foot TeamRemove/QueueMission are recorded leaves. Garrison Unlimbo supplies success and declared post-placement fields; failed rows returnfalse. UnInit, DoAction, TeamRemove, SetDestination and spatial-threat refresh record calls without executing their full effects. No damage result is substituted into a claimed Dominator chain.'],
        entry_points=dict(action_constructor=0x6DD000,action_read=0x6DD5B0,action_execute=0x6DD8B0,
            action6=0x6DEBF9,live_house_selector=0x6E45E0,all_to_hunt=0x501400,
            garrison_release=0x457DE0,no_exit_cleanup=0x4585C0,
            infantry_target=0x51B1F0,techno_target=0x6FCDB0,infantry_entry=0x51BF90,
            infantry_scatter=0x51D0D0,queue_mission=0x5B35E0,random_seed=0x65C6D0,
            random_ranged=0x65C7E0,country_registration=0x668C8B))
    result['environment']=dict(RA2_DIR='retail install containing gamemd.exe and mapsmd03.mix',
        VERA20K_CAMPAIGN_START_ASSETS='existing empty directory; selected prefixes request no assets',
        checkout_prerequisite='ini/rulesmd.ini is the extracted retail RULESMD.INI')
    return result


def all_to_hunt_source_paths():
    paths=dict(SOURCE_PATHS)
    paths.update(source_map=Path(__file__).with_name('unit_source_scatter.py'),
        source_state=Path(__file__).with_name('unit_scatter_state.py'))
    return paths


def crate_placement_control(case):
    """Actual108/Place/Overlay constructor on the shared small flat map.

    Reuse the source-aware movement map and Overlay constructor's TMP/registry
    priors. No placement, FNPC, Mark, random or timer return is supplied.
    """
    from tools.spatial_oracle.unit_source_scatter import make_source_fixture, CELLS
    from tools.spatial_oracle.bridge_constructor import D, REGISTRIES
    f=ActionFixture();u=f.u
    make_source_fixture(dict(seed=31,live_entry=True),uc=u)
    u.mem_map(D,0x200000)
    overlay=f.allocate(0x400)
    native=OriginalBridgeConstructor.__new__(OriginalBridgeConstructor)
    native.uc=u;native.typ=overlay;native.overlay_id=0;native.required=[]
    native.cell=CELLS+(10*32+10)*0x200
    native.prepare_success('success')
    u.mem_write(overlay,bytes(0x400));u.mem_write(overlay,dwords(0x7EF600))
    u.mem_write(overlay+0x294,dwords(0));u.mem_write(overlay+0x2AA,b'\1')
    rules=D+0x40000
    u.mem_write(rules+0xF8,dwords(overlay));u.mem_write(rules+0x100,dwords(overlay))
    u.mem_write(rules+0x1678,struct.pack('<d',case.get('regen_minutes',3.0)))
    u.mem_write(0x87F7E8+0xF4,dwords(16,16,0,0,16,16))
    u.mem_write(0x87F7E8+0x158,bytes(16*256))
    u.mem_write(0xA8B230,dwords(SCENARIO));u.mem_write(SCENARIO+0x214,dwords(1000))
    u.mem_write(0xA8B238,dwords(0));u.mem_write(0xA8ED84,dwords(100))
    u.mem_write(0xA8E9A0,b'\1');u.mem_write(0xA8ED2C,dwords(D+0x48000))
    f.call(0x47B2F0,[]);f.call(0x5FC310,[])
    # Slot EmptyCell has its own original static initializer; the native image
    # starts at zero here, and the same actual value is retained in output.
    OriginalBridgeConstructor.empty_registries(u,f.allocate)
    if case.get('slots_full'):
        u.mem_write(0x87F7E8+0x158,(dwords(50,777,100)+packed(5,5))*256)
    for y in range(32):
        for x in range(32):
            cell=CELLS+(y*32+x)*0x200
            if case.get('all_occupied'):
                u.mem_write(cell+0x44,dwords(0))
            if case.get('blocked'):
                u.mem_write(cell+0x124,dwords(0xFF))
            if case.get('water'):
                u.mem_write(cell+0xEC,dwords(2))
    read=f.read_action(f"108,0,{case.get('scalar',0)},0,0,0,0,A",ini_capacity=512)
    requested=case.get('waypoint',[10,10])
    if not case.get('missing_waypoint'):
        u.mem_write(SCENARIO+0x632,packed(*requested))
    waypoint=list(struct.unpack('<2h',u.mem_read(SCENARIO+0x632,4)))
    for stream in (SCENARIO+0x218,0x886B88,0xABE890):
        f.call(0x65C6D0,[31],ecx=stream)
    proxy=SimpleNamespace(u=u,scenario=SCENARIO)
    before=CampaignReader.rng_state(proxy)
    events=[];rng_calls=[]
    lifecycle_calls=[];overlay_object=None
    observe_lifecycle=case['name']=='money'
    def lifecycle_snapshot(*,live_object=False):
        pending=f.read32(0xB0F6A8);array=f.read32(0xB0F69C)
        state=dict(cursor=f.read32(SCENARIO+0x214),
            registry_counts={f'{p:08X}':f.read32(p+16) for p in REGISTRIES},
            pending_native_ids=[f.read32(f.read32(array+n*4)+0x10) for n in range(pending)])
        if live_object:
            state.update(native_id=f.read32(overlay_object+0x10),
                alive=u.mem_read(overlay_object+0x90,1)[0],
                limbo=u.mem_read(overlay_object+0x81,1)[0],
                on_map=u.mem_read(overlay_object+0x74,1)[0])
        return state
    lifecycle_before=lifecycle_snapshot() if observe_lifecycle else None
    addresses={0x56BEC0:'place',0x56DC20:'find_nearby',0x4A17C0:'slot_place',
        0x4A18F0:'validate_create',0x5FC380:'overlay_constructor',0x5FC570:'overlay_mark'}
    def observe(vm,pc,size,data):
        nonlocal overlay_object
        sp=vm.reg_read(UC_X86_REG_ESP)
        if observe_lifecycle:
            receiver=vm.reg_read(UC_X86_REG_ECX)
            if pc==0x5FC380:
                assert overlay_object is None
                overlay_object=receiver
                lifecycle_calls.append(dict(pc=f'{pc:08X}',call='overlay_constructor'))
            elif overlay_object is not None:
                if pc==0x410230 and f.read32(sp+4)==overlay_object+4:
                    lifecycle_calls.append(dict(pc=f'{pc:08X}',call='create_id',
                        cursor_before=f.read32(SCENARIO+0x214)))
                elif pc==0x68BCB0 and receiver==SCENARIO:
                    lifecycle_calls.append(dict(pc=f'{pc:08X}',call='next_unique_id',
                        cursor_before=f.read32(SCENARIO+0x214)))
                elif receiver==overlay_object and pc in (0x5FC570,0x5F65F0,0x7258D0,0x5FDF70,0x5F3B80):
                    names={0x5FC570:'mark',0x5F65F0:'uninit',0x7258D0:'announce_expired',
                           0x5FDF70:'overlay_destructor',0x5F3B80:'object_destructor'}
                    lifecycle_calls.append(dict(pc=f'{pc:08X}',call=names[pc]))
                elif pc in (0x725C70,0x7C8B3D):
                    lifecycle_calls.append(dict(pc=f'{pc:08X}',call='drain' if pc==0x725C70 else 'crt_free'))
        if pc in addresses:
            row=dict(call=addresses[pc])
            if pc==0x56BEC0:
                row.update(cell=list(struct.unpack('<2h',vm.mem_read(sp+4,4))),
                    scalar=struct.unpack('<i',vm.mem_read(sp+8,4))[0])
            elif pc==0x56DC20:
                row.update(speed=f.read32(sp+12))
            events.append(row)
        if pc in (0x65C780,0x65C7E0):
            rng_calls.append(dict(address=f'{pc:08X}',receiver=f'{vm.reg_read(UC_X86_REG_ECX):08X}',
                args=list(struct.unpack('<2i',vm.mem_read(sp+4,8))) if pc==0x65C7E0 else []))
        if pc in (0x47FDE0,0x47FB90):
            output=f.read32(sp+4);vm.mem_write(output,bytes(16))
            return_from_sink(vm,4,output)
        elif pc==0x6D2790:
            return_from_sink(vm,20,0)
        elif pc==0x4068E0:
            return_from_sink(vm,0,0)
    u.hook_add(UC_HOOK_CODE,observe)
    returned=f.call(0x6DD8B0,[0,0,0,0],ecx=ACTION)&255
    slots=[list(struct.unpack('<3i2h',u.mem_read(0x87F7E8+0x158+i*16,16))) for i in range(256)]
    placed=[dict(xy=[x,y],overlay=struct.unpack('<i',u.mem_read(CELLS+(y*32+x)*0x200+0x44,4))[0],
        data=u.mem_read(CELLS+(y*32+x)*0x200+0x11E,1)[0])
        for y in range(32) for x in range(32)
        if struct.unpack('<i',u.mem_read(CELLS+(y*32+x)*0x200+0x44,4))[0]!=-1]
    f.check_code()
    result=dict(input=case,native_read=read,native_waypoint=waypoint,
        output=dict(returned=returned,first_slot=slots[0],second_slot=slots[1],
            occupied_slots=sum(row[3:]!=[0,0] for row in slots),
            cells=placed if not case.get('all_occupied') else [],
            all_occupied_prior=bool(case.get('all_occupied')),scenario_id_cursor=f.read32(SCENARIO+0x214),
            empty_cell=list(struct.unpack('<2h',u.mem_read(0x89E748,4)))),
        events=events,rng_calls=rng_calls,rng_before=before,rng_after=CampaignReader.rng_state(proxy))
    if observe_lifecycle:
        assert overlay_object is not None
        after_placement=lifecycle_snapshot(live_object=True)
        before_drain=CampaignReader.rng_state(proxy)
        # Shared constructor/deferred Windows transport; the actual drain and
        # Overlay/Object destructors execute. No lifecycle result is supplied.
        OriginalBridgeConstructor.prepare_deferred_services(u)
        f.call(0x725C70,[])
        after_drain=lifecycle_snapshot()
        after_drain['free_calls']=sum(row['call']=='crt_free' for row in lifecycle_calls)
        after_rng=CampaignReader.rng_state(proxy)
        assert before_drain==after_rng
        result['lifecycle']=dict(before=lifecycle_before,after_placement=after_placement,
            after_drain=after_drain,calls=lifecycle_calls,
            rng_before_drain=before_drain,rng_after_drain=after_rng)
        f.check_code()
    return result


def crate_retail_inputs():
    """One actual active ALL07 map and original counted input readers."""
    from tools.spatial_oracle.tag_lifecycle import PowerEventControls
    directory=configured_gamemd().parent
    probes=[];raw_map=None
    for name in ('langmd.mix','language.mix','expandmd01.mix','mapsmd03.mix'):
        data=(directory/name).read_bytes();member=mix(data).get(mix_hash('ALL07SMD.MAP'))
        probes.append(dict(archive=name,sha256=hashlib.sha256(data).hexdigest(),
            member_sha256=None if member is None else hashlib.sha256(member).hexdigest(),
            member_bytes=None if member is None else len(member)))
        if member is not None and raw_map is None:raw_map=member
    assert [row['archive'] for row in probes if row['member_sha256']] == ['mapsmd03.mix']
    values=sections(raw_map);rules_raw=(Path(__file__).parents[2]/'ini/rulesmd.ini').read_bytes()
    rules=sections(rules_raw);identity='04F0AACC';f=ActionFixture()
    tokens=values['Actions'][identity].split(',');actions=[]
    for index in range(2):
        action=f.read_action(','.join(tokens[1+index*8:9+index*8]),ini_capacity=512)
        wp=action['waypoint'];f.ordered_ini({'Waypoints':values['Waypoints']})
        f.u.mem_write(SP+0x10,str(wp).encode()+b'\0')
        for reg,value in ((UC_X86_REG_ESP,SP),(UC_X86_REG_EBX,INI),
                (UC_X86_REG_ESI,wp),(UC_X86_REG_EDI,SCENARIO+0x632+wp*4)):
            f.u.reg_write(reg,value)
        run_checked(f.u,0x68BDE7,0x68BE35,required_addresses=(0x5276D0,))
        action.update(raw_waypoint=values['Waypoints'][str(wp)],
            native_waypoint=list(struct.unpack('<2h',f.u.mem_read(SCENARIO+0x632+wp*4,4))))
        actions.append(action)
    # Exact selected section only: this is a ReadString field-block probe,
    # not a second Rules loader or a full OverlayType::ReadINI execution.
    f.ordered_ini({'CRATE':rules['CRATE']})
    section=f.allocate(16);buffer=f.allocate(128)
    f.u.mem_write(section,b'CRATE\0');f.u.mem_write(buffer,bytes(128))
    cell_anim_length=f.call(0x528A10,[section,0x833544,0x889F64,buffer,128],ecx=INI)
    cell_anim=dict(rules_section=rules['CRATE'],map_override=values.get('CRATE'),
        key='CellAnim',capacity=128,default='',native_read_length=cell_anim_length,
        native_read=bytes(f.u.mem_read(buffer,128)).split(b'\0')[0].decode('latin1'))
    flags=FlagsFixture();f.check_code()
    return dict(map='ALL07SMD.MAP',map_sha256=hashlib.sha256(raw_map).hexdigest(),
        source_scan=probes,selected_source='mapsmd03.mix',
        rules_file='ini/rulesmd.ini',rules_sha256=hashlib.sha256(rules_raw).hexdigest(),
        trigger_id=identity,trigger=values['Triggers'][identity],events=values['Events'][identity],
        actions=values['Actions'][identity],tags={k:v for k,v in values['Tags'].items() if v.endswith(','+identity)},
        native_events=PowerEventControls.read_event(values['Events'][identity]),
        native_flags=flags.execute(values['Triggers'][identity],[1,1,1,1,0]),native_actions=actions,
        raw_rules={name:rules[name] for name in ('CrateRules','Powerups')},
        raw_overrides={name:values[name] for name in ('CrateRules','Powerups') if name in values},
        crate_cell_anim=cell_anim,
        limits='Physical registered-source probes bind this install and existing AssetManager priority; no new general resolver. Native Event/Action/flags/Waypoints field readers execute. CrateRules/Powerups values are raw source records here, not a whole Rules loader or whole-map placement execution.')


def crate_generate():
    from tools.native_oracle import image_sha256,file_span,image_bytes
    from tools.spatial_oracle.crate_pickup import execute as pickup_execute
    from tools.spatial_oracle.tag_lifecycle import PowerEventControls,execute as tag_execute
    placement=[crate_placement_control(case) for case in (
        dict(name='money',scalar=0),dict(name='random_sentinel20',scalar=20),
        dict(name='scalar276_low_byte20',scalar=276),dict(name='negative_scalar_low_byte255',scalar=-1),
        dict(name='missing_waypoint_zero_prior',missing_waypoint=True),
        dict(name='full_slots',slots_full=True),dict(name='already_occupied_cells',all_occupied=True),
        dict(name='water_speed_zero_table_reject',water=True))]
    pickups=[]
    for extra in (
        dict(name='ordinary_money'),dict(name='no_tag',tag=False),
        dict(name='no_crate_trigger',crate_trigger=False),dict(name='callback_kill',mutation='kill'),
        dict(name='callback_owner1',mutation='owner1'),
        dict(name='callback_data10',mutation='selection10',prefix_only=True),
        dict(name='callback_data0',selection=10,mutation='selection0'),
        dict(name='dead_no_tag_bypasses_alive_gate',tag=False,alive=False),
        dict(name='random_money_amount',solo_money=0)):
        case=dict(mode=0,selection=0,solo_choices=[2,0,0],same_images=True,solo_money=5000)|extra
        f=ActionFixture();f.u.mem_map(SCRATCH+0x10000,0x10000)
        pickups.append(pickup_execute(case,native_fixture=f));f.check_code()
    m=PowerEventControls(reader_inputs=[]);pscene=m.arena+0xB0000
    m.u.mem_write(0xA8B230,dwords(pscene))
    for stream in (pscene+0x218,0x886B88,0xABE890):m.fixture.call(0x65C6D0,[31],ecx=stream)
    predicate_rng=CampaignReader.rng_state(SimpleNamespace(u=m.u,scenario=pscene));classification=[]
    for kind in (49,50):
        for name,raised,bypass,persist in (
            ('matching',kind,False,0),('wrong_raise',8,False,0),
            ('scenario_init',kind,True,0),('initial_persistence1',kind,False,1)):
            # predicate supplies this sentinel immediately before its call.
            m.u.mem_write(m.event+0x54,dwords(0xABCD1234))
            event54=struct.unpack('<i',m.u.mem_read(m.event+0x54,4))[0]
            before=list(struct.unpack('<3i',m.u.mem_read(m.timer,12)))
            m.predicate(f'{kind}_{name}',f'1,{kind},0,-7',house_index=None,
                raised=raised,actor_index=None,scenario_init=bypass,initial_persistence=persist)
            row=m.predicates[-1];row.update(timer_before=before,
                timer_after=list(struct.unpack('<3i',m.u.mem_read(m.timer,12))),event54_before=event54,
                three_rng_unchanged=predicate_rng==CampaignReader.rng_state(SimpleNamespace(u=m.u,scenario=pscene)))
            row['inputs']['initial_persistence']=persist
            assert row['three_rng_unchanged']
        classification.append(dict(kind=kind,state_a=m.fixture.call(0x71F950,[],ecx=m.event)&255,
            state_b=m.fixture.call(0x71F9C0,[],ecx=m.event)&255))
    m.assert_unchanged()
    histories=[tag_execute(dict(name=f'crate_logic_repeat{repeat}',repeat=repeat,polling=True,lifecycle=True,
        observe_object_cleanup=True,
        triggers=[dict(events=[[50,0]],actions=[[28,7]])],
        operations=[['shared'],['register_logic',0],['poll'],['scenario_flags',0,0,0,1],
            ['poll'],['poll'],['scenario_flags',0,0,0,1],['poll'],['drain']])) for repeat in (0,2)]
    image=image_bytes()
    ranges=(('action108',0x6DF69B,0x35),('waypoint',0x68BCC0,0x14),
        ('empty_cell_initializer',0x47B2F0,0xF),('empty_cell_crt_pointer',0x812A24,4),
        ('place',0x56BEC0,0x160),('slot_place_timer',0x4A17C0,0x130),
        ('slot_validate_create',0x4A18F0,0x1B0),('pickup_selection',0x481A00,0x199),
        ('pickup_money',0x482465,0x86),('logic_poll',0x55AFB0,0x255),
        ('overlay_constructor',0x5FC380,0x12E),('overlay_uninit',0x5F65F0,0x92),
        ('overlay_destructor',0x5FDF70,0x96),('object_destructor',0x5F3B80,0x204),
        ('overlay_type_constructor_defaults',0x5FE250,0x70),('cell_anim_read',0x5FE865,0x58))
    return dict(schema_version=1,native_sha256=image_sha256(),
        placement_controls=placement,pickup_controls=pickups,event_controls=m.predicates,
        classification=classification,predicate_rng_seed31=predicate_rng,logic_histories=histories,
        retail=crate_retail_inputs(),bytes={name:dict(address=f'{a:08X}',size=n,
            hex=file_span(image,a,n)[1].hex()) for name,a,n in ranges})


def crate_metadata():
    return provenance(
        scope='Bounded active Action108 placement, original solo Money pickup and synchronous49 caller mutation controls,49/50 predicates, Logic50 histories and one retail ALL07 reader binding. Not whole crate/world/campaign parity.',
        assumptions=[
            'Reuse ActionFixture, make_source_fixture and OriginalBridgeConstructor prepare_success/empty_registries on one VM; flat32x32 allocated Cell table, Size16x16/LocalSize0,0,16,16, empty occupants/Terrain, resident1x1 TMP metadata, OverlayType/raw Rules state, GameActive1, mode0/frame100 and spare native registry capacity are supplied world priors. Land0 speed rows1.0, land1 rows0.0 and remaining image-default rows0.0; water contrast deliberately reaches Float5 then fails this supplied zero table. Original FNPC, slot creation, Overlay constructor/Mark, timer arithmetic and original RNG execute. No physical full-map load or pathfinding/movement integration is claimed.',
            'Place controls supply CrateRegen3 minutes and selected image/type priors; actual original three RandomClass streams seeded31 and full hex before/after are retained. Missing waypoint row supplies zero-initialized absent Scenario slot; native GetWaypointCoords reads it and FNPC executes, rather than an invented missing-index reject. Malformed out-of-range waypoint indexing is excluded. Full dword scalar20 is random sentinel;276 stores low byte20. Timer draw executes in campaign too.',
            'The same existing money placement row additionally observes the real Overlay constructor/ID, five registry baselines and increments, Alive0/limbo1/on_map0, pending object identity and actual725C70 -> Overlay/Object destructors/free. Action construction already occupies one AllAbstract/B0F670 entry, so its retained1 baseline is not an Overlay leak. Each of the five arrays loses the Overlay during drain, cursor stays1001 and all three full RNG states remain unchanged by the drain. No released-memory host object is prescribed.',
            'Pickup reuses crate_pickup.execute on the reader VM via crate_speed_effect.fixture optional VM. Original actor/vtables, neutral House income1, empty storage and balances100/200, one owned building, CrateTrigger/raw images, fixed solo choices and money5000 are explicit priors, not full constructors/INI layering. Original AddCredits executes; FillSilos/nonempty storage/conversion and multiplayer regeneration are excluded.',
            '49 invokes a declared sink with actual Tag/actor/EmptyCell/force/source ABI. Named sink controls kill actor, change owner or change Cell data; they do not execute Tag action bodies. Original caller rereads Alive only after nonnull Tag callback, then dirty34BE, data and later House/images. No Tag deletion, attachment lifecycle or whole callback-effect parity is claimed by this sink.',
            'Original47B2F0 establishes global89E748 EmptyCell(0,0); its CRT pointer and bytes are retained. It is BSS, not file-backed data. Dirty34BE writes are conditional on CrateTrigger and outside the nonnull-Tag branch, while callback death exits before dirty and removal.',
            'PowerEventControls provides eight49/50 predicate controls and four classification calls with explicit persistence, timer, retained, actor/source/House null priors. All three seeded states are checked unchanged per leaf. Existing tag_lifecycle.execute owns two Logic/Tag/Register/Action28 histories: dirty producer is supplied by scenario_flags, not a claimed connected native pickup-to-Logic world execution. Its existing observe_object_cleanup mode seeds and observes all three full states unchanged after each operation; no Object is constructed by these histories. Original timer/completed/pending fields and cleanup calls remain in the inherited history shape.',
            'retail.crate_cell_anim retains the actual CRATE section and absent ALL07 override, then executes only original ReadString528A10 with the exact CellAnim key, empty default and capacity128. Its empty result combines with the saved static OverlayType constructor+29C zero store and ReadINI missing-key retain branch; this is not whole OverlayType construction/ReadINI, arbitrary animation lookup or all language-layer execution evidence.',
            'Selected actual ALL07SMD from mapsmd03 is bound by higher-priority-source absence probes, member/archive SHA, raw route/Tags/rules and native Action/Event/flags/Waypoints field readers. Rules/Powerups raw sections are source evidence, not a full Rules reader claim. Placement uses small synthetic geometry, not the distant retail waypoint coordinates. Existing production asset policy remains authoritative.'],
        substitutions=[
            'Inherited cached INI/CRT/heap/TLS/Windows transports. Screen rectangle calls return empty rectangles; dirty-screen and diagnostic printf are recorded/no-op presentation sinks. The money-row drain uses OriginalBridgeConstructor Windows SEH/IsBadReadPtr transport and inherited external-free sink; original gameplay cleanup is not replaced.',
            'Tag6E53A0 pickup callback is an explicit mutation sink. No Place, FNPC, Mark, RandomRanged, Money amount, AddCredits or predicate return is supplied. Inherited source fixture has unused movement observer seams; selected Place does not invoke them.'],
        entry_points=dict(action_execute=0x6DD8B0,action108=0x6DF69B,waypoint=0x68BCC0,
            place=0x56BEC0,slot_place=0x4A17C0,slot_create=0x4A18F0,
            pickup=0x481A00,tag49=0x6E53A0,add_credits=0x4F9950,
            has_occurred=0x71E940,state_a=0x71F950,state_b=0x71F9C0,
            logic=0x55AFB0,empty_cell_init=0x47B2F0,overlay_constructor=0x5FC380,
            object_uninit=0x5F65F0,deferred_drain=0x725C70,
            overlay_destructor=0x5FDF70,object_destructor=0x5F3B80))


def crate_source_paths():
    paths=dict(SOURCE_PATHS)
    paths.update(crate_pickup=Path(__file__).with_name('crate_pickup.py'),
        crate_actor=Path(__file__).with_name('crate_speed_effect.py'),
        source_map=Path(__file__).with_name('unit_source_scatter.py'),
        source_state=Path(__file__).with_name('unit_scatter_state.py'))
    return paths


class WakeupFixture(ActionFixture):
    """One existing action VM; real class slots and deferred Mission writes."""

    TABLES = {'infantry': 0x7EB058, 'unit': 0x7F5C70,
              'aircraft': 0x7E22A4, 'building': 0x7E3EBC}
    # Checked original loop-body boundaries, after each pointer load.
    VISITS = {0x6E01DA: UC_X86_REG_ESI, 0x6DDC15: UC_X86_REG_ESI,
              0x6DDC8F: UC_X86_REG_ESI}

    def __init__(self):
        super().__init__()
        self.actor_ptrs = [self.allocate(0x800) for _ in range(12)]
        self.actor_ids = {p: i for i, p in enumerate(self.actor_ptrs)}
        self.house_ptrs = [self.allocate(0x100) for _ in range(3)]
        self.trigger_ptrs = [self.allocate(0x80) for _ in range(8)]
        self.tag_ptrs = [self.allocate(0x80) for _ in range(12)]
        self.lists = [self.allocate(0x100) for _ in range(2)]
        self.capturing_wakeup = False
        for table in self.TABLES.values():
            assert self.read32(table + 0x184) == 0x5B3040
            assert self.read32(table + 0x1E8) == (
                0x41BA90 if table == self.TABLES['aircraft'] else 0x5B35E0)
        extra = [(0x5B3040, 0x5B3052), (0x5B35E0, 0x5B3645),
                 (0x41BA90, 0x41BAF0), (0x6E5380, 0x6E539F),
                 (0x65C6D0, 0x65C77D)]
        extra += [(table, table + 0x500) for table in self.TABLES.values()]
        extra += [(self.read32(table + 0x2C), self.read32(table + 0x2C) + 6)
                  for table in self.TABLES.values()]
        for a, b in extra:
            self.ranges.append((a, b))
            self.original.append(bytes(self.u.mem_read(a, b - a)))
        self.u.hook_add(UC_HOOK_CODE, self.observe_wakeup)

    def publish_wakeup(self, techno, foot):
        for base, order, slot in ((0xA8EC78, techno, 0), (0x8B3DC0, foot, 1)):
            pointer = self.lists[slot]
            self.u.mem_write(pointer, dwords(*(self.actor_ptrs[i] for i in order)))
            self.u.mem_write(base + 4, dwords(pointer, 12, 0, len(order), 12))

    def observe_wakeup(self, u, pc, _size, _data):
        if not self.capturing_wakeup:
            return
        sp = u.reg_read(UC_X86_REG_ESP)
        if pc in self.VISITS:
            self.visited.append(self.actor_ids[u.reg_read(self.VISITS[pc])])
        if pc == 0x5B3040:
            self.effective_queries.append(self.actor_ids[u.reg_read(UC_X86_REG_ECX)])
        elif pc in (0x5B35E0, 0x41BA90):
            actor = self.actor_ids[u.reg_read(UC_X86_REG_ECX)]
            self.queue_bodies.append(dict(actor=actor, entry=f'{pc:08X}'))
            if pc == 0x41BA90 or self.actors[actor]['category'] != 'aircraft':
                self.queue_calls.append(dict(actor=actor, mission=self.integer(sp + 4),
                                             arg=self.integer(sp + 8)))
        elif pc == 0x6E5380:
            self.tag_queries.append(dict(actor=self.tag_ptrs.index(u.reg_read(UC_X86_REG_ECX)),
                trigger=None if self.read32(sp + 4) == 0 else
                    self.trigger_ptrs.index(self.read32(sp + 4))))
        if pc in (0x65C780, 0x65C7E0):
            self.rng_calls.append(f'{pc:08X}')
        if pc in (0x5B3570, 0x4D7390, 0x70D4A0, 0x6EA870):
            self.unexpected_effects.append(f'{pc:08X}')

    def actor_state(self, pointer):
        return dict(current=self.integer(pointer + 0xAC), queued=self.integer(pointer + 0xB4),
                    bypass=self.u.mem_read(pointer + 0xB8, 1)[0])

    def wakeup_case(self, case):
        self.capturing_wakeup = False
        u = self.u
        defaults = dict(category='infantry', current=0, queued=2, health=100,
            alive=True, cell_marked=True, limbo=False, house=0, group=-1,
            tag_chain=[0], bypass=165)
        self.actors = [dict(defaults, **actor) for actor in case.get('actors', [{}])]
        assert len(self.actors) <= len(self.actor_ptrs)
        links = {}
        for p in self.trigger_ptrs:
            u.mem_write(p, bytes(0x80))
            # All are distinct live instances with the SAME supplied type.
            u.mem_write(p + 0x24, dwords(TYPE))
        for i, actor in enumerate(self.actors):
            p = self.actor_ptrs[i]
            u.mem_write(p, bytes(0x800))
            u.mem_write(p, dwords(self.TABLES[actor['category']]))
            u.mem_write(p + 0x6C, dwords(actor['health']))
            for offset, name in ((0x90, 'alive'), (0x74, 'cell_marked'), (0x81, 'limbo')):
                u.mem_write(p + offset, bytes((actor[name],)))
            u.mem_write(p + 0x21C, dwords(0 if actor['house'] is None else self.house_ptrs[actor['house']]))
            u.mem_write(p + 0x214, dwords(actor['group']))
            u.mem_write(p + 0xAC, dwords(actor['current'], 17, actor['queued']))
            u.mem_write(p + 0xB8, bytes((actor['bypass'],)))
            # Nonzero mission/timer/target/Team priors must survive deferred Queue.
            u.mem_write(p + 0xBC, dwords(137, 31, 19, 113, 7, 45))
            u.mem_write(p + 0x2B4, dwords(0x12345678))
            u.mem_write(p + 0x5D4, dwords(0x23456789))
            chain = actor['tag_chain']
            if chain is None:
                u.mem_write(p + 0x34, dwords(0))
            else:
                tag = self.tag_ptrs[i]
                u.mem_write(tag, bytes(0x80))
                u.mem_write(tag + 0x28, dwords(self.trigger_ptrs[chain[0]] if chain else 0))
                u.mem_write(p + 0x34, dwords(tag))
                for n, item in enumerate(chain):
                    next_item = chain[n + 1] if n + 1 < len(chain) else None
                    assert item not in links or links[item] == next_item
                    links[item] = next_item
        for i, following in links.items():
            u.mem_write(self.trigger_ptrs[i] + 0x28,
                dwords(0 if following is None else self.trigger_ptrs[following]))
        order = list(range(len(self.actors)))
        self.techno_order = case.get('techno_order', order)
        self.foot_order = case.get('foot_order',
            [i for i in order if self.actors[i]['category'] != 'building'])
        self.publish_wakeup(self.techno_order, self.foot_order)
        for stream in (SCENARIO + 0x218, 0x886B88, 0xABE890):
            self.call(0x65C6D0, [31], ecx=stream)
        kind, scalar = case['action_kind'], case.get('scalar', -7)
        raw = f'{kind},0,{scalar},0,0,0,0,A'
        self.visited, self.effective_queries, self.tag_queries = [], [], []
        self.queue_calls, self.queue_bodies, self.rng_calls, self.unexpected_effects = [], [], [], []
        rng_before = self.rng_hashes()
        self.capturing_wakeup = True
        read = self.read_action(raw, ini_capacity=512)
        rng_after_read = self.rng_hashes()
        assert rng_after_read == rng_before and not self.rng_calls
        incoming, trigger = case.get('incoming_house', 0), case.get('trigger', 0)
        before = [bytes(u.mem_read(p, 0x800)) for p in self.actor_ptrs[:len(self.actors)]]
        result = self.call(0x6DD8B0,
            [0 if incoming is None else self.house_ptrs[incoming], 0,
             0 if trigger is None else self.trigger_ptrs[trigger], 0], ecx=ACTION) & 255
        self.capturing_wakeup = False
        after = [bytes(u.mem_read(p, 0x800)) for p in self.actor_ptrs[:len(self.actors)]]
        timers_unchanged = all(a[0xBC:0xD4] == b[0xBC:0xD4] for a, b in zip(before, after))
        changed = [(i, offset) for i, (a, b) in enumerate(zip(before, after))
                   for offset, (left, right) in enumerate(zip(a, b)) if left != right]
        permitted = set(range(0xB4, 0xB9))
        other_unchanged = all(offset in permitted for _, offset in changed)
        assert self.rng_hashes() == rng_before and not self.rng_calls
        assert not self.unexpected_effects and timers_unchanged and other_unchanged
        self.check_code()
        return dict(name=case['name'], action_kind=kind, raw_record=raw,
            incoming_house=incoming, trigger=trigger, scalar=scalar,
            actors=self.actors, techno_order=self.techno_order, foot_order=self.foot_order,
            native_read=read, ret=result, visited=self.visited,
            effective_queries=self.effective_queries, tag_queries=self.tag_queries,
            queue_calls=self.queue_calls, queue_bodies=self.queue_bodies,
            after=[self.actor_state(p) for p in self.actor_ptrs[:len(self.actors)]],
            rng_before=rng_before, rng_after_read=rng_after_read,
            rng_after=self.rng_hashes(), rng_calls=self.rng_calls,
            mission_timers_unchanged=timers_unchanged, other_actor_bytes_unchanged=other_unchanged)


def wakeup_retail_inputs():
    """Retain the actual ALL02 attacked-guards → disabled prisoners binding.

    Existing native reader owners establish Countries, mission-name ids and
    initial enablement. References/actions remain exact retail strings here;
    their production binding is separate from these native scalar controls.
    """
    from tools.input_oracle.campaign_start import prepare_house_rules
    from tools.spatial_oracle.tag_lifecycle import execute as tag_lifecycle
    from tools.spatial_oracle.trigger_event_records import execute as event_reader

    # Init_Game52BB64 calls Init_Base_Mix_Files5301A0 before its later
    # Init_Mix_Files530460 call52C59C. The original EXPANDMD99..00 loop
    # formats5301C4/constructs530217 before numbered MAPSMD530A2E.
    # Pin the official patch member here rather than inventing another MIX
    # priority resolver. The Rust test compares its production-resolved map
    # bytes with this identity, rejecting any higher-priority override.
    archive = (configured_gamemd().parent / 'expandmd01.mix').read_bytes()
    raw_map = mix(archive)[mix_hash('ALL02UMD.MAP')]
    rules_path = Path(__file__).parents[2] / 'ini' / 'rulesmd.ini'
    raw_rules = rules_path.read_bytes()
    langrule_path = rules_path.with_name('langrule.ini')
    layers = [('RULESMD.INI', raw_rules)]
    if langrule_path.is_file():
        layers.append(('LANGRULE.INI', langrule_path.read_bytes()))
    layers.append(('ALL02UMD.MAP', raw_map))
    values, rule_values = sections(raw_map), sections(raw_rules)
    m = CampaignReader()
    m.invoke(0x6832C0, m.scenario)
    rules_pointer = prepare_house_rules(m, rule_values)
    m.seed_streams(31)
    rng_before = m.rng_state()
    registrations = []
    for label, raw in layers:
        entries = sections(raw)
        m.ordered_ini({'Countries': entries.get('Countries', {})})
        m.u.reg_write(UC_X86_REG_ESI, INI)
        m.u.reg_write(UC_X86_REG_EDI, rules_pointer)
        m.region(0x668C8B, 0x668CDB)
        registrations.append(dict(layer=label, entries=[dict(index=i,
            name=m.string(m.read32(m.read32(0xA83C9C) + i * 4) + 0x24))
            for i in range(m.read32(0xA83CA8))]))
    houses = []
    for name in ('Americans', 'Germans', 'Neutral'):
        m.ordered_ini({name: values[name]})
        country = m.invoke(0x475540, INI,
            (m.cstring(name), m.cstring('Country'), 0xFFFFFFFF))
        houses.append(dict(name=name, raw=values[name], native_country=country))
    missions = [dict(name=name, native_id=struct.unpack('<i', dwords(
        m.invoke(0x5B3910, m.cstring(name))))[0])
        for name in ('Sleep', 'Guard', 'Area Guard', 'Harmless')]
    rng_after = m.rng_state()
    assert rng_before == rng_after
    m.check_code()
    flags, routes = FlagsFixture(), {}
    for identity in ('095D099C', '0958149C', '095D033C'):
        trigger = values['Triggers'][identity]
        tag_records = {key: raw for key, raw in values['Tags'].items()
                       if raw.endswith(',' + identity)}
        attachments = [dict(section=section, key=key, raw=raw)
            for section in ('Infantry', 'Units', 'Aircraft', 'Structures')
            for key, raw in values.get(section, {}).items()
            if any(tag in raw.split(',') for tag in tag_records)]
        native_flags = flags.execute(trigger, [1, 1, 1, 1, 0])
        native_events = event_reader(values['Events'][identity])
        definition = dict(flags=native_flags['output'][:4], actions=[],
            events=[[row['kind'], row['value']] for row in native_events['conditions']])
        ctor = tag_lifecycle(dict(name='ALL02_' + identity + '_flags_ctor_only',
            triggers=[definition], operations=[['shared']]))
        routes[identity] = dict(trigger=trigger, tags=tag_records,
            attachments=attachments, events_raw=values['Events'][identity],
            actions_raw=values['Actions'][identity], native_flags=native_flags,
            native_events=[dict(kind=row['kind'], value=row['value'])
                for row in native_events['conditions']],
            initial_enabled=ctor['history'][0]['tags'][0]['triggers'][0]['enabled'])
    action = ActionFixture()
    native_action = action.read_action('81,0,0,0,0,0,0,A', ini_capacity=512)
    action.check_code()
    return dict(map='ALL02UMD.MAP', archive='expandmd01.mix',
        archive_sha256=hashlib.sha256(archive).hexdigest(),
        map_sha256=hashlib.sha256(raw_map).hexdigest(),
        rules_file='ini/rulesmd.ini', rules_sha256=hashlib.sha256(raw_rules).hexdigest(),
        source_selection=dict(policy='fixed official YR patch member; production identity must match',
            native_init_base_mix=0x5301A0, native_init_base_call=0x52BB64,
            native_expand_format=0x5301C4, native_expand_construct=0x530217,
            native_expand_descending_loop=0x530289, native_init_mix=0x530460,
            native_init_mix_call=0x52C59C, native_numbered_map_format=0x530A2E,
            boundary='Instruction-read archive-registration order, not native filesystem/mount execution. Existing Rust AssetManager owns lookup priority; map digest mismatch must fail for loose/language/later-expansion overrides.'),
        layers=[dict(layer=label, sha256=hashlib.sha256(raw).hexdigest()) for label, raw in layers],
        basic=dict(Player=values['Basic']['Player']), houses=houses,
        country_registration=registrations, mission_from_name=missions,
        routes=routes, native_action81_read=native_action,
        country_mission_rng=dict(unchanged=True,
            before={k: hashlib.sha256(bytes.fromhex(v)).hexdigest() for k, v in rng_before.items()},
            after={k: hashlib.sha256(bytes.fromhex(v)).hexdigest() for k, v in rng_after.items()}),
        limits='Seven real Neutral CIV Sleep records share the disabled Wakeup_Prisoners Tag. Eight Germans INIT Guard/AreaGuard records share the Event6 Germans_Attacked Tag, whose exact two Action53 references include the Wakeup and Prisoners_Panic triggers. The sibling Panic13/1 and its Team action remain raw; its later timer/Team effect is outside the selected Wakeup13/0 interval. Native Country/mission-name/flags/event readers and initial enabled constructors execute. Action references/Tag attachment/actor placement and whole Event6→53→13→81 schedule are not executed natively in this row; their original raw IDs bind the Rust production path. Authored health256 is an encoding, not native HP256. No full Type/Rules load or future Guard/Scatter/cadence/RNG claim.')


def wakeup_generate():
    from tools.native_oracle import image_sha256

    f = WakeupFixture()
    cases = []
    def add(name, kind, actor=None, **kwargs):
        cases.append(dict(name=name, action_kind=kind,
            actors=[{} if actor is None else actor], **kwargs))
    for kind, mission, house in ((81, 0, 0), (82, 0, 1), (83, 23, 0)):
        base = dict(current=mission, house=house, group=-7)
        add(f'action{kind}_admitted', kind, base)
        add(f'action{kind}_empty_registry', kind, base, techno_order=[], foot_order=[])
        for field, value in (('alive', False), ('cell_marked', False),
                             ('health', 0), ('health', -1), ('limbo', True)):
            add(f'action{kind}_{field}_{value}', kind, dict(base, **{field: value}))
        for category in ('unit', 'aircraft'):
            add(f'action{kind}_{category}', kind, dict(base, category=category))
        add(f'action{kind}_current_none_queued_admitted', kind,
            dict(base, current=-1, queued=mission))
        add(f'action{kind}_current_none_queued_none', kind, dict(base, current=-1, queued=-1))
        add(f'action{kind}_sticky_unchanged', kind, dict(base, current=6))
    for name, actor in (
        ('harmless', dict(current=23)), ('building_excluded', dict(category='building')),
        ('no_tag', dict(tag_chain=None)), ('empty_tag', dict(tag_chain=[])),
        ('different_instance_same_type', dict(tag_chain=[1])),
        ('linked_instance', dict(tag_chain=[2, 1, 0])),
        ('zero_health_limbo', dict(health=0, limbo=True)),
        ('foreign_house_irrelevant', dict(house=1)),
        ('queued_guard_still_clears_bypass', dict(queued=5)),
        ('current_attack_queued_sleep', dict(current=1, queued=0))):
        add('action81_' + name, 81, actor)
    add('action81_null_trigger', 81, trigger=None)
    add('action81_null_incoming_ignored', 81, incoming_house=None)
    add('action82_same_house_excluded', 82)
    add('action82_harmless_excluded', 82, dict(current=23, house=1))
    add('action82_null_incoming_foreign', 82, dict(house=1), incoming_house=None)
    add('action82_scalar_not_house_selector', 82, dict(house=1), scalar=1)
    add('action83_foreign_house_excluded', 83, dict(current=23, house=1))
    add('action83_sleep_excluded', 83)
    add('action83_null_incoming_excluded', 83, dict(current=23), incoming_house=None)
    add('action83_scalar_not_house_selector', 83, dict(current=23), scalar=1)
    actors = [{}, dict(current=23), dict(current=6)]
    cases.append(dict(name='action81_global_order', action_kind=81,
        actors=actors, techno_order=[1, 2, 0]))
    return dict(schema=1, native_sha256=image_sha256(),
        scope='Actions81..83 live Tag/Mission deferred Guard queue',
        rng_seed=31, mission_priors=dict(suspended=17, handler_state=137,
            mission_start_frame=31, ai_counter=19, dispatch_start=113,
            dispatch_clock_pad=7, dispatch_delay=45),
        controls=[f.wakeup_case(case) for case in cases],
        retail=wakeup_retail_inputs(), code_unchanged=True)


def wakeup_metadata():
    return provenance(
        scope='Original Actions81..83 constructor/read/dispatch, Tag identity membership, effective Mission and deferred Queue.',
        entry_points=dict(action_constructor=0x6DD000, action_read=0x6DD5B0,
            counted_read_string=0x528A10, action_execute=0x6DD8B0,
            action81=0x6DDB67, wakeup_attached=0x6E01C0,
            action82=0x6DDBF6, action83=0x6DDC71,
            tag_contains=0x6E5380, effective_mission=0x5B3040,
            queue_mission=0x5B35E0, aircraft_queue=0x41BA90,
            spring=0x7265C0, spring_execute_call=0x726605,
            spring_house_lookup=0x502D30, rng_seed=0x65C6D0,
            country_registration=0x668C8B, read_country=0x475540,
            mission_from_name=0x5B3910),
        assumptions=[
            'One existing ActionFixture VM executes unchanged original constructor, cached ReadString512, CRT strtok/atoi, full Read and full Execute. Actor memory, House pointers, active Techno/Foot order, Tags and live Trigger instances are supplied inputs. No actor/Tag/House constructor, map load or full campaign is claimed.',
            'Original class vtables are used unchanged: Infantry7EB058, Unit7F5C70, Aircraft7E22A4 and Building7E3EBC. Their actual WhatAmI, GetMission and QueueMission slots execute; no virtual answer or mission mutation is substituted. Aircraft Queue41BA90 forwards these admitted Sleep/Harmless controls to5B35E0.',
            'All supplied live Trigger pointers share one type, distinguishing pointer identity from type/name equality. Actual Tag6E5380 follows+28 links until NULL; Action81 uses the executing Trigger third argument and ignores incoming House/source/scalar. It requires alive+90 and cell-marked+74, excludes Building6, and does not inspect Health or Limbo.',
            'Actions82/83 use Execute first House argument directly, not scalar or retained Trigger+2C. The original active caller Spring7265E7..726605 separately reads TriggerType+A4 HouseType+B4 country index, calls502D30 and pushes its result for EACH action. That caller identity is instruction-read evidence; these controls begin at Execute and do not certify country registration.',
            'Action82 scans Foot forward, admits effective Sleep0 with House!=incoming; Action83 admits effective Harmless23 with House==incoming. Both require Health>0,alive,cell-marked,!limbo and do not inspect Tag. Action84 Group selection is outside this corpus.',
            'Actual GetMission returns current+AC unless-1, then queued+B4. Every admitted outer Queue receives Guard5,0. Original Queue writes queued+B4 and clears bypass+B8 while preserving current/suspended, handler/cadence/dispatch timers, target, Team and all other supplied actor bytes. No Commence, EnterIdle, readiness, detach, Team removal or RNG draw is part of this leaf. Later normal AI/Guard/target acquisition are outside these bounded controls.',
            'Three complete original-layout RNG streams are initialized by actual65C6D0(seed31) before the reader. Hashes of their complete bytes are identical after Read/Execute; entry observations record zero65C780/65C7E0 draws. Timers+BC..D3 and all nonqueue actor bytes are checked unchanged.',
            'The action bodies reread the live list pointer/count and actor fields at each forward ordinal. Deferred Queue has no list-mutating callback in the ordinary native chain. The corpus supplies stable lists and makes no arbitrary callback-mutation or whole-world ordering claim.',
            'Physical expandmd01.mix ALL02UMD.MAP and retained retail ini/rulesmd.ini plus optional LANGRULE have hashes. Init_Game52BB64 calls Init_Base_Mix_Files5301A0 before later Init_Mix_Files52C59C→530460; original EXPANDMD99..00 format5301C4/constructor530217/loop530289 precede MAPSMD530A2E. This instruction-read source policy selects the official patch member, not the older mapsmd03 member. Rust production AssetManager owns priority and must match this fixed member digest; higher-priority overrides must fail the input binding. Native archive mounting/file IO is not executed here. Exact raw Germans_Attacked/Wakeup_Prisoners and sibling Prisoners_Panic Trigger/Tag/Events/Actions plus seven prisoners/eight guards bind the production Rust test. Existing Country/mission-name/flag/event readers and Tag constructors execute; the sibling Panic timer/Team effect, complete Event6→53→13→81 native scheduling, real reference registration, actor placement and whole campaign remain outside this row.',
            'CampaignReader requires VERA20K_CAMPAIGN_START_ASSETS to name an existing empty directory; this bounded Country/name prefix requests no assets. Country/name reader streams are seed31 after setup and unchanged; constructor setup draws and the separate Tag fixture terminal next_random are outside the wakeup zero-draw claim. Action84 requires the separate Group ownership/lifecycle mechanism and remains a residual, not part of this corpus or implementation.',
        ],
        substitutions=[
            'Inherited ActionFixture supplies existing CRT thread-data, heap and cached INI transports only. No native wakeup, Tag, Mission or class vtable bytes are patched and no gameplay leaf is substituted.'
        ])


class AiActivationFixture(ActionFixture):
    """Compose the existing action and AI fixtures on one original image."""

    def __init__(self, *, seed=31):
        from tools import ai_team_oracle as ai
        from tools.native_oracle import NativeCallTrace
        super().__init__()
        self.ai_module = ai
        self.ai = ai.TeamEmu(image_owner=self)
        self.houses = [ai.HOUSE, ai.ENEMY, ai.FAKE + 0x40000]
        self.house_types = [ai.HOUSE_TYPES + i * 0x300 for i in range(3)]
        self.house_array, self.live = self.allocate(12), self.allocate(0x48)
        self.phase = 'setup'
        self.events, self.writes, self.raw_draws = [], [], []
        self.native_calls = []
        self.trace = NativeCallTrace(self.u, self.read32, self.native_calls)
        for i, (house, typ, country) in enumerate(zip(self.houses, self.house_types, (0, 7, 0))):
            self.u.mem_write(house, bytes(0x8000))
            self.u.mem_write(typ, bytes(0x200))
            self.u.mem_write(house + 0x24, dwords(ai.IHOUSE_VTABLE))
            self.u.mem_write(house + 0x30, dwords(i))
            self.u.mem_write(house + 0x34, dwords(typ))
            self.u.mem_write(typ + 0xB8, dwords(country))
        self.type_objects = ai.prepare_selector(self.ai, team_types=[False, True],
            teams=[dict(type=1, created=7, formed=True)], triggers=[(0, None, 100.0)])
        # Exactly the existing eligibility_row's unconditional multiplayer
        # path: no side, TechLevel0, <all> owner, no zone relation, no Max,
        # empty TaskForce, one enabled trigger admitting every difficulty.
        self.ai.write32(ai.GAME_MODE, 1)
        self.ai.write32(ai.HOUSE + 0x1D4, 10)
        self.ai.rules_vector(0x1394, [9, 2, 9])
        self.ai.write32(ai.TASK_FORCE + 0x9C, 0)
        for typ in self.type_objects:
            self.ai.write32(typ + 0xE4, ai.TASK_FORCE)
            self.ai.write32(typ + 0xB8, -1)
        trigger = ai.TRIGGERS
        self.ai.write32(trigger + 0x98, -1)
        self.ai.write32(trigger + 0xA0, 2)
        self.u.mem_write(trigger + 0xA4, b'\x01')
        self.u.mem_write(trigger + 0xD0, b'\x01')
        self.u.mem_write(trigger + 0xD2, b'\x01\x01\x01')
        self.ai.rules_vector(0x115C, [300, 600, 900])
        self.configure_selection({})
        for pointer in (SCENARIO + 0x218, 0x886B88, 0xABE890):
            self.call(0x65C6D0, [seed], ecx=pointer)
        for begin, end in ((0x6E45E0, 0x6E462B), (0x502D30, 0x502D60),
                           (0x510ED0, 0x510FA6), (0x726910, 0x726914),
                           (ai.BLOCK_START, ai.BLOCK_END), (0x6F0AB0, 0x6F0FB0),
                           (0x41E720, 0x41F000), (0x65C6D0, 0x65C88D)):
            self.ranges.append((begin, end))
            self.original.append(bytes(self.u.mem_read(begin, end - begin)))
        self.u.hook_add(UC_HOOK_CODE, self.observe_ai)
        self.u.hook_add(UC_HOOK_MEM_WRITE, self.observe_ai_write)

    def house_index(self, pointer):
        return self.houses.index(pointer) if pointer in self.houses else None

    def configure_selection(self, case):
        self.u.mem_write(0xA8022C, dwords(self.house_array))
        self.u.mem_write(0xA80238, dwords(3))
        self.u.mem_write(self.house_array, dwords(*(self.houses[i] for i in case.get('order', [0, 1, 2]))))
        self.u.mem_write(SCENARIO + 0x1180, dwords(*case.get('slots', [1, 0, -1, -1, -1, -1, -1, -1])))
        self.u.mem_write(self.live, bytes(0x48))
        retained = case.get('trigger_owner', 2)
        self.u.mem_write(self.live + 0x2C, dwords(self.houses[retained] if retained is not None else 0))
        for typ, country in zip(self.house_types, case.get('countries', [0, 7, 0])):
            self.u.mem_write(typ + 0xB8, dwords(country))

    def rng_full(self):
        return CampaignReader.rng_state(SimpleNamespace(u=self.u, scenario=SCENARIO))

    def observe_ai(self, u, pc, _size, _data):
        if self.phase == 'setup':
            return
        sp, ecx = u.reg_read(UC_X86_REG_ESP), u.reg_read(UC_X86_REG_ECX)
        self.trace.returned(pc, sp)
        if pc in (0x65C780, 0x65C7E0, 0x41E720):
            spec = {0x65C780: ('rng_next', 0, 0), 0x65C7E0: ('rng_ranged', 2, 8),
                    0x41E720: ('native_eligibility', 3, 12)}[pc]
            self.trace.entered(pc, sp, spec)
        if pc in (0x65C79D, 0x65C84B):
            self.raw_draws.append(u.reg_read(UC_X86_REG_ESI) & 0xFFFFFFFF)
        if pc == 0x6E45E0:
            self.events.append(dict(call='live_house_selector', selector=self.integer(sp + 8),
                live_trigger=bool(self.read32(sp + 4))))
        elif pc in (0x502D30, 0x510F60, 0x510ED0, 0x726910):
            self.events.append(dict(call='house_selector_leaf', entry=f'{pc:08X}',
                argument=self.integer_reg(UC_X86_REG_ECX) if pc != 0x726910 else 'live_trigger'))
        elif pc == 0x6F0AB0:
            house = u.reg_read(UC_X86_REG_EDX)
            self.events.append(dict(call='native_team_selector', house=self.house_index(house),
                active=bool(u.mem_read(house + 0x1F2, 1)[0])))
        elif pc == 0x6F09C0:
            self.events.append(dict(call='create_team_boundary', team_type=self.type_objects.index(ecx),
                house=self.house_index(self.read32(sp + 4))))
            # Existing AI oracle's TeamType::Create_Team boundary. Its body,
            # object allocation, member admission and later missions are excluded.
            return_from_sink(u, 4, 0)

    def observe_ai_write(self, u, _access, address, size, value, _data):
        if self.phase == 'setup':
            return
        for i, house in enumerate(self.houses):
            if house <= address < house + 0x8000:
                self.writes.append(dict(house=i, offset=address - house, size=size,
                    value=value, pc=f'{u.reg_read(UC_X86_REG_EIP):08X}'))

    def clear_observations(self):
        assert not self.trace.pending
        self.events.clear(); self.writes.clear(); self.raw_draws.clear()
        self.native_calls.clear(); self.ai.events.clear()

    def house_summary(self, index):
        pointer = self.houses[index]
        return dict(latches=[self.u.mem_read(pointer + off, 1)[0] for off in (0x1EE, 0x1EF, 0x1F2, 0x1F3)],
            team_timer=list(struct.unpack('<iii', self.u.mem_read(pointer + 0x5798, 12))))

    def action(self, kind, case):
        assert kind in (74, 75)
        self.phase = 'setup'
        self.configure_selection(case)
        read = self.read_action(f"{kind},0,{case.get('selector', 0)},0,0,0,0,A", ini_capacity=512)
        before = [bytes(self.u.mem_read(p, 0x8000)) for p in self.houses]
        summaries = [self.house_summary(i) for i in range(3)]
        old_team = bytes(self.u.mem_read(self.ai_module.TEAMS, 0x100))
        team_count = self.read32(self.ai_module.TEAM_ARRAY[1])
        rng_before = self.rng_hashes()
        self.clear_observations(); self.phase = 'action'
        incoming = case.get('incoming', 1)
        result = self.call(0x6DD8B0, [self.houses[incoming] if incoming is not None else 0,
            0, 0 if case.get('null_trigger') else self.live, 0], ecx=ACTION) & 255
        self.phase = 'setup'
        after = [bytes(self.u.mem_read(p, 0x8000)) for p in self.houses]
        assert self.rng_hashes() == rng_before and not self.native_calls
        assert old_team == bytes(self.u.mem_read(self.ai_module.TEAMS, 0x100))
        assert team_count == self.read32(self.ai_module.TEAM_ARRAY[1]) and not self.ai.events
        assert all(row['offset'] == 0x1F2 and row['size'] == 1 and row['value'] == (kind == 74)
                   for row in self.writes)
        assert all(all(j == 0x1F2 for j, (a, b) in enumerate(zip(left, right)) if a != b)
                   for left, right in zip(before, after))
        return dict(kind=kind, input=case, native_read=read, result=result,
            houses_before=summaries, houses_after=[self.house_summary(i) for i in range(3)],
            events=list(self.events), writes=list(self.writes), rng_unchanged=True,
            rng_hashes=rng_before, existing_team_unchanged=True)

    def team_update(self, frame, *, house=0):
        ai = self.ai_module
        pointer = self.houses[house]
        self.u.mem_write(ai.FRAME, dwords(frame))
        self.u.mem_write(ai.BLOCK_STACK, bytes(0x1000))
        self.u.reg_write(UC_X86_REG_ESP, ai.BLOCK_STACK + 0x800)
        self.u.reg_write(UC_X86_REG_ESI, pointer)
        self.u.reg_write(UC_X86_REG_EBP, 0)
        before = self.house_summary(house)
        old_team = bytes(self.u.mem_read(ai.TEAMS, 0x100))
        team_count = self.read32(ai.TEAM_ARRAY[1])
        rng_before = self.rng_hashes()
        self.clear_observations(); self.phase = 'consumer'
        run_checked(self.u, ai.BLOCK_START, ai.BLOCK_END)
        self.phase = 'setup'
        assert not self.trace.pending
        assert old_team == bytes(self.u.mem_read(ai.TEAMS, 0x100))
        assert self.read32(ai.TEAM_ARRAY[1]) == team_count and not self.ai.events
        assert before['latches'] == self.house_summary(house)['latches']
        self.check_code()
        return dict(frame=frame, house=house, before=before, after=self.house_summary(house),
            events=list(self.events), native_calls=list(self.native_calls),
            raw_rng_words=list(self.raw_draws), writes=list(self.writes),
            existing_team_unchanged=True, rng_before=rng_before, rng_after=self.rng_hashes(),
            scenario_rng_after_full=self.rng_full()['scenario'],
            autocreate_after=[self.u.mem_read(p + 0xA9, 1)[0] for p in self.type_objects])


def ai_activation_action_controls():
    fixture = AiActivationFixture()
    cases = [dict(name='country0_not_incoming_or_retained'), dict(name='country7', selector=7),
        dict(name='duplicate_country_reversed', order=[2, 1, 0]),
        dict(name='player_at4475', selector=4475),
        dict(name='player_at4477_unmapped', selector=4477),
        dict(name='retained_house8997', selector=8997),
        dict(name='retained_house_null', selector=8997, trigger_owner=None),
        dict(name='null_trigger_valid_country', null_trigger=True),
        dict(name='minus_one', selector=-1), dict(name='missing_country999', selector=999),
        dict(name='9001_is_not_retained', selector=9001),
        dict(name='null_incoming_valid_country', incoming=None),
        dict(name='idempotent_store', same_prior=True)]
    controls = []
    for kind in (74, 75):
        for case in cases:
            for i, house in enumerate(fixture.houses):
                for off, value in ((0x1EE, i & 1), (0x1EF, (i + 1) & 1),
                                   (0x1F2, (kind == 74) if case.get('same_prior') else (kind == 75)),
                                   (0x1F3, i & 1)):
                    fixture.u.mem_write(house + off, bytes([value]))
                fixture.u.mem_write(house + 0x5798, dwords(41 + i, 777 + i, 108 + i))
            controls.append(fixture.action(kind, case))
    fixture.check_code()
    return dict(country_priors=[0, 7, 0], controls=controls,
        rng_full_seed31=fixture.rng_full(), code_unchanged=True)


def ai_activation_histories():
    ai_cases = [
        dict(name='stop_campaign_due_repeats', game_mode=0,
             steps=[dict(action=75), dict(frame=10), dict(frame=609), dict(frame=610), dict(frame=1210)]),
        dict(name='inverse74_minimal_skirmish', initial_active=False,
             steps=[dict(action=74), dict(frame=10)]),
        dict(name='stop_restart_stop_same_house', steps=[dict(action=75), dict(frame=10),
             dict(action=74), dict(frame=610), dict(action=75), dict(frame=1210)]),
        dict(name='stop_at_cap_preserves_old_defense_team', cap=1,
             steps=[dict(action=75), dict(frame=10)]),
        dict(name='stop_seed0_ranged_rejects_one_raw_word', seed=0,
             steps=[dict(action=75), dict(frame=10)]),
        dict(name='stop_before_expiry', steps=[dict(action=75), dict(frame=9)]),
        dict(name='stop_human_outer_gate', human=True, steps=[dict(action=75), dict(frame=10)]),
        dict(name='stop_campaign_control_outer_gate', game_mode=0, control=True,
             steps=[dict(action=75), dict(frame=10)]),
        dict(name='stop_multiplayer_control_is_not_human', control=True,
             steps=[dict(action=75), dict(frame=10)]),
        dict(name='stop_passive_outer_gate', passive=True, steps=[dict(action=75), dict(frame=10)]),
        dict(name='stop_paused_zero_timer', timer=[-1, 123, 0], steps=[dict(action=75), dict(frame=10)]),
        dict(name='stop_paused_positive_timer', timer=[-1, 123, 3], steps=[dict(action=75), dict(frame=10)]),
        dict(name='stop_wrapped_timer', timer=[0x7FFFFFF0, 123, 0x20],
             steps=[dict(action=75), dict(frame=-0x7FFFFFF0)]),
        dict(name='retail_country16_and17_stop_campaign', game_mode=0, countries=[16, 17, 0],
             steps=[dict(action=75, selector=16), dict(action=75, selector=17),
                    dict(frame=10), dict(frame=10, house=1)]),
    ]
    rows = []
    for case in ai_cases:
        fixture = AiActivationFixture(seed=case.get('seed', 31))
        ai = fixture.ai_module
        fixture.u.mem_write(ai.GAME_MODE, dwords(case.get('game_mode', 1)))
        fixture.ai.rules_vector(0x13CC, [case.get('cap', 8)] * 3)
        for house, typ in zip(fixture.houses, fixture.house_types):
            fixture.u.mem_write(house + 0x1EC, bytes([case.get('human', False), case.get('control', False)]))
            fixture.u.mem_write(house + 0x1EE, b'\x00\x01')
            fixture.u.mem_write(house + 0x1F2, bytes([case.get('initial_active', True), 0]))
            fixture.u.mem_write(house + 0x184, dwords(1))
            fixture.u.mem_write(house + 0x565C, dwords(100))
            fixture.u.mem_write(house + 0x5798, dwords(*case.get('timer', [0, 123, 10])))
            fixture.u.mem_write(typ + 0x1A6, bytes([case.get('passive', False)]))
        fixture.configure_selection(case)
        initial_rng = fixture.rng_full()
        steps = []
        for step in case['steps']:
            if 'action' in step:
                steps.append(dict(action=fixture.action(step['action'], dict(case,
                    selector=step.get('selector', 0)))))
            else:
                steps.append(dict(update=fixture.team_update(step['frame'], house=step.get('house', 0))))
        final_rng = fixture.rng_full()
        assert all(initial_rng[key] == final_rng[key] for key in ('main', 'mapgen'))
        rows.append(dict(input=case, initial_rng_full=initial_rng, steps=steps,
            final_rng_full=final_rng, other_rng_streams_unchanged=True, code_unchanged=True))
    return rows


def ai_activation_retail():
    from tools.input_oracle.campaign_start import prepare_house_rules
    from tools.spatial_oracle.tag_lifecycle import PowerEventControls
    archive = (configured_gamemd().parent / 'mapsmd03.mix').read_bytes()
    raw_map = mix(archive)[mix_hash('ALL05UMD.MAP')]
    rules_path = Path(__file__).parents[2] / 'ini' / 'rulesmd.ini'
    raw_rules = rules_path.read_bytes()
    values, rule_values = sections(raw_map), sections(raw_rules)
    reader = CampaignReader()
    reader.invoke(0x6832C0, reader.scenario)
    rules = prepare_house_rules(reader, rule_values)
    registrations = []
    for label, entries in (('RULESMD.INI', rule_values), ('ALL05UMD.MAP', values)):
        reader.ordered_ini({'Countries': entries.get('Countries', {})})
        reader.u.reg_write(UC_X86_REG_ESI, INI); reader.u.reg_write(UC_X86_REG_EDI, rules)
        reader.region(0x668C8B, 0x668CDB)
        registrations.append(dict(layer=label, entries=[dict(index=i,
            name=reader.string(reader.read32(reader.read32(0xA83C9C) + i * 4) + 0x24))
            for i in range(reader.read32(0xA83CA8))]))
    houses = []
    for name in values['Houses'].values():
        reader.ordered_ini({name: values[name]})
        country = reader.invoke(0x475540, INI,
            (reader.cstring(name), reader.cstring('Country'), 0xFFFFFFFF))
        if country in (16, 17):
            houses.append(dict(name=name, raw=values[name], native_country=country))
    identity = '092A219C'
    raw_actions = values['Actions'][identity]
    action = ActionFixture()
    reads = [action.read_action(body, ini_capacity=512) for body in
             ('75,0,16,0,0,0,0,A', '75,0,17,0,0,0,0,A')]
    assert all(body['raw'] in raw_actions for body in reads)
    flags = FlagsFixture()
    begin = '0778A83C'
    enable_begin = '0E5FA1BC'
    related_routes = {}
    for route in (begin, enable_begin):
        related_routes[route] = dict(raw_trigger=values['Triggers'][route],
            raw_event=values['Events'][route], raw_actions=values['Actions'][route],
            native_event=PowerEventControls.read_event(values['Events'][route]),
            native_flags=flags.execute(values['Triggers'][route], [1, 1, 1, 1, 0]),
            tags={key: body for key, body in values['Tags'].items() if body.endswith(',' + route)})
    begin_reads = [action.read_action(body, ini_capacity=512) for body in
                   ('74,0,16,0,0,0,0,A', '74,0,17,0,0,0,0,A')]
    assert all(body['raw'] in values['Actions'][begin] for body in begin_reads)
    result = dict(map='ALL05UMD.MAP', archive='mapsmd03.mix',
        archive_sha256=hashlib.sha256(archive).hexdigest(), map_bytes=len(raw_map),
        map_sha256=hashlib.sha256(raw_map).hexdigest(),
        rules_file='ini/rulesmd.ini', rules_sha256=hashlib.sha256(raw_rules).hexdigest(),
        trigger_id=identity, raw_trigger=values['Triggers'][identity],
        raw_event=values['Events'][identity], raw_actions=raw_actions,
        tags={key: body for key, body in values['Tags'].items() if body.endswith(',' + identity)},
        native_flags=flags.execute(values['Triggers'][identity], [1, 1, 1, 1, 0]),
        native_event=PowerEventControls.read_event(values['Events'][identity]),
        native_action75_reads=reads, country_registration=registrations, target_houses=houses,
        native_action74_reads=begin_reads, conditional_begin_routes=related_routes,
        rules_team_delays=[reader.read32(reader.read32(rules + 0x115C) + i * 4) for i in range(3)],
        explicit_team=dict(id='08AE9CBC', raw=values['08AE9CBC']),
        limits='Physical mapsmd03 member and original selected scalar/Country readers only. Production AssetManager resolution must independently match this map hash. Full layering, elapsed scheduling, explicit Team admission, world placement and whole campaign outcome are excluded. The preceding Action4 and both Action53 references are retained raw, not executed here.0778A83C supplies real74 Country16/17 records but starts disabled;0E5FA1BC contains its53 enable behind Event55 value17. That prerequisite and its timing relative to the elapsed240 stop are not executed or assumed satisfied.')
    reader.check_code(); action.check_code()
    return result


def ai_activation_generate():
    from tools.native_oracle import image_sha256, NATIVE_FPCW
    return dict(schema_version=1, native_sha256=image_sha256(), native_fpcw=f'0x{NATIVE_FPCW:X}',
        actions=ai_activation_action_controls(), histories=ai_activation_histories(), retail=ai_activation_retail())


def ai_activation_metadata():
    return provenance(
        scope='Original74/75 constructor/read/dispatch and shared House selector; connected ordinary House team-timer/selector/native RNG/eligibility on explicit state; physical ALL05 Country16/17 elapsed240 binding. No complete campaign AI, new-Team lifecycle or whole-map outcome claim.',
        assumptions=[
            'The existing ActionFixture image owns constructor6DD000, reader6DD5B0, Execute6DD8B0, CRT/TLS/heap/INI and Scenario. TeamEmu attaches its existing storage/selector preparation to that same image; no result flag is transferred between VMs.',
            'Three supplied0x8000-byte House windows, Country0/7/0 (or explicit16/17/0), registered order, live retained House, Player@ slots, four independent latches and team-timer priors are explicit. Action controls compare every supplied House byte and all three complete RNG streams; successful Execute writes only+1F2, including idempotent stores. Neither null Trigger nor -1 selects a House; incoming House does not choose the target.',
            'House Update4F8A00..4F8B08 executes as a contiguous original block, ESI=House, EBP=0, zero incoming local stack and declared frame/start/unused-word/duration, game-mode, human/control, passive and difficulty1 priors. Other House Update phases and scheduling are excluded. The unused timer+4 becomes the actual zero local at4F8AE3, not a duration field.',
            'The native selector6F0AB0 and actual65C7E0 execute with three seed31 streams, except the explicitly named seed0 rejection control. Full initial/final states and each update Scenario state, ranged args/returns and every raw XOR word are retained. Main/MapGen remain byte-identical. A ranged request can consume multiple raw words; no supplied draw answers or reduction model is used.',
            'Existing selector preparation supplies one formed base-defense Team of a different type, cap8 (or explicit1), one nondefense trigger weight100 and one empty TaskForce. Positive74 runs real41E720 with multiplayer mode, enabled all-difficulty trigger, <all> owner, condition-1, no side/zone relation/Max and TechLevel0. This bounded skirmish control does not resolve campaign/factory/recruitment eligibility tracked under issue1508.',
            'Inactive selection still reaches the original cancellation traversal with an empty pick vector. No eligibility, weighted draw, eviction or Team creation request occurs; old Team bytes/count and independent House latches are checked unchanged. Timer rearm and its native ranged request remain live when ordinary outer gates pass.',
            'ALL05 evidence reads the physical mapsmd03.mix member and checkout retail RULESMD.INI with hashes, executes original Country registration/read, counted Event13, flags and scalar75 readers. Production AssetManager must separately match its source identity. No original whole map loading or complete layering is certified.',
            'VERA20K_CAMPAIGN_START_ASSETS must name an existing empty directory for the reused CampaignReader; no file in that directory is requested by the selected prefixes.'],
        substitutions=[
            'Inherited action/AI CRT, heap, cached INI, platform and fixture-memory transports. All selected original gameplay instructions remain unchanged.',
            'TeamType Create_Team6F09C0 is a recorded RET4 boundary returning0 after the actual original selector asks for it; no Team allocation/admission/member/mission result is supplied as parity. Existing AI team deletion callback remains the shared oracle boundary; covered rows assert it is never reached.'],
        entry_points=dict(action_constructor=0x6DD000, action_reader=0x6DD5B0,
            action_execute=0x6DD8B0, action74=0x6DF2D1, action75=0x6DF310,
            house_selector=0x6E45E0, house_team_block=0x4F8A00,
            team_selector=0x6F0AB0, eligibility=0x41E720,
            create_team_boundary=0x6F09C0, random_seed=0x65C6D0,
            random_ranged=0x65C7E0, country_registration=0x668C8B))


def chrono_reinforcement_retail():
    archive=(configured_gamemd().parent/'mapsmd03.mix').read_bytes()
    raw=mix(archive)[mix_hash('ALL07SMD.MAP')]
    values=sections(raw)
    definitions={}
    for identity in ('0964C97C','04E3AA8C'):
        team=values[identity]
        definitions[identity]=dict(team=team,task_force_id=team['TaskForce'],
            task_force=values[team['TaskForce']],script_id=team['Script'],script=values[team['Script']])
    routes={identity:dict(trigger=values['Triggers'][identity],events=values['Events'][identity],
        actions=values['Actions'][identity]) for identity in ('0964B13C','098DB96C')}
    return raw,dict(map='ALL07SMD.MAP',archive='mapsmd03.mix',
        archive_sha256=hashlib.sha256(archive).hexdigest(),map_sha256=hashlib.sha256(raw).hexdigest(),
        definitions=definitions,routes=routes,
        waypoints={key:values['Waypoints'][key] for key in ('26','27')},
        general=values.get('General',{}))


def chrono_reinforcement_generate(fixture=None):
    from tools.native_oracle import image_sha256
    from tools.spatial_oracle.team_creation import (reinforcement_fixture,execute_reinforcement_rows,
        initialize_chrono_unit,execute_chrono_reinforcement_rows)
    from tools.spatial_oracle.anytown_damage import anytown_resident
    raw,retail=chrono_reinforcement_retail()
    q=fixture or reinforcement_fixture(unit_geometry=True)
    assert 'tables' in getattr(q,'unit_geometry_startup',{}),'Chrono fixture requires original Foot startup before baseline placement'
    if not hasattr(q,'orca_type'):execute_reinforcement_rows(q,[])
    theater=q.resident.theater
    anytown_resident.Resident.ensure_tiles(q.resident,theater,
        {row['tile'] for row in q.resident.case['supplied_cells'] if row['tile']!=0xFFFF},uc=q.u)
    initialize_chrono_unit(q,'AMCV',raw)
    geometry_prior={f'{address:08X}':q.m.read32(address) for address in
        (0xA8F240,0xA8F234,0xAC13C8,0xAC13BC,0xB0EC38,0xB1D0B8,0xB1D0AC)}
    cases=[dict(name='NULL_TEAM',entries=[],origin=[87,50],action='107,1,-1,0,0,0,0,G'),
        dict(name='NO_WAYPOINT',entries=[['MTNK',1]],origin=[87,50],waypoint_override=-1),
        dict(name='EMPTY_TASK_FORCE',entries=[],origin=[87,50]),
        dict(name='NEGATIVE_WAYPOINT',entries=[],origin=[87,50],waypoint_override=-2),
        dict(name='MINIMUM_WAYPOINT',entries=[],origin=[87,50],waypoint_override=-2147483648),
        dict(name='04E3AA8C',entries=[['MTNK',1]],origin=[87,50],script_steps=[(11,0)],
            retail_team='04E3AA8C',retail_action='107,1,04E3AA8C,0,0,0,0,AB'),
        dict(name='0964C97C',entries=[['AMCV',1]],origin=[87,50],script_steps=[(11,0)],
            retail_team='0964C97C',retail_action='107,1,0964C97C,0,0,0,0,AA'),
        dict(name='ORDER_TWO_UNITS',entries=[['MTNK',2]],origin=[87,50]),
        dict(name='TYPED_INFANTRY',entries=[['E1',1]],origin=[87,50]),
        dict(name='TYPED_AIRCRAFT',entries=[['ORCA',1]],origin=[87,50]),
        dict(name='PROCESS_UNIT',entries=[['MTNK',1]],origin=[87,50],first_process=True,
            process_frames=[1,2,3,4,63,64,65]),
        dict(name='PROCESS_UNIT_WITH_TARGET',entries=[['MTNK',1]],origin=[87,50],first_process=True,
            process_target=True,process_frames=[1,2,3,4,63,64,65])]
    rows=execute_chrono_reinforcement_rows(q,cases)
    m,u=q.m,q.u
    p=m.read32(m.read32(0xA8EC7C)+(m.read32(0xA8EC88)-1)*4)
    original_ammo=bytes(u.mem_read(p+0x2FC,4));cpu=u.context_save();ammo=[]
    try:
        for value in (-2147483648,-2,-1,0,1,2147483647):
            u.mem_write(p+0x2FC,dwords(value));u.reg_write(UC_X86_REG_EDI,p)
            run_checked(u,0x65EDC8,0x65EDCE,required_addresses=[0x65EDC8])
            ammo.append(dict(before=value,after=struct.unpack('<i',u.mem_read(p+0x2FC,4))[0]))
    finally:u.mem_write(p+0x2FC,original_ammo);u.context_restore(cpu)
    # Complete states are interned by content, keeping repeated unchanged
    # Main/MapGen streams compact without discarding their actual bytes.
    states={}
    def intern(streams):
        result={}
        for key,raw_state in streams.items():
            digest=hashlib.sha256(bytes.fromhex(raw_state)).hexdigest()
            states[digest]=raw_state;result[key]=digest
        return result
    for row in rows:
        for key in ('rng_before','rng_after'):row[key]=intern(row[key])
        if row['creation']:row['creation']['rng']=intern(row['creation']['rng'])
        for step in row.get('process',[]):step['rng']=intern(step['rng'])
    return dict(schema_version=1,native_sha256=image_sha256(),retail=retail,
        unit_geometry_startup=q.unit_geometry_startup,
        geometry_prior=geometry_prior,
        type_reads=q.chrono_unit_reads,rng_states=states,cases=rows,
        ammo_decrement=dict(entry='0065EDC8',end='0065EDCE',
            scope='Original DEC fragment with supplied prior values; field and CPU restored. Numeric overflow only, not whole107 boundary coverage.',rows=ammo),
        inherited_crop=dict(map_sha256=q.world['physical_map_sha256'],
            size=q.resident.case['size'],local_size=q.resident.case['local_size'],
            waypoint_cell=[88,49],waypoint_level=q.u.mem_read(q.resident.ptrs[88,49]+0x11B,1)[0],
            frame=1,game_active=1,scenario_started=0,
            rules_reader='0066D530',rules_layers=q.inputs['ai_rules_layers'],
            chrono_delay=struct.unpack('<i',q.u.mem_read(q.rules+0xBEC,4))[0],
            chrono_reinf_delay=struct.unpack('<i',q.u.mem_read(q.rules+0xBF0,4))[0],
            supplied_cells=q.resident.case['supplied_cells'],physical_tmp=q.resident.assets,
            game_mode=struct.unpack('<i',q.u.mem_read(0xA8B238,4))[0],
            unmodified_native_code_sha256=q.resident.code_hash))


def chrono_reinforcement_metadata():
    record=provenance(scope='Original107 reader/full Execute->65EC30->65DD30, physical Unit/Infantry/Aircraft construction and placement, original Teleport factory/piggyback/MoveTo/finalize and two bounded Process histories on the existing FootMissions crop.',
        assumptions=[
            'ActionFixture reader methods attach to the existing FootMissions allocator/image. No constructor or dispatch result crosses VMs. Resolved TeamType/ScriptType/TaskForce priors use the existing prepare_reinforcement_case owner; their full definition readers are excluded. The original TAction constructor, tokenizer, reference lookup and Read execute on those registered native Team identities.',
            'Physical ALL07SMD.MAP in mapsmd03.mix binds ChronoMCV Trigger0964B13C ->Team0964C97C/AMCV and ChronoSupport098DB96C ->Team04E3AA8C/MTNK. The gameplay controls transplant their destination to waypointG=(88,49) on the existing Anytown crop. Actual authored AA/AB geometry, full campaign startup and outcomes are excluded; actual authored records and map hash remain in the corpus.',
            'AMCV uses original7470D0/747620 on physical RULESMD, optional LANGRULE and ALL07 map in that order, with physical ARTMD. Existing physical MTNK/E1/ORCA readers, ordinary constructors, spare registries and partial Houses/crop come from team_creation/FootMissions. Each case retains genuine object IDs, constructor Ammo, native member and temporary chain order, FNPC arguments/results, actual Unlimbo, pending/timer/coordinate writes and three complete RNG streams interned by hash.',
            'NULL_TEAM, NO_WAYPOINT and EMPTY_TASK_FORCE are genuine original early returns. Native65EC30 dereferences a null created head before its late check and dereferences the null success head when every placement fails. Those undefined/fault paths are not converted into invented safe return values. Full transport/cargo paths and arbitrary all-blocked maps are excluded.',
            'Cases retain their genuinely constructed live actors and native registry/RNG history. Each preparation resets creation depth, recent-cell ring, player and Team definition/waypoint priors as declared by the existing reinforcement fixture. No post-case scalar deletion is inserted into107. Finalization clears temporary Object+30 links in reverse success order, Mark0 then pending3 then Commence. Two Process histories record all supplied frames, states, timers and RNG calls: the ordinary initially NULL target acquires existing hostile195 at timer expiry; a second control calls original6FCDB0 to set the genuine existing victim as target before Process (input.process_target_id). Other actors/TeamAI/global phases, device audio and rendering are excluded.',
            'The existing Resident.ensure_tiles owner supplies actual relocated TMP bytes for all55 non-sentinel tile types referenced by the180-cell crop, including newly reached tile140/clat09.tem. Historical default slots/payload remain unchanged. Extra bodies avoid the existing raw-zone planes; the supplied raw land/slope/zone priors still exclude whole native map initialization. No terrain/FNPC return is supplied.',
            'Existing unit_unlimbo.initialize_unit_geometry executes the original Infantry table813490 (the existing infantry_entry_raw.STARTUP), full Teleport table8150B8 and five Unit GEOMETRY_CALLS before the four baseline actors are constructed/placed. Native Unit B1D0AC and Infantry A8F234 become416; their level copies and Teleport B0EC38 become104. Full before/after field bytes and unchanged three-stream RNG are recorded. Aircraft inherits Object raw receivers5F60A0/5F6120: the existing FootMissions Object table8141D8 establishes AC13C8/AC13BC104/416 before the typed Aircraft case, and geometry_prior records them. Historical fixture payloads retain their omitted startup boundaries and are not silently regenerated. Earlier new107 zero-Unit and zero-Infantry diagnostics remain external and are superseded by this corpus.',
            'Six Ammo overflow controls execute only original65EDC8..65EDCE with EDI and the signed field prior supplied on a genuine actor. Original field bytes and CPU state are restored afterward. This proves the DEC fragment, including wrap, not every full107 arithmetic input.'],
        substitutions=[
            'Inherited FootMissions physical lexical INI caches, successful heap, asset/file, partial House and cropped-world transports remain explicit. Existing Mission.locomotor_com_transport now routes the actual Teleport CLSID at7E9A90 to original CreateInstance6C4C10, established by factory vtable7F3D38+0C and its718000 constructor call. Existing OleRun/Interlocked OS boundaries remain. No gameplay factory, FNPC, subcell, Unlimbo, COM QueryInterface/AddRef/BeginPiggyback, MoveTo, RNG, Mark, Commence or class cleanup return is supplied.'],
        entry_points=dict(action_read=0x6DD5B0,execute=0x6DD8B0,chrono_dispatch=0x6DF672,
            chrono_reinforce=0x65EC30,create_members=0x65DD30,fnpc=0x56DC20,subcell=0x481180,
            teleport_factory=0x6C4C10,teleport_ctor=0x718000,teleport_move=0x718100,
            process=0x7192F0,mark=0x4D3780,commence=0x5B3570,unit_ammo_init=0x7355A8,
            infantry_ammo_init=0x517D7A,ammo_decrement=0x65EDC8))
    record['command']='python -m tools.spatial_oracle.trigger_action_values --chrono-reinforcement --check'
    record['required_inputs']={'VERA20K_SHRAPNEL_INPUTS':'existing physical RULESMD/ARTMD/SOUNDMD/theater owner',
        'VERA20K_ANYTOWN_INPUTS':'existing physical Anytown map/shot assets plus all55 crop TMPs from retail ra2.mix/isotemp.mix'}
    return record


def tech_level_retail():
    """Physical MD layers and original727516..7275A5 counted Action reader."""
    root = configured_gamemd().parent
    archive_raw = (root/'mapsmd03.mix').read_bytes()
    archive = mix(archive_raw)
    patch_raw = (root/'expandmd01.mix').read_bytes()
    rules_raw = mix(patch_raw)[mix_hash('RULESMD.INI')]
    rules = sections(rules_raw)
    sources = [dict(file='RULESMD.INI', archive='expandmd01.mix',
        archive_sha256=hashlib.sha256(patch_raw).hexdigest(),
        sha256=hashlib.sha256(rules_raw).hexdigest())]
    routes, layers = [], []
    for filename, identity, names in (
        ('SOV03UMD.MAP','0D3F1D6C',('TTNK',)),
        ('SOV04DMD.MAP','068A958C',('NAIRON',)),
        ('SOV07TMD.MAP','093BB0FC',('TERROR','DTRUCK')),
        ('ALL05UMD.MAP','068397DC',('ORCA','BEAG'))):
        raw = archive[mix_hash(filename)]
        values = sections(raw)
        sources.append(dict(file=filename, archive='mapsmd03.mix',
            archive_sha256=hashlib.sha256(archive_raw).hexdigest(),
            sha256=hashlib.sha256(raw).hexdigest()))
        f = ActionFixture()
        f.call(0x7C8F5E, [])
        f.ordered_ini({'Actions': {identity: values['Actions'][identity]}})
        trigger = f.allocate(0xB4)
        f.u.mem_write(trigger, bytes(0xB4))
        f.u.mem_write(trigger+0x24, identity.encode()+b'\0')
        f.u.mem_write(SP+0x248, dwords(INI))
        f.u.reg_write(UC_X86_REG_ESP, SP)
        f.u.reg_write(UC_X86_REG_EBP, trigger)
        f.u.reg_write(UC_X86_REG_EBX, 0)
        run_checked(f.u, 0x727516, 0x7275A5)
        ptr, actions = f.read32(trigger+0xB0), []
        while ptr:
            actions.append(dict(kind=f.integer(ptr+0x2C),
                inline_type=f.string(ptr+0x54), value=f.integer(ptr+0x90),
                args=[f.integer(ptr+n) for n in (0x34,0x38,0x3C,0x40)],
                waypoint=f.integer(ptr+0x44)))
            ptr = f.read32(ptr+0x28)
        f.check_code()
        routes.append(dict(map=filename, trigger_id=identity,
            trigger=values['Triggers'][identity], events=values['Events'][identity],
            raw_actions=values['Actions'][identity], actions=actions,
            tags={key: body for key,body in values['Tags'].items()
                  if body.endswith(','+identity)}))
        for name in names:
            scalar = ActionFixture()
            scalar.call(0x7C8F5E, [])
            pointer = scalar.allocate(0x800)
            scalar.u.mem_write(pointer, bytes(0x800))
            scalar.u.mem_write(pointer+0x24, name.encode()+b'\0')
            scalar.u.reg_write(UC_X86_REG_ESI, pointer)
            run_checked(scalar.u, 0x711082, 0x71108C)
            constructor = scalar.integer(pointer+0x634)
            passes = []
            # Optional files are absent in the supported install. Keep that
            # absence explicit rather than substituting the RA2 base INIs.
            for label, entries in [('RULESMD.INI',rules),
                    ('LANGRULE.INI',None),(filename[:-4]+'.INI',None),
                    (filename,values)]:
                if entries is None:
                    assert not (root/label).exists(), label
                    passes.append(dict(layer=label, absent=True,
                        raw_key=None, value=scalar.integer(pointer+0x634)))
                    continue
                section = entries.get(name,{})
                scalar.ordered_ini({name: section})
                scalar.u.reg_write(UC_X86_REG_ESP, SP)
                scalar.u.reg_write(UC_X86_REG_EBP, pointer)
                scalar.u.reg_write(UC_X86_REG_EBX, pointer+0x24)
                scalar.u.reg_write(UC_X86_REG_ESI, INI)
                run_checked(scalar.u, 0x714570, 0x71458A,
                    required_addresses=(0x5276D0,0x714584))
                passes.append(dict(layer=label, absent=False,
                    raw_key=section.get('TechLevel'),
                    value=scalar.integer(pointer+0x634)))
            scalar.check_code()
            layers.append(dict(map=filename,type=name,constructor=constructor,passes=passes))
    return dict(sources=sources,routes=routes), layers


def tech_level_dispatch_controls():
    """Original106 on declared global Type/House registration priors."""
    specs = [
        ('retail_ttnk','TTNK',1,[('TTNK','Vehicle',12),('NAIRON','Building',11)],2),
        ('case_miss','ttnk',1,[('TTNK','Vehicle',12)],2),
        ('missing_type','MISSING',1,[('TTNK','Vehicle',12)],2),
        ('reverse_duplicate','TTNK',1,[('TTNK','Vehicle',12),('NAIRON','Building',11),('TTNK','Aircraft',9)],2),
        ('negative','TTNK',-2,[('TTNK','Vehicle',12)],2),
        ('i32_min','TTNK',-2147483648,[('TTNK','Vehicle',12)],2),
        ('i32_max','TTNK',2147483647,[('TTNK','Vehicle',12)],2),
        ('no_houses','TTNK',1,[('TTNK','Vehicle',12)],0),
        ('empty_registry','TTNK',1,[],2)]
    output = []
    for name, target, value, types, house_count in specs:
        f = ActionFixture()
        f.call(0x7C8F5E, [])
        proxy = SimpleNamespace(u=f.u,scenario=SCENARIO,
            invoke=lambda pc,this,args: f.call(pc,list(args),ecx=this))
        CampaignReader.seed_streams(proxy,31)
        pointers, houses, writes, calls = [], [], [], []
        for identity, category, prior in types:
            pointer = f.allocate(0x700)
            f.u.mem_write(pointer,bytes(0x700))
            f.u.mem_write(pointer+0x24,identity.encode()+b'\0')
            f.u.mem_write(pointer+0x634,dwords(prior))
            pointers.append(pointer)
        for _ in range(house_count):
            pointer=f.allocate(0x200)
            f.u.mem_write(pointer,bytes(0x200))
            houses.append(pointer)
        for slot,items in ((0xA8EB04,pointers),(0xA8022C,houses)):
            storage=f.allocate(max(4,4*len(items)))
            f.u.mem_write(storage,dwords(*items) if items else dwords(0))
            f.u.mem_write(slot,dwords(storage))
            f.u.mem_write(slot+12,dwords(len(items)))
        def observe_write(u,_access,address,size,stored,_data):
            for owner,items,offset in (('type',pointers,0x634),('house',houses,0x1FC)):
                if address in [p+offset for p in items]:
                    signed=struct.unpack('<i',dwords(stored))[0] if owner=='type' else stored
                    writes.append(dict(owner=owner,index=items.index(address-offset),size=size,value=signed))
        def observe_rng(u,pc,_size,_data):
            if pc in (0x65C780,0x65C7E0):
                calls.append(f'{pc:08X}')
        raw=f'106,9,{target},0,0,0,0,{value}'
        read=f.read_action(raw)
        before=f.rng_hashes()
        f.u.hook_add(UC_HOOK_MEM_WRITE,observe_write)
        f.u.hook_add(UC_HOOK_CODE,observe_rng)
        returned=f.call(0x6DD8B0,[0,0,0,0],ecx=ACTION)&255
        f.check_code()
        output.append(dict(name=name,input=dict(raw_action=raw,
            types=[dict(id=i,category=c,tech_level=v) for i,c,v in types],
            house_dirty=[False]*house_count,incoming_house=None),
            read=dict(kind=read['kind'],value=read['value'],inline_type=f.string(ACTION+0x54)),
            output=dict(returned=returned,tech_levels=[f.integer(p+0x634) for p in pointers],
                house_dirty=[bool(f.u.mem_read(p+0x1FC,1)[0]) for p in houses],writes=writes),
            rng_seed=31,rng_before=before,rng_after=f.rng_hashes(),rng_calls=calls))
    return output


def tech_level_lifecycle():
    """Compose existing reset owner; observe live prefix and Type retirement."""
    from tools.rules_oracle.select_anim_reset import fresh_reset
    from tools.native_inspect import selected_ranges
    from tools.native_oracle import image_bytes
    m,rules=fresh_reset()
    u=m.u
    original=selected_ranges(image_bytes(),None,None,code_only=True)
    for start,end in ((0x4E7A60,0x4E7A96),(0x4E73E0,0x4E7416)):
        u.reg_write(UC_X86_REG_ESP,SP)
        run_checked(u,start,end)
    types={name:m.invoke(0x41CEF0,m.cstring(name)) for name in ('ORCA','BEAG')}
    def state(name):
        items=m.read32(0xA8EB04)
        return dict(name=name,global_count=m.read32(0xA8EB10),
            aircraft_count=m.read32(0xA8B228),
            types=[dict(id=m.string(m.read32(items+i*4)+0x24),category='Aircraft',
                tech_level=struct.unpack('<i',u.mem_read(m.read32(items+i*4)+0x634,4))[0])
                for i in range(m.read32(0xA8EB10))])
    stages=[state('constructors')]
    u.mem_write(0xA80238,dwords(0))
    action=m.alloc(0x94)
    u.mem_write(action,bytes(0x94))
    u.mem_write(action+0x2C,dwords(106))
    raw_actions=['106,9,ORCA,0,0,0,0,11','106,9,BEAG,0,0,0,0,3']
    for name,value in (('ORCA',11),('BEAG',3)):
        u.mem_write(action+0x54,bytes(25))
        u.mem_write(action+0x54,name.encode()+b'\0')
        u.mem_write(action+0x90,dwords(value))
        assert m.invoke(0x6DD8B0,action,(0,0,0,0))&255==1
    stages.append(state('actions'))
    passes=[dict(name='prefix_missing_key',sections={'ORCA':{'Name':'ORCA'},'BEAG':{'Name':'BEAG'}}),
        dict(name='prefix_present_key',sections={'ORCA':{'TechLevel':'7'},'BEAG':{'TechLevel':'8'}})]
    for row in passes:
        m.rules_cache(row['sections'])
        m.invoke(0x668BF0,rules,(RULES,))
        stages.append(state(row['name']))
    m.rules_cache({})
    # ResetReader observes free before its existing transport rewrites ESP.
    # A second later code hook would see the returned stack, not the argument.
    m.retiring={ptr:dict(name=name,pointer=ptr,family='Aircraft') for name,ptr in types.items()}
    m.record_reset=True
    u.reg_write(UC_X86_REG_ESP,SP)
    u.reg_write(UC_X86_REG_ECX,rules)
    u.mem_write(SP+4,dwords(RULES))
    run_checked(u,0x6686C0,0x6689E7,count=2_000_000)
    m.record_reset=False
    events=[dict(event='delete_type',id=row['name']) for row in m.reset_events
            if row['event']=='delete_type']
    assert sorted(row['id'] for row in events)==sorted(types)
    stages.append(state('destructive_reset'))
    for name in ('ORCA','BEAG'):
        m.invoke(0x41CEF0,m.cstring(name))
    stages.append(state('new_constructors'))
    assert all(bytes(u.mem_read(x['address'],len(x['bytes'])))==x['bytes'] for x in original)
    return dict(input=dict(raw_actions=raw_actions,prefix_passes=passes),
        stages=stages,events=events,code_unchanged=True)


def tech_level_generate(root, *, production=True):
    from tools.native_oracle import image_sha256
    retail,layers=tech_level_retail()
    return dict(schema=1,native_sha256=image_sha256(),retail=retail,
        dispatch_controls=tech_level_dispatch_controls(),scalar_layers=layers,
        lifecycle=tech_level_lifecycle(),
        production_history=tech_level_production_history(root) if production else None)


def tech_level_production_history(root):
    from tools.sidebar_oracle.factory_loss import aircraft_tech_setup,aircraft_tech_history
    retail,_=tech_level_retail()
    row=next(row for row in retail['routes'] if row['map']=='ALL05UMD.MAP')
    for source in retail['sources']:
        if source['file'] in ('RULESMD.INI',row['map']):
            assert hashlib.sha256((root/source['file']).read_bytes()).hexdigest()==source['sha256']
    # Preserve the actual counted reader's order. The subsequent Action53
    # enables another trigger and is outside this production common path.
    actions=[a for a in row['actions'] if a['kind']==106]
    tokens=row['raw_actions'].split(',')
    assert int(tokens[0])==len(row['actions']) and len(tokens)==1+8*len(row['actions'])
    raw=[','.join(tokens[1+i*8:1+(i+1)*8]) for i,a in enumerate(row['actions']) if a['kind']==106]
    return aircraft_tech_history(aircraft_tech_setup(root),raw,actions)


def tech_level_metadata(root):
    record = provenance(scope='Original106 exact global lookup/write/all-House dirty, selected '
        'physical MD counted records/scalar layers, Type prefix/reset and bounded ALL05 paid '
        'Aircraft production/Strip/event history. No full campaign or Event55 replay.',
        assumptions=[
            'Dispatch controls supply minimal named Type shells in explicit native global registration order and House dirty fields, not full constructors. Same-name reverse control uses distinct Vehicle/Aircraft identities.',
            'Selected four maps come from physical mapsmd03.mix and patched RULESMD from expandmd01.mix. Real727516..7275A5 counted reader and714570..71458A scalar reader execute; optional layers absent in supported installation. No RA2 base INI is used.',
            'Lifecycle reuses select_anim_reset.fresh_reset, actual Type vector CRT/constructor/destructor, full Process on declared two-section caches and reset before first post-reset Process. Prefix7/8 are custom inputs, not ALL05 named INI. New objects and deleted objects are distinct lifetimes.',
            'Three RNG seed31 states use existing trigger ActionFixture SHA256 format. Leaf observations do not assert downstream recheck has no RNG/timer/detach effects.',
            'ALL05 production reuses ReadyBuildingFixture with explicit RULESMD/optionalLANGRULE/ALL05 layers; Battle/Hills gameplay reads are excluded. Hills provides only the existing flat terrain crop. Local Country is Player, not its ParentCountry Americans.',
            'Healthy completed GACNST/GAPILE/GAPOWR/GAAIRC and already-open sidebar, TechLevel10 and credits10000 are declared played-world priors. Aircraft real constructors/CRT, actual type readers, General production/prerequisite readers, original paid Factory AI,106, House dirty suffix, wholeStrip and same-frame receive execute.',
            'Original Building health sampler440042..440074 and House power consumer4F84D9..4F84F1 establish the four completed buildings power cache before production. Native input geometry is the existing16x16 map/32x32 coordinate crop/33x33 class-height plane. Only the admitted local production House is registered in this bounded fixture; inherited unrelated actors do not run AI.',
            'BeginProduction starts at the recorded initial_frame. Each payment visit increments FRAME before FactoryAI; no frame is rewritten to fit a refund. rng_seed_states imports the three complete native states after fixture setup without reseeding, so comparisons bound the production segment rather than full constructor chronology.',
            'All three raw Factory timer dwords and writes are observed. Canonical writes name the actual Factory and relative offsets; its unused running middle word can retain native stack bytes. Rust CdTimer comparisons cover start/duration, not a claim about that raw middle word.',
            'Native type9/count-reader materialization is copied into the joined runtime Action input shell before realExecute. This is an explicit parser-to-runtime input boundary, not a replacement gameplay decision. Event55 and original full-map/GUI startup remain excluded.',
        ],substitutions=[
            'Existing original-reader lexical cache, allocation/free, CRT/TLS/Windows and joined physical asset/audio service boundaries are reused. No admission/cancel/refund/timer answer is supplied. Lifecycle free is observed without heap storage reuse.',
            'Joined Mission COM transport forwards Fly activation to the original6C4A90 factory. Reused type visual-asset boundaries do not certify rendered aircraft/assets or mounted theater/archive precedence. Audio/window/device inputs remain declared existing transports.',
        ],entry_points=dict(action_reader=0x6DD5B0,action_execute=0x6DD8B0,
            action106=0x6DF5D1,tech_level_reader=0x714570,type_reset=0x6686C0,
            house_tail=0x4F9265,can_build=0x4F7870,strip_recalculate=0x6AA600,
            building_health_sample=0x440042,house_power_consumer=0x4F84D9,
            begin_production=0x4FA350,factory_ai=0x4C9B20,
            held_aircraft_delete=0x41C210,factory_abandon=0x4C9FF0,
            event_receive=0x6474C7))
    record['command']='python -m tools.spatial_oracle.trigger_action_values --tech-level --check'
    record['required_inputs']={'VERA20K_TRIGGER_TECH_INPUTS':'existing joined retail asset cache plus physical ALL05UMD.MAP; --inputs is equivalent'}
    receipt=root/'extraction-receipt.json'
    record['physical_input_sources']=json.loads(receipt.read_text(encoding='utf-8')) if receipt.exists() else {
        'files':[dict(file=p.name,sha256=hashlib.sha256(p.read_bytes()).hexdigest())
                 for p in sorted(root.iterdir()) if p.is_file() and p.suffix.lower()!='.json']}
    for row in record['physical_input_sources']['files']:
        assert hashlib.sha256((root/row['file']).read_bytes()).hexdigest()==row['sha256']
    for name in record['physical_input_sources'].get('missing_candidate_names',[]):
        assert not (root/name).exists(),name
    return record

def explosion_weapon_fields(m):
    """Real constructor/current-default read controls, in the retained registry."""
    name='Action42FieldControl'
    count=len(m.weapon_registry())
    pointer=m.invoke(0x772FA0,m.cstring(name));assert pointer
    rows=[];pending={};active=[]
    def state():
        wh=m.read32(pointer+0xAC)
        return dict(damage=struct.unpack('<i',m.u.mem_read(pointer+0xA4,4))[0],
                    warhead=m.string(wh+0x24) if wh else None)
    def observe(u,pc,size,data):
        sp=u.reg_read(UC_X86_REG_ESP)
        if (pc,sp) in pending:
            row=pending.pop((pc,sp));row['returned']=struct.unpack('<i',dwords(u.reg_read(UC_X86_REG_EAX)))[0]
            if row['key']=='Warhead':row['read_string']=m.string(row['output_address'])
        if pc not in (0x5276D0,0x528A10):return
        section,key=m.string(m.read32(sp+4)),m.string(m.read32(sp+8))
        if section!=name or key not in ('Damage','Warhead') or (key=='Damage')!=(pc==0x5276D0):return
        row=dict(entry=f'{pc:08X}',return_pc=f'{m.read32(sp):08X}',section=section,key=key)
        if key=='Damage':row['default']=struct.unpack('<i',dwords(m.read32(sp+12)))[0];cleanup=16
        else:
            row.update(default=m.string(m.read32(sp+12)),capacity=m.read32(sp+20),output_address=m.read32(sp+16));cleanup=24
        active.append(row);pending[m.read32(sp),sp+cleanup]=row
    hook=m.u.hook_add(UC_HOOK_CODE,observe);before_rng=m.rng_state();constructor=state()
    try:
        for label,keys in (
            ('missing_section',None),('set_both',{'Damage':'37','Warhead':'AP'}),
            ('missing_keys',{'Range':'1'}),('empty_keys',{'Damage':'','Warhead':''}),
            ('signed_and_none',{'Damage':'-17','Warhead':'none'}),
            ('missing_after_clear',{'Range':'1'}),('case_lookup',{'Damage':'90','Warhead':'ap'})):
            m.ordered_ini({name:keys} if keys is not None else {'Unrelated':{'Marker':'1'}})
            active.clear();before=state();returned=m.invoke(0x772080,pointer,(INI,))&255
            assert not pending,pending
            rows.append(dict(name=label,keys=keys,before=before,returned=returned,after=state(),
                reads=[{k:v for k,v in row.items() if k!='output_address'} for row in active]))
    finally:m.u.hook_del(hook)
    m.check_code()
    return dict(constructor=constructor,rows=rows,registry_count_before=count,
        rng_before=before_rng,rng_after=m.rng_state(),
        scope='One real appended Weapon; complete original771C70/772080 on ordered cache controls. These custom controls exclude physical loading and full Rules Process.')


def do_explosion_generate(*,receipt_path=None):
    """Two independent physical map Process branches from one full root state."""
    from tools.rules_oracle.weapon_speed_order import RetailRulesInputs,retail_history
    from tools.native_oracle import image_sha256
    inputs=RetailRulesInputs(configured_gamemd().parent);m=retail_history(inputs);failure=None
    try:
        root=m.campaign_root(('ALL06UMD.MAP','SOV03UMD.MAP'))
        common=m.snapshot();maps=[]
        for name,trigger in (('ALL06UMD.MAP','048BB6BC'),('SOV03UMD.MAP','0B3BE93C')):
            m.restore(common);registry=m.map_process(root['pointers'][name],name)
            raw,_=m.string_read(root['pointers'][name],'Actions',trigger)
            # The authored Action42 record, from the physical map INI bytes.
            tokens=raw.split(',');records=[tokens[1+i*8:9+i*8] for i in range(int(tokens[0]))]
            selected=[r for r in records if r[0]=='42'];assert len(selected)==1
            maps.append(dict(map=name,weapon_registry=registry,trigger_id=trigger,
                raw_action=','.join(selected[0]),selected_weapon_index=int(selected[0][2])))
        fields=explosion_weapon_fields(m)
        for key in ('rng_before','rng_after'):fields.pop(key)
        # Keep the consumed INI/map identities; every other physical request
        # (art SHPs, absent theater variants) is pinned by count and digest.
        receipt=inputs.receipt();requests=receipt.pop('requests')
        receipt.update(inis=[row for row in requests if row['file'].upper().endswith(('.INI','.MAP'))],
            request_count=len(requests),absent_count=sum(row['absent'] for row in requests),
            requests_sha256=hashlib.sha256(json.dumps(requests,sort_keys=True).encode()).hexdigest())
        return dict(schema_version=1,native_sha256=image_sha256(),sources=receipt,
            startup=dict(arena_bytes=0xC000000,physical=root['physical'],initializers=root['initializers'],
                optional_pass_guards=root['optional'],initialization_depth=root['initialization_depth'],
                theater=root['theater'],root_weapons=root['weapons']),
            maps=maps,weapon_field_controls=fields)
    except Exception as error:
        failure=dict(type=type(error).__name__,message=str(error),phase=m.phase,
            pc=f'{m.u.reg_read(UC_X86_REG_EIP):08X}',sp=f'{m.u.reg_read(UC_X86_REG_ESP):08X}')
        raise
    finally:
        if receipt_path:
            receipt=dict(status='stopped' if failure else 'returned',failure=failure,
                registry_events=m.registry_events,phase_visits=m.phase_visits,
                budgets=m.budgets,phase_seconds=m.phase_seconds,native_events=m.native_events,
                arena_used=m.cursor-0x24000000,arena_bytes=0xC000000)
            Path(receipt_path).write_text(json.dumps(receipt,indent=2)+'\n',encoding='utf-8')
        m.close()


def do_explosion_source_paths():
    return dict(SOURCE_PATHS,**{name:Path(__file__).parents[2]/path for name,path in {
        'native_full_registry':'tools/rules_oracle/weapon_speed_order.py',
        'single_arena':'tools/rules_oracle/bridge_anim_lists.py',
        'bullet_reader':'tools/projectile_oracle/bridge_render_inputs.py',
        'native_platform':'tools/spatial_oracle/engineer_repair_admission.py',
        'palette_platform':'tools/projectile_oracle/bridge_render_inputs_palette.py',
    }.items()})


def do_explosion_metadata():
    return provenance(scope='Physical RULESMD/ARTMD plus independent ALL06/SOV03 full native type histories: the retained WeaponType array order, Damage and Warhead that TAction42 (6E2390) indexes, and Weapon constructor/ReadINI Damage/Warhead controls.',
        assumptions=[
            '535B30/4739F0/4741F0/525A60 parse actual source-selected bytes. Physical maps are loaded before a common full original Rules root history, then each original687744..687754 map Process runs from an in-memory CPU/RAM/transport snapshot. Optional LANGRULE and campaign basename INI guards execute473C50 and must report absent; a present file stops until its real pass is supported.',
            'Original constructors/static type initializers, cold readers, init-depth kernels, media-2 (6BD7CB passes ECX), palette/Convert bodies, map Theater read and InitTheater5349C0, and destructive Reset6686C0/full Process668BF0 execute. The 192MiB successful heap is one existing Lists allocator, not native malloc-failure equivalence. Physical INI checkDigest0 is explicit; whole campaign digest/UI startup and theater tile/World loading are excluded.',
            'The authored 42 record is read from each physical map [Actions] entry; its third field is the Weapon ordinal Execute42 passes to 6E2390. Action42 body behavior (coordinate, SelectAnim/Anim, Flash, AreaDamage) is outside this corpus and rests on instruction reading.',
            'Weapon constructor771C9D/771CA9 initializes Damage0/WarheadNULL. Original772080 reads exact Damage at7722D4..7722E8 using5276D0 with CURRENT Damage; Warhead at77294C..772981 uses528A10/current binding, nonempty75E3B0. Missing and empty controls are executable, not inferred: empty Damage reads0; empty Warhead retains; none clears; case-insensitive lookup preserves stored identity.',
        ],substitutions=[
            'Existing CampaignReader/Reader/Lists owns unchanged PE, INI caches, CRT/heap/GUID/clock/atomic transport. Existing NativeAudioPlatform owns RawFile bytes/Win32 and wsprintfA ASCII one-%s/%d formatting for InitTheater and direct CALL [7E14B0] sites. RetailRulesInputs selects actual archive bytes, records absent 8.3 names, and rejects unproven byte conflicts; it supplies only IO bytes/absence, not gameplay read/constructor/parser results. Existing RGB565 opaque surface/getter and palette transport remain explicit. No instruction patches, native RNG/timer calculations or gameplay-return stubs.',
        ],entry_points=dict(physical_ini_ctor=0x535B30,file_ctor=0x4739F0,physical_load=0x4741F0,physical_parse=0x525A60,
            init_theater=0x5349C0,rules_reset=0x6686C0,rules_process=0x668BF0,weapon_ctor=0x771C70,weapon_read=0x772080,
            weapon_find_or_allocate=0x772FA0,map_process=0x687744,action42=0x6E2390))

def _map_lighting_retail():
    root = native.configured_gamemd().parent
    matches = {}
    candidates = []
    for name in ('expandmd01.mix', 'ra2md.mix', 'ra2.mix'):
        path = root / name
        raw = path.read_bytes()
        entries = mix(raw)
        candidates.append((name, raw, entries))
        for nested in ('multimd.mix', 'multi.mix', 'localmd.mix', 'local.mix'):
            val = entries.get(mix_hash(nested))
            if val is not None:
                candidates.append((name + ' -> ' + nested, val, mix(val)))
    for name in ('TRANSYLV.YRO', 'RULESMD.INI', 'MPBATTLEMD.INI', 'ARTMD.INI'):
        if (root / name).is_file():
            raw = (root / name).read_bytes()
            matches.setdefault(name, []).append(dict(archive='retail loose file', sha256=hashlib.sha256(raw).hexdigest(), bytes=len(raw), sections=sections(raw)))
        for archive, archive_raw, entries in candidates:
            raw = entries.get(mix_hash(name))
            if raw is not None:
                matches.setdefault(name, []).append(dict(archive=archive, archive_sha256=hashlib.sha256(archive_raw).hexdigest(), sha256=hashlib.sha256(raw).hexdigest(), bytes=len(raw), sections=sections(raw)))
    for val in matches.get('TRANSYLV.YRO', []):
        s = val.pop('sections')
        selected = {}
        for key, raw in s.get('Actions', {}).items():
            cells = raw.split(',')
            n = int(cells[0])
            assert len(cells) == 1 + n * 8
            actions = [','.join(cells[1 + i * 8:9 + i * 8]) for i in range(n)]
            if any((int(a.split(',')[0]) in (66, 73) for a in actions)):
                selected[key] = dict(raw=raw, actions=actions, trigger=s.get('Triggers', {}).get(key), events=s.get('Events', {}).get(key))
        val['selected'] = selected
        val['Lighting'] = s.get('Lighting', {})
        val['Waypoints'] = s.get('Waypoints', {})
        val['Basic'] = s.get('Basic', {})
        val['CombatDamage'] = s.get('CombatDamage', {})
        val['Super'] = s.get('Super', {})
        val['General'] = s.get('General', {})
        val['AudioVisual'] = s.get('AudioVisual', {})
        val['Triggers'] = s.get('Triggers', {})
        val['Events'] = s.get('Events', {})
        val['Actions'] = s.get('Actions', {})
    for val in matches.get('RULESMD.INI', []):
        s = val.pop('sections')
        val['CombatDamage'] = {k: v for k, v in s.get('CombatDamage', {}).items() if k == 'C4Warhead'}
        val['Super'] = s.get('Super', {})
        val['General'] = {k: v for k, v in s.get('General', {}).items() if k in ('AmbientChangeRate', 'AmbientChangeStep')}
        val['AudioVisual'] = {k: v for k, v in s.get('AudioVisual', {}).items() if k in ('AmbientChangeRate', 'AmbientChangeStep')}
    for name in ('MPBATTLEMD.INI', 'ARTMD.INI'):
        for val in matches.get(name, []):
            val.pop('sections')
    return matches

def _map_lighting_ambient_controls():
    rows = []
    for value in (110, 75, 0, -1, 100, 2147483647, -2147483648):
        for storm in (False, True):
            for nuke, chrono, dominator in ((0, 0, 0), (1, 0, 0), (0, 1, 0), (0, 0, 2)):
                f = ActionFixture()
                u = f.u
                definition = f.read_action(f'73,0,{value},0,0,0,0,A')
                u.mem_write(SCENARIO + 13608, dwords(87, 87, 43))
                u.mem_write(SCENARIO + 13620, dwords(90, 90, 100, 50, 8))
                u.mem_write(SCENARIO + 4680, dwords(12, 2779096485, 8))
                u.mem_write(11139764, bytes([storm]))
                u.mem_write(11139772, dwords(nuke))
                u.mem_write(11139760, dwords(chrono))
                u.mem_write(11139776, dwords(dominator))
                before = f.rng_hashes()
                calls = []

                def observe(uc, pc, _sz, _data):
                    if pc in (5480704, 5489280, 5483776, 4908224, 6670032, 6670208, 6670304):
                        calls.append(f'{pc:08X}')
                h = u.hook_add(UC_HOOK_CODE, observe)
                result = f.call(7198896, [0, 0, 0, 0], ecx=ACTION) & 255
                u.hook_del(h)
                f.check_code()
                rows.append(dict(input=dict(value=value, storm=storm, nuke=nuke, chrono=chrono, dominator=dominator, current=87, target=43, normal=87, timer=[12, 8]), definition=definition, returned=result, after=dict(normal=f.integer(SCENARIO + 13608), current=f.integer(SCENARIO + 13612), target=f.integer(SCENARIO + 13616), normal_rgb_ground_level=list(struct.unpack('<5i', u.mem_read(SCENARIO + 13620, 20))), timer=[f.integer(SCENARIO + 4680), f.integer(SCENARIO + 4688)]), calls=calls, rng_before=before, rng_after=f.rng_hashes()))
    return dict(native_sha256=native.image_sha256(), cases=rows, scope='Original action constructor/token reader/Execute73; supplied scenario/global priors; no consumer timing or render output claim')

def _map_lighting_flash_controls():
    f = ActionFixture()
    u = f.u
    constants = initialize_waypoint_map(f)
    initializers = list(struct.unpack('<15I', u.mem_read(8472744, 60)))
    for entry in initializers:
        f.call(entry, [])
    constants['scenario_waypoint_bridge_height'] = f.integer(11556028)
    f.call(4701168, [], ecx=f.cell)
    f.call(4701168, [], ecx=11263056)
    u.mem_write(8942784, dwords(8304340, f.allocate(4096), 1024, 1, 0, 10))
    f.call(8163166, [])
    u.mem_write(8942048, dwords(RULES))
    f.call(6706768, [], ecx=RULES)
    root = native.configured_gamemd().parent
    raw_rules = mix((root / 'expandmd01.mix').read_bytes())[mix_hash('RULESMD.INI')]
    physical = sections(raw_rules)
    f.ordered_ini({name: physical[name] for name in ('CombatDamage', 'Super')})
    name = f.allocate(8)
    u.mem_write(name, b'Super\x00')
    wh = f.call(7726000, [], ecx=name)
    ctor = dict(bright=u.mem_read(wh + 336, 1)[0], size_f32_le_hex=bytes(u.mem_read(wh + 316, 4)).hex())
    answered = f.call(7721888, [INI], ecx=wh) & 255
    from unicorn.x86_const import UC_X86_REG_ESI, UC_X86_REG_EDI
    u.reg_write(UC_X86_REG_ESP, 269475840)
    u.reg_write(UC_X86_REG_ESI, RULES)
    u.reg_write(UC_X86_REG_EDI, INI)
    native.run_checked(u, 6734596, 6734668, count=100000)
    assert f.read32(RULES + 4008) == wh
    inputs = dict(rules_sha256=hashlib.sha256(raw_rules).hexdigest(), read_returned=answered, ctor=ctor, after=dict(bright=u.mem_read(wh + 336, 1)[0], size_f32_le_hex=bytes(u.mem_read(wh + 316, 4)).hex()), c4=f.string(f.read32(RULES + 4008) + 36))
    rows = []
    for kind in (64, 65, 66):
        for case in (dict(name='transylv_EA_flat_crop', cell=[68, 77]), dict(name='raised_slope', cell=[68, 77], level=3, slope=1), dict(name='bridge100', cell=[68, 77], level=3, slope=1, flags=256), dict(name='bridge400', cell=[68, 77], flags=1024), dict(name='scalar_not_tail', cell=[68, 77], scalar=131)):
            install_waypoint_cell(f, case)
            scalar = case.get('scalar', 130)
            definition = f.read_action(f'{kind},0,{scalar},0,0,0,0,EA')
            u.mem_write(SCENARIO + 1586 + 130 * 4, struct.pack('<hh', 12, 11))
            u.mem_write(SCENARIO + 1586 + scalar * 4, struct.pack('<hh', *case['cell']))
            u.mem_write(11070328, dwords(2))
            u.mem_write(11259204, dwords(60))
            u.mem_write(11259216, b'\x00')
            u.mem_write(8560628, dwords(15, 5))
            calls = []

            def observe(uc, pc, _sz, _data):
                sp = uc.reg_read(UC_X86_REG_ESP)
                if pc == 4761120:
                    calls.append(dict(call='Flash', damage=f.integer_reg(UC_X86_REG_ECX), warhead=f.string(uc.reg_read(UC_X86_REG_EDX) + 36), arguments=list(struct.unpack('<5i', uc.mem_read(sp + 4, 20)))))
                elif pc == 6287952:
                    raise AssertionError('Unexpected retail Super bright light constructor')
            h = u.hook_add(UC_HOOK_CODE, observe)
            before = f.rng_hashes()
            result = f.call(7198896, [0, 0, 0, 0], ecx=ACTION) & 255
            u.hook_del(h)
            f.check_code()
            rows.append(dict(input=case, definition=definition, returned=result, calls=calls, light_constructor_calls=0, rng_before=before, rng_after=f.rng_hashes()))
    # Flash queries the shared detail latch before its Bright test, including
    # stock calls which create no light. Keep this original wrapper/55AF60 in
    # the same physical C4 fixture; only the downstream Light constructor is
    # a recorded return boundary in these explicit presentation-state controls.
    original_code = {
        start: native.file_span(native.image_bytes(), start, length)[1]
        for start, length in ((0x48A620, 0xE0), (0x55AF60, 0x50))
    }
    assert all(bytes(u.mem_read(start, len(raw))) == raw for start, raw in original_code.items())
    base = dict(detail=2, fps=60, minimum=15, buffer=5, reduced=0,
                bright=1, force=0, flags=0, damage=300)
    creation_inputs = [
        dict(base, name='detail0_cannot_force', detail=0, force=1),
        dict(base, name='detail0_flag_but_not_bright', detail=0, flags=1, bright=0),
        dict(base, name='detail0_flag_force', detail=0, fps=0, flags=1, force=1, bright=0),
        dict(base, name='detail0_high_flag_no_bypass', detail=0, flags=16, force=1),
        dict(base, name='lowfps_cannot_force', fps=10, force=1),
        dict(base, name='lowfps_flag_force', fps=10, flags=1, force=1, bright=0),
        dict(base, name='lowfps_flag_but_not_bright', fps=10, flags=1, bright=0),
        dict(base, name='lowfps_high_flag_no_bypass', fps=10, flags=16, force=1),
        dict(base, name='fps_equal_minimum', fps=15),
        dict(base, name='stock_no_bright_normalfps', bright=0),
        dict(base, name='hysteresis_band_reduced', fps=17, reduced=1),
        dict(base, name='hysteresis_band_not_reduced', fps=17),
        dict(base, name='hysteresis_recovery_equal', fps=20, reduced=1),
    ]
    history_input = dict(base, name='stock_no_bright_lowfps_then_sample', fps=10, bright=0)
    creation_admission = []
    hysteresis_history = None
    admission_rng_before = f.rng_hashes()
    for case in [*creation_inputs, history_input]:
        u.mem_write(0xA8EB78, dwords(case['detail']))
        u.mem_write(0xABCD44, dwords(case['fps']))
        u.mem_write(0x829FF4, dwords(case['minimum'], case['buffer']))
        u.mem_write(0xABCD50, bytes([case['reduced']]))
        # Declared Bright branch prior on the genuinely read C4 Warhead.
        # Its original constructor/read established CombatLightSize=0 above.
        u.mem_write(wh + 0x150, bytes([case['bright']]))
        observation = dict(input=case, queries=0, threshold=None,
                           bright_test_reached=False, constructor_sizes=[])

        def observe_admission(uc, pc, _size, _data):
            sp = uc.reg_read(UC_X86_REG_ESP)
            if pc == 0x55AF60:
                observation['queries'] += 1
            elif pc == 0x48A639:
                # Actual original55AF60 return, before Flash's unsigned gate.
                observation['threshold'] = uc.reg_read(UC_X86_REG_EAX)
            elif pc == 0x48A65A:
                observation['bright_test_reached'] = True
            elif pc == 0x5FF250:
                observation['constructor_sizes'].append(f.integer(sp + 16))
                # Flash needs the returned object for its flags OR at +14.
                # The real allocator already supplied this24-byte object; no
                # constructor fields, lifetime, manager or pixels are claimed.
                return_from_sink(uc, 16, uc.reg_read(UC_X86_REG_ECX))

        hook = u.hook_add(UC_HOOK_CODE, observe_admission)
        try:
            u.reg_write(UC_X86_REG_EDX, wh)
            f.call(0x48A620, [17536, 19840, 0, case['force'], case['flags']],
                   ecx=case['damage'])
        finally:
            u.hook_del(hook)
        observation['reduced_after'] = u.mem_read(0xABCD50, 1)[0]
        if case is history_input:
            # Retain the Flash-produced latch across one explicit FPS sample.
            # The omitted-Flash control changes only its initial latch; both
            # next queries execute the same original55AF60, not Python math.
            u.mem_write(0xABCD44, dwords(17))
            with_threshold = f.call(0x55AF60, [])
            with_reduced = u.mem_read(0xABCD50, 1)[0]
            u.mem_write(0xABCD50, bytes([case['reduced']]))
            without_threshold = f.call(0x55AF60, [])
            hysteresis_history = dict(
                input=dict(first_flash=case, next_fps=17),
                after_flash_reduced=observation['reduced_after'],
                flash_queries=observation['queries'],
                flash_constructor_sizes=observation['constructor_sizes'],
                with_flash_next_threshold=with_threshold,
                with_flash_next_reduced=with_reduced,
                without_flash_next_threshold=without_threshold,
                without_flash_next_reduced=u.mem_read(0xABCD50, 1)[0])
        else:
            creation_admission.append(observation)
        assert all(bytes(u.mem_read(start, len(raw))) == raw for start, raw in original_code.items())
    f.check_code()
    return dict(native_sha256=native.image_sha256(), warhead_inputs=inputs,
        constants=constants, cases=rows, creation_admission=creation_admission,
        hysteresis_history=hysteresis_history,
        creation_code_sha256={f'{start:08X}': hashlib.sha256(raw).hexdigest()
                             for start, raw in original_code.items()},
        creation_code_unchanged=True, admission_rng_before=admission_rng_before,
        admission_rng_after=f.rng_hashes(),
        scope='Original Warhead ctor/full read and C4 reference selected physical-cache reader; Execute64..66/GetWaypointLocation/Flash body. Retail no-bright arm produces no CombatLight, but the original Flash detail query can mutate shared hysteresis. Declared creation-state controls execute Flash/55AF60; only5FF250 is an observed constructor return boundary. Declared terrain crop and FPS transitions, not played Transylv or render parity.')

def _map_lighting_ambient_history():
    from tools.spatial_oracle.building_body_rules import SP
    from unicorn.x86_const import UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_FPCW
    f = ActionFixture()
    u = f.u
    u.mem_write(8942048, dwords(RULES))
    f.call(6706768, [], ecx=RULES)
    f.call(8163166, [])

    def rates():
        return dict(rate_f64_le_hex=bytes(u.mem_read(RULES + 5736, 8)).hex(), step_f64_le_hex=bytes(u.mem_read(RULES + 5744, 8)).hex())
    root = native.configured_gamemd().parent
    raw_rules = mix((root / 'expandmd01.mix').read_bytes())[mix_hash('RULESMD.INI')]
    raw_map = (root / 'Transylv.yro').read_bytes()
    layers = []
    ctor = rates()
    for name, raw in (('RULESMD.INI', raw_rules), ('Transylv.yro', raw_map)):
        source = sections(raw)
        chosen = {sec: {k: v for k, v in source.get(sec, {}).items() if k in ('AmbientChangeRate', 'AmbientChangeStep')} for sec in ('General', 'AudioVisual')}
        f.ordered_ini(chosen)
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ESI, RULES)
        u.reg_write(UC_X86_REG_EDI, INI)
        u.reg_write(UC_X86_REG_FPCW, 3711)
        native.run_checked(u, 6731223, 6731301, count=100000)
        assert u.reg_read(UC_X86_REG_ESP) == SP
        layers.append(dict(file=name, sha256=hashlib.sha256(raw).hexdigest(), keys=chosen, after=rates()))
    u.mem_write(SCENARIO + 13608, dwords(60, 60, 60))
    u.mem_write(SCENARIO + 4680, dwords(0, 2779096485, 0))
    u.mem_write(11139772, dwords(0, 0))
    u.mem_write(11139764, b'\x00')
    u.mem_write(11139760, dwords(0))
    definition = f.read_action('73,0,110,0,0,0,0,A')
    f.call(7198896, [0, 0, 0, 0], ecx=ACTION)
    events = []

    def observe(uc, pc, _sz, _data):
        sp = uc.reg_read(UC_X86_REG_ESP)
        if pc == 4908224:
            events.append(dict(call='UpdateCellLighting'))
            return_from_sink(uc, 0, 0)
        elif pc == 5194480:
            events.append(dict(call='Redraw', argument=f.read32(sp + 4)))
            return_from_sink(uc, 4, 0)
    h = u.hook_add(UC_HOOK_CODE, observe)
    rows = []
    before = f.rng_hashes()
    for frame in (1000, 1001, 1179, 1180, 1359, 1360, 1539, 1540, 1719, 1720, 1900):
        u.mem_write(11070852, dwords(frame))
        events.clear()
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_EBP, SCENARIO)
        u.reg_write(UC_X86_REG_EBX, RULES)
        u.reg_write(UC_X86_REG_EDI, frame)
        u.reg_write(UC_X86_REG_FPCW, 3711)
        native.run_checked(u, 5616445, 5616855, count=10000)
        rows.append(dict(frame=frame, normal=f.integer(SCENARIO + 13608), current=f.integer(SCENARIO + 13612), target=f.integer(SCENARIO + 13616), timer=[f.integer(SCENARIO + 4680), f.integer(SCENARIO + 4688)], events=list(events)))
    u.hook_del(h)
    f.check_code()
    return dict(native_sha256=native.image_sha256(), definition=definition, constructor_rates=ctor, layers=layers, rates=rates(), initial=dict(normal=60, current=60, target=60, timer=[0, 0]), rows=rows, rng_before=before, rng_after=f.rng_hashes(), scope='Original Rules constructor then AudioVisual retained ReadDouble66B5D7..66B625 using physical selected key caches, then Execute73 and actual Logic55B33D..55B4D7. Declared initial map60 and sparse frame observations; cell relight/redraw output sinks only. Whole physical INI load/Process/mode chronology, gameplay and pixels excluded.')

def _map_lighting_rate_controls():
    from tools.spatial_oracle.building_body_rules import SP
    from unicorn.x86_const import UC_X86_REG_ESI, UC_X86_REG_EDI, UC_X86_REG_FPCW
    f = ActionFixture()
    u = f.u
    f.call(8163166, [])
    f.call(6706768, [], ecx=RULES)

    def state():
        return dict(rate_f64_le_hex=bytes(u.mem_read(RULES + 5736, 8)).hex(), step_f64_le_hex=bytes(u.mem_read(RULES + 5744, 8)).hex())
    ctor = state()
    rows = []
    for case in (dict(name='wrong_general_ignored', cache={'General': {'AmbientChangeRate': '1.5', 'AmbientChangeStep': '.9'}}), dict(name='av_exact', cache={'AudioVisual': {'AmbientChangeRate': '.001', 'AmbientChangeStep': '.07'}}), dict(name='missing_retains', cache={}), dict(name='wrong_key_case_ignored', cache={'AudioVisual': {'ambientchangerate': '9', 'Ambientchangestep': '9'}}), dict(name='signed_zero', cache={'AudioVisual': {'AmbientChangeRate': '0', 'AmbientChangeStep': '-.1'}})):
        before = state()
        f.ordered_ini(case['cache'])
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ESI, RULES)
        u.reg_write(UC_X86_REG_EDI, INI)
        u.reg_write(UC_X86_REG_FPCW, 3711)
        native.run_checked(u, 6731223, 6731301, count=100000)
        assert u.reg_read(UC_X86_REG_ESP) == SP
        rows.append(dict(input=case, before=before, after=state()))
    f.check_code()
    return dict(native_sha256=native.image_sha256(), constructor=ctor, cases=rows, scope='Original Rules constructor and retained AudioVisual two-key reader66B5D7..66B625; declared ordered cached keys. Physical lexical-empty elision and full Process not included. Cached-empty and malformed float reads expose uninitialized output and are excluded; physical empty omission is separate.')

def map_lighting_generate():
    physical = _map_lighting_retail()
    map_row = physical['TRANSYLV.YRO'][0]
    retail_map = {key: map_row[key] for key in
                  ('archive','sha256','bytes','selected','Lighting','Waypoints','General','AudioVisual','CombatDamage','Super')}
    selected_waypoints = {str(130): retail_map['Waypoints']['130']}
    retail_map['Waypoints'] = selected_waypoints
    return dict(schema_version=1,
        native_sha256=native.image_sha256(),
        retail=dict(map=retail_map,rules=physical['RULESMD.INI']),
        ambient_controls=_map_lighting_ambient_controls()['cases'],
        flash_controls=_map_lighting_flash_controls(),
        ambient_history=_map_lighting_ambient_history(),
        rate_controls=_map_lighting_rate_controls())


def map_lighting_metadata():
    return provenance(
        scope='Original Execute73 normal/target writes and64..66 local light producer, retained AV scalar readers, and bounded Transylv110 ambient fade; no full scene or pixel parity claim.',
        assumptions=[
            'Physical configured MD install: loose Transylv.yro and expandmd01.mix RULESMD.INI, with original hashes; lexical cache transport uses existing sections/ordered_ini. Physical INI loading, optional LANGRULE, full mode/Process chronology and full terrain are excluded.',
            '56 declared Execute73 scenario/effect-state controls keep current ambient, RGB/Ground/Level and fade timer explicit. Storm activation alone gates target3530; original action remains reachable in YR dispatcher.',
            'Retail C4Warhead=Super is resolved by original C4 reader and factory; original Warhead constructor/full Read runs. Stock Super Bright=0 and CombatLightSize=0 select the actual no-light Flash leaf.15 control producers cover50/100/300 and distinct scalar versus tail waypoint.',
            'Thirteen additional creation-admission controls supply explicit detail/FPS/minimum/buffer/reduced/Bright/force/flags/damage priors on that same physical C4 object. Actual48A620 calls the original55AF60 before Bright; the constructor size, latch and threshold outputs are executed, not supplied. A stock no-Bright low-FPS Flash retains its mutated latch into a declared next-FPS sample and original55AF60 query; an omitted-Flash control changes only the latch prior.',
            'History uses original Rules ctor, original retained AudioVisual ReadDouble kernels on physical selected keys and actual Logic fade instructions. Initial ambient60 is a declared prior corresponding to map Lighting.Ambient; this comparison does not execute the map lighting reader.',
            'Action64/65 controls establish shared producer sizes; they are not used by this official map. Invalid native array/cell indices, non-finite float values and cached-empty/malformed uninitialized ReadDouble outputs are excluded. Existing ini_value.rs intentionally returns deterministic zero for malformed data; no parser exception is introduced here.',
            'Native controller globals correspond to existing superweapon_oracle: stormA9FAB4, nukeA9FABC, chronoA9FAB0, PsyDomA9FAC0. Current root data has no applicable Transylv AV/C4 override; production Rust retail tests must independently parse layers.',
        ],
        substitutions=[
            'Existing ActionFixture supplies allocator/delete/TLS/Win32 fixture seams and preallocated registries. Native action, Rules, Warhead, waypoint and Flash instructions are unchanged.',
            'History UpdateCellLighting4AE4C0 and MapRedraw4F42F0 are recorded output sinks; no sampled colors, terrain relight, palette composition or rendered output is asserted.',
            'Only in the additional creation-admission/hysteresis controls, LightCtor5FF250 records its actual size argument and returns its real allocated receiver with ret16; original Flash and55AF60 bytes are checked against the immutable executable before/after. No light constructor, lifetime, manager or Bright-positive rendered result is certified.',
        ],
        entry_points=dict(action_execute=0x6DD8B0,small_light=0x6DDDF6,
            medium_light=0x6DDE4B,large_light=0x6DDEA0,ambient=0x6DF292,
            rules_ctor=0x665650,ambient_rules=0x66B5D7,c4_reader=0x66C304,
            warhead_factory=0x75E3B0,warhead_read=0x75D3A0,
            waypoint_location=0x68BD00,flash=0x48A620,light_ctor=0x5FF250,
            minimum_frame_rate=0x55AF60,
            ambient_logic=0x55B33D,cell_refresh_sink=0x4AE4C0,redraw_sink=0x4F42F0))


def map_lighting_source_paths():
    return dict(SOURCE_PATHS,
        scalar_reader_owner=Path(__file__).parents[1]/'rules_oracle/bridge_landing_inputs.py')


def nuke_strike_retail(root):
    """Physical map row selection plus the unchanged TAction constructor/reader."""
    raw = (root / 'HighExpR.yro').read_bytes()
    candidates = [(key, value) for key, value in mix(raw).items()
                  if b'[Basic]' in value or b'[Map]' in value]
    assert len(candidates) == 1
    entry_hash, map_raw = candidates[0]
    assert map_raw == (root / 'HighExpR.map').read_bytes()
    data = sections(map_raw)
    fixture = ActionFixture()
    actions = []
    for trigger_id, row in data['Actions'].items():
        tokens = row.split(',')
        assert len(tokens) == 1 + 8 * int(tokens[0])
        for ordinal in range(int(tokens[0])):
            fields = tokens[1 + ordinal * 8:1 + (ordinal + 1) * 8]
            if int(fields[0]) != 95:
                continue
            raw_action = ','.join(fields)
            materialized = fixture.read_action(raw_action)
            assert materialized['kind'] == 95
            actions.append(dict(trigger_id=trigger_id, trigger=data['Triggers'][trigger_id],
                event=data['Events'][trigger_id], ordinal=ordinal, raw=raw_action,
                materialized=materialized,
                waypoint_raw=data['Waypoints'][str(materialized['waypoint'])]))
    fixture.check_code()
    assert len(actions) == 6
    return dict(archive=dict(name='HighExpR.yro',sha256=hashlib.sha256(raw).hexdigest()),
        map=dict(entry_hash=entry_hash,sha256=hashlib.sha256(map_raw).hexdigest(),bytes=len(map_raw)),
        actions=actions)


def nuke_strike_sources(root):
    required = ('RULESMD.INI','ARTMD.INI','MPBattleMD.ini','HighExpR.yro',
                'HighExpR.map','Hills.map','DRAGON.SHP','NKMSLDN.SHP')
    result = []
    for name in (*required,'LANGRULE.INI'):
        path = root / name
        if not path.is_file():
            if name != 'LANGRULE.INI':
                raise FileNotFoundError(path)
            result.append(dict(file=name,absent=True))
            continue
        raw = path.read_bytes()
        result.append(dict(file=name,bytes=len(raw),sha256=hashlib.sha256(raw).hexdigest()))
    return result


def nuke_strike_dispatch_case(name,cell,wp,*,flags=0,ground=0,damage=600,
                              weapon_speed=10,incoming_house=False,fire_accepts=True):
    """Original Execute95/factory/math; Construct and Fire are explicit ABI sinks."""
    from tools import superweapon_oracle as s
    e=s.Emu();action=s.BASE+0x50000;scene=e.read32(s.SCENARIO_PTR)
    e.write32(action+0x2C,95);e.write32(action+0x44,wp)
    e.uc.mem_write(scene+0x632+wp*4,struct.pack('<hh',*cell))
    e.write32(s.WEAPON_TYPES,s.ITEMS);e.write32(s.ITEMS+8,s.WEAPON)
    e.write32(s.WEAPON+0xA0,s.BULLET_TYPE);e.write32(s.WEAPON+0xA4,damage)
    e.write32(s.WEAPON+0xA8,weapon_speed);e.write32(s.WEAPON+0xAC,s.WARHEAD)
    e.write32(s.CELL+0x140,flags);e.write32(0xB0E6D4,416)
    e.write32(s.BULLET,s.BULLET_VT);e.write32(s.BULLET_VT+0x1F0,s.STUB_FIRE)
    rngs={'scenario':scene+0x218,'main':0x886B88,'mapgen':0xABE890}
    for pointer in rngs.values():e.invoke(0x65C6D0,ecx=pointer,args=[31])
    state=lambda:{key:bytes(e.uc.mem_read(pointer,0x3F4)) for key,pointer in rngs.items()}
    before=state();visits=[]
    def observer(u,pc,_size,_data):
        if pc in (0x6DD8B0,0x6DD9DA,0x6E3410,0x68BCC0,0x46B050,0x46B260,0x65C780,0x65C7E0):
            visits.append(f'{pc:08X}')
    e.uc.hook_add(UC_HOOK_CODE,observer)
    def ground_fn(m):m.events.append(['ground',s.read_coord(m,m.arg(0))]);return ground
    def cell_lookup(m):m.events.append(['cell_lookup',s.read_cell(m,m.arg(0))]);return s.CELL
    def coord_lookup(m):m.events.append(['coord_lookup',s.read_coord(m,m.arg(0))]);return s.CELL
    def find_weapon(m):
        m.events.append(['weapon_type',s.read_name(m,m.uc.reg_read(UC_X86_REG_ECX))]);return 2
    def co_create(m):m.write32(m.arg(4),s.BULLET);m.events.append(['co_create']);return 0
    def construct(m):
        assert m.uc.reg_read(UC_X86_REG_ECX)==s.BULLET
        m.events.append(['construct',dict(projectile=m.arg(0)==s.BULLET_TYPE,
            target_cell=m.arg(1)==s.CELL,owner=m.arg(2),damage=s.i32(m.arg(3)),
            warhead=m.arg(4)==s.WARHEAD,speed=s.i32(m.arg(5)),bright=m.arg(6)&255)])
    def fire(m):
        assert m.uc.reg_read(UC_X86_REG_ECX)==s.BULLET
        bits=struct.unpack('<QQQ',m.uc.mem_read(m.arg(1),24))
        m.events.append(['fire',dict(origin=s.read_coord(m,m.arg(0)),
            velocity_bits=[f'{value:016x}' for value in bits],accepts=fire_accepts)])
        return int(fire_accepts)
    e.hook(0x578080,ground_fn,4);e.hook(0x5657A0,cell_lookup,4);e.hook(0x565730,coord_lookup,4)
    e.hook(0x773030,find_weapon,0)
    e.write32(0x7E15FC,s.STUBS+0x1D0);e.hook(s.STUBS+0x1D0,co_create,0x14)
    e.hook(0x4664C0,construct,0x1C);e.hook(s.STUB_FIRE,fire,8)
    originals=[(address,bytes(e.uc.mem_read(address,size))) for address,size in (
        (0x6DD8B0,0x37A0),(0x6E3410,0x1D5),(0x46B050,0x64),(0x46B260,13),
        (0x68BCC0,20),(0x4CACB0,128),(0x4CAD00,128))]
    returned=e.invoke(0x6DD8B0,ecx=action,args=[s.HOUSE if incoming_house else 0,0,0,0],
        required=[0x6DD9DA,0x6E3410,0x68BCC0,0x46B050,0x46B260])&255
    after=state();assert before==after
    assert originals==[(address,bytes(e.uc.mem_read(address,len(raw)))) for address,raw in originals]
    return dict(name=name,input=dict(cell=list(cell),waypoint=wp,flags=flags,ground=ground,
        damage=damage,weapon_speed=weapon_speed,incoming_house=incoming_house,fire_accepts=fire_accepts),
        result=returned,events=e.events,installed_weapon=e.read32(s.BULLET+0x130)==s.WEAPON,
        native_visits=visits,rng_hashes_before={key:hashlib.sha256(raw).hexdigest() for key,raw in before.items()},
        rng_hashes_after={key:hashlib.sha256(raw).hexdigest() for key,raw in after.items()},code_unchanged=True)


def nuke_strike_flight(root):
    """Real Bullet activation, Configure, Fire and complete AI visits to impact."""
    from tools.projectile_oracle import guided_step as g
    from tools.projectile_oracle.bridge_render_art_state import ArtStateReader
    from tools.projectile_oracle.bridge_render_inputs import lexical
    from tools.projectile_oracle.ifv_impact import (
        initialize_effect_world,initialize_effect_registries,retirement_transport)
    from tools.projectile_oracle.ifv_launch import (
        bullet_activation_transport,bullet_addref_transport)
    from tools.native_oracle import initialize_empty_windows_seh,image_bytes
    from tools.native_inspect import selected_ranges
    art,_=lexical((root/'ARTMD.INI').read_bytes(),
        {'AAHeatSeeker2','DRAGON','FV','GiantNukeDown','NKMSLDN'})
    # create owns original Bullet/Cell/Homing initializers and the existing
    # small allocated flat-cell table, not HighExpR topography.
    m=ArtStateReader(art,root=root)
    m,_,cells,_=g.create(False,reader=m);u=m.u
    initialize_effect_registries(m)
    initialize_effect_world(m,cells,map_size=(32,32))
    # Explicit native map Size prior. Without Map+F4, ordinary Bullet AI
    # correctly culls the otherwise allocated target outside its map.
    u.mem_write(0x87F7E8+0xF4,dwords(32,32))
    weapon=m.invoke(0x772FA0,m.cstring('NukePayload'));layers=[]
    for name in ('RULESMD.INI','LANGRULE.INI','MPBattleMD.ini','HighExpR.map'):
        path=root/name
        if not path.is_file():continue
        cached,_=lexical(path.read_bytes(),{'NukePayload','GiantNukeDown','NUKE'})
        m.rules_cache(cached);m.invoke(0x772080,weapon,(RULES,))
        projectile=m.read32(weapon+0xA0)
        m.read_layer(projectile,cached);m.invoke(0x7729F0,weapon)
        warhead=m.read32(weapon+0xAC);m.invoke(0x75D3A0,warhead,(RULES,))
        layers.append(dict(file=name,keys=cached,damage=g.i32(u,weapon+0xA4),
            weapon_speed=g.i32(u,weapon+0xA8),warhead=m.string(warhead+0x24),
            projectile=m.string(projectile+0x24),
            vertical=bool(u.mem_read(projectile+0x2C0,1)[0]),
            acceleration=g.i32(u,projectile+0x2D0),
            detonation_altitude=g.i32(u,projectile+0x2BC)))
    scenario=m.read32(0xA8B230)
    u.mem_write(scenario+0x632+4,struct.pack('<hh',16,20))
    u.mem_write(0xB0E6D4,dwords(416));u.mem_write(0xA8ED84,dwords(0))
    action=m.alloc(0x94);u.mem_write(action,bytes(0x94))
    u.mem_write(action+0x2C,dwords(95));u.mem_write(action+0x44,dwords(1))
    initialize_empty_windows_seh(u)
    for pointer in (scenario+0x218,0x886B88,0xABE890):
        m.invoke(0x65C6D0,pointer,(31,))
    rng=lambda:CampaignReader.rng_state(SimpleNamespace(u=u,scenario=scenario))
    before_rng=rng();before_id=g.i32(u,scenario+0x214)
    originals=selected_ranges(image_bytes(),None,None,code_only=True)
    events=[];bullets=[];frames=[];visits=set()
    def observe(_u,pc,_size,_data):
        if bullet_activation_transport(m,pc,events):return
        if bullet_addref_transport(m,pc,events):return
        if retirement_transport(m,pc,events):return
        if pc==0x6E34E1:bullets.append(u.reg_read(UC_X86_REG_EAX))
        if pc in (0x6DD9DA,0x6E3410,0x46B050,0x6C5090,0x466380,
                  0x4664C0,0x468670,0x467E53,0x65C780,0x65C7E0):
            visits.add(f'{pc:08X}')
    hook=u.hook_add(UC_HOOK_CODE,observe)
    try:
        returned=m.invoke(0x6DD8B0,action,(0,0,0,0))&255
        assert returned==1 and len(bullets)==1
        bullet=bullets[0]
        launch=dict(position=g.xyz(u,bullet+0x9C),fire_coord=g.xyz(u,bullet+0x134),
            target_coord=g.xyz(u,bullet+0x140),velocity=g.vec(u,bullet+0xE8),
            maximum_speed=g.i32(u,bullet+0x110),damage=g.i32(u,bullet+0x6C),
            source_is_null=m.read32(bullet+0xB0)==0,
            weapon=m.string(m.read32(bullet+0x130)+0x24),
            warhead=m.string(m.read32(bullet+0x128)+0x24),
            alive=bool(u.mem_read(bullet+0x90,1)[0]),
            last_cell=list(struct.unpack('<hh',u.mem_read(bullet+0x14C,4))),
            arm_timer=[g.i32(u,bullet+0xC4),g.i32(u,bullet+0xCC)],
            native_id_delta=g.i32(u,scenario+0x214)-before_id)
        for frame in range(1,450):
            u.mem_write(0xA8ED84,dwords(frame));u.mem_write(SP,dwords(RET_MAGIC))
            u.reg_write(UC_X86_REG_ESP,SP);u.reg_write(UC_X86_REG_ECX,bullet)
            run_checked(u,0x4666E0,(RET_MAGIC,0x467E53),count=500000,
                context=dict(case='trigger95_descending_payload',frame=frame))
            impact=u.reg_read(UC_X86_REG_EIP)==0x467E53
            frames.append(dict(frame=frame,position=g.xyz(u,bullet+0x9C),
                velocity_bits=g.vec(u,bullet+0xE8)['bits'],impact_entry=impact))
            if impact:break
        assert frames[-1]['impact_entry'],'did not reach native NUKE impact entry'
        after_rng=rng();assert before_rng==after_rng
        assert all(bytes(u.mem_read(row['address'],len(row['bytes'])))==row['bytes'] for row in originals)
        required={'006DD9DA','006E3410','0046B050','006C5090','00466380','004664C0','00468670','00467E53'}
        assert required<=visits
        return dict(input=dict(map_size=[32,32],flat_level=6,flat_slope=0,
            target_cell=[16,20],waypoint=1,binary_frame=0,rng_seed=31),
            result=returned,layers=layers,launch=launch,frames=frames,
            native_visits=sorted(visits),platform_events=events,
            rng_hashes_before={key:hashlib.sha256(bytes.fromhex(raw)).hexdigest() for key,raw in before_rng.items()},
            rng_hashes_after={key:hashlib.sha256(bytes.fromhex(raw)).hexdigest() for key,raw in after_rng.items()},
            code_unchanged=True)
    finally:u.hook_del(hook)


def nuke_strike_generate(root):
    from tools.native_oracle import image_sha256
    previous=os.environ.get('VERA20K_PROJECTILE_RENDER_ASSETS')
    os.environ['VERA20K_PROJECTILE_RENDER_ASSETS']=str(root)
    try:
        sources=nuke_strike_sources(root)
        retail=nuke_strike_retail(root)
        rows=[]
        for action in retail['actions']:
            packed=int(action['waypoint_raw'])
            rows.append(nuke_strike_dispatch_case(action['trigger_id'],
                (packed%1000,packed//1000),action['materialized']['waypoint']))
        for flags in (0,0x100,0x400,0x500):
            rows.append(nuke_strike_dispatch_case('elevated_bridge_'+hex(flags),
                (67,19),1,flags=flags,ground=416))
        rows.append(nuke_strike_dispatch_case('signed_cell_negative',(-2,-32768),17,
            ground=-104,damage=-17,weapon_speed=255,incoming_house=True))
        rows.append(nuke_strike_dispatch_case('fire_refused',(67,19),1,fire_accepts=False))
        return dict(schema_version=1,native_sha256=image_sha256(),sources=sources,
            retail=retail,dispatch_controls=rows,production_flight=nuke_strike_flight(root))
    finally:
        if previous is None:os.environ.pop('VERA20K_PROJECTILE_RENDER_ASSETS',None)
        else:os.environ['VERA20K_PROJECTILE_RENDER_ASSETS']=previous


def nuke_strike_source_paths():
    return dict(SOURCE_PATHS,**{name:Path(__file__).parents[2]/path for name,path in {
        'superweapon_transport':'tools/superweapon_oracle.py',
        'generic_emulator':'tools/ai_base_building_oracle.py',
        'guided_world':'tools/projectile_oracle/guided_step.py',
        'impact_world':'tools/projectile_oracle/ifv_impact.py',
        'bullet_platform':'tools/projectile_oracle/ifv_launch.py',
        'bullet_reader':'tools/projectile_oracle/bridge_render_inputs.py',
        'bullet_art':'tools/projectile_oracle/bridge_render_art_state.py',
    }.items()})


def nuke_strike_metadata():
    return provenance(scope='Original Execute95/factory/launch arithmetic controls plus selected physical HighExpR records and a real descending NukePayload from full construct/Fire through202 BulletAI calls to NUKE impact entry; no full map or downstream detonation parity claim.',
        assumptions=[
            'Six Action95 records are selected from physical HighExpR.yro inner map and materialized by original6DD000/6DD5B0. Archive/inner-map identities and exact raw records are retained. Source order is preserved; the host supplies the moment of each action call rather than running the complete Trigger/Tag scheduler.',
            'Twelve isolated controls execute original6DD8B0 case95/6E3410, GetWaypoint68BCC0, factory46B050, SetWeapon46B260 and sine/cosine arithmetic. Explicit shells supply Weapon Damage/WH/projectile, incoming House and flat ground/bridge flags. The hard-coded50/brightfalse/sourceNULL, coordinate lifting, scale-100 velocity and ignored Fire refusal are native outputs. These control inputs600/10 are supplied scalars, not the map350/converted-speed values.',
            'The separate production control reuses guided_step.create and impact world owners: actual Bullet/Cell/Homing/effect initializers, explicit32x32 Map Size at+F4 and existing world size storage, mapped flat level6/slope0 cells, no actors/overlays/live bridges, GameActive1 and supplied waypoint1=(16,20). Native map geometry is a declared crop, not HighExpR terrain. Earlier unrelated HoverMissile/Hills setup initializes the common fixture; selected NukePayload/GiantNukeDown/NUKE are then read in RULESMD, optionalLANGRULE, MPBattleMD and HighExpR order.',
            'Unique selected physical lexical strings feed existing native INI cache transport; original Weapon772080/postpass7729F0, BulletType46BEE0 and Warhead75D3A0 execute. Full physical INI/archive startup and original Rules discovery/reset chronology are excluded. The unused stored Weapon Speed is therefore recorded as a component-history output, not certified complete-game speed.',
            'Original Bullet activation46B050 reaches registered factory6C5090, constructor466380, Configure4664C0, SetWeapon46B260 and whole Fire468670 including original Unlimbo/coordinate placement. Full BulletAI runs once per host-supplied frame until467E53 before the NUKE impact block. No native arithmetic/timer/RNG/gameplay branch is replaced in that production segment; all three RNG streams are seeded natively31 immediately before95 and compared after this bounded history.',
            'Downstream ScreenNukeFlash/NUKEBALL, delayed detonation/AreaDamage, receiver callbacks, radiation, retirement and rendered nuclear output are outside this new corpus. Existing tools/superweapon_oracle sections nuke_impact/nuke_wait/nuke_flash and projectile/combat owners provide separately bounded evidence; this sample does not prove their entire combined lifecycle or all map targets.',
        ],substitutions=[
            'Isolated controls record Ground578080/Cell5657A0/565730/WeaponLookup773030/CoCreate/Construct4664C0/Fire as explicit ABI boundaries; full production control removes those gameplay sinks and reuses existing physical asset IO, allocation/CRT/TLS/COM-activation/Windows atomic/readability transports. Original code bytes remain unchanged.',
            'ArtStateReader retains its existing unrelated voxel-loading boundary after scalar reads. Exact physical SHP bytes are supplied by existing Reader IO. No device renderer/audio or whole native startup is run.',
        ],entry_points=dict(action_execute=0x6DD8B0,action95=0x6E3410,waypoint=0x68BCC0,
            bullet_factory=0x46B050,registered_factory=0x6C5090,bullet_constructor=0x466380,
            configure=0x4664C0,set_weapon=0x46B260,bullet_fire=0x468670,bullet_ai=0x4666E0,
            impact_entry=0x467E53))


if __name__=='__main__':
    import argparse
    import os
    selector = argparse.ArgumentParser(add_help=False)
    selected_modes = selector.add_mutually_exclusive_group()
    selected_modes.add_argument('--damage-waypoint', action='store_true')
    selected_modes.add_argument('--click-shroud', action='store_true')
    selected_modes.add_argument('--super-charge', action='store_true')
    selected_modes.add_argument('--superweapon-state', action='store_true')
    selected_modes.add_argument('--superweapon-reset', action='store_true')
    selected_modes.add_argument('--reveal-zone', action='store_true')
    selected_modes.add_argument('--forced-map', action='store_true')
    selected_modes.add_argument('--inline-type', action='store_true')
    selected_modes.add_argument('--flash-team', action='store_true')
    selected_modes.add_argument('--reshroud', action='store_true')
    selected_modes.add_argument('--scenario-timer', action='store_true')
    selected_modes.add_argument('--chrono-house', action='store_true')
    selected_modes.add_argument('--all-change-house', action='store_true')
    selected_modes.add_argument('--conceal-area', action='store_true')
    selected_modes.add_argument('--music-theme', action='store_true')
    selected_modes.add_argument('--all-to-hunt', action='store_true')
    selected_modes.add_argument('--crate', action='store_true')
    selected_modes.add_argument('--wakeup', action='store_true')
    selected_modes.add_argument('--ai-activation', action='store_true')
    selected_modes.add_argument('--chrono-reinforcement', action='store_true')
    selected_modes.add_argument('--tech-level', action='store_true')
    selected_modes.add_argument('--do-explosion', action='store_true')
    selected_modes.add_argument('--map-lighting', action='store_true')
    selected_modes.add_argument('--nuke-strike', action='store_true')
    selector.add_argument('--inputs', type=Path)
    selected, remaining = selector.parse_known_args()
    if selected.map_lighting:
        if selected.inputs is not None:
            selector.error('--map-lighting reads configured physical retail assets, not --inputs')
        finish_vectors(map_lighting_generate,
            Path(__file__).with_name('trigger_map_lighting.json'),
            provenance=map_lighting_metadata,argv=remaining,
            source_paths=map_lighting_source_paths())
    elif selected.nuke_strike:
        inputs_root=selected.inputs or Path(os.environ.get('VERA20K_TRIGGER_NUKE_INPUTS',''))
        finish_vectors(lambda:nuke_strike_generate(inputs_root),
            Path(__file__).with_name('trigger_nuke_strike.json'),
            provenance=nuke_strike_metadata,argv=remaining,source_paths=nuke_strike_source_paths())
    elif selected.do_explosion:
        if os.environ.get('VERA20K_NATIVE_STATE_CACHE'):
            selector.error('VERA20K_NATIVE_STATE_CACHE is no longer supported; unset it to read fresh retail inputs')
        if selected.inputs is not None:
            selector.error('--do-explosion reads configured physical MD assets, not --inputs')
        receipt_path=os.environ.get('VERA20K_TRIGGER_EXPLOSION_RECEIPTS') if '--write' in remaining else None
        finish_vectors(lambda:do_explosion_generate(receipt_path=receipt_path),
            Path(__file__).with_name('trigger_do_explosion.json'),provenance=do_explosion_metadata,
            argv=remaining,source_paths=do_explosion_source_paths())
    elif selected.superweapon_reset:
        if selected.inputs is not None:
            selector.error('--superweapon-reset reads configured physical MD assets, not --inputs')
        receipt_path = os.environ.get('VERA20K_TRIGGER_SUPER_RESET_RECEIPTS') if '--write' in remaining else None
        finish_vectors(lambda: superweapon_reset_generate(receipt_path=receipt_path),
            Path(__file__).with_name('trigger_superweapon_reset.json'),
            provenance=superweapon_reset_metadata, argv=remaining, source_paths=SOURCE_PATHS)
    elif selected.superweapon_state:
        if selected.inputs is not None:
            selector.error('--superweapon-state reads configured physical MD assets, not --inputs')
        receipt_path = os.environ.get('VERA20K_TRIGGER_SUPER_STATE_RECEIPTS') if '--write' in remaining else None
        finish_vectors(lambda: superweapon_state_generate(receipt_path=receipt_path),
            Path(__file__).with_name('trigger_superweapon_state.json'),
            provenance=superweapon_state_metadata, argv=remaining, source_paths=SOURCE_PATHS)
    elif selected.tech_level:
        from tools.spatial_oracle.building_construction import JOINED_ROOT
        inputs_root = selected.inputs or Path(os.environ.get('VERA20K_TRIGGER_TECH_INPUTS', str(JOINED_ROOT)))
        finish_vectors(lambda: tech_level_generate(inputs_root),
            Path(__file__).with_name('trigger_object_tech_level.json'),
            provenance=lambda: tech_level_metadata(inputs_root), argv=remaining,
            source_paths=dict(SOURCE_PATHS,
                reset_owner=Path(__file__).parents[1]/'rules_oracle/select_anim_reset.py',
                factory_loss_owner=Path(__file__).parents[1]/'sidebar_oracle/factory_loss.py',
                sidebar_owner=Path(__file__).parents[1]/'sidebar_oracle/cameo_order.py',
                construction_owner=Path(__file__).with_name('building_construction.py'),
                engineer_owner=Path(__file__).with_name('engineer_repair_admission.py'),
                joined_owner=Path(__file__).with_name('building_death_anims.py'),
                com_owner=Path(__file__).parent/'anytown_damage/mission.py'))
    elif selected.chrono_reinforcement:
        if selected.inputs is not None:
            selector.error('--chrono-reinforcement reuses the configured retail FootMissions inputs, not --inputs')
        from tools.spatial_oracle.team_creation import source_paths as team_source_paths
        finish_vectors(chrono_reinforcement_generate,
            Path(__file__).with_name('trigger_chrono_reinforcement.json'),
            provenance=chrono_reinforcement_metadata,argv=remaining,
            source_paths=dict(SOURCE_PATHS,**team_source_paths()))
    elif selected.ai_activation:
        if selected.inputs is not None:
            selector.error('--ai-activation reads configured retail ALL05, not --inputs')
        finish_vectors(ai_activation_generate,
            Path(__file__).with_name('trigger_ai_activation.json'),
            provenance=ai_activation_metadata, argv=remaining,
            source_paths=dict(SOURCE_PATHS,
                ai_team=Path(__file__).parents[1] / 'ai_team_oracle.py',
                rng_owner=Path(__file__).parents[1] / 'rmg_oracle' / 'gen_rng_vectors.py'))
    elif selected.crate:
        if selected.inputs is not None:
            selector.error('--crate uses configured retail assets, not --inputs')
        finish_vectors(crate_generate, Path(__file__).with_name('trigger_crate.json'),
            provenance=crate_metadata, argv=remaining, source_paths=crate_source_paths())
    elif selected.wakeup:
        if selected.inputs is not None:
            selector.error('--wakeup uses declared object/Tag/Mission priors, not --inputs')
        finish_vectors(wakeup_generate,
            Path(__file__).with_name('trigger_wakeup.json'),
            provenance=wakeup_metadata, argv=remaining,
            source_paths=dict(SOURCE_PATHS,
                wakeup_event_reader=Path(__file__).with_name('trigger_event_records.py')))
    elif selected.all_to_hunt:
        if selected.inputs is not None:
            selector.error('--all-to-hunt reads configured retail ALL06, not --inputs')
        finish_vectors(all_to_hunt_generate,
            Path(__file__).with_name('trigger_all_to_hunt.json'),
            provenance=all_to_hunt_metadata, argv=remaining, source_paths=all_to_hunt_source_paths())
    elif selected.music_theme:
        if selected.inputs is not None:
            selector.error('--music-theme uses its declared Theme priors, not --inputs')
        finish_vectors(music_theme_generate,
            Path(__file__).with_name('trigger_music_theme.json'),
            provenance=music_theme_metadata, argv=remaining, source_paths=SOURCE_PATHS)
    elif selected.conceal_area:
        if selected.inputs is not None:
            selector.error('--conceal-area reads configured retail RULESMD/SOV01, not --inputs')
        finish_vectors(conceal_area_generate,
            Path(__file__).with_name('trigger_conceal_area.json'),
            provenance=conceal_area_metadata, argv=remaining, source_paths=SOURCE_PATHS)
    elif selected.all_change_house:
        if selected.inputs is not None:
            selector.error('--all-change-house uses declared House/Techno priors, not --inputs')
        finish_vectors(all_change_house_generate,
            Path(__file__).with_name('trigger_all_change_house.json'),
            provenance=all_change_house_metadata, argv=remaining, source_paths=SOURCE_PATHS)
    elif selected.chrono_house:
        if selected.inputs is not None:
            selector.error('--chrono-house uses declared Foot/map priors, not --inputs')
        finish_vectors(chrono_house_generate,
            Path(__file__).with_name('trigger_chrono_house.json'),
            provenance=chrono_house_metadata, argv=remaining, source_paths=SOURCE_PATHS)
    elif selected.scenario_timer:
        if selected.inputs is not None:
            selector.error('--scenario-timer uses declared timer priors, not --inputs')
        finish_vectors(scenario_timer_generate,
            Path(__file__).with_name('scenario_timer.json'),
            provenance=scenario_timer_metadata, argv=remaining, source_paths=SOURCE_PATHS)
    elif selected.reshroud:
        if selected.inputs is not None:
            selector.error('--reshroud uses declared sparse priors, not --inputs')
        finish_vectors(reshroud_generate,
            Path(__file__).with_name('trigger_reshroud.json'),
            provenance=reshroud_metadata, argv=remaining, source_paths=SOURCE_PATHS)
    elif selected.flash_team:
        if selected.inputs is not None:
            selector.error('--flash-team uses declared member-list priors, not --inputs')
        finish_vectors(flash_team_generate,
            Path(__file__).with_name('trigger_flash_team.json'),
            provenance=flash_team_metadata, argv=remaining, source_paths=SOURCE_PATHS)
    elif selected.inline_type:
        if selected.inputs is not None:
            selector.error('--inline-type uses explicit action reader inputs, not --inputs')
        finish_vectors(inline_type_generate,
            Path(__file__).with_name('trigger_inline_type.json'),
            provenance=inline_type_metadata, argv=remaining, source_paths=SOURCE_PATHS)
    elif selected.forced_map:
        if selected.inputs is not None:
            selector.error('--forced-map uses declared synthetic priors, not --inputs')
        finish_vectors(forced_map_generate,
            Path(__file__).with_name('trigger_forced_map.json'),
            provenance=forced_map_metadata,argv=remaining,source_paths=SOURCE_PATHS)
    elif selected.reveal_zone:
        if selected.inputs is not None:
            selector.error('--reveal-zone uses declared synthetic priors, not --inputs')
        finish_vectors(reveal_zone_generate,
            Path(__file__).with_name('trigger_reveal_zone.json'),
            provenance=reveal_zone_metadata,argv=remaining,source_paths=SOURCE_PATHS)
    elif selected.super_charge:
        from tools.spatial_oracle.building_construction import JOINED_ROOT
        inputs_root = selected.inputs or Path(os.environ.get('VERA20K_TRIGGER_SUPER_INPUTS', str(JOINED_ROOT)))
        sources = dict(SOURCE_PATHS, **{name: Path(__file__).parents[2] / path for name, path in {
            'building_receiver_transport': 'tools/spatial_oracle/building_construction.py',
            'damage_rules_owner': 'tools/rules_oracle/bridge_landing_inputs.py',
            'particle_registry_boundary': 'tools/procedural_drawing_oracle/electric_bolt.py',
        }.items()})
        receipt_path = os.environ.get('VERA20K_TRIGGER_SUPER_RECEIPTS') if '--write' in remaining else None
        finish_vectors(lambda: super_charge_generate(inputs_root, receipt_path=receipt_path),
            Path(__file__).with_name('trigger_super_charge_percent.json'),
            provenance=super_charge_metadata, argv=remaining, source_paths=sources)
    elif selected.damage_waypoint:
        from tools.spatial_oracle.building_construction import JOINED_ROOT
        inputs_root = selected.inputs or Path(os.environ.get('VERA20K_TRIGGER_DAMAGE_INPUTS', str(JOINED_ROOT)))
        sources = dict(SOURCE_PATHS, **{name: Path(__file__).parents[2] / path for name, path in {
            'building_receiver_transport': 'tools/spatial_oracle/building_construction.py',
            'building_admission_prior': 'tools/spatial_oracle/refinery_dock.py',
            'authored_health_owner': 'tools/spatial_oracle/object_health.py',
            'damage_rules_owner': 'tools/rules_oracle/bridge_landing_inputs.py',
            'particle_registry_boundary': 'tools/procedural_drawing_oracle/electric_bolt.py',
        }.items()})
        finish_vectors(lambda: damage_waypoint_generate(inputs_root),
            Path(__file__).with_name('trigger_damage_at_waypoint.json'),
            provenance=damage_waypoint_metadata, argv=remaining, source_paths=sources)
    elif selected.click_shroud:
        if selected.inputs is not None:
            selector.error('--click-shroud uses declared click/Cell priors, not --inputs')
        finish_vectors(click_shroud_generate,
            Path(__file__).with_name('click_shroud.json'),
            provenance=click_shroud_metadata, argv=remaining, source_paths=SOURCE_PATHS)
    else:
        if selected.inputs is not None:
            selector.error('--inputs requires --damage-waypoint or --super-charge')
        finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata,
                       argv=remaining,source_paths=SOURCE_PATHS)
