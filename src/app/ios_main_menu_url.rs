//! Open the iOS menu's official-site link in the system's browser.
//!
//! UIApplication's asynchronous public API keeps the game menu intact while
//! iOS backgrounds the app. No embedded web view or third-party URL dependency.

use std::ffi::{c_char, c_void};

type Object = *mut c_void;
type Selector = *const c_void;

#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> Object;
    fn sel_registerName(name: *const c_char) -> Selector;
    fn objc_msgSend();
}

#[link(name = "UIKit", kind = "framework")]
unsafe extern "C" {}

/// Called only from the main-thread menu activation, after pointer release.
/// Returns whether the request was dispatched, not the asynchronous result of
/// loading the website. The system controls the selected browser and its UI.
pub(super) fn open_website() -> bool {
    // SAFETY: this public Darwin query has no preconditions.
    if unsafe { libc::pthread_main_np() } == 0 {
        return false;
    }
    // SAFETY: selectors and class names are NUL-terminated constants. The
    // signatures below match Foundation/UIKit's declared Objective-C methods.
    // Their convenience constructors return autoreleased objects, live through
    // this event callback; openURL retains its arguments for asynchronous work.
    unsafe {
        let send0: unsafe extern "C" fn(Object, Selector) -> Object =
            std::mem::transmute(objc_msgSend as *const ());
        let send1: unsafe extern "C" fn(Object, Selector, Object) -> Object =
            std::mem::transmute(objc_msgSend as *const ());
        let open: unsafe extern "C" fn(Object, Selector, Object, Object, Object) =
            std::mem::transmute(objc_msgSend as *const ());
        let string = send1(
            objc_getClass(c"NSString".as_ptr()),
            sel_registerName(c"stringWithUTF8String:".as_ptr()),
            c"https://youlidefuchou.com".as_ptr().cast_mut().cast(),
        );
        let url = send1(
            objc_getClass(c"NSURL".as_ptr()),
            sel_registerName(c"URLWithString:".as_ptr()),
            string,
        );
        let options = send0(
            objc_getClass(c"NSDictionary".as_ptr()),
            sel_registerName(c"dictionary".as_ptr()),
        );
        let app = send0(
            objc_getClass(c"UIApplication".as_ptr()),
            sel_registerName(c"sharedApplication".as_ptr()),
        );
        if url.is_null() || options.is_null() || app.is_null() {
            return false;
        }
        // Empty options dispatches an ordinary HTTPS URL to the user's default
        // browser. A nil completion handler is explicitly supported by UIKit.
        open(
            app,
            sel_registerName(c"openURL:options:completionHandler:".as_ptr()),
            url,
            options,
            std::ptr::null_mut(),
        );
    }
    true
}
