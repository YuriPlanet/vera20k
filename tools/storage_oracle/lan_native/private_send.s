.intel_syntax noprefix
.text
.global _stub
# IPXManager5414C0: thiscall(payload, length, guaranteed, connectionID), RET16.
# Separate bounded transcript; no connection or command-queue writes.
_stub:
 pushfd
 pushad
 cld
 mov ebp, esp
 mov eax, 1
 lock xadd DWORD PTR [0x11000008], eax
 cmp eax, 4096
 jae overflow
 mov ebx, eax
 shl eax, 12
 lea edx, [eax+0x11001000]
 mov eax, DWORD PTR [ebp+36]
 mov DWORD PTR [edx], eax
 mov ecx, DWORD PTR [ebp+44]
 mov DWORD PTR [edx+4], ecx
 cmp ecx, 4064
 ja badlength
 mov eax, DWORD PTR [ebp+48]
 mov DWORD PTR [edx+8], eax
 mov eax, DWORD PTR [ebp+52]
 mov DWORD PTR [edx+12], eax
 lea edi, [edx+32]
 mov esi, DWORD PTR [ebp+40]
 rep movsb
 inc ebx
 mov DWORD PTR [edx+16], ebx
 jmp done
badlength:
 lock inc DWORD PTR [0x11000010]
 jmp done
overflow:
 lock inc DWORD PTR [0x1100000c]
done:
 popad
 popfd
 mov eax, 1
 ret 0x10
