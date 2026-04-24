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

/// Callback signature the C++ side passes into `scan_folder` so Rust can
/// register each font filename synchronously. `register_cx` is opaque to
/// Rust — pass it back verbatim.
pub type RegisterFontFn = unsafe extern "C" fn(register_cx: *mut c_void, filename: *const c_char);

#[repr(C)]
pub struct FontProviderVTable {
    pub scan_folder: unsafe extern "C" fn(
        userdata: *mut c_void,
        folder_uri: *const c_char,
        register_fn: RegisterFontFn,
        register_cx: *mut c_void,
    ),
    pub open_font: unsafe extern "C" fn(
        userdata: *mut c_void,
        folder_uri: *const c_char,
        filename: *const c_char,
        out_data: *mut *const u8,
        out_len: *mut u32,
    ) -> bool,
}

/// Mirror of `dm_noesis_texture_info` in noesis_shim.h — texture metadata
/// returned by the provider's `get_info` callback.
#[repr(C)]
pub struct TextureInfoFfi {
    pub width: u32,
    pub height: u32,
    pub x: u32,
    pub y: u32,
    pub dpi_scale: f32,
}

#[repr(C)]
pub struct TextureProviderVTable {
    pub get_info: unsafe extern "C" fn(
        userdata: *mut c_void,
        uri: *const c_char,
        out: *mut TextureInfoFfi,
    ) -> bool,
    pub load_texture: unsafe extern "C" fn(
        userdata: *mut c_void,
        uri: *const c_char,
        out_width: *mut u32,
        out_height: *mut u32,
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

    pub fn dm_noesis_font_provider_create(
        vtable: *const FontProviderVTable,
        userdata: *mut c_void,
    ) -> *mut c_void;
    pub fn dm_noesis_font_provider_destroy(provider: *mut c_void);
    pub fn dm_noesis_set_font_provider(provider: *mut c_void);
    pub fn dm_noesis_set_font_fallbacks(families: *const *const c_char, count: u32);
    pub fn dm_noesis_set_font_default_properties(
        size: f32,
        weight: i32,
        stretch: i32,
        style: i32,
    );

    pub fn dm_noesis_texture_provider_create(
        vtable: *const TextureProviderVTable,
        userdata: *mut c_void,
    ) -> *mut c_void;
    pub fn dm_noesis_texture_provider_destroy(provider: *mut c_void);
    pub fn dm_noesis_set_texture_provider(provider: *mut c_void);

    pub fn dm_noesis_gui_load_xaml(uri: *const c_char) -> *mut c_void;
    pub fn dm_noesis_gui_load_application_resources(uri: *const c_char) -> bool;
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

    pub fn dm_noesis_view_mouse_move(view: *mut c_void, x: i32, y: i32) -> bool;
    pub fn dm_noesis_view_mouse_button_down(view: *mut c_void, x: i32, y: i32, button: i32)
        -> bool;
    pub fn dm_noesis_view_mouse_button_up(view: *mut c_void, x: i32, y: i32, button: i32) -> bool;
    pub fn dm_noesis_view_mouse_double_click(
        view: *mut c_void,
        x: i32,
        y: i32,
        button: i32,
    ) -> bool;
    pub fn dm_noesis_view_mouse_wheel(view: *mut c_void, x: i32, y: i32, delta: i32) -> bool;
    pub fn dm_noesis_view_scroll(view: *mut c_void, x: i32, y: i32, value: f32) -> bool;
    pub fn dm_noesis_view_hscroll(view: *mut c_void, x: i32, y: i32, value: f32) -> bool;

    pub fn dm_noesis_view_touch_down(view: *mut c_void, x: i32, y: i32, id: u64) -> bool;
    pub fn dm_noesis_view_touch_move(view: *mut c_void, x: i32, y: i32, id: u64) -> bool;
    pub fn dm_noesis_view_touch_up(view: *mut c_void, x: i32, y: i32, id: u64) -> bool;

    pub fn dm_noesis_view_key_down(view: *mut c_void, key: i32) -> bool;
    pub fn dm_noesis_view_key_up(view: *mut c_void, key: i32) -> bool;
    pub fn dm_noesis_view_char(view: *mut c_void, codepoint: u32) -> bool;

    pub fn dm_noesis_view_activate(view: *mut c_void);
    pub fn dm_noesis_view_deactivate(view: *mut c_void);
}
