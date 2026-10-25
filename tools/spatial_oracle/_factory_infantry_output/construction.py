"""Additive caller adapters over the sealed original-code observation owners.

No native gameplay function is implemented or answered here. The producer's
human radio ingress and construction visits call the frozen existing owners.
"""
from pathlib import Path
import argparse, ast, copy, hashlib, importlib.util, json, struct, traceback

HERE=Path(__file__).resolve().parent
PARENT=HERE
from .runtime import load_module
EXPECTED_MANIFEST='3ae9a5220673979933261a154bace85a2958404bfb8541bdb6a7b478b90fad15'

def load(name, p):
    return load_module(name, p)

def generate(control='construction'):
    adapter=load('sealed_admitted_prerequisite',PARENT/'admitted.py')
    producer=adapter.load('sealed_producer_prerequisite','producer.py')
    bc,native,er=producer.bc,producer.native,producer.er
    from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
    from unicorn.x86_const import UC_X86_REG_EAX,UC_X86_REG_EBP,UC_X86_REG_ESI,UC_X86_REG_EDI,UC_X86_REG_EBX,UC_X86_REG_ESP,UC_X86_REG_EIP,UC_X86_REG_ECX
    opening={};yard=0;visits=[];calls=[];writes=[];returns=[];hook_ids=[]
    def signed(u,p):return struct.unpack('<i',u.mem_read(p,4))[0]
    def snap(f,p):
        return dict(authorities=adapter.measured(f,p),construction=f.snapshot(p))
    def call(f,p,label,pc,this,*args):
        f.phase=label;row=dict(label=label,pc=f'0x{pc:08X}',this=this,args=list(args),before=snap(f,p),request_start=len(f.draws),advance_start=len(f.advances));visits.append(row)
        row['result']=bc.invoke(f.u,pc,this,*args)
        row.update(after=snap(f,p),requests=copy.deepcopy(f.draws[row['request_start']:]),advances=copy.deepcopy(f.advances[row['advance_start']:]))
        return row['result']
    def pre_unlimbo(f,p):
        nonlocal yard
        u,r=f.u,f.read32
        opening['before']=snap(f,p)
        opening['request_start']=len(f.draws);opening['advance_start']=len(f.advances)
        watched={0x43B740:'BuildingCtor',0x440580:'BuildingUnlimbo',0x44D5D0:'BuildingOwner',0x44D6A0:'EnterIdle',0x447780:'BeginMode',0x449A50:'MissionConstruction',0x445F80:'GrandOpening',0x4509D0:'UpdateAnimation',0x5B3060:'MissionAI',0x5B35E0:'QueueMission',0x5B3570:'Commence',0x65A970:'Transmit',0x43C2D0:'BuildingReceive',0x6F4AB0:'TechnoReceive',0x65ACB0:'FirstContact',0x423AC0:'AnimAI',0x725C70:'DeferredDrain'}
        def observe(_u,pc,size,_data):
            for ret,row in returns[:]:
                if pc==ret and u.reg_read(UC_X86_REG_ESP)>row['sp']:
                    row.update(result=u.reg_read(UC_X86_REG_EAX),after=snap(f,p));returns.remove((ret,row))
            if pc in watched:
                sp=u.reg_read(UC_X86_REG_ESP)
                row=dict(kind=watched[pc],pc=f'0x{pc:08X}',frame=r(bc.FRAME),phase=f.phase,this=u.reg_read(UC_X86_REG_ECX),sp=sp,caller=f'0x{r(sp):08X}',args=list(struct.unpack('<4I',u.mem_read(sp+4,16))))
                calls.append(row);returns.append((r(sp),row))
        ranges=[(p+0xAC,p+0xD4),(p+0xF8,p+0x110),(p+0x534,p+0x540),(p+0x6C8,p+0x6E8)]
        def written(_u,_access,a,n,v,_data):
            if any(a<b and lo<a+n for lo,b in ranges):writes.append(dict(pc=f'0x{u.reg_read(UC_X86_REG_EIP):08X}',phase=f.phase,frame=r(bc.FRAME),offset=hex(a-p),width=n,before=bytes(u.mem_read(a,n)).hex(),value=v))
        hook_ids.extend([u.hook_add(UC_HOOK_CODE,observe),u.hook_add(UC_HOOK_MEM_WRITE,written)])
        yard=f.allocate(0x1000);coord=f.allocate(16);u.mem_write(coord,bc.dwords(8*256+128,12*256+128,0))
        if signed(u,f.types['GACNST']+0xDF8)==-1:
            f.phase='original_selected_yard_type_registration';u.reg_write(UC_X86_REG_ESP,producer.SP);u.reg_write(UC_X86_REG_ESI,f.types['GACNST']);u.reg_write(UC_X86_REG_EBX,0);u.reg_write(UC_X86_REG_EDI,0xFFFFFFFF)
            bc.run_checked(u,0x45E2ED,0x45E362)
        call(f,p,'original_yard_whole_constructor',0x43B740,yard,f.types['GACNST'],er.HOUSE)
        call(f,p,'original_yard_whole_unlimbo',0x440580,yard,coord,64)
        opening['yard_admitted']=dict(pointer=yard,snapshot=f.snapshot(yard),type_index=signed(u,f.types['GACNST']+0xDF8),rng=f.rng())
        # Original HousePlace4FB1F1 sends this HELLO before the child's Unlimbo.
        call(f,p,'original_house_place_order_yard_hello_before_child_unlimbo',0x65A970,yard,2,0,p)
    def post_unlimbo(f,p):
        if f.u.mem_read(p+0x81,1)!=b'\0':raise ValueError('Original selected producer Unlimbo refused')
        call(f,p,'original_house_place_order_child_c_after_unlimbo',0x65A970,p,0xC,0,yard)
        call(f,p,'original_house_place_order_yard_break_after_placement',0x65ACB0,yard,3)
        opening['creation']=snap(f,p)
    # Extend only the immutable external caller adapter, retaining its native
    # constructor/Unlimbo/House corridors, hooks, guards and cleanup exclusions.
    derive=next(n for n in ast.parse((PARENT/'admitted.py').read_text(encoding='utf-8')).body if isinstance(n,ast.FunctionDef) and n.name=='derive_setup')
    class Derive(ast.NodeTransformer):
        def visit_Assign(self,n):
            if ast.unparse(n).startswith('fn = Hooks().visit(fn)'):
                return [n,*ast.parse("fn = HumanIngress().visit(fn)").body]
            if ast.unparse(n).startswith('ns = dict(vars(m))'):
                return [n,*ast.parse('ns.update(pre_unlimbo=pre_unlimbo,post_unlimbo=post_unlimbo)').body]
            return self.generic_visit(n)
    class HumanIngress(ast.NodeTransformer):
        def visit_Expr(self,n):
            if isinstance(n.value,ast.Call) and n.value.args and isinstance(n.value.args[0],ast.Constant) and n.value.args[0].value=='original_stock_gapile_unlimbo':
                return [*ast.parse('pre_unlimbo(f,producer)').body,n,*ast.parse('post_unlimbo(f,producer)').body]
            return self.generic_visit(n)
    derive=Derive().visit(derive)
    ns=dict(vars(adapter));ns.update(pre_unlimbo=pre_unlimbo,post_unlimbo=post_unlimbo,HumanIngress=HumanIngress)
    mod=ast.fix_missing_locations(ast.Module(body=[derive],type_ignores=[]))
    exec(compile(mod,str(HERE/'<derived-human-ingress-caller>'),'exec'),ns)
    def open_producer(f,p):
        u,r=f.u,f.read32;rows=[];completion=None
        for frame in range(1,160):
            u.mem_write(bc.FRAME,bc.dwords(frame));start=len(f.events);di=len(f.draws);ai=len(f.advances)
            f.phase='joined_original_building_construction_'+str(frame)
            u.reg_write(UC_X86_REG_ECX,p)
            bc.run_block(u,p,(0x43FB20,0x43FC39));bc.building_update(u,p)
            # Existing native Logic55B613 count/order. Unrelated objects and
            # yard logic are excluded; only the selected producer's live Anims
            # receive their original visit, including new appends this frame.
            i=0
            while i<r(0x87F788):
                q=r(r(0x87F77C)+4*i)
                anims=[r(r(0xA8E9AC)+4*j) for j in range(r(0xA8E9B8))]
                slots=[r(p+0x55C+4*j) for j in range(21)]
                if q in anims and (q in slots or r(q+0xCC)==p):
                    f.phase='joined_original_producer_anim_'+str(frame);bc.invoke(u,0x423AC0,q)
                i+=1
            event=r(p+0x6A0)
            if event and r(event+0x138):bc.invoke(u,0x4055C0,event)
            new=f.events[start:]
            if any(e.get('event')=='grand_opening' for e in new):completion=frame
            rows.append(dict(frame=frame,snapshot=snap(f,p),events=copy.deepcopy(new),requests=copy.deepcopy(f.draws[di:]),advances=copy.deepcopy(f.advances[ai:])))
            if completion is not None and frame==completion+1:break
        opening.update(completion_frame=completion,frames=rows,requests=copy.deepcopy(f.draws[opening['request_start']:]),advances=copy.deepcopy(f.advances[opening['advance_start']:]),before_yard_removal=snap(f,p))
        if completion is None:raise ValueError('Native admitted producer did not finish construction')
        call(f,p,'original_expiry_remove_research_yard_after_opening',0x5F65F0,yard)
        call(f,p,'original_deferred_remove_research_yard_after_opening',0x725C70,0)
        call(f,p,'original_house_power_after_yard_removal',0x508C30,er.HOUSE)
        opening.update(final=snap(f,p),calls=copy.deepcopy(calls),writes=copy.deepcopy(writes),visits=copy.deepcopy(visits),supplied_opening_fields=[],native_code_unchanged=f.code_unchanged())
        for h in hook_ids:u.hook_del(h)
        return opening
    # Remove all linked-prior opening stores from the immutable setup closure.
    gen=next(n for n in ast.parse((PARENT/'admitted.py').read_text(encoding='utf-8')).body if isinstance(n,ast.FunctionDef) and n.name=='generate')
    class Opening(ast.NodeTransformer):
        def visit_FunctionDef(self,n):
            if n.name=='setup':
                kept=[];remove=False
                for child in n.body:
                    text=ast.unparse(child)
                    if text.startswith('corpus ='):remove=True
                    if isinstance(child,ast.Return):
                        kept.extend(ast.parse("joined['construction'] = open_producer(f,p)").body);remove=False
                    if not remove:kept.append(child)
                n.body=kept;return n
            return self.generic_visit(n)
    gen=Opening().visit(gen);gen.name='observe_joined_prerequisites'
    # The immutable factory's loop is adapted by its existing AST State visitor;
    # change that caller's range by extending State.visit_Call, not gameplay.
    class QueueOrigin(ast.NodeTransformer):
        def visit_FunctionDef(self,n):
            if n.name=='visit_Call' and n.args.args and n.args.args[0].arg=='self':
                n.body.insert(0,ast.parse("if isinstance(node.func,ast.Name) and node.func.id=='range' and len(node.args)==2 and all(isinstance(a,ast.Constant) for a in node.args) and [a.value for a in node.args]==[0,400]:\n node.args=[ast.Call(func=ast.Name(id='queue_start_frame',ctx=ast.Load()),args=[],keywords=[]),ast.Constant(value=1600)]").body[0])
            if n.name=='joined_snapshot':return n
            return self.generic_visit(n)
    gen=QueueOrigin().visit(gen)
    # Existing generated queue namespace receives one external frame supplier.
    class QueueNS(ast.NodeTransformer):
        def visit_Expr(self,n):
            if ast.unparse(n).startswith('ns.update(setup_producer=setup, joined_snapshot=joined_snapshot)'):
                return [n,*ast.parse('ns.update(queue_start_frame=queue_start_frame)').body]
            return self.generic_visit(n)
    gen=QueueNS().visit(gen)
    ns2=dict(vars(adapter));ns2.update(derive_setup=ns['derive_setup'],open_producer=open_producer,queue_start_frame=lambda:opening['frames'][-1]['frame']+1)
    mod2=ast.fix_missing_locations(ast.Module(body=[gen],type_ignores=[]))
    exec(compile(mod2,str(HERE/'<derived-construction-queue-caller>'),'exec'),ns2)
    fault=None;receipt=None
    try:receipt=ns2['observe_joined_prerequisites']()
    except Exception as e:fault=dict(type=type(e).__name__,message=str(e),traceback=traceback.format_exc())
    return dict(schema_version=1,kind='native-admitted-construction-human-output-prerequisites',control=control,parent_manifest_sha256=EXPECTED_MANIFEST,opening=opening,admitted_queue=receipt,fault=fault,
        bounds=['Whole selected GAPILE constructor/Unlimbo/House admission plus exact human HELLO-before-Unlimbo/C/BREAK order, then original construction-owner visits; no producer opening field is written by this driver.',
                'The construction yard is separately whole native constructor/Unlimbo admitted at authored8,12, then originally expired after the producer opens; full MCV deployment/yard construction and building-factory charge/click ingress are excluded.',
                'Original producer header/update/mission corridors and its live Anim visits execute. Whole BuildingAI/TechnoAI/HouseAI/MainTick/render/network/checksum remain excluded.',
                'Authored coordinates, House/country and physical map cache priors remain explicit in linked native owners. Queue frames begin after construction, never rewind the simulation frame.'])

