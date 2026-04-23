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

// ────────────────────────────────────────────────────────────────────────────
// XamlProvider + View / Renderer FFI (Phase 4.C). See cpp/noesis_shim.h for
// pointer-ownership contracts.
// ────────────────────────────────────────────────────────────────────────────

#[repr(C)]
pub struct XamlProviderVTable {
    pub load_xaml: unsafe extern "C" fn(
        userdata: *mut c_void,
        uri: *const c_char,
        out_data: *mut *const u8,
        out_len: *mut u32,
    ) -> bool,
}

unsafe extern "C" {
    pub fn dm_noesis_xaml_provider_create(
        vtable: *const XamlProviderVTable,
        userdata: *mut c_void,
    ) -> *mut c_void;
    pub fn dm_noesis_xaml_provider_destroy(provider: *mut c_void);
    pub fn dm_noesis_set_xaml_provider(provider: *mut c_void);

    pub fn dm_noesis_gui_load_xaml(uri: *const c_char) -> *mut c_void;
    pub fn dm_noesis_base_component_release(obj: *mut c_void);

    pub fn dm_noesis_view_create(framework_element: *mut c_void) -> *mut c_void;
    pub fn dm_noesis_view_destroy(view: *mut c_void);
    pub fn dm_noesis_view_set_size(view: *mut c_void, width: u32, height: u32);
    pub fn dm_noesis_view_set_projection_matrix(view: *mut c_void, matrix: *const f32);
    pub fn dm_noesis_view_update(view: *mut c_void, time_seconds: f64) -> bool;
    pub fn dm_noesis_view_set_flags(view: *mut c_void, flags: u32);
    pub fn dm_noesis_view_get_renderer(view: *mut c_void) -> *mut c_void;

    pub fn dm_noesis_renderer_init(renderer: *mut c_void, render_device: *mut c_void);
    pub fn dm_noesis_renderer_shutdown(renderer: *mut c_void);
    pub fn dm_noesis_renderer_update_render_tree(renderer: *mut c_void) -> bool;
    pub fn dm_noesis_renderer_render_offscreen(renderer: *mut c_void) -> bool;
    pub fn dm_noesis_renderer_render(renderer: *mut c_void, flip_y: bool, clear: bool);
}
