.intel_syntax noprefix
.text
.global _stub
_stub:
 pushfd
 lock inc DWORD PTR [0x1000001c]
 popfd
 mov eax, 1
 ret
