//! Stops the node cleanly when Windows shuts down, restarts or logs off.
//!
//! The node is a console program without windows, and Windows ends it at shutdown without notice (it gets no
//! console event either, as it loads user32.dll), which leaves its database to be replayed at the next start.
//! A hidden top-level window of this app receives WM_QUERYENDSESSION instead.
//!
//! The node is stopped right there, in WM_QUERYENDSESSION, while Windows waits on the "apps are preventing
//! shutdown" screen (ShutdownBlockReasonCreate): by WM_ENDSESSION the app's own window loop may end the process
//! at any moment, mid-stop. If the shutdown is then cancelled, the node is started again. This process also
//! asks to be told first (SetProcessShutdownParameters), and the node asks to be ended last.
//!
//! What happens is written to gui.log next to the app's settings, since nobody watches a shutdown.

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Shutdown::{ShutdownBlockReasonCreate, ShutdownBlockReasonDestroy};
use windows_sys::Win32::System::Threading::SetProcessShutdownParameters;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW, TranslateMessage, MSG,
    WM_ENDSESSION, WM_QUERYENDSESSION, WNDCLASSW, WS_OVERLAPPED,
};

/// The hidden window's class name (also how a test finds it)
pub const WINDOW_CLASS: &str = "GrapheneNodeSessionEnd";
/// The highest shutdown level an application may take: Windows notifies it before others
const FIRST_TO_SHUT_DOWN: u32 = 0x3FF;
const GUI_LOG_LIMIT: u64 = 1 << 20;

pub struct Actions {
    /// Stops the node and returns once it has exited or given up; true if a node was running
    pub stop_node: Box<dyn Fn() -> bool + Send + Sync>,
    /// Starts the node again, after a cancelled shutdown
    pub start_node: Box<dyn Fn() + Send + Sync>,
}

struct Handler {
    reason: Vec<u16>,
    log_path: PathBuf,
    actions: Actions,
    stopped_for_session: AtomicBool,
}

static HANDLER: OnceLock<Handler> = OnceLock::new();

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Appends a line to gui.log, starting it over past 1 MB.
pub fn gui_log(path: &std::path::Path, line: &str) {
    if std::fs::metadata(path).map(|m| m.len() > GUI_LOG_LIMIT).unwrap_or(false) {
        std::fs::remove_file(path).ok();
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        writeln!(f, "{secs} {line}").ok();
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let Some(h) = HANDLER.get() else { return DefWindowProcW(hwnd, msg, wparam, lparam) };
    match msg {
        WM_QUERYENDSESSION => {
            gui_log(&h.log_path, &format!("WM_QUERYENDSESSION (flags {lparam:#x}): stopping the node"));
            ShutdownBlockReasonCreate(hwnd, h.reason.as_ptr());
            let t = Instant::now();
            let was_running = (h.actions.stop_node)();
            h.stopped_for_session.store(was_running, Ordering::SeqCst);
            gui_log(&h.log_path, &format!("node {} in {} ms", if was_running { "stopped" } else { "was not running" },
                                          t.elapsed().as_millis()));
            1
        }
        WM_ENDSESSION => {
            if wparam != 0 {
                gui_log(&h.log_path, "WM_ENDSESSION: the session ends");
            } else {
                gui_log(&h.log_path, "WM_ENDSESSION: shutdown cancelled");
                if h.stopped_for_session.swap(false, Ordering::SeqCst) {
                    (h.actions.start_node)();
                }
            }
            ShutdownBlockReasonDestroy(hwnd);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Starts the watch on a thread of its own. `reason` is what Windows shows while it waits.
pub fn watch(reason: &str, log_path: PathBuf, actions: Actions) {
    let handler = Handler { reason: wide(reason), log_path, actions, stopped_for_session: AtomicBool::new(false) };
    if HANDLER.set(handler).is_err() {
        return;
    }
    unsafe { SetProcessShutdownParameters(FIRST_TO_SHUT_DOWN, 0) };
    std::thread::spawn(|| unsafe {
        let log = |m: String| gui_log(&HANDLER.get().unwrap().log_path, &m);
        let class = wide(WINDOW_CLASS);
        let instance = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        if RegisterClassW(&wc) == 0 {
            log(format!("session end watch: RegisterClassW failed ({})", GetLastError()));
            return;
        }
        // A top-level window, never shown: message-only windows do not get the session broadcasts
        let hwnd = CreateWindowExW(0, class.as_ptr(), class.as_ptr(), WS_OVERLAPPED, 0, 0, 0, 0,
                                   std::ptr::null_mut(), std::ptr::null_mut(), instance, std::ptr::null());
        if hwnd.is_null() {
            log(format!("session end watch: CreateWindowExW failed ({})", GetLastError()));
            return;
        }
        log("session end watch: ready".into());
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}
