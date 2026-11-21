//! A `rules.wasm` in Rust against the first engine step of RFC 0001.
//!
//! Every 15th frame it pays house `HOUSE` 100 credits and logs it; on frame 30
//! it sets Red Alert 2 local variable 5. The host calls are declared by hand
//! here; the SDK will generate them.
#![no_std]

mod vera {
    #[link(wasm_import_module = "vera:sim@1.0")]
    unsafe extern "C" {
        pub fn frame() -> i32;
        pub fn log(ptr: *const u8, len: i32);
        pub fn var_set(scope: i64, var: i32, value: i64) -> i32;
        pub fn action(
            kind: i32,
            target: i64,
            params: *const ActionParams,
            len: i32,
            out: *mut i64,
        ) -> i32;
    }

    /// `ActionParams`, 72 bytes (RFC appendix "Fixed layouts").
    #[repr(C)]
    #[derive(Default)]
    pub struct ActionParams {
        pub id: i32,
        pub arg: i32,
        pub arg2: i32,
        pub reserved: i32,
        pub value: i64,
        pub value2: i64,
        pub cell: i64,
        pub house: i64,
        pub other: i64,
        pub text: i32,
        pub text_len: i32,
        pub extra: i32,
        pub extra_len: i32,
    }
}

const ADD_MONEY: i32 = 1015;
/// The provisional house reference for house array index 0 (bit 47).
const HOUSE: i64 = 1 << 47;

fn log(line: &str) {
    unsafe { vera::log(line.as_ptr(), line.len() as i32) }
}

#[unsafe(no_mangle)]
pub extern "C" fn on_scenario_start() {
    log("minimal-rules: started");
}

#[unsafe(no_mangle)]
pub extern "C" fn on_frame() {
    let frame = unsafe { vera::frame() };
    if frame % 15 == 0 {
        let params = vera::ActionParams {
            value: 100,
            ..Default::default()
        };
        let mut handle = 0;
        let status = unsafe {
            vera::action(
                ADD_MONEY,
                HOUSE,
                &params,
                size_of::<vera::ActionParams>() as i32,
                &mut handle,
            )
        };
        if status == 0 {
            log("minimal-rules: paid 100");
        }
    }
    if frame == 30 {
        unsafe { vera::var_set(0, 5, 1) };
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
