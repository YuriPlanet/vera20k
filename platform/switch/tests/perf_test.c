#include <assert.h>
#include <stdio.h>
#include <string.h>
#include <switch.h>

static u64 now_tick = 1, ticks[4][4];
static Handle current = 1;
static bool locked, fail_ticks, fail_clocks, fail_close;
static char output[65536];

void mutexLock(Mutex *mutex) { (void)mutex; assert(!locked); locked = true; }
void mutexUnlock(Mutex *mutex) { (void)mutex; assert(locked); locked = false; }
Handle threadGetCurHandle(void) { return current; }
bool hosversionAtLeast(u32 major, u32 minor, u32 micro) {
    (void)minor; (void)micro; return major <= 22;
}
u64 armGetSystemTick(void) { return now_tick; }
u64 armGetSystemTickFreq(void) { return 1000; }
Result svcGetThreadId(u64 *id, Handle handle) { *id = 100 + handle; return 0; }
Result svcGetThreadCoreMask(s32 *preferred, u64 *mask, Handle handle) {
    (void)handle; *preferred = -1; *mask = 7; return 0;
}
Result svcGetThreadPriority(s32 *priority, Handle handle) {
    *priority = handle == 1 ? 44 : 59; return 0;
}
Result svcGetInfo(u64 *value, int kind, Handle handle, u64 core) {
    assert(kind == InfoType_ThreadTickCount && core < 4 && handle < 4);
    if (fail_ticks && handle == 2) return 0x1234;
    *value = ticks[handle][core]; return 0;
}
Result clkrstInitialize(void) { return fail_clocks ? 0xf201 : 0; }
void clkrstExit(void) {}
Result clkrstOpenSession(ClkrstSession *session, PcvModuleId module, u32 unk) {
    assert(unk == 3); session->session = module; return 0;
}
Result __real_threadClose(Thread *thread) { (void)thread; return fail_close ? 0xbeef : 0; }
void clkrstCloseSession(ClkrstSession *session) { (void)session; }
Result clkrstGetClockRate(ClkrstSession *session, u32 *rate) {
    const u32 rates[] = {1785000000, 768000000, 1600000000};
    assert(session->session < 3); *rate = rates[session->session]; return 0;
}
void platform_perf_log(const char *line) {
    assert(!locked);
    assert(strlen(output) + strlen(line) + 2 < sizeof(output));
    strcat(output, line); strcat(output, "\n");
}

#include "../perf.c"

int main(void) {
    platform_perf_thread_enter("main");
    platform_perf_poll();
    current = 2;
    platform_perf_thread_enter("runtime");
    platform_perf_thread_name("rayon-0");
    current = 3;
    platform_perf_thread_enter("temporary");
    ticks[3][2] = 1000;
    // Closing a temporary thread retains its final CPU delta before handle reuse.
    Thread temporary = {.handle = 3};
    fail_close = true;
    assert(__wrap_threadClose(&temporary) == 0xbeef);
    ticks[3][2] += 1000;
    fail_close = false;
    assert(__wrap_threadClose(&temporary) == 0);
    ticks[1][0] = 5000;
    ticks[2][1] = 2000;
    now_tick = 10001;
    platform_perf_poll();
    assert(strstr(output, "tracked=2 failures=0 dropped=0"));
    assert(strstr(output, "cpu_pct_by_core=[50.00,20.00,20.00,0.00]"));
    assert(strstr(output, "role=rayon-0 priority=59 preferred=-1 affinity=0x7"));
    assert(strstr(output, "configured_hz cpu=1785000000 gpu=768000000 emc=1600000000"));
    assert(!strstr(output, "role=temporary"));

    output[0] = 0;
    now_tick = 20001;
    ticks[1][0] += 1000;
    fail_ticks = fail_clocks = true;
    platform_perf_poll();
    assert(strstr(output, "tracked=2 failures=1"));
    assert(strstr(output, "cpu_pct_by_core=[10.00,0.00,0.00,0.00]"));
    assert(strstr(output, "valid=0 rc=0x1234"));
    assert(strstr(output, "configured_hz cpu=0 gpu=0 emc=0 rc=0xf201/0xf201/0xf201"));
    return 0;
}
