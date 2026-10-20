.intel_syntax noprefix
.text
.global _stub
# IPXManager5415F0: thiscall(payloadOut, lengthOut, connectionIDOut), RET12.
# Only original WaitForPlayers648710's receive call may consume FrameSync19.
_stub:
 pushfd
 pushad
 cld
 mov ebp, esp
 lock inc DWORD PTR [0x11000014]
 cmp DWORD PTR [0x11000024], 1
 jne empty
 cmp DWORD PTR [ebp+36], 0x64932f
 jne wrongcaller
 cmp DWORD PTR [0x11000040], 14
 jne badlength
 cmp BYTE PTR [0x11000048], 0x19
 jne badlength
 mov edi, DWORD PTR [ebp+40]
 mov esi, 0x11000048
 mov ecx, 14
 rep movsb
 mov edi, DWORD PTR [ebp+44]
 mov DWORD PTR [edi], 14
 mov edi, DWORD PTR [ebp+48]
 mov eax, DWORD PTR [0x11000044]
 mov DWORD PTR [edi], eax
 mov eax, DWORD PTR [0x11000028]
 mov DWORD PTR [0x1100002c], eax
 mov DWORD PTR [0x11000024], 0
 popad
 popfd
 mov eax, 1
 ret 0x0c
badlength:
 lock inc DWORD PTR [0x11000010]
 jmp empty
wrongcaller:
 lock inc DWORD PTR [0x11000018]
empty:
 popad
 popfd
 xor eax, eax
 ret 0x0c
