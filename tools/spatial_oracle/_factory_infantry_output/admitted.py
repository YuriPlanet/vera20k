"""Join unchanged native producer owners to the existing human queue observer.

The two immutable observation drivers retain their bodies. This additive AST
adapter changes only their caller setup/lifetime and adds measured snapshots;
it never implements or answers a gamemd gameplay body.
"""
from pathlib import Path
import argparse, ast, copy, hashlib, importlib.util, json, struct

HERE = Path(__file__).resolve().parent
from .runtime import load_module

def load(name, filename):
    return load_module(name, HERE/filename)

def measured(f, p):
    u,r=f.u,f.read32
    signed=lambda a:struct.unpack('<i',u.mem_read(a,4))[0]
    def counter(offset):
        a=0x200A0000+offset;ptr,n=r(a+4),r(a+8)
        if n>1024:raise ValueError(('unbounded counter',hex(offset),n))
        return dict(pointer=ptr,size=n,total=signed(a+16),values=list(struct.unpack('<'+'i'*n,u.mem_read(ptr,n*4))) if n else [])
    return dict(active_game=u.mem_read(0xA8E9A0,1)[0],rng=f.rng(),
        producer=dict(pointer=p,abstract_flags=r(p+0x14),type=r(p+0x520),type_index=signed(f.types['GAPILE']+0xDF8),
            alive=u.mem_read(p+0x90,1)[0],limbo=u.mem_read(p+0x81,1)[0],logic=u.mem_read(p+0x98,1)[0],marked=u.mem_read(p+0x74,1)[0],
            primary=u.mem_read(p+0x3D3,1)[0],mission=signed(p+0xAC),queued=signed(p+0xB4),status=signed(p+0xBC),
            mission_timer=list(struct.unpack('<3i',u.mem_read(p+0xC8,12))),stage=signed(p+0xF8),
            body_timer=list(struct.unpack('<4i',u.mem_read(p+0x100,16))),body=signed(p+0x534),queued_body=signed(p+0x538),
            construction_done=u.mem_read(p+0x6DD,1)[0],placed=u.mem_read(p+0x6E4,1)[0],old_operational=u.mem_read(p+0x6C8,1)[0],
            radio=dict(vtable=r(p+0xE0),pointer=r(p+0xE4),size=r(p+0xE8),contacts=[r(r(p+0xE4)+4*i) for i in range(r(p+0xE8))])),
        house=dict(owned_quantity=signed(0x200A02F0),power=signed(0x200A53A4),drain=signed(0x200A53A8),
            factory_counts={hex(o):signed(0x200A0000+o) for o in (0x5378,0x537C,0x5380,0x5384,0x5388)},
            counters={hex(o):counter(o) for o in (0x5500,0x5550,0x5528,0x5578,0x55C8)},
            dirty=list(u.mem_read(0x200A5778,2))))


def derive_setup(m, admitted):
    fn=next(n for n in ast.parse(Path(m.__file__).read_text(encoding='utf-8')).body if isinstance(n,ast.FunctionDef) and n.name=='generate')
    # A supplied current VM is shared with the queue. The original native
    # observer, checked execution owner and constructor corridors are reused.
    for i,node in enumerate(fn.body):
        if isinstance(node,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='f' for t in node.targets):
            fn.body[i]=ast.parse('f = current_fixture').body[0]
        if isinstance(node,ast.Assign) and any(isinstance(t,ast.Tuple) and [getattr(x,'id',None) for x in t.elts]==['producer','coordinate'] for t in node.targets):
            fn.body[i]=ast.parse('producer, coordinate = current_producer, f.allocate(16)').body[0]
        if isinstance(node,ast.Expr) and isinstance(node.value,ast.Call) and isinstance(node.value.func,ast.Attribute) and node.value.func.attr=='mem_write' and isinstance(node.value.args[0],ast.Name) and node.value.args[0].id=='coordinate':
            fn.body[i]=ast.parse('u.mem_write(coordinate, bc.dwords(14*256+128,14*256+128,0))').body[0]
        if isinstance(node,ast.Try):
            kept=[]
            for n in node.body:
                text=ast.unparse(n)
                if any(label in text for label in ('original_find_e1_factory_after_native_admission','original_native_admitted_producer_uninit','original_native_admitted_producer_deferred_drain','original_removed_house_power_consumer','original_occupied_gapowr_cell_unlimbo_control')):continue
                if text.startswith('blocked =') or 'u.mem_write(blocked,' in text:continue
                kept.append(n)
            node.body=kept
    class Hooks(ast.NodeTransformer):
        def visit_Expr(self,node):
            if isinstance(node.value,ast.Call) and isinstance(node.value.func,ast.Attribute) and node.value.func.attr=='hook_add':
                target='h_code' if 'UC_HOOK_CODE' in ast.unparse(node) else 'h_write'
                return ast.Assign(targets=[ast.Name(id=target,ctx=ast.Store())],value=node.value)
            return self.generic_visit(node)
        def visit_Return(self,node):
            # Only the outer returned receipt owns these observer hooks.
            if isinstance(node.value,ast.Call) and isinstance(node.value.func,ast.Name) and node.value.func.id=='dict' and any(k.arg=='schema_version' for k in node.value.keywords):
                return ast.parse('u.hook_del(h_code)\nu.hook_del(h_write)').body + [node]
            return node
    fn=Hooks().visit(fn);fn.name='setup_shared_producer'
    ns=dict(vars(m));ns.update(current_fixture=admitted['f'],current_producer=admitted['p'])
    module=ast.fix_missing_locations(ast.Module(body=[fn],type_ignores=[]))
    exec(compile(module,str(HERE/'<derived-producer-setup>'),'exec'),ns)
    return ns['setup_shared_producer']


def generate():
    factory=load('immutable_factory_observer','factory.py')
    producer=load('immutable_producer_observer','producer.py')
    inputs=factory.death.joined_inputs(factory.ASSETS,damage_fires=True)
    joined={}
    def setup(f,inputs,p):
        joined.update(f=f,p=p)
        joined['initial_runtime_prior']=measured(f,p)
        receipt=derive_setup(producer,joined)(inputs)
        joined['admission']=copy.deepcopy(receipt)
        if receipt['fault'] or not receipt['original_code_unchanged']:raise ValueError(('producer admission',receipt['fault']))
        return dict(whole_native=True,abstract_flags=f.read32(p+0x14),admission_ref='admission')
    def joined_snapshot(f,p,old):
        return old | {'admitted_authorities':measured(f,p)}
    fn=next(n for n in ast.parse(Path(factory.__file__).read_text(encoding='utf-8')).body if isinstance(n,ast.FunctionDef) and n.name=='generate')
    # Semantic caller setup selection replaces fragile original line numbers. No native
    # owners, whole-native bodies or queue/movement decisions are replaced.
    body=[]
    for n in fn.body:
        text = ast.unparse(n)
        if text.startswith("f.building('GAPILE', barracks, radio,"):
            body.extend(ast.parse('barracks_constructor = setup_producer(f, inputs, barracks)').body)
            continue
        if text == 'barracks_constructor = f.building_constructor':
            continue
        if text in ("u.mem_write(barracks + 116, b'\\x00')", "assert bc.invoke(u, 4452736, barracks, 3) & 255 == 1",
                    'u.mem_write(barracks + 172, bc.dwords(5))', 'u.mem_write(barracks + 180, bc.dwords(-1))',
                    'u.mem_write(barracks + 1332, bc.dwords(1))', 'f.house_prior(er.HOUSE, (barracks,))'):
            continue
        body.append(n)
    fn.body=body
    class State(ast.NodeTransformer):
        def visit_FunctionDef(self,node):
            if node.name=='state':
                for n in node.body:
                    if isinstance(n,ast.Return):n.value=ast.Call(func=ast.Name(id='joined_snapshot',ctx=ast.Load()),args=[ast.Name(id='f',ctx=ast.Load()),ast.Name(id='barracks',ctx=ast.Load()),n.value],keywords=[])
                return node
            return self.generic_visit(node)
        def visit_Call(self,node):
            if isinstance(node.func,ast.Name) and node.func.id=='range' and len(node.args)==2 and all(isinstance(a,ast.Constant) for a in node.args) and [a.value for a in node.args]==[0,400]:node.args[1].value=1600
            return self.generic_visit(node)
    fn=State().visit(fn);fn.name='joined_admitted_queue';ns=dict(vars(factory));ns.update(setup_producer=setup,joined_snapshot=joined_snapshot)
    module=ast.fix_missing_locations(ast.Module(body=[fn],type_ignores=[]))
    exec(compile(module,str(HERE/'<derived-queue-caller>'),'exec'),ns)
    receipt=ns['joined_admitted_queue']('queue','physical_clear',inputs)
    joined.pop('f');joined.pop('p')
    return dict(schema_version=1,kind='native-admitted-producer-human-queue-join',joined=joined,queue=receipt,
        corrections=['Legacy producer fixture/House prior writes and duplicate Mark are removed from the caller adapter.',
                     'Only known linked construction-ready fields are supplied and listed. Whole native constructor/Unlimbo/House roster/radio/type-counter state is preserved.',
                     'Earlier ActiveGame0 inference is rejected; runtime before/after is1. Native B0F720 expiry membership is retained.'],
        bounds=['Whole producer construction timing, later producer BuildingAI/HouseAI, full startup/match scheduler/render/network/checksum are excluded.',
                'Original native House power consumer executes after admission; no extra human power producer is supplied. Two-product loop is bounded at1600 frames.',
                'The retained original queue observer reports its historical supplied-producer bounds; this additive adapter and joined admission/opening receipt supersede those bounds only for the new control.'])

