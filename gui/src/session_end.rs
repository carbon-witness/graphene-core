//! Stops the node cleanly when Windows shuts down, restarts or logs off.
//!
//! The node is a console program without windows, and Windows ends it at shutdown without notice (it gets no
//! console event either, as it loads user32.dll), which leaves its database to be replayed at the next start.
//! A hidden top-level window of this app receives WM_QUERYENDSESSION / WM_ENDSESSION instead: it asks Windows
//! to wait (ShutdownBlockReasonCreate, shown on the "apps are preventing shutdown" screen), stops the node,
//! and lets the shutdown go on.

use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Shutdown::{ShutdownBlockReasonCreate, ShutdownBlockReasonDestroy};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW, TranslateMessage, MSG,
    WM_ENDSESSION, WM_QUERYENDSESSION, WNDCLASSW, WS_OVERLAPPED,
};

/// The hidden window's class name (also how a test finds it)
pub const WINDOW_CLASS: &str = "GrapheneNodeSessionEnd";

struct Handler {
    reason: Vec<u16>,
    stop_node: Box<dyn Fn() + Send + Sync>,
}

static HANDLER: OnceLock<Handler> = OnceLock::new();

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match (msg, HANDLER.get()) {
        (WM_QUERYENDSESSION, Some(h)) => {
            ShutdownBlockReasonCreate(hwnd, h.reason.as_ptr());
            1 // agree; the stop happens in WM_ENDSESSION, which Windows waits for
        }
        (WM_ENDSESSION, Some(h)) => {
            if wparam != 0 {
                (h.stop_node)();
            }
            ShutdownBlockReasonDestroy(hwnd);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Starts the watch on a thread of its own. `reason` is what Windows shows while it waits; `stop_node` must
/// stop the node and return once it has exited or given up (Windows is waiting meanwhile).
pub fn watch(reason: &str, stop_node: impl Fn() + Send + Sync + 'static) {
    if HANDLER.set(Handler { reason: wide(reason), stop_node: Box::new(stop_node) }).is_err() {
        return;
    }
    std::thread::spawn(|| unsafe {
        let class = wide(WINDOW_CLASS);
        let instance = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        if RegisterClassW(&wc) == 0 {
            eprintln!("session end watch: RegisterClassW failed ({})", GetLastError());
            return;
        }
        // A top-level window, never shown: message-only windows do not get the session broadcasts
        let hwnd = CreateWindowExW(0, class.as_ptr(), class.as_ptr(), WS_OVERLAPPED, 0, 0, 0, 0,
                                   std::ptr::null_mut(), std::ptr::null_mut(), instance, std::ptr::null());
        if hwnd.is_null() {
            eprintln!("session end watch: CreateWindowExW failed ({})", GetLastError());
            return;
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}
