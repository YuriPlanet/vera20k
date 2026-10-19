.intel_syntax noprefix
.text
.global _stub
_stub:
 pushfd
 pushad
 cld
 mov ebp, esp
 lock inc DWORD PTR [0x10000014]
 cmp DWORD PTR [0x10000024], 1
 jne empty
 mov eax, DWORD PTR [ebp+36]
 cmp eax, DWORD PTR [0x10000030]
 jne wrongcaller
 mov ecx, DWORD PTR [0x10000040]
 cmp ecx, 455
 je lengthok
 cmp ecx, 51
 jne badlength
 cmp DWORD PTR [0x10000054], 0x0f
 jne badlength
 cmp eax, 0x48d552
 je lengthok
 cmp eax, 0x48db00
 jne badlength
lengthok:
 mov edx, ecx
 mov edi, DWORD PTR [ebp+40]
 mov esi, 0x10000054
 rep movsb
 mov edi, DWORD PTR [ebp+44]
 mov DWORD PTR [edi], edx
 mov edi, DWORD PTR [ebp+48]
 mov esi, 0x10000044
 mov ecx, 3
 rep movsd
 mov edi, DWORD PTR [ebp+52]
 mov ax, WORD PTR [0x10000050]
 mov WORD PTR [edi], ax
 mov eax, DWORD PTR [0x10000028]
 mov DWORD PTR [0x1000002c], eax
 mov DWORD PTR [0x10000024], 0
 popad
 popfd
 mov eax, 1
 ret 0x10
badlength:
 lock inc DWORD PTR [0x10000010]
 jmp empty
wrongcaller:
 lock inc DWORD PTR [0x10000018]
empty:
 popad
 popfd
 xor eax, eax
 ret 0x10
