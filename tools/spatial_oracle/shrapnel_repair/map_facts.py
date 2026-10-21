"""Read-only raw retail MAP inspection; data extraction, not a gameplay oracle.
LCW cases follow src/util/lcw.rs; LZO is the installed liblzo2 decoder.
Only exact authored INI spellings are inspected. No layered rules are inferred.
"""
from pathlib import Path
import base64, ctypes, ctypes.util, functools, os, struct

def sections(b):
    out={}; section=None
    for lineno,line in enumerate(b.decode('latin1').splitlines(),1):
        t=line.strip()
        if not t or t.startswith(';'): continue
        if t.startswith('[') and t.endswith(']'):
            section=t[1:-1]; out.setdefault(section,[])
        elif section and '=' in line:
            key,val=line.split('=',1); out[section].append((key.strip(),val.strip(),lineno))
    return out

def lcw(b):
    out=bytearray(); i=0
    while i<len(b):
        c=b[i];i+=1
        if not c&128:
            n=(c>>4)+3; d=((c&15)<<8)|b[i];i+=1;p=len(out)-d
            assert 0<=p<len(out)
            for j in range(n):out.append(out[p+j])
        elif not c&64:
            n=c&63
            if not n: break
            out.extend(b[i:i+n]);i+=n
        elif c==254:
            n=struct.unpack_from('<H',b,i)[0]; i+=2
            out.extend(bytes([b[i]])*n);i+=1
        else:
            if c==255:
                n,p=struct.unpack_from('<HH',b,i);i+=4
            else:
                n=(c&63)+3;p=struct.unpack_from('<H',b,i)[0];i+=2
            assert p<len(out)
            for j in range(n):out.append(out[p+j])
    return bytes(out)

@functools.cache
def lzo2():
    """liblzo2, loaded on first LZO use so that importing this module needs no native library."""
    library=os.environ.get('VERA20K_LZO2_LIBRARY') or ctypes.util.find_library('lzo2')
    if not library:raise RuntimeError('Install liblzo2 or set VERA20K_LZO2_LIBRARY to its shared library')
    lib=ctypes.CDLL(library)
    lib.lzo1x_decompress_safe.argtypes=[ctypes.c_void_p,ctypes.c_size_t,ctypes.c_void_p,ctypes.POINTER(ctypes.c_size_t),ctypes.c_void_p]
    lib.lzo1x_decompress_safe.restype=ctypes.c_int
    return lib

def lzo(b,n):
    src=ctypes.create_string_buffer(b);dst=ctypes.create_string_buffer(n);size=ctypes.c_size_t(n)
    r=lzo2().lzo1x_decompress_safe(src,len(b),dst,ctypes.byref(size),None)
    assert r==0 and size.value==n,(r,size.value,n)
    return dst.raw[:size.value]

def unpack(rows,codec):
    b=base64.b64decode(''.join(r[1] for r in rows));i=0;out=bytearray()
    while i<len(b):
        sn,dn=struct.unpack_from('<HH',b,i);i+=4
        v=lcw(b[i:i+sn]) if codec=='lcw' else lzo(b[i:i+sn],dn)
        assert len(v)==dn,(len(v),dn);out.extend(v);i+=sn
    return bytes(out)


def decode_cells(raw):
    """Decode authored MAP cell packs; no derived native Cell state is inferred."""
    parsed=sections(raw)
    iso=unpack(parsed['IsoMapPack5'],'lzo')
    overlays=unpack(parsed['OverlayPack'],'lcw')
    frames=unpack(parsed['OverlayDataPack'],'lcw')
    cells={}
    for offset in range(0,len(iso),11):
        x,y=struct.unpack_from('<hH',iso,offset)
        if (x,y)==(0,0):break
        x,y,tile,subtile,level,ice=struct.unpack_from('<hHiBBB',iso,offset)
        linear=y*512+x
        cells[x,y]=dict(tile=tile,subtile=subtile,level=level,ice=ice,
                       overlay=None if overlays[linear]==255 else overlays[linear],
                       frame=frames[linear])
    return parsed,cells
