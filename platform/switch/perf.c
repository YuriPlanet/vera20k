#include <switch.h>
#include <stdio.h>
#include <string.h>
#include "perf.h"

#define PERF_THREADS 64
#define PERF_CORES 4

typedef struct {
    Handle handle;
    u64 id;
    char name[24];
    u64 ticks[PERF_CORES];
    bool valid;
} PerfThread;

static Mutex registry_lock;
static PerfThread threads[PERF_THREADS];
static u64 retired[PERF_CORES];
static u64 previous_poll;
static unsigned dropped_threads;
extern void platform_perf_log(const char *message);

static Result read_ticks(Handle handle, u64 ticks[PERF_CORES]) {
    Result result = 0;
    InfoType kind = hosversionAtLeast(13, 0, 0)
        ? InfoType_ThreadTickCount : InfoType_ThreadTickCountDeprecated;
    for (unsigned core = 0; core < PERF_CORES; ++core) {
        Result rc = svcGetInfo(&ticks[core], kind, handle, core);
        if (R_FAILED(rc)) result = rc;
    }
    return result;
}

void platform_perf_thread_enter(const char *name) {
    Handle handle = threadGetCurHandle();
    mutexLock(&registry_lock);
    for (unsigned i = 0; i < PERF_THREADS; ++i) {
        if (threads[i].handle) continue;
        PerfThread *slot = &threads[i];
        *slot = (PerfThread){.handle = handle};
        svcGetThreadId(&slot->id, handle);
        snprintf(slot->name, sizeof(slot->name), "%s", name);
        slot->valid = R_SUCCEEDED(read_ticks(handle, slot->ticks));
        mutexUnlock(&registry_lock);
        return;
    }
    ++dropped_threads;
    mutexUnlock(&registry_lock);
}

void platform_perf_thread_name(const char *name) {
    Handle handle = threadGetCurHandle();
    mutexLock(&registry_lock);
    for (unsigned i = 0; i < PERF_THREADS; ++i) {
        if (threads[i].handle == handle) {
            snprintf(threads[i].name, sizeof(threads[i].name), "%s", name);
            break;
        }
    }
    mutexUnlock(&registry_lock);
}

// The caller holds registry_lock, including across the close transaction.
static void retire_thread(Handle handle, const u64 ticks[PERF_CORES], bool valid) {
    for (unsigned i = 0; i < PERF_THREADS; ++i) {
        PerfThread *slot = &threads[i];
        if (slot->handle != handle) continue;
        if (valid && slot->valid) {
            for (unsigned core = 0; core < PERF_CORES; ++core)
                retired[core] += ticks[core] - slot->ticks[core];
        }
        *slot = (PerfThread){0};
        break;
    }
}

void platform_perf_thread_close(unsigned handle) {
    mutexLock(&registry_lock);
    u64 ticks[PERF_CORES] = {0};
    bool valid = R_SUCCEEDED(read_ticks(handle, ticks));
    retire_thread(handle, ticks, valid);
    mutexUnlock(&registry_lock);
}

extern Result __real_threadClose(Thread *);
Result __wrap_threadClose(Thread *thread) {
    // Read before close destroys the handle; retire only after a successful close.
    // Keeping the lock prevents a concurrent poll from advancing the baseline.
    mutexLock(&registry_lock);
    Handle handle = thread->handle;
    u64 ticks[PERF_CORES] = {0};
    bool valid = R_SUCCEEDED(read_ticks(handle, ticks));
    Result result = __real_threadClose(thread);
    if (R_SUCCEEDED(result)) retire_thread(handle, ticks, valid);
    mutexUnlock(&registry_lock);
    return result;
}

void platform_perf_thread_exit(void) {
    platform_perf_thread_close(threadGetCurHandle());
}

static void report_clocks(void) {
    const PcvModuleId modules[] = {PcvModuleId_CpuBus, PcvModuleId_GPU, PcvModuleId_EMC};
    u32 rates[3] = {0};
    Result results[3];
    Result init = clkrstInitialize();
    for (unsigned i = 0; i < 3; ++i) {
        results[i] = init;
        if (R_FAILED(init)) continue;
        ClkrstSession session = {0};
        results[i] = clkrstOpenSession(&session, modules[i], 3);
        if (R_SUCCEEDED(results[i])) {
            results[i] = clkrstGetClockRate(&session, &rates[i]);
            clkrstCloseSession(&session);
        }
    }
    if (R_SUCCEEDED(init)) clkrstExit();
    char line[256];
    snprintf(line, sizeof(line),
        "Clock sample source=clkrst configured_hz cpu=%u gpu=%u emc=%u rc=%#x/%#x/%#x; "
        "actual_pll_hz=unavailable (zero with error is not a measured clock)",
        rates[0], rates[1], rates[2], results[0], results[1], results[2]);
    platform_perf_log(line);
}

void platform_perf_poll(void) {
    u64 now = armGetSystemTick();
    if (!previous_poll) {
        previous_poll = now;
        mutexLock(&registry_lock);
        memset(retired, 0, sizeof(retired));
        for (unsigned i = 0; i < PERF_THREADS; ++i) {
            if (threads[i].handle)
                threads[i].valid = R_SUCCEEDED(read_ticks(threads[i].handle, threads[i].ticks));
        }
        mutexUnlock(&registry_lock);
        return;
    }
    u64 elapsed = now - previous_poll;
    if (elapsed < armGetSystemTickFreq() * 10) return;
    previous_poll = now;
    u64 started = armGetSystemTick();
    char lines[PERF_THREADS][320];
    unsigned count = 0, failures = 0, dropped;
    u64 totals[PERF_CORES] = {0};
    mutexLock(&registry_lock);
    memcpy(totals, retired, sizeof(totals));
    memset(retired, 0, sizeof(retired));
    dropped = dropped_threads;
    for (unsigned i = 0; i < PERF_THREADS; ++i) {
        PerfThread *slot = &threads[i];
        if (!slot->handle) continue;
        u64 ticks[PERF_CORES] = {0}, delta[PERF_CORES] = {0};
        Result tick_rc = read_ticks(slot->handle, ticks);
        bool sampled = slot->valid && R_SUCCEEDED(tick_rc);
        if (sampled) {
            for (unsigned core = 0; core < PERF_CORES; ++core) {
                delta[core] = ticks[core] - slot->ticks[core];
                totals[core] += delta[core];
            }
        } else {
            ++failures;
        }
        slot->valid = R_SUCCEEDED(tick_rc);
        memcpy(slot->ticks, ticks, sizeof(ticks));
        s32 preferred = -1, priority = -1;
        u64 mask = 0;
        Result mask_rc = svcGetThreadCoreMask(&preferred, &mask, slot->handle);
        Result priority_rc = svcGetThreadPriority(&priority, slot->handle);
        double scale = 100.0 / (double)elapsed;
        snprintf(lines[count++], sizeof(lines[0]),
            "Thread id=%llu role=%s priority=%d preferred=%d affinity=%#llx "
            "cpu_pct_by_core=[%.2f,%.2f,%.2f,%.2f] valid=%u rc=%#x/%#x/%#x",
            (unsigned long long)slot->id, slot->name, priority, preferred,
            (unsigned long long)mask, delta[0] * scale, delta[1] * scale,
            delta[2] * scale, delta[3] * scale, sampled,
            tick_rc, mask_rc, priority_rc);
    }
    mutexUnlock(&registry_lock);
    char line[320];
    double scale = 100.0 / (double)elapsed;
    snprintf(line, sizeof(line),
        "Thread window wall_ms=%.2f tracked=%u failures=%u dropped=%u "
        "cpu_pct_by_core=[%.2f,%.2f,%.2f,%.2f] "
        "coverage=main+wrapped_threadCreate includes_retired=true",
        (double)elapsed * 1000.0 / armGetSystemTickFreq(), count, failures, dropped,
        totals[0] * scale, totals[1] * scale, totals[2] * scale, totals[3] * scale);
    platform_perf_log(line);
    for (unsigned i = 0; i < count; ++i) platform_perf_log(lines[i]);
    report_clocks();
    snprintf(line, sizeof(line), "Thread/clock probe wall_ms=%.3f",
        (double)(armGetSystemTick() - started) * 1000.0 / armGetSystemTickFreq());
    platform_perf_log(line);
}
