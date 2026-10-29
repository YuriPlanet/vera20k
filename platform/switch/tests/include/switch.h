#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef uint8_t u8;
typedef int8_t s8;
typedef uint16_t u16;
typedef uint32_t u32;
typedef uint64_t u64;
typedef uint32_t Result;
typedef uint32_t Handle;
typedef struct { void *address; } SharedMemory;
typedef struct { Handle session; } Service;
typedef struct { Handle handle; int unused; } Thread;
typedef void (*ThreadFunc)(void *);
typedef int64_t s64;
typedef int32_t s32;
typedef int InfoType;
typedef u32 Mutex;
typedef int PcvModuleId;
typedef Service ClkrstSession;
#define InfoType_ThreadTickCount 25
#define InfoType_ThreadTickCountDeprecated 0xF0000002
#define PcvModuleId_CpuBus 0
#define PcvModuleId_GPU 1
#define PcvModuleId_EMC 2
void mutexLock(Mutex *);
void mutexUnlock(Mutex *);
Handle threadGetCurHandle(void);
bool hosversionAtLeast(u32, u32, u32);
Result svcGetThreadId(u64 *, Handle);
Result svcGetThreadCoreMask(s32 *, u64 *, Handle);
Result svcGetThreadPriority(s32 *, Handle);
Result clkrstInitialize(void);
void clkrstExit(void);
Result clkrstOpenSession(ClkrstSession *, PcvModuleId, u32);
void clkrstCloseSession(ClkrstSession *);
Result clkrstGetClockRate(ClkrstSession *, u32 *);
typedef struct {
    bool in_send_pid;
    int out_handle_attrs[8];
    Handle *out_handles;
} Dispatch;

#define NX_PACKED __attribute__((packed))
#define INVALID_HANDLE 0
#define Perm_Rw 3
#define SfOutHandleAttr_HipcCopy 1
#define CUR_PROCESS_HANDLE 0xFFFF8001u
#define Module_Kernel 1
#define KernelError_OutOfResource 132
#define InfoType_ResourceLimit 9
#define InfoType_TotalMemorySize 6
#define InfoType_UsedMemorySize 7
#define LimitableResource_Sessions 4
#define R_MODULE(result) ((result) & 0x1FF)
#define R_DESCRIPTION(result) (((result) >> 9) & 0x1FFF)
#define R_FAILED(result) ((result) != 0)
#define R_SUCCEEDED(result) ((result) == 0)

Result svcConnectToNamedPort(Handle *, const char *);
Result svcCloseHandle(Handle);
void svcSleepThread(int64_t);
Result svcGetInfo(u64 *, int, Handle, u64);
Result svcGetResourceLimitCurrentValue(s64 *, Handle, int);
Result svcGetResourceLimitLimitValue(s64 *, Handle, int);
void shmemLoadRemote(SharedMemory *, Handle, size_t, int);
Result shmemMap(SharedMemory *);
Result shmemClose(SharedMemory *);
static inline void *shmemGetAddr(SharedMemory *memory) { return memory->address; }
Result threadCreate(Thread *, ThreadFunc, void *, void *, size_t, int, int);
Result threadStart(Thread *);
Result threadClose(Thread *);
u64 armGetSystemTick(void);
u64 armGetSystemTickFreq(void);
Result test_dispatch(Service *, u32, const void *, size_t, void *, Dispatch);
#define serviceDispatch(service, command, ...) \
    test_dispatch(service, command, NULL, 0, NULL, (Dispatch){__VA_ARGS__})
#define serviceDispatchIn(service, command, input, ...) \
    test_dispatch(service, command, &(input), sizeof(input), NULL, (Dispatch){__VA_ARGS__})
#define serviceDispatchInOut(service, command, input, output, ...) \
    test_dispatch(service, command, &(input), sizeof(input), &(output), (Dispatch){__VA_ARGS__})
