"""Original whole BulletType reader's retained Airburst byte histories."""
from pathlib import Path
import tempfile
from tools.native_oracle import finish_vectors,provenance,image_bytes,file_span
from tools.projectile_oracle.bridge_render_inputs import BulletReader
from tools.spatial_oracle.building_body_rules import RULES

def generate():
    with tempfile.TemporaryDirectory() as folder:
        m=BulletReader({},Path(folder),asset_names=())
        p=m.construct('SHOT')
        _,text=file_span(image_bytes(),0x401000,0x3E0000)
        scenario=m.read32(0xA8B230)
        rngs=(0x886B88,scenario+0x218,0xABE890)
        before=[bytes(m.u.mem_read(a,0x3F4)) for a in rngs]
        rows=[]
        for sections in ({'SHOT':{'Airburst':'yes'}},{'SHOT':{'Airburst':'invalid'}},
                         {'SHOT':{'Name':'missing key'}},{'OTHER':{'Name':'missing section'}},
                         {'SHOT':{'Airburst':'no'}},{'SHOT':{'Airburst':'invalid'}},
                         {'SHOT':{'Airburst':'yes'}},{'SHOT':{'Airburst':''}}):
            m.rules_cache(sections)
            m.invoke(0x46BEE0,p,(RULES,))
            rows.append(dict(sections=sections,
                airburst=bool(m.u.mem_read(p+0x294,1)[0])))
        after=[bytes(m.u.mem_read(a,0x3F4)) for a in rngs]
        if before!=after: raise RuntimeError('Whole reader changed RNG state')
        if text!=bytes(m.u.mem_read(0x401000,len(text))): raise RuntimeError('Original text changed')
        return dict(rows=rows)

def metadata():
    return provenance(scope='Original BulletType ctor46BBC0 and fullReadINI46BEE0; eight ordered synthetic Rules histories for retained Airburst byte+294.',
        assumptions=['Empty fixed ART and file transport; no physical retail assets are read. Existing original type/vector/INI/CRT initializer owners execute; supplied INI indexes contain only listed lexical strings.',
            'Type SHOT is reused through full readers, including missing section, missing key, invalid and empty tokens. All three RNG storages and complete original text must remain unchanged.',
            'This establishes reader retention, not a retail unit firing root, original save/load, whole Rules Process order, child fan, flight or damage.'],
        substitutions=['Existing native Reader allocator/free/TLS and file transport; asset_names=() explicitly reads no input files. Default unrelated ColorScheme storage is supplied by BulletReader.'],
        entry_points=dict(constructor=0x46BBC0,read_ini=0x46BEE0))

if __name__=='__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=metadata)
