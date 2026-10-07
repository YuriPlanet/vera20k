
char __thiscall FUN_00703860(int *param_1,undefined4 param_2,int param_3)

{
  char cVar1;
  int iVar2;
  undefined4 uVar3;
  
  iVar2 = (**(code **)(*param_1 + 0x84))();
  if ((*(char *)(iVar2 + 0xc9a) == '\0') || (*(char *)((int)param_1 + 0x41a) == '\0')) {
    iVar2 = (**(code **)(*param_1 + 0x84))();
    if ((*(char *)(iVar2 + 0xc9a) != '\0') &&
       ((*(char *)((int)param_1 + 0x41a) == '\0' && (DAT_00a8ed6b == '\0')))) {
      return '\x05';
    }
    if (((param_1[0x88] != 0) && (DAT_00a8ed6b == '\0')) &&
       (iVar2 = (**(code **)(*param_1 + 0x2c))(), iVar2 != 6)) {
      if (param_1[0x88] == 2) {
        if ((char)param_2 != '\0') {
          if (param_3 == 0) {
            return '\x05';
          }
          uVar3 = *(undefined4 *)(param_3 + 0x30);
          param_2 = CONCAT22((short)(param_1[0x28] + (param_1[0x28] >> 0x1f & 0xffU) >> 8),
                             (short)(param_1[0x27] + (param_1[0x27] >> 0x1f & 0xffU) >> 8));
          FUN_005657a0(&param_2);
          cVar1 = FUN_004870d0(uVar3);
          if (cVar1 == '\0') {
            return '\x05';
          }
          return '\x03';
        }
        if (DAT_00b73550 == 0) {
          return '\x03';
        }
        if (*(char *)((int)param_1 + 0x41a) != '\0') {
          return '\x03';
        }
        param_2 = CONCAT22((short)(param_1[0x28] + (param_1[0x28] >> 0x1f & 0xffU) >> 8),
                           (short)(param_1[0x27] + (param_1[0x27] >> 0x1f & 0xffU) >> 8));
        uVar3 = *(undefined4 *)(DAT_00a83d4c + 0x30);
        FUN_005657a0(&param_2);
        cVar1 = FUN_004870d0(uVar3);
        if (cVar1 != '\0') {
          return '\x03';
        }
        if (DAT_00a8b238 == 0) {
          return '\x05';
        }
        if (param_1[0x87] == 0) {
          return '\x05';
        }
        if (DAT_00a83d4c == 0) {
          return '\x05';
        }
        cVar1 = FUN_004f9a50(param_1[0x87]);
        if (cVar1 == '\0') {
          return '\x05';
        }
        cVar1 = FUN_004f9a50(DAT_00a83d4c);
        if (cVar1 == '\0') {
          return '\x05';
        }
        return '\x03';
      }
      param_3 = param_1[0x89];
      if (0 < param_3) {
        iVar2 = FUN_007c5f00();
        if (iVar2 < 0x40) {
          return '\x01';
        }
        if (iVar2 < 0x80) {
          return '\x02';
        }
        if (iVar2 < 0xc0) {
          return '\x03';
        }
        if (((char)param_2 == '\0') && (*(char *)((int)param_1 + 0x41a) != '\0')) {
          return '\x03';
        }
        return (0xfe < iVar2) + '\x04';
      }
    }
  }
  return '\0';
}

