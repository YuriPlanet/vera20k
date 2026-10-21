"""Original publication interleaving caller over the existing initialized owner.

No native implementation is copied. Only the selected stopping predicate changes
(one actual PLACE, frame268), and one whole original Building WhatAction/cell
ClickedAction is supplied immediately before or after that Strip visit. The
original registered startup, constructors, INI readers, payment, events, and
Unlimbo remain in initialized.generate and its imported native owners.
"""
from pathlib import Path
import ast
import copy
import hashlib
import struct
import traceback

from tools.spatial_oracle import building_construction as bc
from tools.spatial_oracle import building_death_anims as death
from tools.spatial_oracle import engineer_repair_admission as er
from tools.spatial_oracle._factory_infantry_output import initialized, runtime as rt
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESP, UC_X86_REG_EIP

HERE = Path(__file__).resolve().parent
EXPECTED_P10 = '2864c48044891746cf597cd0d4f4a5e8e6554b208ddccfa47313d1a22bbe68ea'


def derived_generate():
    fn = next(n for n in ast.parse(Path(initialized.__file__).read_text(encoding='utf-8')).body
              if isinstance(n, ast.FunctionDef) and n.name == 'generate')
    changed = dict(stop=0, product_count=0, terminal_assertions=0)

    class StopAfterFirstPlace(ast.NodeTransformer):
        def visit_Constant(self, node):
            if isinstance(node.value, str) and node.value.startswith('\nif len(delivered)!=2 or any(now['):
                node.value = "return now['frame']==268 and len(delivered)==1\n"
                changed['stop'] += 1
            return node

        def visit_Call(self, node):
            if isinstance(node.func, ast.Name) and node.func.id == 'require' and len(node.args) == 2 and \
                    isinstance(node.args[1], ast.Constant) and node.args[1].value == 'Both native products missing':
                node.args[0] = ast.parse("len(observer['primary_boundary']) == 1", mode='eval').body
                node.args[1] = ast.Constant(value='First native actual product missing')
                changed['product_count'] += 1
            return self.generic_visit(node)

        def visit_For(self, node):
            if ast.unparse(node.iter) == "observer['primary_boundary']":
                changed['terminal_assertions'] += 1
                return None
            return self.generic_visit(node)

    fn = StopAfterFirstPlace().visit(fn)
    rt.require(changed == dict(stop=1, product_count=1, terminal_assertions=1),
               'Frozen initialized caller shape changed: ' + str(changed))
    module = ast.fix_missing_locations(ast.Module(body=[fn], type_ignores=[]))
    ns = dict(vars(initialized))
    exec(compile(module, str(HERE / '<derived-first-native-place-boundary>'), 'exec'), ns)
    return ns['generate'], dict(changes=changed,
        original_owner=str(Path(initialized.__file__).resolve().relative_to(rt.REPO_ROOT)))


def generate(order, assets=None):
    rt.require(order in ('before_strip', 'after_strip'), 'Unknown publication phase control')
    rt.configure_assets(assets)
    rt.require(rt.metadata()['publication_phase']['parent_manifest_sha256'] == EXPECTED_P10,
               'Initialized parent seal identity changed')
    generate, adaptation = derived_generate()
    vm = {}
    boundaries = []
    calls = []
    pending = []
    streams = {}
    original_fixture = death.joined_fixture
    original_invoke = bc.invoke

    def refs(f):
        result = {}
        for stream, raw in f.rng().items():
            rt.require(len(bytes.fromhex(raw)) == 0x3F4, 'Incomplete RNG buffer')
            digest = hashlib.sha256(bytes.fromhex(raw)).hexdigest()
            streams.setdefault(stream + ':' + digest, dict(stream=stream, bytes=raw, sha256=digest))
            result[stream] = stream + ':' + digest
        return result

    def ring(f, out=True):
        r = f.read32
        count, head, tail, base = (0xA802C8, 0xA802CC, 0xA802D0, 0xA802D4) if out else \
            (0x8B41F8, 0x8B41FC, 0x8B4200, 0x8B4204)
        n = r(count)
        rt.require(n <= 128, 'Unbounded original event ring')
        return dict(count=n, head=r(head), tail=r(tail),
                    events=[dict(slot=(r(head)+i)&127,
                                 bytes=bytes(f.u.mem_read(base+(((r(head)+i)&127)*0x6F), 0x6F)).hex())
                            for i in range(n)])

    def snapshot(f):
        r, u = f.read32, f.u
        factory = r(er.HOUSE + 0x53B0)
        product = r(factory + 0x58) if factory else 0
        building = vm.get('producer', 0)
        return dict(frame=r(bc.FRAME), priority=r(0xA8E7AC), cursor=r(r(0xA8B230)+0x214),
            rng=refs(f), house=dict(pointer=er.HOUSE, credits=struct.unpack('<i', u.mem_read(er.HOUSE+0x30C, 4))[0],
                infantry_factory=factory),
            producer=None if not building else dict(pointer=building, type=r(building+0x520),
                vtable=r(building), archive=r(building+0x218), location=list(struct.unpack('<3i', u.mem_read(building+0x9C, 12)))),
            factory=None if not factory else dict(pointer=factory, stage=r(factory+0x24),
                changed=u.mem_read(factory+0x5D, 1)[0], product=product,
                balance=struct.unpack('<i', u.mem_read(factory+0x60, 4))[0], suspended=u.mem_read(factory+0x70, 1)[0]),
            product=None if not product else dict(pointer=product, id=r(product+0x10),
                vtable=r(product), limbo=u.mem_read(product+0x81, 1)[0], alive=u.mem_read(product+0x90, 1)[0],
                archive=r(product+0x218), nav=r(product+0x5A4),
                location=list(struct.unpack('<3i', u.mem_read(product+0x9C, 12))),
                mission=struct.unpack('<i', u.mem_read(product+0xAC, 4))[0]),
            outlist=ring(f), dolist=ring(f, False))

    watch = {0x447540:'building_what_action_cell', 0x4436F0:'building_clicked_action_cell',
             0x443860:'set_rally', 0x4C6780:'target_event_constructor', 0x4C6AE0:'place_event_constructor',
             0x4C9C60:'factory_take_changed', 0x4CA130:'factory_is_complete',
             0x4C6CB0:'event_execute', 0x4FB0E0:'house_place', 0x443C60:'building_exit',
             0x51DFF0:'infantry_unlimbo', 0x70C610:'archive_assignment'}

    def capture_fixture(*args, **kwargs):
        f = original_fixture(*args, **kwargs)
        rt.require('f' not in vm, 'Expected one warmed original VM')
        vm['f'] = f
        u, r = f.u, f.read32

        def observe(_u, pc, size, _data):
            if r(bc.FRAME) != 268:
                return
            # Existing exact WINMM transport only; the class click reaches its
            # original event append without replacing native eligibility.
            if pc in (0x4439FD, 0x443B4B):
                calls.append(dict(kind='timeGetTime_import_transport', pc=hex(pc),
                    frame=268, supplied_wall_milliseconds=0,
                    boundary='Imported WINMM timestamp only; event stamp/eligibility remain native'))
                u.reg_write(UC_X86_REG_EAX, 0)
                u.reg_write(UC_X86_REG_EIP, pc+size)
            for ret, row in pending[:]:
                if pc == ret and u.reg_read(UC_X86_REG_ESP) > row['sp']:
                    row.update(result=u.reg_read(UC_X86_REG_EAX), returned_sp=u.reg_read(UC_X86_REG_ESP),
                               after=snapshot(f))
                    if row['kind'].endswith('event_constructor'):
                        row['returned_event_bytes'] = bytes(u.mem_read(row['this'], 0x6F)).hex()
                    pending.remove((ret, row))
            if pc not in watch:
                return
            sp, this = u.reg_read(UC_X86_REG_ESP), u.reg_read(UC_X86_REG_ECX)
            row = dict(kind=watch[pc], pc=hex(pc), phase=f.phase, frame=268,
                       caller=hex(r(sp)), sp=sp, this=this, edx=u.reg_read(UC_X86_REG_EDX),
                       args=list(struct.unpack('<6I', u.mem_read(sp+4, 24))), before=snapshot(f))
            if pc == 0x4C6CB0:
                row['event_bytes'] = bytes(u.mem_read(this, 0x6F)).hex()
            if pc == 0x51DFF0:
                row['requested_xyz'] = list(struct.unpack('<3i', u.mem_read(r(sp+4), 12)))
            calls.append(row)
            pending.append((r(sp), row))

        vm['hook'] = u.hook_add(UC_HOOK_CODE, observe)
        return f

    def click(f):
        u, r = f.u, f.read32
        house_count, house_vector = r(er.HOUSE+0x78), r(er.HOUSE+0x6C)
        rt.require(house_count == 1, 'Expected exactly the admitted stock GAPILE')
        p = r(house_vector)
        vm['producer'] = p
        rt.require(r(p+0x520) == f.types['GAPILE'], 'Selected producer differs')
        xy = f.allocate(4)
        u.mem_write(xy, struct.pack('<2h', 20, 20))
        row = dict(label='authored_clear_cell_actual_building_click', producer=p,
                   cell=[20,20], before=snapshot(f), request_start=len(f.draws), advance_start=len(f.advances))
        boundaries.append(row)
        old_phase = f.phase
        f.phase = 'external_actual_building_what_action_then_cell_click_' + order
        action = original_invoke(u, r(r(p)+0x70), p, xy, 0, 0)
        row['what_action'] = dict(entry=hex(r(r(p)+0x70)), args=[xy,0,0], result=action)
        rt.require(action == 1, 'Whole stock human Building WhatAction did not admit Move/rally')
        row['click'] = dict(entry=hex(r(r(p)+0x140)), args=[action,xy,0,0])
        row['click']['result'] = original_invoke(u, r(r(p)+0x140), p, action, xy, 0, 0)
        row.update(after=snapshot(f), requests=copy.deepcopy(f.draws[row['request_start']:]),
                   raw_advances=copy.deepcopy(f.advances[row['advance_start']:]))
        f.phase = old_phase

    def invoke(u, entry, this, *args):
        f = vm.get('f')
        if f is None or entry != 0x6A8B30 or f.read32(bc.FRAME) != 268:
            return original_invoke(u, entry, this, *args)
        rt.require(not vm.get('injected'), 'Selected Strip was visited twice')
        vm['injected'] = True
        r = f.read32
        vm['producer'] = r(r(er.HOUSE+0x6C))
        row = dict(label='actual_strip268_publication', entry=hex(entry), this=this, args=list(args), before=snapshot(f))
        boundaries.append(row)
        if order == 'before_strip':
            click(f)
        row['immediately_before_strip'] = snapshot(f)
        row['result'] = original_invoke(u, entry, this, *args)
        row['immediately_after_strip'] = snapshot(f)
        if order == 'after_strip':
            click(f)
        row['after'] = snapshot(f)
        return row['result']

    death.joined_fixture = capture_fixture
    bc.invoke = invoke
    receipt, failure = None, None
    try:
        receipt = generate('no_rally')
        rt.require(receipt['status'] == 'PASS', 'Original caller failed: ' + str(receipt['failure']))
        rt.require(vm.get('injected'), 'Selected original Strip/input boundary was not reached')
        vm['final'] = snapshot(vm['f'])
        vm['native_code_unchanged'] = vm['f'].code_unchanged()
        rt.require(vm['native_code_unchanged'], 'Native code changed')
        delivered = receipt['full_original_gi_observer']['primary_boundary']
        rt.require(len(delivered)==1 and delivered[0]['limbo']==0, 'Actual first product was not placed')
        event_order = [bytes.fromhex(x['event_bytes'])[0] for x in calls if x['kind']=='event_execute']
        # EventClass stores type at offset0; retain complete111-byte receipts
        # rather than projecting values from a caller-supplied command object.
        vm['actual_executed_event_types'] = event_order
    except Exception as exc:
        failure = dict(type=type(exc).__name__, message=str(exc), traceback=traceback.format_exc())
        if 'f' in vm:
            vm['failed_state'] = snapshot(vm['f'])
    finally:
        death.joined_fixture = original_fixture
        bc.invoke = original_invoke
        if 'hook' in vm:
            vm['f'].u.hook_del(vm['hook'])
    vm.pop('f', None)
    vm.pop('hook', None)
    return dict(schema=1, status='PASS' if failure is None else 'FAIL', order=order,
        inherited_parent=dict(packet='P10', manifest_sha256=EXPECTED_P10),
        caller_adaptation=adaptation, control_state=vm,
        boundaries=boundaries, original_calls=calls, complete_rng_buffers=streams,
        full_original_initialized_first_place=receipt, failure=failure,
        bounds=['Restricted warmed P10 original schedule and physical/House/type priors are reused. No whole MainTick, window pump, pixel discovery, network, other actors, or terminal movement is executed.',
                'Whole original stock human Building WhatAction447540 establishes admitted action1; its actual vt1404436F0 caller4437A8 reaches443860 and appends native event1E. Selected input cell20,20 and the before/afterStrip interleaving are authored caller inputs.',
                'BeforeStrip is reachable from original MainTick55D3A2/55D3B6 message pump, GameWndProc777640→TacticalMouse6930A0 WM_LBUTTONUP→BandBox4AB9B0→Selection4AE750→vt1404AE8C4. This upstream route is instruction-established, not executed window/pixel parity.',
                'AfterStrip is an explicit sibling control demonstrating the event-order consequence; it does not assert equivalence to every fresh-click input route.',
                'No Factory stage, changed byte, held object, rally outcome, coordinate, mission, event eligibility, or RNG answer is supplied. Original code sections are guarded. WINMM imported timestamps use the existing supplied0 transport.',
                'Stop after actual firstPLACE268, before its next InfantryAI visit. No retirement/rally arrival/whole-object completion claim. Original no-rally/rally seals and current helper files are untouched.'])


