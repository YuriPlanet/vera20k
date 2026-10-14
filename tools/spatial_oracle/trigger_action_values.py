"""Original TAction constructor/read, references and selected action dispatch.

Pins +90 materialization independently of ActionID and the four +34..40
parameters. The first 61 numeric rows are retained unchanged. Additional rows
identify their bounded reference, input or EVA observation with ``case_kind``.

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
"""
from pathlib import Path
import hashlib
import json
import struct
from functools import lru_cache
from types import SimpleNamespace

from unicorn import UC_HOOK_CODE
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

    def read_action(self, raw, *, ini_capacity=None):
        u = self.u
        u.mem_write(ACTION,b'\xA5'*0x94)
        self.call(0x6DD000,[],ecx=ACTION)
        counted = '1,'+raw
        if ini_capacity is None:
            u.mem_write(BUFFER,counted.encode('ascii')+b'\0')
        else:
            # Same native ReadString512 contract as the TriggerType action
            # caller727516..72752E, with supplied cache rather than file IO.
            self.ini(HEAP+0x7000,counted)
            self.call(0x528A10,[TYPE+0x1F8,HEAP+0x7000,0x889F64,BUFFER,ini_capacity],ecx=INI)
        read = self.string(BUFFER)
        self.call(0x7C9CC2,[BUFFER,0x817F70],cdecl=True)
        self.call(0x6DD5B0,[],ecx=ACTION)
        return dict(raw=raw,kind=self.integer(ACTION+0x2C),
                    value=self.integer(ACTION+0x90),
                    args=[self.integer(ACTION+n) for n in (0x34,0x38,0x3C,0x40)],
                    waypoint=self.integer(ACTION+0x44),read=read)

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


def initialize_animation_sounds(f):
    local = mix(mix((configured_gamemd().parent/'ra2md.mix').read_bytes())[
        mix_hash('localmd.mix')])
    raw = local[mix_hash('SOUNDMD.INI')]
    physical = sections(raw)
    selected = {name:physical[name] for name in ('Defaults','Explosion06','ExplosionShard')}
    selected['SoundList'] = {k:v for k,v in physical['SoundList'].items()
                             if v in ('Explosion06','ExplosionShard')}
    proxy = Sound.__new__(Sound)
    proxy.__dict__ = f.__dict__
    proxy.samples = []
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
    """Compose original action/ART/Anim owners; only IO/OS/display are seams."""
    def __init__(self):
        super().__init__()
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
        elif pc==0x5F5850:
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
        for entry in (0x6D1830,0x6D18C0,0x6D1BB0,0x49F2F0,
                      0x561710,0x5617A0,0x5617C0,0x5617E0,0x561910,0x5638D0):
            self.call(entry,0,[])
        # Execute the same original Cell/TAction startup constant groups used
        # by initialize_waypoint_map. The TAction height fields are independent
        # of Cell's initialized level conversion and cannot use image zeros.
        for start in (0x8129FC,0x814EA8):
            for i in range(14):self.call(self.read32(start+i*4),0,[])
        assert struct.unpack('<Q',u.mem_read(0xB0CD48,8))[0]==STANDARD_Z_MULTIPLIER_BITS
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

    def pointer_state(self, p):
        return dict(cell=list(struct.unpack('<hh',self.uc.mem_read(p+0x24,4))),
            level=struct.unpack('<b',self.uc.mem_read(p+0x11B,1))[0],
            counter=struct.unpack('<i',self.uc.mem_read(p+0x130,4))[0],
            gap=struct.unpack('<i',self.uc.mem_read(p+0x134,4))[0],
            flags=self.read32(p+0x140),alt=self.read32(p+0x12C),
            cache=list(struct.unpack('<bb',self.uc.mem_read(p+0x120,2))))

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

    def __init__(self, inputs):
        from tools.native_oracle import NATIVE_FPCW, NativeCallTrace
        from unicorn.x86_const import UC_X86_REG_FPCW
        super().__init__()
        u = self.u
        self.native_fpcw, self.fpcw_register = NATIVE_FPCW, UC_X86_REG_FPCW
        self.house, self.super, self.super_type = (self.allocate(n)
            for n in (0x6000, 0x100, 0x200))
        self.items, self.house_type = self.allocate(64), self.allocate(0x400)
        for pointer, size in ((self.house, 0x6000), (self.super, 0x100),
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


if __name__=='__main__':
    import argparse
    import os
    selector = argparse.ArgumentParser(add_help=False)
    selected_modes = selector.add_mutually_exclusive_group()
    selected_modes.add_argument('--damage-waypoint', action='store_true')
    selected_modes.add_argument('--super-charge', action='store_true')
    selected_modes.add_argument('--reveal-zone', action='store_true')
    selected_modes.add_argument('--forced-map', action='store_true')
    selected_modes.add_argument('--inline-type', action='store_true')
    selected_modes.add_argument('--flash-team', action='store_true')
    selected_modes.add_argument('--reshroud', action='store_true')
    selector.add_argument('--inputs', type=Path)
    selected, remaining = selector.parse_known_args()
    if selected.reshroud:
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
    else:
        if selected.inputs is not None:
            selector.error('--inputs requires --damage-waypoint or --super-charge')
        finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata,
                       argv=remaining,source_paths=SOURCE_PATHS)
