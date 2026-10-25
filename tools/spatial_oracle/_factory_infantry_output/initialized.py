"""Original registered startup then the existing human producer/two-GI caller.

P10 manifest 2864c48044891746cf597cd0d4f4a5e8e6554b208ddccfa47313d1a22bbe68ea.
Only original dispatcher invocations, observation and caller adaptation are added;
shared constructors/readers/Factory/Radio/Walk retain their imported native owners.
"""
from pathlib import Path
import ast, copy, hashlib, json, struct, sys, traceback
from .runtime import require
from . import runtime as rt, idle
TABLE = (0x81572C, 0x815764)
COUNT = 100000000
WALL = 500000000

class ContinueCaller(ast.NodeTransformer):

    def __init__(self):
        self.stops = self.budgets = 0

    def visit_FunctionDef(self, node):
        if node.name == 'should_stop':
            node.body = ast.parse("\nif len(delivered) != 2 or any(now['barracks']['contacts']):\n    return False\nreturn all(s['nav'] == 0 and s['archive'] == 0 and s['mission'] == 5\n    and not s['tether'] and not any(s['contacts'])\n    and [x//256 for x in s['location'][:2]] == [20,20]\n    for s in now['delivered_infantry'])\n").body
            self.stops += 1
        if node.name == 'visit_Call' and node.body and isinstance(node.body[0], ast.If) and ('run_checked' in ast.unparse(node.body[0].test)):
            first = node.body[0]
            for child in ast.walk(first):
                if isinstance(child, ast.Constant) and child.value == 200000000:
                    child.value = WALL
            first.body.extend(ast.parse("\nfor keyword in n.keywords:\n    if keyword.arg == 'count':\n        keyword.value = ast.Constant(value=100_000_000)\n").body)
            self.budgets += 1
        return self.generic_visit(node)

def generate(control, live_continuation=None, *, drive_startup=False):
    require(sys.flags.optimize == 0, 'Original emulation requires normal Python')
    require(control in ('no_rally', 'rally'), 'Unknown initialized native control')
    from tools.spatial_oracle._factory_infantry_output import runtime as rt, idle
    from tools.spatial_oracle import building_death_anims as death, building_construction as bc
    from tools import native_oracle as native, native_inspect
    from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE, UC_MEM_READ
    from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW, UC_X86_REG_EDI, UC_X86_REG_EBP, UC_X86_REG_EDX
    source = Path(idle.__file__).read_text(encoding='utf-8')
    image = native.image_bytes()
    _, table_bytes = native.file_span(image, TABLE[0], TABLE[1] - TABLE[0])
    entries = list(struct.unpack('<14I', table_bytes))
    require(entries == [7710848, 7710880, 7710928, 7710960, 7710992, 7711024, 7711056, 7711088, 7711152, 7711200, 7711248, 7711296, 7711312, 7711344], 'Original registered Walk table changed')
    startup = []
    calls = []
    writes = []
    consumer_reads = {}
    consumer_writes = {}
    hooks = []
    boundary_count = 0
    notifications = []
    gate_rows = []
    gate_rng = {}
    drive_observations = []

    def globals_snapshot(f):
        return dict(walk_region_b45bb0_b45c30=bytes(f.u.mem_read(11819952, 128)).hex(), foot_empty_8b3da8=list(struct.unpack('<3i', f.u.mem_read(9125288, 12))), walk_empty_b45be8=list(struct.unpack('<3i', f.u.mem_read(11820008, 12))), level_height_b45c28=struct.unpack('<i', f.u.mem_read(11820072, 4))[0], fpcw=f.u.reg_read(UC_X86_REG_FPCW), rng=f.rng())
    original_fixture = death.joined_fixture

    def fixture_boundary(*args, **kwargs):
        nonlocal boundary_count
        if drive_startup:
            require('drive_startup' not in kwargs, 'Duplicate selected startup input')
            kwargs['drive_startup'] = True
        f = original_fixture(*args, **kwargs)
        if drive_startup:
            drive_observations.append(f.drive_startup)
        boundary_count += 1
        require(boundary_count == 1, 'Expected one original runtime VM')
        u, r = (f.u, f.read32)
        require(bytes(u.mem_read(TABLE[0], len(table_bytes))) == table_bytes, 'Runtime registered table differs')
        before = globals_snapshot(f)
        old_phase = f.phase
        f.phase = 'original_registered_walk_crt_startup_before_selected_object_constructors'
        returns = []

        def code(_u, pc, size, _data):
            for ret, row in returns[:]:
                if pc == ret and u.reg_read(UC_X86_REG_ESP) > row['sp']:
                    row.update(result=u.reg_read(UC_X86_REG_EAX), after=globals_snapshot(f))
                    returns.remove((ret, row))
            if pc in entries or pc in (5058816, 8175315, 8163248, 5024832, 5025104, 5025248, 8150784):
                sp = u.reg_read(UC_X86_REG_ESP)
                row = dict(pc=hex(pc), caller=hex(r(sp)), sp=sp, this=u.reg_read(UC_X86_REG_ECX), args=list(struct.unpack('<4I', u.mem_read(sp + 4, 16))), before=globals_snapshot(f))
                calls.append(row)
                returns.append((r(sp), row))

        def written(_u, _access, address, size, value, _data):
            if address < 11820080 and 11819952 < address + size or (address < 9125300 and 9125288 < address + size):
                writes.append(dict(pc=hex(u.reg_read(UC_X86_REG_EIP)), address=address, size=size, value=value, before=bytes(u.mem_read(address, size)).hex()))
        h1 = u.hook_add(UC_HOOK_CODE, code)
        h2 = u.hook_add(UC_HOOK_MEM_WRITE, written)
        require(r(8466688) == 5058816, 'Original Foot EMPTY CRT registration changed')
        di, ai = (len(f.draws), len(f.advances))
        entry = dict(dispatcher=8175315, table_begin=TABLE[0], table_end=TABLE[1], table_bytes=table_bytes.hex(), entries=entries, before=before)
        startup.append(entry)
        try:
            entry['foot_empty_registered_slot'] = dict(table_begin=8466688, table_end=8466692, bytes=bytes(u.mem_read(8466688, 4)).hex(), before=globals_snapshot(f))
            entry['foot_empty_registered_slot']['result'] = bc.invoke(u, 8175315, 0, 8466688, 8466692)
            entry['foot_empty_registered_slot']['after'] = globals_snapshot(f)
            entry['result'] = bc.invoke(u, 8175315, 0, *TABLE)
            entry.update(after=globals_snapshot(f), requests=copy.deepcopy(f.draws[di:]), raw_advances=copy.deepcopy(f.advances[ai:]), native_code_unchanged=f.code_unchanged())
            require([int(x['pc'], 16) for x in calls if int(x['pc'], 16) in entries] == entries, 'Original dispatcher initializer order differs')
            require(entry['native_code_unchanged'], 'Original native code changed')
            print('original Walk CRT initialized height ' + str(entry['after']['level_height_b45c28']) + ', FPCW ' + hex(entry['after']['fpcw']), flush=True)
        except Exception:
            entry['failure'] = traceback.format_exc()
            entry['failed_state'] = globals_snapshot(f)
            raise
        finally:
            u.hook_del(h1)
            u.hook_del(h2)
            f.phase = old_phase

        def observed(memory, access, address, size, value, _data):
            if not (address < 11820080 and 11819952 < address + size):
                return
            pc = u.reg_read(UC_X86_REG_EIP)
            key = (pc, address, size)
            target = consumer_reads if access == UC_MEM_READ else consumer_writes
            row = target.setdefault(key, dict(pc=hex(pc), address=address, bytes=size, count=0, first_frame=r(bc.FRAME), last_frame=r(bc.FRAME), first_phase=f.phase, last_phase=f.phase, first_bytes=bytes(u.mem_read(address, size)).hex()))
            row.update(count=row['count'] + 1, last_frame=r(bc.FRAME), last_phase=f.phase, last_bytes=bytes(u.mem_read(address, size)).hex())
        hooks.extend([(u, u.hook_add(UC_HOOK_MEM_READ, observed, begin=11819952, end=11820079)), (u, u.hook_add(UC_HOOK_MEM_WRITE, observed, begin=11819952, end=11820079))])
        coordinate_returns = []
        gate_pcs = {7716418, 7716429, 7716443, 7716457, 7716498, 7716550, 7716592, 7716597, 7716614, 7716638, 7716640, 7716655, 7716708}

        def words(p, n):
            return list(struct.unpack('<' + 'i' * n, u.mem_read(p, 4 * n)))

        def refs():
            result = {}
            for name, raw in f.rng().items():
                digest = hashlib.sha256(bytes.fromhex(raw)).hexdigest()
                gate_rng.setdefault(digest, dict(stream=name, bytes=raw))
                result[name] = digest
            return result

        def gate_state(owner):
            loco = r(owner + 1652) - 4
            return dict(location=words(owner + 156, 3), tube_684=words(owner + 1668, 1)[0], nav=r(owner + 1444), archive=r(owner + 536), mission=words(owner + 172, 1)[0], queued=words(owner + 180, 1)[0], doing=words(owner + 1732, 1)[0], idle_latch_6b3=u.mem_read(owner + 1715, 1)[0], mission_timer=words(owner + 200, 3), movement_timer=words(owner + 1404, 3), foot_empty=words(9125288, 3), walk_empty=words(11820008, 3), height=words(11820072, 1)[0], fpcw=u.reg_read(UC_X86_REG_FPCW), walk=dict(base=loco, raw=bytes(u.mem_read(loco, 56)).hex(), destination=words(loco + 28, 3), paid_head=words(loco + 40, 3), moving=u.mem_read(loco + 52, 1)[0], motion=u.mem_read(loco + 54, 1)[0]), rng=refs())

        def observe_loop(_u, pc, size, _data):
            frame = r(bc.FRAME)
            sp = u.reg_read(UC_X86_REG_ESP)
            for ret, row in coordinate_returns[:]:
                if pc == ret and u.reg_read(UC_X86_REG_ESP) > row['sp']:
                    row.update(result=u.reg_read(UC_X86_REG_EAX), returned_coordinate=words(u.reg_read(UC_X86_REG_EAX), 3), after=gate_state(row['owner']))
                    coordinate_returns.remove((ret, row))
            if pc in (5223922, 5223934):
                producer = u.reg_read(UC_X86_REG_EDI)
                row = dict(pc=hex(pc), frame=frame, phase=f.phase, producer=producer, producer_vtable=r(producer), actual_vt48=r(r(producer) + 72), current_player_house=r(11025740), eax=u.reg_read(UC_X86_REG_EAX))
                if pc == 5223934:
                    row['returned_coordinate'] = words(u.reg_read(UC_X86_REG_EAX), 3)
                notifications.append(row)
            if pc == 6683248 and r(sp) == 5223990:
                packed = r(sp + 4)
                notifications.append(dict(pc=hex(pc), caller=hex(r(sp)), frame=frame, phase=f.phase, ecx_event=u.reg_read(UC_X86_REG_ECX), packed_cell=packed, cell=list(struct.unpack('<2h', struct.pack('<I', packed))), producer=u.reg_read(UC_X86_REG_EDI), current_player_house=r(11025740), stack=words(sp, 5)))
            if pc == 5094896 and 7716352 <= r(sp) < 7716608:
                owner = u.reg_read(UC_X86_REG_ECX)
                row = dict(pc=hex(pc), frame=frame, phase=f.phase, owner=owner, caller=hex(r(sp)), sp=sp, before=gate_state(owner))
                gate_rows.append(row)
                coordinate_returns.append((r(sp), row))
            if pc not in gate_pcs:
                return
            loco = u.reg_read(UC_X86_REG_EBP)
            try:
                owner = r(loco + 12)
            except Exception:
                return
            if r(owner) != 8302680:
                return
            row = dict(pc=hex(pc), frame=frame, phase=f.phase, owner=owner, eax=u.reg_read(UC_X86_REG_EAX), edx=u.reg_read(UC_X86_REG_EDX), state=gate_state(owner))
            if pc in (7716550, 7716614):
                row['queried_coordinate'] = words(u.reg_read(UC_X86_REG_EAX), 3)
            gate_rows.append(row)
        hooks.append((u, u.hook_add(UC_HOOK_CODE, observe_loop)))
        return f

    class CorrectedCaller(ContinueCaller):

        def visit_FunctionDef(self, node):
            node = super().visit_FunctionDef(node)
            if node.name == 'should_stop' and control == 'no_rally':
                node.body = ast.parse("\nif len(delivered)!=2 or any(now['barracks']['contacts']):return False\nreturn all(s['nav']==0 and s['archive']==0 and s['mission']==5\n    and not s['tether'] and not any(s['contacts']) for s in now['delivered_infantry'])\n").body
            if node.name == 'configure' and control == 'no_rally':
                node.body = [n for n in node.body if not (isinstance(n, ast.If) and ast.unparse(n.test) == "control == 'rally'")]
            return node
    continuation = CorrectedCaller()
    retained_vm = {}

    def retain_native_vm(f, final):
        require(not retained_vm, 'Expected one terminal native continuation boundary')
        retained_vm.update(f=f, final=copy.deepcopy(final))

    changed = {'observer': 0, 'capture': 0, 'failure_boundary': 0}
    live_captures = []
    fn = next((n for n in ast.parse(source).body if isinstance(n, ast.FunctionDef) and n.name == 'generate'))

    class Adapter(ast.NodeTransformer):

        def visit_Assign(self, node):
            text = ast.unparse(node)
            if text == 'fn = Observer().visit(fn)':
                changed['observer'] += 1
                return ast.parse('fn = corrected.visit(Observer().visit(fn))').body[0]
            if text == 'primary_boundary = []':
                changed['failure_boundary'] += 1
                return ast.parse("primary_boundary = [vm['snapshot'](p) for p in registry] if 'snapshot' in vm else []").body[0]
            if live_continuation is not None and text == 'code_unchanged = f.code_unchanged()':
                live_captures.append(text)
                return [node, ast.parse("retain_native_vm(vm['f'], final)").body[0]]
            return self.generic_visit(node)

        def visit_Try(self, node):
            if any((isinstance(n, ast.If) and ast.unparse(n.test).startswith("receipt['fault'] or") for n in node.body)):
                body = []
                for n in node.body:
                    if isinstance(n, ast.If) and ast.unparse(n.test) == 'capture is not None':
                        continue
                    if isinstance(n, ast.If) and ast.unparse(n.test).startswith("receipt['fault'] or"):
                        body.extend(ast.parse('if capture is not None:\n capture(receipt)').body)
                        changed['capture'] += 1
                    body.append(n)
                node.body = body
            return self.generic_visit(node)
    fn = Adapter().visit(fn)
    require(changed == dict(observer=1, capture=1, failure_boundary=1), 'Caller AST shape changed')
    require(len(live_captures) == int(live_continuation is not None), 'Live continuation AST boundary changed')
    module = ast.fix_missing_locations(ast.Module(body=[fn], type_ignores=[]))
    derived = rt.HERE / '<registered-startup-idle-caller>'
    ns = dict(vars(idle))
    ns.update(corrected=continuation, retain_native_vm=retain_native_vm, compare_primary=lambda c, a: dict(scope='Corrected original startup; comparison with historical zero-threshold control is reported after execution'))
    exec(compile(module, str(derived), 'exec'), ns)
    primaries = []
    observer = None
    failure = None
    death.joined_fixture = fixture_boundary
    try:
        observer = ns['generate'](False, 'rally', primaries.append)
        require(continuation.stops == 1 and continuation.budgets == 1, 'Selected exit adapter count differs')
        require(observer['fault'] is None and len(primaries) == 1, 'Original joined loop failed: ' + str(observer['fault']))
        require(observer['native_code_unchanged'], 'Original code changed')
        require(len(observer['primary_boundary']) == 2, 'Both native products missing')
        for p in observer['primary_boundary']:
            require(p['nav'] == p['archive'] == 0 and p['mission'] == 5 and (not p['tether']) and (not any(p['contacts'])), 'Native terminal mission/navigation/contact state not reached')
            require(p['walk'] and (not p['walk']['moving']) and (not p['walk']['motion']) and (not any(p['walk']['destination'])) and (not any(p['walk']['paid_head'])), 'Native private Walk terminal cleanup not reached')
    except Exception as exc:
        failure = dict(type=type(exc).__name__, message=str(exc), traceback=traceback.format_exc())
        if hasattr(exc, 'original_drive_startup'):
            drive_observations.append(exc.original_drive_startup)
    finally:
        death.joined_fixture = original_fixture
        for u, hook in hooks:
            u.hook_del(hook)
    consumer_instructions = {}
    for row in list(consumer_reads.values()) + list(consumer_writes.values()):
        pc = int(row['pc'], 16)
        if pc not in consumer_instructions:
            args = native_inspect.parser().parse_args(['disasm', hex(pc), '--bytes', '16'])
            consumer_instructions[pc] = native_inspect.inspect(image, args)['matches'][0]
    result = dict(schema=1, status='PASS' if failure is None else 'FAIL', selected_control=control, original_registered_walk_startup=startup, startup_calls=calls, startup_writes=writes, actual_house_place_notification_coordinates=notifications, walk_completion_gate_observations=gate_rows, gate_complete_rng_states=gate_rng, selected_loop_global_reads=list(consumer_reads.values()), selected_loop_global_writes=list(consumer_writes.values()), original_global_consumer_instructions=list(consumer_instructions.values()), full_original_primary=primaries[0] if primaries else None, full_original_gi_observer=observer, failure=failure, bounds=['Actual registered14-entry Walk group is executed through original7CBED3 after inherited unrelated prior actors and before selected GAPILE/E1 object constructors.', 'No scalar104, EMPTY, x87 result, object lifecycle, movement or RNG answer is supplied.', 'Original caller uses inherited PC53/chop FPCW; exact before/after bits and original x87/libm calls are retained.', 'No-rally uses the existing same exit/construction caller but suppresses the external authored SetRally click; the real default Archive remains native0. Foundation exit CRT executes for both controls.', 'Only stopping predicate and100M/500s wholeInfAI observer budgets differ from historical515/522 controls.', 'Restricted native Strip→deliveredInfAI→Factory→local event schedule, explicit physical map/House/prior actors and full-startup exclusions remain.', 'Historical zero-threshold515/522/707 receipts remain immutable and do not establish stock Walk completion timing.'])
    if drive_observations:
        result['original_registered_drive_startup'] = drive_observations[0]
    if live_continuation is not None:
        # Seal both existing complete controls and remove observation hooks
        # before the optional caller executes. Only the live VM is retained;
        # no constructor, Map, type, lifecycle or RNG state is reconstructed.
        if result['status'] != 'PASS':
            return result  # Keep the complete failed original receipt; no continuation runs.
        identity = rt.metadata()['initialized_controls'][control]
        require(rt.canonical_sha(rt.normalize(result['full_original_primary'])) == identity['complete_primary_sha256'],
                'Complete original primary changed before live continuation')
        require(rt.canonical_sha(rt.normalize(result['full_original_gi_observer'])) == identity['complete_private_sha256'],
                'Complete original private observation changed before live continuation')
        require(set(retained_vm) == {'f', 'final'}, 'Live terminal native VM missing')
        result = copy.deepcopy(result)
        result['live_continuation'] = live_continuation(retained_vm['f'], retained_vm['final'])
    return result
