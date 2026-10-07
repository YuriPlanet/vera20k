
int __fastcall FUN_0070be50(int param_1)

{
  int iVar1;
  int iVar2;
  
  iVar1 = (**(code **)(*(int *)(param_1 + 4) + 0x10))(param_1 + 4);
  iVar2 = FUN_007c5f00();
  return (iVar2 + iVar1) % 400;
}

