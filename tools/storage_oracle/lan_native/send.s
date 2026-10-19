.intel_syntax noprefix
.text
.global _stub
_stub:
 pushfd
 pushad
 cld
 mov ebp, esp
 mov eax, 1
 lock xadd DWORD PTR [0x10000008], eax
 cmp eax, 4096
 jae overflow
 mov ebx, eax
 shl eax, 9
 lea edx, [eax+0x10001000]
 mov eax, DWORD PTR [ebp+36]
 mov DWORD PTR [edx], eax
 mov ecx, DWORD PTR [ebp+44]
 mov DWORD PTR [edx+4], ecx
 cmp ecx, 455
 ja badlength
 mov eax, DWORD PTR [ebp+48]
 mov DWORD PTR [edx+8], eax
 mov esi, DWORD PTR [ebp+52]
 test esi, esi
 jz nodest
 mov DWORD PTR [edx+12], 1
 lea edi, [edx+16]
 mov ecx, 3
 rep movsd
nodest:
 mov eax, DWORD PTR [ebp+56]
 mov DWORD PTR [edx+28], eax
 mov eax, DWORD PTR [ebp+60]
 mov DWORD PTR [edx+32], eax
 mov eax, DWORD PTR [0x1000001c]
 mov DWORD PTR [edx+36], eax
 lea edi, [edx+48]
 mov esi, DWORD PTR [ebp+40]
 mov ecx, DWORD PTR [ebp+44]
 rep movsb
 inc ebx
 mov DWORD PTR [edx+44], ebx
 jmp done
badlength:
 lock inc DWORD PTR [0x10000010]
 jmp done
overflow:
 lock inc DWORD PTR [0x1000000c]
done:
 popad
 popfd
 mov eax, 1
 ret 0x18
