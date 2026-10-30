#include <switch.h>
#include <errno.h>
#include <reent.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#ifdef VERA_PERF
#include "perf.h"
#endif
#ifndef VERA_WGPU
#include <EGL/egl.h>
#include <GLES2/gl2.h>
#endif

extern int rust_main(void);
extern void *__aarch64_read_tp(void);
static PadState pad;
static bool user_save_mounted;
typedef struct {
    uint32_t count;
    struct {
        uint32_t id;
        uint32_t x;
        uint32_t y;
    } touches[16];
} VeraTouchState;
#ifndef VERA_WGPU
static EGLDisplay display;
static EGLContext context;
static EGLSurface surface;
static GLuint program;
static GLuint vertex_buffer;
#endif

// Based on bevy-horizon compat.rs (MIT), see UPSTREAM.md. C/newlib uses
// libnx's software thread pointer; LLVM local-exec TLS reads TPIDR_EL0.
static void setup_rust_tls(void) {
    void *pointer = __aarch64_read_tp();
    __asm__ volatile("msr tpidr_el0, %0" : : "r"(pointer) : "memory");
}

void userAppInit(void) { setup_rust_tls(); }
int main(int argc, char **argv) {
    (void)argc;
    (void)argv;
#ifdef VERA_WGPU
#ifdef VERA_CLIENT
    setenv("MESA_LOG_FILE", "sdmc:/switch/vera20k/client-mesa.log", 1);
#else
    setenv("MESA_LOG_FILE", "sdmc:/switch/vera20k/gpu-mesa.log", 1);
#endif
    if (getenv("VERA_MESA_DEBUG")) {
        setenv("MESA_LOG_LEVEL", "debug", 1);
    } else {
#ifdef VERA_PERF
        setenv("MESA_LOG_LEVEL", "info", 1);
#else
        unsetenv("MESA_LOG_LEVEL");
#endif
    }
#ifdef VERA_PERF
    setenv("NVK_SWITCH_PERF_LOG", "true", 1);
#endif
    // See third_party/mesa-switch-graphics-ubo.patch for the NVK/Eden path.
    setenv("NVK_SWITCH_GRAPHICS_UBO_CBUF", "true", 1);
#endif
#ifdef VERA_PERF
    platform_perf_thread_enter("main");
#endif
    return rust_main();
}

typedef struct { ThreadFunc entry; void *argument; } ThreadStart;
extern Result __real_threadCreate(Thread *, ThreadFunc, void *, void *, size_t, int, int);
static void rust_thread_start(void *argument) {
    setup_rust_tls();
    ThreadStart start = *(ThreadStart *)argument;
    free(argument);
#ifdef VERA_PERF
    platform_perf_thread_enter("runtime");
#endif
    start.entry(start.argument);
#ifdef VERA_PERF
    platform_perf_thread_exit();
#endif
}

Result __wrap_threadCreate(Thread *thread, ThreadFunc entry, void *argument,
                          void *stack, size_t size, int priority, int core) {
    ThreadStart *start = malloc(sizeof(*start));
    if (!start) return MAKERESULT(Module_Libnx, LibnxError_OutOfMemory);
    *start = (ThreadStart){entry, argument};
    Result result = __real_threadCreate(thread, rust_thread_start, start, stack, size, priority, core);
    if (R_FAILED(result)) free(start);
    return result;
}

// newlib's arc4random -> getentropy needs libnx's platform entropy source.
int _getentropy_r(struct _reent *reent, void *buffer, size_t length) {
    if (length > 256) { reent->_errno = EIO; return -1; }
    randomGet(buffer, length);
    return 0;
}

ssize_t getrandom(void *buffer, size_t length, unsigned int flags) {
    (void)flags;
    if (!buffer && length) { errno = EFAULT; return -1; }
    randomGet(buffer, length);
    return (ssize_t)length;
}

long sysconf(int name) {
    if (name == _SC_PAGESIZE) return 0x1000;
    if (name == _SC_NPROCESSORS_CONF || name == _SC_NPROCESSORS_ONLN) {
        uint64_t mask = 0;
        Result result = svcGetInfo(&mask, InfoType_CoreMask, CUR_PROCESS_HANDLE, 0);
        return R_SUCCEEDED(result) ? __builtin_popcountll(mask & 7) : 1;
    }
    errno = EINVAL;
    return -1;
}

void platform_console_init(void) {
    consoleInit(NULL);
    padConfigureInput(1, HidNpadStyleSet_NpadStandard);
    padInitializeDefault(&pad);
    hidInitializeTouchScreen();
    hidInitializeMouse();
    hidInitializeKeyboard();
}
bool platform_running(void) { return appletMainLoop(); }
int32_t platform_applet_type(void) { return appletGetAppletType(); }
uint64_t platform_memory_size(int32_t info_type) {
    uint64_t value = 0;
    return R_SUCCEEDED(svcGetInfo(&value, (InfoType)info_type, CUR_PROCESS_HANDLE, 0)) ? value : 0;
}
uint64_t platform_buttons(void) { padUpdate(&pad); return padGetButtonsDown(&pad); }
uint64_t platform_buttons_held(void) { return padGetButtons(&pad); }
void platform_sticks(int32_t *lx, int32_t *ly, int32_t *rx, int32_t *ry) {
    HidAnalogStickState left = padGetStickPos(&pad, 0);
    HidAnalogStickState right = padGetStickPos(&pad, 1);
    *lx = left.x;
    *ly = left.y;
    *rx = right.x;
    *ry = right.y;
}
bool platform_focused(void) { return appletGetFocusState() == AppletFocusState_InFocus; }
bool platform_touch(uint32_t *x, uint32_t *y) {
    HidTouchScreenState state = {0};
    if (!hidGetTouchScreenStates(&state, 1) || state.count <= 0) return false;
    *x = state.touches[0].x;
    *y = state.touches[0].y;
    return true;
}
uint32_t platform_touches(VeraTouchState *output) {
    HidTouchScreenState state = {0};
    if (!output) return 0;
    output->count = 0;
    if (!hidGetTouchScreenStates(&state, 1) || state.count <= 0) return 0;
    uint32_t count = (uint32_t)state.count;
    if (count > 16) count = 16;
    for (uint32_t index = 0; index < count; ++index) {
        output->touches[index].id = state.touches[index].finger_id;
        output->touches[index].x = state.touches[index].x;
        output->touches[index].y = state.touches[index].y;
    }
    output->count = count;
    return count;
}
bool platform_mouse(int32_t *delta_x, int32_t *delta_y, int32_t *wheel_x,
                    int32_t *wheel_y, uint32_t *buttons, uint32_t *pressed_buttons,
                    uint32_t *released_buttons) {
    static uint64_t last_sample;
    static uint32_t last_buttons;
    // Handheld mode allows a USB or Bluetooth mouse too, so the display mode
    // no longer gates it. Instead the mouse stays authoritative only while it
    // is actually being used: after this idle period the caller falls back to
    // the sticks and the touch screen.
    static uint64_t last_activity_tick;
    const uint64_t idle_limit = armGetSystemTickFreq() * 10ULL; // 10 s
    HidMouseState states[17] = {0};
    size_t count = hidGetMouseStates(states, 17);
    if (!count || !(states[0].attributes & HidMouseAttribute_IsConnected)) {
        last_sample = 0;
        last_buttons = 0;
        return false;
    }
    int32_t dx = 0, dy = 0, wx = 0, wy = 0;
    // libnx returns newest first. Consume each relative sample once, including
    // history collected between slower rendered frames.
    size_t available = last_sample ? count : 1;
    while (available && states[available - 1].sampling_number <= last_sample) --available;
    for (size_t index = 0; index < available; ++index) {
        dx += states[index].delta_x;
        dy += states[index].delta_y;
        wx += states[index].wheel_delta_x;
        wy += states[index].wheel_delta_y;
    }
    uint64_t now = armGetSystemTick();
    bool moved = dx != 0 || dy != 0 || wx != 0 || wy != 0;
    // A button edge must also count: a click right after the idle limit would
    // otherwise be dropped while the pointer sits where the sticks left it.
    uint32_t pressed_now = 0, released_now = 0;
    {
        uint32_t previous = last_buttons;
        for (size_t index = available; index-- > 0;) {
            pressed_now |= states[index].buttons & ~previous;
            released_now |= ~states[index].buttons & previous;
            previous = states[index].buttons;
        }
    }
    if (moved || pressed_now || released_now) {
        last_activity_tick = now;
    } else if (last_activity_tick
               && now - last_activity_tick > idle_limit) {
        last_sample = 0;
        last_buttons = 0;
        last_activity_tick = 0;
        return false;
    }
    if (delta_x) *delta_x = dx;
    if (delta_y) *delta_y = dy;
    if (wheel_x) *wheel_x = wx;
    if (wheel_y) *wheel_y = wy;
    if (buttons) *buttons = states[0].buttons;
    last_sample = states[0].sampling_number;
    last_buttons = states[0].buttons;
    if (pressed_buttons) *pressed_buttons = pressed_now;
    if (released_buttons) *released_buttons = released_now;
    return true;
}
uint64_t platform_keyboard(uint64_t *keys) {
    HidKeyboardState state = {0};
    if (!keys) return 0;
    for (uint32_t index = 0; index < 4; ++index) keys[index] = 0;
    if (appletGetOperationMode() != AppletOperationMode_Console ||
        !hidGetKeyboardStates(&state, 1)) return 0;
    for (uint32_t index = 0; index < 4; ++index) keys[index] = state.keys[index];
    return state.modifiers;
}
Result platform_soft_keyboard_show(char *output, size_t output_size) {
    if (!output || output_size < 2) return 1;
    output[0] = '\0';
    SwkbdConfig config;
    Result result = swkbdCreate(&config, 0);
    if (R_FAILED(result)) return result;
    swkbdConfigMakePresetDefault(&config);
    swkbdConfigSetType(&config, SwkbdType_All);
    swkbdConfigSetStringLenMax(&config, 1024);
    swkbdConfigSetHeaderText(&config, "VERA20k");
    swkbdConfigSetGuideText(&config, "A 确定   B 取消");
    result = swkbdShow(&config, output, output_size);
    swkbdClose(&config);
    return result;
}
bool platform_touch_finger(uint32_t finger_id, uint32_t *x, uint32_t *y) {
    HidTouchScreenState state = {0};
    if (!hidGetTouchScreenStates(&state, 1) || state.count <= 0) return false;
    for (int32_t index = 0; index < state.count; ++index) {
        if (state.touches[index].finger_id == finger_id) {
            if (x) *x = state.touches[index].x;
            if (y) *y = state.touches[index].y;
            return true;
        }
    }
    return false;
}
Result platform_commit_sdmc(void) { return fsdevCommitDevice("sdmc"); }
Result platform_commit_user_save(void) {
    return user_save_mounted ? fsdevCommitDevice("vera20k-user")
        : MAKERESULT(Module_Libnx, LibnxError_NotFound);
}
void platform_unmount_user_save(void) {
    if (user_save_mounted) {
        fsdevUnmountDevice("vera20k-user");
        user_save_mounted = false;
        appletUnlockExit();
    }
}
Result platform_mount_user_save(void) {
    if (user_save_mounted) return 0;
    Result result = accountInitialize(AccountServiceType_Application);
    if (R_FAILED(result)) return result;
    AccountUid uid = {0};
    result = accountGetPreselectedUser(&uid);
    accountExit();
    // Never fall back to a different account's saves when selection is absent.
    if (R_FAILED(result) || !accountUidIsValid(&uid)) {
        return R_FAILED(result) ? result : MAKERESULT(Module_Libnx, LibnxError_NotFound);
    }
    result = fsdevMountSaveData("vera20k-user", FS_SAVEDATA_CURRENT_APPLICATIONID, uid);
    if (R_SUCCEEDED(result)) {
        result = appletLockExit();
        if (R_FAILED(result)) {
            fsdevUnmountDevice("vera20k-user");
        } else {
            user_save_mounted = true;
        }
    }
    return result;
}
bool platform_romfs_init(void) { return R_SUCCEEDED(romfsInit()); }
void platform_romfs_exit(void) { romfsExit(); }
void platform_present(void) { consoleUpdate(NULL); }
void platform_console_exit(void) { consoleExit(NULL); }
extern const uint8_t _start[] __attribute__((visibility("hidden")));
bool platform_is_nro(void) { return memcmp(_start + 0x10, "NRO0", 4) == 0; }

#ifdef VERA_WGPU
// The default NWindow lives until libnx shuts down after Rust has dropped wgpu.
void *platform_window_init(void) {
    if (!platform_running()) return NULL;
    consoleExit(NULL);
    NWindow *window = nwindowGetDefault();
    if (R_FAILED(nwindowSetDimensions(window, 1280, 720))) return NULL;
    return window;
}
#else
// Stop libnx's CPU text framebuffer before presenting an EGL surface. The
// caller keeps handling input and completion through the Rust runtime.
void platform_graphics_init(void) {
    if (!platform_running()) return;
    consoleExit(NULL);
    display = eglGetDisplay(EGL_DEFAULT_DISPLAY);
    if (display == EGL_NO_DISPLAY || !eglInitialize(display, NULL, NULL)) return;
    if (!eglBindAPI(EGL_OPENGL_ES_API)) return;
    const EGLint attributes[] = {
        EGL_RENDERABLE_TYPE, EGL_OPENGL_ES2_BIT,
        EGL_RED_SIZE, 8, EGL_GREEN_SIZE, 8, EGL_BLUE_SIZE, 8, EGL_ALPHA_SIZE, 8,
        EGL_NONE,
    };
    EGLConfig config;
    EGLint count = 0;
    if (!eglChooseConfig(display, attributes, &config, 1, &count) || !count) return;
    NWindow *window = nwindowGetDefault();
    nwindowSetDimensions(window, 1280, 720);
    surface = eglCreateWindowSurface(display, config, window, NULL);
    if (surface == EGL_NO_SURFACE) return;
    const EGLint context_attributes[] = {
        EGL_CONTEXT_CLIENT_VERSION, 2,
        EGL_NONE,
    };
    context = eglCreateContext(display, config, EGL_NO_CONTEXT, context_attributes);
    if (context == EGL_NO_CONTEXT ||
        !eglMakeCurrent(display, surface, surface, context)) return;
    eglSwapInterval(display, 1);

    static const char vertex_source[] =
        "attribute vec2 position;\n"
        "attribute vec3 color;\n"
        "varying vec3 vertex_color;\n"
        "void main() {\n"
        "  gl_Position = vec4(position, 0.0, 1.0);\n"
        "  vertex_color = color;\n"
        "}\n";
    static const char fragment_source[] =
        "precision mediump float;\n"
        "varying vec3 vertex_color;\n"
        "void main() { gl_FragColor = vec4(vertex_color, 1.0); }\n";
    GLuint vertex_shader = glCreateShader(GL_VERTEX_SHADER);
    GLuint fragment_shader = glCreateShader(GL_FRAGMENT_SHADER);
    glShaderSource(vertex_shader, 1, (const GLchar *const[]){vertex_source}, NULL);
    glShaderSource(fragment_shader, 1, (const GLchar *const[]){fragment_source}, NULL);
    glCompileShader(vertex_shader);
    glCompileShader(fragment_shader);
    program = glCreateProgram();
    glAttachShader(program, vertex_shader);
    glAttachShader(program, fragment_shader);
    glBindAttribLocation(program, 0, "position");
    glBindAttribLocation(program, 1, "color");
    glLinkProgram(program);
    glDeleteShader(vertex_shader);
    glDeleteShader(fragment_shader);

    static const GLfloat vertices[] = {
        -0.65f, -0.60f, 0.92f, 0.20f, 0.12f,
         0.65f, -0.60f, 0.12f, 0.65f, 0.24f,
         0.00f,  0.68f, 0.18f, 0.35f, 0.92f,
    };
    glGenBuffers(1, &vertex_buffer);
    glBindBuffer(GL_ARRAY_BUFFER, vertex_buffer);
    glBufferData(GL_ARRAY_BUFFER, sizeof(vertices), vertices, GL_STATIC_DRAW);
    glEnableVertexAttribArray(0);
    glVertexAttribPointer(0, 2, GL_FLOAT, GL_FALSE, 5 * sizeof(GLfloat), (void *)0);
    glEnableVertexAttribArray(1);
    glVertexAttribPointer(1, 3, GL_FLOAT, GL_FALSE, 5 * sizeof(GLfloat),
                          (void *)(2 * sizeof(GLfloat)));
}

bool platform_graphics_ready(void) {
    return program != 0 && glIsProgram(program) && surface != EGL_NO_SURFACE;
}

void platform_graphics_present(void) {
    if (!platform_graphics_ready()) return;
    glViewport(0, 0, 1280, 720);
    glClearColor(0.012f, 0.055f, 0.085f, 1.0f);
    glClear(GL_COLOR_BUFFER_BIT);
    glUseProgram(program);
    glDrawArrays(GL_TRIANGLES, 0, 3);
    eglSwapBuffers(display, surface);
}

void platform_graphics_exit(void) {
    if (context != EGL_NO_CONTEXT) {
        if (program) glDeleteProgram(program);
        if (vertex_buffer) glDeleteBuffers(1, &vertex_buffer);
        eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
        if (surface != EGL_NO_SURFACE) eglDestroySurface(display, surface);
        eglDestroyContext(display, context);
    }
    if (display != EGL_NO_DISPLAY) eglTerminate(display);
}
#endif
