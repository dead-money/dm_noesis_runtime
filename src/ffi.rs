//! Hand-written declarations matching `cpp/noesis_shim.h`.
//!
//! When the shim grows past ~30 functions, switch to `bindgen` driven from a
//! `wrapper.h`. For Phase 0 the surface is too small to justify the build dep.

use std::os::raw::{c_char, c_void};

#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warning = 3,
    Error = 4,
}

pub type LogFn = unsafe extern "C" fn(
    userdata: *mut c_void,
    file: *const c_char,
    line: u32,
    level: LogLevel,
    channel: *const c_char,
    message: *const c_char,
);

unsafe extern "C" {
    pub fn dm_noesis_set_license(name: *const c_char, key: *const c_char);
    pub fn dm_noesis_set_log_handler(cb: Option<LogFn>, userdata: *mut c_void);
    pub fn dm_noesis_init();
    pub fn dm_noesis_shutdown();
    pub fn dm_noesis_version() -> *const c_char;
}
