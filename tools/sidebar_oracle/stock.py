"""Decode named stock fixture inputs from original MIX archives.

This is fixture-input preparation, not a native blitter oracle. Production Rust
must independently parse the same SHA-identified bytes in retail input tests.
Requires cryptography for the encrypted MIX index.
"""
from pathlib import Path
import struct,zlib,os
from functools import lru_cache
from tools.native_oracle import configured_gamemd

def mix_hash(name):
    b=name.upper().encode();r=len(b)%4
    return zlib.crc32(b+(bytes([r])+bytes([b[len(b)-r]])*(3-r) if r else b''))

def mix(data):
    marker,flags=struct.unpack_from('<HH',data)
    if marker==0 and flags&2:
        # Only encrypted indexes need cryptography; importing this module does not.
        from cryptography.hazmat.primitives.ciphers import Cipher,modes
        from cryptography.hazmat.decrepit.ciphers.algorithms import Blowfish
        b=data[4:84][::-1];n=int('51bcda086d39fce4565160d651713fa2e8aa54fa6682b04aabdd0e6af8b0c1e6d1fb4f3daa437f15',16)
        key=((pow(int.from_bytes(b[:40],'big'),65537,n)<<312)+pow(int.from_bytes(b[40:],'big'),65537,n)).to_bytes(80,'big')[-56:][::-1]
        def dec(b):return Cipher(Blowfish(key),modes.ECB()).decryptor().update(b)
        count=struct.unpack_from('<H',dec(data[84:92]))[0];size=(6+count*12+7)&~7
        idx=dec(data[84:84+size]);body=84+size
    else:
        start=4 if marker==0 else 0;idx=data[start:];count=struct.unpack_from('<H',idx)[0];body=start+6+count*12
    return {key:data[body+offset:body+offset+size] for key,offset,size in [struct.unpack_from('<III',idx,6+i*12) for i in range(count)]}

def shp(data):
    zero,w,h,count=struct.unpack_from('<4H',data);assert zero==0
    frames=[]
    for i in range(count):
        x,y,fw,fh,fmt=struct.unpack_from('<4HB',data,8+i*24);off=struct.unpack_from('<I',data,28+i*24)[0]
        pixels=bytearray(fw*fh)
        if fmt&2:
            cursor=off
            for row in range(fh):
                end=cursor+struct.unpack_from('<H',data,cursor)[0];cursor+=2;col=0
                while cursor<end:
                    v=data[cursor];cursor+=1
                    if v: pixels[row*fw+col]=v;col+=1
                    else: col+=data[cursor];cursor+=1
                assert col<=fw
        else:pixels[:]=data[off:off+fw*fh]
        frames.append(dict(x=x,y=y,w=fw,h=fh,format=fmt,zero=pixels.count(0),pixels=bytes(pixels)))
    return w,h,frames

@lru_cache(maxsize=1)
def stock_archives():
 root=configured_gamemd().parent
 ra2=mix((root/'ra2.mix').read_bytes());yr=mix((root/'ra2md.mix').read_bytes())
 return {name:mix(parent[mix_hash(name)]) for name,parent in
   [('sidec01.mix',ra2),('sidec02.mix',ra2),('sidec02md.mix',yr)]}

def stock_bytes(archive,name):
 return stock_archives()[archive][mix_hash(name)]
