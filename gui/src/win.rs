//! Thin wrappers over the Win32 calls the supervisor needs: processes and named events.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE, WAIT_TIMEOUT};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, GetExitCodeProcess, OpenEventW, OpenProcess, QueryFullProcessImageNameW, SetEvent,
    TerminateProcess, WaitForSingleObject, CREATE_NO_WINDOW, EVENT_MODIFY_STATE,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
};

/// An owned kernel handle, closed on drop.
pub struct Handle(HANDLE);

// Kernel handles may be used from any thread
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

fn last_error(what: &str) -> String {
    format!("{what} (Windows error {})", unsafe { GetLastError() })
}

/// Starts the node with no console window, detached from this process's lifetime.
pub fn spawn(exe: &Path, args: &[String]) -> Result<u32, String> {
    let child = Command::new(exe)
        .args(args)
        .current_dir(exe.parent().unwrap_or(Path::new(".")))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Cannot start {}: {e}", exe.display()))?;
    // Dropping Child neither waits for nor kills the process; it is watched by PID from here on
    Ok(child.id())
}

pub struct Process {
    pub pid: u32,
    handle: Handle,
}

impl Process {
    pub fn open(pid: u32) -> Option<Process> {
        let h = unsafe {
            OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE, 0, pid)
        };
        if h.is_null() {
            None
        } else {
            Some(Process { pid, handle: Handle(h) })
        }
    }

    pub fn is_alive(&self) -> bool {
        unsafe { WaitForSingleObject(self.handle.0, 0) == WAIT_TIMEOUT }
    }

    pub fn exit_code(&self) -> Option<u32> {
        let mut code = 0u32;
        if self.is_alive() || unsafe { GetExitCodeProcess(self.handle.0, &mut code) } == 0 {
            None
        } else {
            Some(code)
        }
    }

    pub fn image_path(&self) -> Option<PathBuf> {
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = unsafe { QueryFullProcessImageNameW(self.handle.0, 0, buf.as_mut_ptr(), &mut len) };
        (ok != 0).then(|| PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])))
    }

    pub fn terminate(&self) -> Result<(), String> {
        if unsafe { TerminateProcess(self.handle.0, 1) } == 0 {
            return Err(last_error("Cannot terminate the node"));
        }
        Ok(())
    }
}

/// A named manual-reset event; the node exits cleanly when it is set (--shutdown-event).
pub struct Event {
    handle: Handle,
}

impl Event {
    pub fn create(name: &str) -> Result<Event, String> {
        let h = unsafe { CreateEventW(std::ptr::null(), 1, 0, wide(name).as_ptr()) };
        if h.is_null() {
            return Err(last_error("Cannot create the shutdown event"));
        }
        Ok(Event { handle: Handle(h) })
    }

    /// Opens an event created earlier, by a previous run of this app; it lives on while the node holds it.
    pub fn open(name: &str) -> Option<Event> {
        let h = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, wide(name).as_ptr()) };
        (!h.is_null()).then(|| Event { handle: Handle(h) })
    }

    pub fn set(&self) -> Result<(), String> {
        if unsafe { SetEvent(self.handle.0) } == 0 {
            return Err(last_error("Cannot signal the shutdown event"));
        }
        Ok(())
    }
}

/// PIDs of running processes whose executable file name is `exe_name` (case-insensitive).
pub fn find_processes(exe_name: &str) -> Vec<u32> {
    let mut pids = Vec::new();
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snap == INVALID_HANDLE_VALUE {
        return pids;
    }
    let _snap = Handle(snap);
    let mut e: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut ok = unsafe { Process32FirstW(snap, &mut e) } != 0;
    while ok {
        let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
        if String::from_utf16_lossy(&e.szExeFile[..len]).eq_ignore_ascii_case(exe_name) {
            pids.push(e.th32ProcessID);
        }
        ok = unsafe { Process32NextW(snap, &mut e) } != 0;
    }
    pids
}
