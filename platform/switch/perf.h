#pragma once

// Observation only: never change clocks, affinity, priority, or thread activity.
void platform_perf_thread_enter(const char *name);
void platform_perf_thread_name(const char *name);
void platform_perf_thread_exit(void);
void platform_perf_thread_close(unsigned handle);
void platform_perf_poll(void);
