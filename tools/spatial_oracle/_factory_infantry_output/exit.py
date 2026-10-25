"""Blocked exit and nonzero rally caller controls over unchanged native owners."""
from pathlib import Path
import argparse, ast, copy, hashlib, importlib.util, json, struct, traceback

HERE=Path(__file__).resolve().parent
BASE=HERE/'construction.py'
from .runtime import load_module
def load(name, p):
    return load_module(name, p)

def generate(control):
    if control != 'rally':
        raise ValueError('Private portable exit caller covers rally only; blocked controls remain linked P2 evidence')
    base=load('new_additive_construction_owner',BASE)
    adapter=load('sealed_exit_adapter',base.PARENT/'admitted.py')
    native_module=adapter.load('sealed_exit_native','producer.py')
    bc,er=native_module.bc,native_module.er
    from tools.spatial_oracle.refinery_dock import cell
    from unicorn import UC_HOOK_CODE,UC_QUERY_TIMEOUT
    from unicorn.x86_const import UC_X86_REG_EAX,UC_X86_REG_ECX,UC_X86_REG_ESP,UC_X86_REG_EIP
    vm={};setup=[];calls=[];returns=[];obstacles=[];rally_cell=cell(20,20)
    def signed(u,p):return struct.unpack('<i',u.mem_read(p,4))[0]
    def vector(f,p):
        ptr,n=f.read32(p+4),f.read32(p+16)
        if n>1024:raise ValueError(('unbounded vector',hex(p),n))
        return dict(pointer=ptr,count=n,items=list(struct.unpack('<'+'I'*n,f.u.mem_read(ptr,n*4))) if n else [])
    def state(f,p):
        return dict(frame=f.read32(bc.FRAME),authorities=adapter.measured(f,p),house_buildings=vector(f,er.HOUSE+0x68),logic=vector(f,0x87F778),pending=vector(f,0xB0F698),archive=f.read32(p+0x218),out_count=f.read32(0xA802C8),do_count=f.read32(0x8B41F8),
            perimeter=[dict(x=x,y=y,pointer=cell(x,y),head=f.read32(cell(x,y)+0xE4),land=signed(f.u,cell(x,y)+0xEC),occupation=[f.read32(cell(x,y)+o) for o in (0x124,0x128)]) for y in range(13,17) for x in range(13,18) if x in (13,17) or y in (13,16)])
    def invoke(f,p,label,pc,this,*args):
        f.phase=label;di,ai=len(f.draws),len(f.advances)
        row=dict(label=label,pc=f'0x{pc:08X}',this=this,args=list(args),before=state(f,p));setup.append(row)
        row['result']=bc.invoke(f.u,pc,this,*args)
        row.update(after=state(f,p),requests=copy.deepcopy(f.draws[di:]),advances=copy.deepcopy(f.advances[ai:]));return row['result']
    def configure(f,p):
        vm.update(f=f,p=p);u,r=f.u,f.read32
        watch={0x44EFB0:'dock_cell',0x51BF90:'infantry_can_enter',0x51DFF0:'infantry_unlimbo',0x443C60:'building_exit',0x4FB0E0:'house_place',0x70C610:'archive_assignment',0x51AA40:'infantry_destination',0x65A970:'radio_transmit',0x4C6780:'target_event_ctor',0x4C6CB0:'event_execute',0x443860:'set_rally',0x56DC20:'find_nearby_passable',0x4C9FF0:'factory_abandon',0x517D90:'infantry_destructor',0x5F65F0:'object_uninit',0x7258D0:'pointer_expiry',0x725C70:'deferred_drain',0x49FA70:'counter_decrement',0x49FA00:'counter_increment'}
        def observe(_u,pc,size,_data):
            if pc in (0x4439FD,0x443B4B):
                calls.append(dict(kind='winmm_wall_clock_transport',pc=f'0x{pc:08X}',frame=r(bc.FRAME),wall_ms=0,boundary='Imported timeGetTime wall timestamp only; original event eligibility remains native.'))
                u.reg_write(UC_X86_REG_EAX,0);u.reg_write(UC_X86_REG_EIP,pc+size)
            for ret,row in returns[:]:
                if pc==ret and u.reg_read(UC_X86_REG_ESP)>row['sp']:
                    row['result']=u.reg_read(UC_X86_REG_EAX);row['rng_after']=f.rng();returns.remove((ret,row))
                    if row['kind']=='dock_cell':row['selected_cell']=list(struct.unpack('<2h',u.mem_read(row['args'][0],4)))
                    if row['kind'] in ('house_place','factory_abandon','deferred_drain'):row['after']=state(f,p)
            if pc in watch:
                sp=u.reg_read(UC_X86_REG_ESP);this=u.reg_read(UC_X86_REG_ECX);args=list(struct.unpack('<6I',u.mem_read(sp+4,24)))
                row=dict(kind=watch[pc],pc=f'0x{pc:08X}',phase=f.phase,frame=r(bc.FRAME),this=this,sp=sp,caller=f'0x{r(sp):08X}',args=args,priority=r(0xA8E7AC))
                if row['kind']=='infantry_can_enter':row.update(cell=list(struct.unpack('<2h',u.mem_read(args[0]+0x24,4))),land=signed(u,args[0]+0xEC),head=r(args[0]+0xE4),occupation=[r(args[0]+o) for o in (0x124,0x128)])
                if row['kind']=='infantry_unlimbo':row['coordinate']=list(struct.unpack('<3i',u.mem_read(args[0],12)))
                if row['kind']=='radio_transmit':row.update(message=args[0],receiver=args[2])
                if row['kind']=='archive_assignment':row['archive_before']=r(this+0x218)
                if row['kind']=='target_event_ctor':row['rng_before']=f.rng()
                calls.append(row);returns.append((r(sp),row))
        vm['hook']=u.hook_add(UC_HOOK_CODE,observe)
        vm['before']=state(f,p)
        exit_ptr=r(f.types['GAPILE']+0xED4)
        vm['exit_list_startup']=dict(type=f.types['GAPILE'],foundation_index=r(f.types['GAPILE']+0xEF0),pointer=exit_ptr,table_before=bytes(u.mem_read(exit_ptr,0x78)).hex(),rng_before=f.rng(),crt_slot='0x008127F0',crt_pointer=r(0x8127F0))
        if r(0x8127F0)!=0x45C2F0:raise ValueError('Original Foundation Exit CRT registration changed')
        invoke(f,p,'original_foundation_exit_table_crt',0x45C2F0,0)
        vm['exit_list_startup'].update(table_after=bytes(u.mem_read(exit_ptr,0x78)).hex(),rng_after=f.rng())
        if control=='rally':
            xy=f.allocate(4);u.mem_write(xy,struct.pack('<2h',20,20))
            invoke(f,p,'original_selected_barracks_set_rally_click',0x443860,p,xy,0)
        vm['configured']=state(f,p)
    def should_stop(f,p,now,delivered,events):
        if len(delivered)!=2 or any(now['barracks']['contacts']):return False
        if control!='rally':return True
        # Native Foot destination assignment clears Archive at 4D8523. Stop at
        # its actual consumer handoff after both links clear, allowing a live
        # destination or an already arrived GI. Crowded final-cell pathfinding
        # remains the separately preserved attempt1 boundary, not this proof.
        return all(s['archive']==0 and (s['nav']==rally_cell or
            (s['nav']==0 and [x//256 for x in s['location'][:2]]==[20,20]))
            for s in now['delivered_infantry'])
    class FactoryControl(ast.NodeTransformer):
        def visit_Call(self,n):
            if control=='rally' and isinstance(n.func,ast.Attribute) and n.func.attr=='run_checked' and len(n.args)>1 and isinstance(n.args[1],ast.Constant) and n.args[1].value==0x51BAB0:
                for keyword in n.keywords:
                    if keyword.arg=='timeout_us':keyword.value=ast.Constant(value=200_000_000)
            return self.generic_visit(n)
        def visit_If(self,n):
            if ast.unparse(n.test).startswith("pending_product and u.mem_read(pending_product + 129, 1) == b'\\x00'"):
                n.test=ast.BoolOp(op=ast.And(),values=[n.test,ast.parse("u.mem_read(pending_product+0x90,1)==b'\\x01'",mode='eval').body])
            if ast.unparse(n.test)=='len(delivered) == 2 and (not any(now[\'barracks\'][\'contacts\']))':
                n.test=ast.parse('should_stop(f,barracks,now,delivered,events)',mode='eval').body
            return self.generic_visit(n)
    class AddControlQueue(ast.NodeTransformer):
        def visit_Assign(self,n):
            if ast.unparse(n).startswith('fn = State().visit(fn)'):return [n,*ast.parse('fn = FactoryControl().visit(fn)').body]
            return self.generic_visit(n)
    fn=next(n for n in ast.parse(BASE.read_text(encoding='utf-8')).body if isinstance(n,ast.FunctionDef) and n.name=='generate')
    class Variant(ast.NodeTransformer):
        def visit_FunctionDef(self,n):
            if n.name=='open_producer':
                n.body.insert(len(n.body)-1,ast.parse('configure(f,p)').body[0])
            return self.generic_visit(n)
        def visit_Assign(self,n):
            if ast.unparse(n).startswith('gen = QueueOrigin().visit(gen)'):return [n,*ast.parse('gen = AddControlQueue().visit(gen)').body]
            return self.generic_visit(n)
        def visit_Expr(self,n):
            if ast.unparse(n).startswith('ns2.update('):return [n,*ast.parse('ns2.update(FactoryControl=FactoryControl,should_stop=should_stop)').body]
            if ast.unparse(n).startswith("return [n, *ast.parse('ns.update(queue_start_frame=queue_start_frame)')"):
                return n
            return self.generic_visit(n)
        def visit_Constant(self,n):
            if n.value=='ns.update(queue_start_frame=queue_start_frame)':n.value='ns.update(queue_start_frame=queue_start_frame,should_stop=should_stop)'
            return n
    fn=Variant().visit(fn);fn.name='variant';module=ast.fix_missing_locations(ast.Module(body=[fn],type_ignores=[]))
    ns=dict(vars(base));ns.update(configure=configure,FactoryControl=FactoryControl,AddControlQueue=AddControlQueue,should_stop=should_stop)
    exec(compile(module,str(HERE/'<derived-exit-control-caller>'),'exec'),ns)
    fault=None;receipt=None;cleanup=[]
    try:
        receipt=ns['variant'](control)
        if receipt['admitted_queue']['queue']['fault']:
            vm['failed_visit_diagnostic']=dict(unicorn_wall_timeout=bool(vm['f'].u.query(UC_QUERY_TIMEOUT)),pc=hex(vm['f'].u.reg_read(UC_X86_REG_EIP)),frame=vm['f'].read32(bc.FRAME))
        if receipt['fault'] or receipt['admitted_queue']['queue']['fault']:
            raise ValueError(('joined observer fault',receipt['fault'],receipt['admitted_queue']['queue']['fault']))
        f,p=vm['f'],vm['p']
        vm['after_product_cleanup']=state(f,p)
        vm['final']=state(f,p);vm['native_code_unchanged']=f.code_unchanged()
        f.u.hook_del(vm['hook'])
    except Exception as e:fault=dict(type=type(e).__name__,message=str(e),traceback=traceback.format_exc())
    vm.pop('f',None);vm.pop('p',None);vm.pop('hook',None)
    return dict(schema_version=1,kind='native-blocked-exit-or-rally-joined-prerequisite',control=control,parent_manifest_sha256=base.EXPECTED_MANIFEST,joined=receipt,control_state=vm,obstacles=obstacles,setup=setup,calls=calls,fault=fault,
        bounds=['All original native constructors, placement, factory charges/completion, local event delivery, radio and delivered InfantryAI retain the frozen shared owners.',
                'Obstacles are whole native stock GAPOWR ctor/Unlimbo with original House admission/power; no raw occupation, cell-admission, fallback choice or output result is supplied. Their later building/construction AI is excluded.',
                'Rally control calls whole native443860 on authored clear20,20 with speech=false, including original nearby-passable search and event0x1E; it is dispatched by the unchanged local loop before subsequent PLACE output.',
                'The loop retains native visits and order. Refusal stops after two dispatched products and empty House factory/events; explicit original deferred cleanup follows. Rally stops only after both delivered GIs release contacts and native Foot consumes Archive into destination (or a GI has already arrived).',
                'Rally final crowded-cell arrival is not claimed: preserved attempt1 reached both GI cell20,20 and then hit the original Foot pathfinding 20M/50s bound at frame706. Its old observer also expected Archive retention, rejected by the original 4D851E call to70C610 (return4D8523), followed by native destination assignment4D852A.',
                'The rally consumer-handoff repeat keeps the original20M instruction bound and allows200s wall time per whole InfantryAI visit instead of50s; this is observer budget only. A remaining failure records Unicorn UC_QUERY_TIMEOUT rather than attributing it to native gameplay.',
                'The all-perimeter refusal leaves its eight authored live obstacle buildings admitted. The separate optional removal failed its frozen2M/10s budget in preserved blocked-all-attempt-2; it is outside held-product refusal cleanup and is not claimed.',
                'Full match startup, producer whole BuildingAI/HouseAI/MainTick/render/network/checksum and unrelated objects remain outside coverage.'])

