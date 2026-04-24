// Narrow C ABI shim over the Noesis Native SDK.
//
// This is the ONLY header dm_noesis/src binds against. Rust declarations live
// in src/ffi.rs and are hand-mirrored — we do NOT bindgen NsCore/NsGui (their
// templates + Ptr<T> + virtual-dispatch surface does not translate cleanly).
//
// Phase 0 surface: lifecycle and version. Render device, View, input, XAML
// loading land in subsequent phases — see ../dm_noesis_bevy/CLAUDE.md for the
// phase plan.

#ifndef DM_NOESIS_SHIM_H
#define DM_NOESIS_SHIM_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum dm_noesis_log_level {
    DM_NOESIS_LOG_TRACE   = 0,
    DM_NOESIS_LOG_DEBUG   = 1,
    DM_NOESIS_LOG_INFO    = 2,
    DM_NOESIS_LOG_WARNING = 3,
    DM_NOESIS_LOG_ERROR   = 4
} dm_noesis_log_level;

typedef void (*dm_noesis_log_fn)(
    void* userdata,
    const char* file,
    uint32_t line,
    dm_noesis_log_level level,
    const char* channel,
    const char* message);

// Optional. Apply per-developer Indie license credentials. Call BEFORE
// dm_noesis_init. Pass empty strings to leave Noesis in trial mode.
void dm_noesis_set_license(const char* name, const char* key);

// Optional. Install a logging callback. Call BEFORE dm_noesis_init to capture
// init-time messages. Pass NULL to clear.
void dm_noesis_set_log_handler(dm_noesis_log_fn cb, void* userdata);

// Initialize Noesis subsystems. Call exactly once per process; Noesis does not
// support re-init after shutdown.
void dm_noesis_init(void);

// Shut Noesis down. Call once at process exit, after all Noesis-owned objects
// have been released.
void dm_noesis_shutdown(void);

// Returns the Noesis runtime build version (e.g. "3.2.12"). The pointer is
// owned by the Noesis runtime; do not free.
const char* dm_noesis_version(void);

// ── Render device (Phase 1) ────────────────────────────────────────────────
//
// The Rust side implements `Noesis::RenderDevice` by:
//   1. Constructing a `dm_noesis_render_device_vtable` of trampoline fn ptrs.
//   2. Calling `dm_noesis_render_device_create(&vtable, userdata)`.
//   3. Receiving back an opaque `void*` that is actually a Noesis::RenderDevice*
//      (specifically, an instance of the C++-internal RustRenderDevice subclass
//      that forwards every virtual into the vtable).
//   4. Calling `dm_noesis_render_device_destroy(device)` exactly once at end of
//      life. The C++-side intrusive ref count handles transitively-owned
//      textures and render targets.

// Texture metadata returned by the `create_texture` vtable slot. Mirrored on
// the Rust side as `crate::ffi::TextureBindingFfi` with the same layout.
typedef struct dm_noesis_texture_binding {
    uint64_t handle;       // 0 reserved invalid; valid handles are nonzero
    uint32_t width;
    uint32_t height;
    bool has_mipmaps;
    bool inverted;
    bool has_alpha;
    uint8_t pad;           // explicit so Rust mirror is unambiguous
} dm_noesis_texture_binding;

// Render-target metadata returned by `create_render_target` / `clone_render_target`.
typedef struct dm_noesis_render_target_binding {
    uint64_t handle;
    dm_noesis_texture_binding resolve_texture;
} dm_noesis_render_target_binding;

// vtable of fn pointers the Rust side fills in. The C++ subclass copies this
// struct on construction and dispatches every virtual through it.
//
// Pointer params marked `void*` carry POD struct pointers whose layouts the
// Rust side mirrors with `#[repr(C)]`:
//   - `out_caps`     → `Noesis::DeviceCaps*`     (= Rust `types::DeviceCaps`)
//   - `tile`/`tiles` → `const Noesis::Tile*`     (= Rust `types::Tile`)
//   - `batch`        → `const Noesis::Batch*`    (= Rust `types::Batch`)
//
// `data` in `create_texture` is `NULL` for dynamic textures, otherwise an
// array of `levels` `const void*` mip pointers (each tightly packed).
typedef struct dm_noesis_render_device_vtable {
    void (*get_caps)(void* userdata, void* out_caps);

    void (*create_texture)(
        void* userdata,
        const char* label, uint32_t width, uint32_t height, uint32_t levels,
        uint32_t format, const void* const* data,
        dm_noesis_texture_binding* out);
    // `format` is forwarded from the texture's create-time format so the Rust
    // side can construct an exact-length `&[u8]` from `data` without having to
    // track per-handle metadata separately.
    void (*update_texture)(
        void* userdata, uint64_t handle, uint32_t level,
        uint32_t x, uint32_t y, uint32_t width, uint32_t height,
        uint32_t format, const void* data);
    void (*end_updating_textures)(void* userdata, const uint64_t* handles, uint32_t count);
    void (*drop_texture)(void* userdata, uint64_t handle);

    void (*create_render_target)(
        void* userdata,
        const char* label, uint32_t width, uint32_t height,
        uint32_t sample_count, bool needs_stencil,
        dm_noesis_render_target_binding* out);
    void (*clone_render_target)(
        void* userdata, const char* label, uint64_t src_handle,
        dm_noesis_render_target_binding* out);
    void (*drop_render_target)(void* userdata, uint64_t handle);

    void (*begin_offscreen_render)(void* userdata);
    void (*end_offscreen_render)(void* userdata);
    void (*begin_onscreen_render)(void* userdata);
    void (*end_onscreen_render)(void* userdata);

    void (*set_render_target)(void* userdata, uint64_t handle);
    void (*begin_tile)(void* userdata, uint64_t handle, const void* tile);
    void (*end_tile)(void* userdata, uint64_t handle);
    void (*resolve_render_target)(
        void* userdata, uint64_t handle, const void* tiles, uint32_t count);

    void* (*map_vertices)(void* userdata, uint32_t bytes);
    void  (*unmap_vertices)(void* userdata);
    void* (*map_indices)(void* userdata, uint32_t bytes);
    void  (*unmap_indices)(void* userdata);

    void (*draw_batch)(void* userdata, const void* batch);
} dm_noesis_render_device_vtable;

// Create a `RustRenderDevice` instance, returning an opaque
// `Noesis::RenderDevice*` with intrusive ref count = 1. Call
// `dm_noesis_render_device_destroy` exactly once to release.
//
// Returns `NULL` on bad input (null vtable).
void* dm_noesis_render_device_create(
    const dm_noesis_render_device_vtable* vtable, void* userdata);

// Release the +1 reference held by `_create`'s caller. The actual destruction
// happens when the last `Ptr<>` goes away — including any Noesis-internal
// references — which transitively releases all `RustTexture` / `RustRenderTarget`
// instances allocated through the device, each calling `drop_texture` /
// `drop_render_target` on the vtable.
void dm_noesis_render_device_destroy(void* device);

// Extract the Rust-side handle stored in a `RustTexture` / `RustRenderTarget`
// instance. Return 0 if the input is null.
//
// Used by the Rust `draw_batch` impl to translate `Batch.pattern/ramps/...`
// pointers back into Rust-side `TextureHandle` values.
uint64_t dm_noesis_texture_get_handle(const void* texture);
uint64_t dm_noesis_render_target_get_handle(const void* surface);

// ── XAML provider (Phase 4.C) ──────────────────────────────────────────────
//
// The Rust side subclasses `Noesis::XamlProvider` via a vtable of fn pointers.
// `dm_noesis_xaml_provider_create` returns a `Noesis::XamlProvider*` (refcount
// = 1) wrapping that vtable; pair with `_destroy`. Install it globally with
// `dm_noesis_set_xaml_provider`.
//
// `load_xaml` callback contract:
//   - Return `true` with `*out_data` / `*out_len` set on success. The pointed
//     bytes must stay valid until Noesis finishes parsing the XAML, which is
//     synchronous with the `GUI::LoadXaml` call that triggered it. In practice
//     the Rust impl owns the bytes (e.g. in a HashMap) and returns a slice
//     into them.
//   - Return `false` to signal not-found; Noesis will produce a load error.

typedef struct dm_noesis_xaml_provider_vtable {
    bool (*load_xaml)(
        void* userdata,
        const char* uri,
        const uint8_t** out_data,
        uint32_t* out_len);
} dm_noesis_xaml_provider_vtable;

void* dm_noesis_xaml_provider_create(
    const dm_noesis_xaml_provider_vtable* vtable, void* userdata);
void dm_noesis_xaml_provider_destroy(void* provider);

// Install `provider` as the global XAML provider, or pass NULL to clear.
void dm_noesis_set_xaml_provider(void* provider);

// ── Font provider (Phase 4.F.1) ────────────────────────────────────────────
//
// Subclass of `Noesis::CachedFontProvider`. CachedFontProvider handles font
// matching (weight/stretch/style) internally once faces are registered; we
// only need two callbacks:
//
//   - `scan_folder(userdata, folder_uri, register_fn, register_cx)` — called
//     the first time a font is requested from a folder. Rust walks its
//     registry and invokes `register_fn(register_cx, filename)` once per
//     font file in that folder. The C++ side forwards each call to
//     `CachedFontProvider::RegisterFont(folder, filename)`, which opens
//     the file via `open_font` below to scan face metadata.
//
//   - `open_font(userdata, folder_uri, filename, out_data, out_len)` —
//     return `true` with `*out_data`/`*out_len` set; the pointed bytes
//     must stay valid until the font-stream reader finishes (same
//     contract as `load_xaml`). Return `false` to signal "not found".

typedef void (*dm_noesis_register_font_fn)(void* register_cx, const char* filename);

typedef struct dm_noesis_font_provider_vtable {
    void (*scan_folder)(
        void* userdata,
        const char* folder_uri,
        dm_noesis_register_font_fn register_fn,
        void* register_cx);

    bool (*open_font)(
        void* userdata,
        const char* folder_uri,
        const char* filename,
        const uint8_t** out_data,
        uint32_t* out_len);
} dm_noesis_font_provider_vtable;

void* dm_noesis_font_provider_create(
    const dm_noesis_font_provider_vtable* vtable, void* userdata);
void dm_noesis_font_provider_destroy(void* provider);

// Install `provider` as the global font provider, or pass NULL to clear.
void dm_noesis_set_font_provider(void* provider);

// ── XAML loading + View + Renderer (Phase 4.C) ─────────────────────────────
//
// Opaque pointer contracts:
//   - dm_noesis_gui_load_xaml returns a FrameworkElement* with refcount = 1.
//     Release with dm_noesis_base_component_release.
//   - dm_noesis_view_create returns an IView* with refcount = 1. Release with
//     dm_noesis_view_destroy.
//   - dm_noesis_view_get_renderer returns a borrowed IRenderer* owned by the
//     View. Do NOT release.

// Load XAML by URI. Returns a FrameworkElement* (+1 ref), or NULL if the
// resolved root isn't a FrameworkElement or the URI wasn't found.
void* dm_noesis_gui_load_xaml(const char* uri);

// Release a BaseComponent-derived object.
void dm_noesis_base_component_release(void* obj);

// Create an IView whose root is `framework_element`. The view retains its own
// reference to the element; the caller's reference is still held by the
// FrameworkElement wrapper until it's dropped.
void* dm_noesis_view_create(void* framework_element);

// Release an IView* obtained from dm_noesis_view_create.
void dm_noesis_view_destroy(void* view);

void dm_noesis_view_set_size(void* view, uint32_t width, uint32_t height);

// `matrix` is 16 floats, row-major (the native Matrix4::GetData() layout).
void dm_noesis_view_set_projection_matrix(void* view, const float* matrix);

bool dm_noesis_view_update(void* view, double time_seconds);

void dm_noesis_view_set_flags(void* view, uint32_t flags);

// Returns the IRenderer* owned by the View. Do NOT release.
void* dm_noesis_view_get_renderer(void* view);

// Initialize the renderer with `render_device`. The RenderDevice pointer is
// the opaque value returned from dm_noesis_render_device_create.
void dm_noesis_renderer_init(void* renderer, void* render_device);
void dm_noesis_renderer_shutdown(void* renderer);
bool dm_noesis_renderer_update_render_tree(void* renderer);
bool dm_noesis_renderer_render_offscreen(void* renderer);
void dm_noesis_renderer_render(void* renderer, bool flip_y, bool clear);

// ── View input (Phase 5) ───────────────────────────────────────────────────
//
// Thin trampolines over `Noesis::IView` input methods. `button` takes a
// `Noesis::MouseButton` value (see InputEnums.h); `key` takes a `Noesis::Key`.
// Out-of-range values are passed through — Noesis ignores unknown keys.
//
// Noesis requires a `MouseMove` at the press coordinate before a
// `MouseButtonDown` hits the correct element; callers must enqueue moves
// before buttons themselves.

bool dm_noesis_view_mouse_move(void* view, int32_t x, int32_t y);
bool dm_noesis_view_mouse_button_down(void* view, int32_t x, int32_t y, int32_t button);
bool dm_noesis_view_mouse_button_up(void* view, int32_t x, int32_t y, int32_t button);
bool dm_noesis_view_mouse_double_click(void* view, int32_t x, int32_t y, int32_t button);
bool dm_noesis_view_mouse_wheel(void* view, int32_t x, int32_t y, int32_t delta);
bool dm_noesis_view_scroll(void* view, int32_t x, int32_t y, float value);
bool dm_noesis_view_hscroll(void* view, int32_t x, int32_t y, float value);

bool dm_noesis_view_touch_down(void* view, int32_t x, int32_t y, uint64_t id);
bool dm_noesis_view_touch_move(void* view, int32_t x, int32_t y, uint64_t id);
bool dm_noesis_view_touch_up(void* view, int32_t x, int32_t y, uint64_t id);

bool dm_noesis_view_key_down(void* view, int32_t key);
bool dm_noesis_view_key_up(void* view, int32_t key);
bool dm_noesis_view_char(void* view, uint32_t codepoint);

void dm_noesis_view_activate(void* view);
void dm_noesis_view_deactivate(void* view);

#ifdef __cplusplus
}
#endif

#endif  // DM_NOESIS_SHIM_H
