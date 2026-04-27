//! Safe wrappers around the Noesis `FrameworkElement`, `IView`, and
//! `IRenderer` opaque pointers (Phase 4.C).
//!
//! ```text
//!   load_xaml(uri) -> FrameworkElement
//!   FrameworkElement + View::create -> View
//!   View::renderer() -> Renderer (borrowed from View)
//!   Renderer: init(device), update_render_tree, render_offscreen, render, shutdown
//! ```
//!
//! Every owning wrapper releases its +1 reference on drop via the Noesis
//! intrusive refcount, which means the Noesis runtime must still be alive
//! (i.e. [`crate::shutdown`] not yet called) at drop time — otherwise the
//! `Release()` path would touch freed state. Keep these wrappers on the
//! stack for the scope of a single frame, dropped before `shutdown`.

use core::marker::PhantomData;
use core::ptr::NonNull;
use std::ffi::{c_void, CStr, CString};

use crate::ffi::{
    dm_noesis_base_component_release, dm_noesis_framework_element_find_name,
    dm_noesis_framework_element_get_name, dm_noesis_gui_load_xaml, dm_noesis_renderer_init,
    dm_noesis_renderer_render, dm_noesis_renderer_render_offscreen, dm_noesis_renderer_shutdown,
    dm_noesis_renderer_update_render_tree, dm_noesis_view_activate, dm_noesis_view_char,
    dm_noesis_view_create, dm_noesis_view_deactivate, dm_noesis_view_destroy,
    dm_noesis_view_get_content, dm_noesis_view_get_renderer, dm_noesis_view_hscroll,
    dm_noesis_view_key_down,
    dm_noesis_view_key_up, dm_noesis_view_mouse_button_down, dm_noesis_view_mouse_button_up,
    dm_noesis_view_mouse_double_click, dm_noesis_view_mouse_move, dm_noesis_view_mouse_wheel,
    dm_noesis_view_scroll, dm_noesis_view_set_flags, dm_noesis_view_set_projection_matrix,
    dm_noesis_view_set_size, dm_noesis_view_touch_down, dm_noesis_view_touch_move,
    dm_noesis_view_touch_up, dm_noesis_view_update,
};
use crate::render_device::Registered as RegisteredDevice;

/// A loaded XAML root. Holds a +1 refcount on the underlying
/// `Noesis::FrameworkElement`; [`View::create`] consumes it and forwards the
/// ownership to the View.
pub struct FrameworkElement {
    ptr: NonNull<c_void>,
}

// SAFETY: `FrameworkElement` wraps a raw pointer to a Noesis-owned
// `Ptr<FrameworkElement>`. Noesis's API contract is "calls on a given object
// are serialized to one thread" — not "the object must stay on one thread
// for its whole lifetime." Moving a FrameworkElement between threads (via
// `Send`) is safe as long as the receiving thread is the only one making
// subsequent calls. Bevy's resource scheduler guarantees that: access to
// a `Resource` is serialized through `ResMut<_>`, and our callers only
// hold the element across a single render-thread borrow.
//
// `Sync` is safe for essentially the same reason: every mutating method
// takes `&mut self`, so `&FrameworkElement` carries no usable calls to
// Noesis — concurrent shared borrows can't race on Noesis state.
unsafe impl Send for FrameworkElement {}
unsafe impl Sync for FrameworkElement {}

impl FrameworkElement {
    /// Load XAML by URI. Returns `None` when the URI is unknown to the
    /// installed `XamlProvider` or when the loaded root is not a
    /// `FrameworkElement`. Requires a provider installed via
    /// [`crate::xaml_provider::set_xaml_provider`].
    ///
    /// # Panics
    ///
    /// Panics if `uri` contains an interior NUL byte.
    #[must_use]
    pub fn load(uri: &str) -> Option<Self> {
        let c = CString::new(uri).expect("uri contained interior NUL");
        // SAFETY: c.as_ptr() is valid for the duration of the call; the
        // C ABI just copies into Noesis::Uri.
        let ptr = unsafe { dm_noesis_gui_load_xaml(c.as_ptr()) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    fn into_raw(self) -> *mut c_void {
        let ptr = self.ptr.as_ptr();
        core::mem::forget(self);
        ptr
    }

    /// Raw `Noesis::FrameworkElement*` for handing to other Noesis APIs that
    /// take one (e.g. event subscription). Borrowed for the lifetime of
    /// `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Look up a descendant by `x:Name`. Returns `None` if no element with
    /// that name exists in this element's namescope, or if the named object
    /// is not itself a `FrameworkElement` (e.g. it's a `Brush` registered in
    /// a `ResourceDictionary`).
    ///
    /// The returned element holds an independent `+1` reference — dropping
    /// it does not affect `self`.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn find_name(&self, name: &str) -> Option<Self> {
        let c = CString::new(name).expect("name contained interior NUL");
        // SAFETY: self.ptr is a live FrameworkElement*; c lives for the call.
        let ptr = unsafe { dm_noesis_framework_element_find_name(self.ptr.as_ptr(), c.as_ptr()) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// The element's `x:Name`, or `None` if it has no name. The returned
    /// string is a borrowed copy — Noesis owns the underlying storage.
    #[must_use]
    pub fn name(&self) -> Option<String> {
        // SAFETY: self.ptr is a live FrameworkElement*; the C entrypoint
        // returns either NULL or a Noesis-owned static-ish string we copy
        // immediately.
        let p = unsafe { dm_noesis_framework_element_get_name(self.ptr.as_ptr()) };
        if p.is_null() {
            None
        } else {
            // SAFETY: p is a NUL-terminated UTF-8 / ASCII string while we
            // hold our element reference; copy out before yielding control.
            Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
        }
    }
}

impl Drop for FrameworkElement {
    fn drop(&mut self) {
        // SAFETY: produced by dm_noesis_gui_load_xaml which returns a +1 ref.
        unsafe { dm_noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// A Noesis view wrapping a loaded XAML root. Owns a +1 refcount on the
/// underlying `Noesis::IView`; its internal `Ptr<FrameworkElement>` keeps
/// the root alive too.
pub struct View {
    ptr: NonNull<c_void>,
}

// SAFETY: same rationale as [`FrameworkElement`] — Noesis serialises
// per-object calls to one thread at a time; every `View` method is `&mut
// self`; Bevy's scheduler prevents concurrent access. Moving a View between
// threads, or holding a `&View` from multiple threads simultaneously (which
// offers no usable mutation), is safe.
unsafe impl Send for View {}
unsafe impl Sync for View {}

impl View {
    /// Create a View whose root is `content`. Consumes the
    /// [`FrameworkElement`] wrapper — its refcount transfers into the view.
    ///
    /// # Panics
    ///
    /// Panics if the Noesis factory returns null (only possible on internal
    /// logic errors once `content` is non-null).
    #[must_use]
    pub fn create(content: FrameworkElement) -> Self {
        let raw = content.into_raw();
        // SAFETY: raw is a live FrameworkElement* with +1 ref.
        let ptr = unsafe { dm_noesis_view_create(raw) };
        // View took its own ref internally; release our +1 on the element so
        // refcount stays balanced (its total is still the original 1).
        unsafe { dm_noesis_base_component_release(raw) };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_view_create returned null"),
        }
    }

    /// Surface size the view lays out against.
    pub fn set_size(&mut self, width: u32, height: u32) {
        unsafe { dm_noesis_view_set_size(self.ptr.as_ptr(), width, height) }
    }

    /// Set the projection matrix. 16 floats, row-major — the native
    /// `Matrix4::GetData()` layout. Typical Noesis-facing projection is an
    /// ortho that maps UI pixel coords into Noesis's clip space (0..width,
    /// 0..height).
    pub fn set_projection_matrix(&mut self, matrix: &[f32; 16]) {
        unsafe { dm_noesis_view_set_projection_matrix(self.ptr.as_ptr(), matrix.as_ptr()) }
    }

    /// Combination of [`RenderFlag`] values — see `NsGui/IView.h` for the
    /// canonical list.
    pub fn set_flags(&mut self, flags: u32) {
        unsafe { dm_noesis_view_set_flags(self.ptr.as_ptr(), flags) }
    }

    /// Recover keyboard focus for this view. Noesis ignores keyboard input
    /// until a view is activated.
    pub fn activate(&mut self) {
        unsafe { dm_noesis_view_activate(self.ptr.as_ptr()) }
    }

    /// Release keyboard focus.
    pub fn deactivate(&mut self) {
        unsafe { dm_noesis_view_deactivate(self.ptr.as_ptr()) }
    }

    /// Pointer position, in physical pixels, origin top-left. Noesis
    /// requires a `mouse_move` at the press coordinate before a
    /// [`Self::mouse_button_down`] or [`Self::touch_down`] will hit-test
    /// correctly; callers must ensure the ordering.
    pub fn mouse_move(&mut self, x: i32, y: i32) -> bool {
        unsafe { dm_noesis_view_mouse_move(self.ptr.as_ptr(), x, y) }
    }

    pub fn mouse_button_down(&mut self, x: i32, y: i32, button: MouseButton) -> bool {
        unsafe { dm_noesis_view_mouse_button_down(self.ptr.as_ptr(), x, y, button as i32) }
    }

    pub fn mouse_button_up(&mut self, x: i32, y: i32, button: MouseButton) -> bool {
        unsafe { dm_noesis_view_mouse_button_up(self.ptr.as_ptr(), x, y, button as i32) }
    }

    pub fn mouse_double_click(&mut self, x: i32, y: i32, button: MouseButton) -> bool {
        unsafe { dm_noesis_view_mouse_double_click(self.ptr.as_ptr(), x, y, button as i32) }
    }

    /// `delta` is signed — Noesis uses Windows-style 120 units per notch.
    pub fn mouse_wheel(&mut self, x: i32, y: i32, delta: i32) -> bool {
        unsafe { dm_noesis_view_mouse_wheel(self.ptr.as_ptr(), x, y, delta) }
    }

    /// Vertical scroll with the cursor at `(x, y)`. `value` is in lines
    /// (per WPF convention — integer lines, fractional allowed).
    pub fn scroll(&mut self, x: i32, y: i32, value: f32) -> bool {
        unsafe { dm_noesis_view_scroll(self.ptr.as_ptr(), x, y, value) }
    }

    /// Horizontal scroll. See [`Self::scroll`].
    pub fn hscroll(&mut self, x: i32, y: i32, value: f32) -> bool {
        unsafe { dm_noesis_view_hscroll(self.ptr.as_ptr(), x, y, value) }
    }

    pub fn touch_down(&mut self, x: i32, y: i32, id: u64) -> bool {
        unsafe { dm_noesis_view_touch_down(self.ptr.as_ptr(), x, y, id) }
    }

    pub fn touch_move(&mut self, x: i32, y: i32, id: u64) -> bool {
        unsafe { dm_noesis_view_touch_move(self.ptr.as_ptr(), x, y, id) }
    }

    pub fn touch_up(&mut self, x: i32, y: i32, id: u64) -> bool {
        unsafe { dm_noesis_view_touch_up(self.ptr.as_ptr(), x, y, id) }
    }

    pub fn key_down(&mut self, key: Key) -> bool {
        unsafe { dm_noesis_view_key_down(self.ptr.as_ptr(), key as i32) }
    }

    pub fn key_up(&mut self, key: Key) -> bool {
        unsafe { dm_noesis_view_key_up(self.ptr.as_ptr(), key as i32) }
    }

    /// Text-input codepoint. Send between the matching
    /// [`Self::key_down`]/[`Self::key_up`] pair for the key that produced
    /// the character.
    pub fn char_input(&mut self, codepoint: u32) -> bool {
        unsafe { dm_noesis_view_char(self.ptr.as_ptr(), codepoint) }
    }

    /// Run layout + record a snapshot for the renderer. Returns `false` when
    /// nothing changed and skipping the render pair is safe.
    pub fn update(&mut self, time_seconds: f64) -> bool {
        unsafe { dm_noesis_view_update(self.ptr.as_ptr(), time_seconds) }
    }

    /// Borrow the renderer owned by this view. The `Renderer` can't outlive
    /// the `View`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns a null renderer — impossible on a
    /// successfully-constructed `View`.
    pub fn renderer(&mut self) -> Renderer<'_> {
        let ptr = unsafe { dm_noesis_view_get_renderer(self.ptr.as_ptr()) };
        Renderer {
            ptr: NonNull::new(ptr).expect("GetRenderer returned null"),
            _view: PhantomData,
        }
    }

    /// Raw `Noesis::IView*` for handing to other Noesis APIs that take one.
    /// Borrowed for the lifetime of this `View`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// The view's content root, as an owning [`FrameworkElement`]. Returns
    /// `None` only if the view has no content (which shouldn't happen on a
    /// successfully-constructed `View` — but guard the contract anyway).
    ///
    /// The returned element is independently refcounted; dropping it does
    /// not affect the view's own internal reference. Useful for `find_name`
    /// lookups against the live tree (e.g. wiring [`crate::events::subscribe_click`]
    /// to a named button after the view is up).
    #[must_use]
    pub fn content(&self) -> Option<FrameworkElement> {
        // SAFETY: self.ptr is a live IView*; the C entrypoint AddRefs the
        // returned content pointer so Rust owns the +1.
        let ptr = unsafe { dm_noesis_view_get_content(self.ptr.as_ptr()) };
        NonNull::new(ptr).map(|ptr| FrameworkElement { ptr })
    }
}

impl Drop for View {
    fn drop(&mut self) {
        // SAFETY: produced by dm_noesis_view_create which returns +1 ref.
        unsafe { dm_noesis_view_destroy(self.ptr.as_ptr()) }
    }
}

/// Mirror of `Noesis::MouseButton` from `NsGui/InputEnums.h`. Ordinals
/// validated at C++ compile time via `static_assert` in `noesis_view.cpp`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left = 0,
    Right = 1,
    Middle = 2,
    XButton1 = 3,
    XButton2 = 4,
}

/// Subset of `Noesis::Key` from `NsGui/InputEnums.h` — the keys Bevy's
/// `KeyCode` can produce. Values are the C++ enum ordinals, validated by
/// `static_assert` in `noesis_view.cpp`. Anything outside this subset can
/// still be sent via [`View::key_down`] with a raw cast; prefer adding a
/// variant here (and a matching assert in C++) to centralize the mapping.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Key {
    None = 0,
    Back = 2,
    Tab = 3,
    Return = 6,
    Pause = 7,
    CapsLock = 8,
    Escape = 13,
    Space = 18,
    PageUp = 19,
    PageDown = 20,
    End = 21,
    Home = 22,
    Left = 23,
    Up = 24,
    Right = 25,
    Down = 26,
    PrintScreen = 30,
    Insert = 31,
    Delete = 32,
    Help = 33,
    D0 = 34,
    D1 = 35,
    D2 = 36,
    D3 = 37,
    D4 = 38,
    D5 = 39,
    D6 = 40,
    D7 = 41,
    D8 = 42,
    D9 = 43,
    A = 44,
    B = 45,
    C = 46,
    D = 47,
    E = 48,
    F = 49,
    G = 50,
    H = 51,
    I = 52,
    J = 53,
    K = 54,
    L = 55,
    M = 56,
    N = 57,
    O = 58,
    P = 59,
    Q = 60,
    R = 61,
    S = 62,
    T = 63,
    U = 64,
    V = 65,
    W = 66,
    X = 67,
    Y = 68,
    Z = 69,
    LWin = 70,
    RWin = 71,
    Apps = 72,
    NumPad0 = 74,
    NumPad1 = 75,
    NumPad2 = 76,
    NumPad3 = 77,
    NumPad4 = 78,
    NumPad5 = 79,
    NumPad6 = 80,
    NumPad7 = 81,
    NumPad8 = 82,
    NumPad9 = 83,
    Multiply = 84,
    Add = 85,
    Subtract = 87,
    Decimal = 88,
    Divide = 89,
    F1 = 90,
    F2 = 91,
    F3 = 92,
    F4 = 93,
    F5 = 94,
    F6 = 95,
    F7 = 96,
    F8 = 97,
    F9 = 98,
    F10 = 99,
    F11 = 100,
    F12 = 101,
    F13 = 102,
    F14 = 103,
    F15 = 104,
    F16 = 105,
    F17 = 106,
    F18 = 107,
    F19 = 108,
    F20 = 109,
    F21 = 110,
    F22 = 111,
    F23 = 112,
    F24 = 113,
    NumLock = 114,
    ScrollLock = 115,
    LeftShift = 116,
    RightShift = 117,
    LeftCtrl = 118,
    RightCtrl = 119,
    LeftAlt = 120,
    RightAlt = 121,
    /// Semicolon / colon on US layouts (`Key_Oem1` / `Key_OemSemicolon`).
    OemSemicolon = 140,
    /// `=` / `+` (`Key_OemPlus`).
    OemPlus = 141,
    OemComma = 142,
    OemMinus = 143,
    OemPeriod = 144,
    /// `/` / `?` (`Key_Oem2` / `Key_OemQuestion`).
    OemSlash = 145,
    /// Backtick / tilde (`Key_Oem3` / `Key_OemTilde`).
    OemTilde = 146,
    /// `[` / `{` (`Key_Oem4` / `Key_OemOpenBrackets`).
    OemOpenBrackets = 149,
    /// `\` / `|` (`Key_Oem5` / `Key_OemPipe`).
    OemPipe = 150,
    /// `]` / `}` (`Key_Oem6` / `Key_OemCloseBrackets`).
    OemCloseBrackets = 151,
    /// `'` / `"` (`Key_Oem7` / `Key_OemQuotes`).
    OemQuotes = 152,
}

/// `Noesis::RenderFlags` bit values mirrored for convenience. See
/// `NsGui/IView.h` for the authoritative list.
#[repr(u32)]
#[allow(non_camel_case_types)]
pub enum RenderFlag {
    Wireframe = 1,
    ColorBatches = 2,
    Overdraw = 4,
    FlipY = 8,
    Ppaa = 16,
    Lcd = 32,
    ShowGlyphs = 64,
    ShowRamps = 128,
    DepthTesting = 256,
}

/// Borrowed handle to the view's renderer. Methods map 1:1 onto
/// `Noesis::IRenderer`; the renderer is owned by the view and must not
/// outlive it.
pub struct Renderer<'a> {
    ptr: NonNull<c_void>,
    _view: PhantomData<&'a mut View>,
}

// SAFETY: mirrors [`View`]. `Renderer` is a transient borrow that shares
// thread-safety properties with the `View` it was produced from.
unsafe impl Send for Renderer<'_> {}
unsafe impl Sync for Renderer<'_> {}

impl Renderer<'_> {
    /// Bind the Noesis renderer to `render_device`. Must be called once
    /// before any of the render methods. Pair with [`Self::shutdown`] before
    /// the device is dropped.
    pub fn init(&mut self, render_device: &RegisteredDevice) {
        // SAFETY: RegisteredDevice owns a live Noesis::RenderDevice* and
        // outlives this call (borrow checker enforces).
        unsafe { dm_noesis_renderer_init(self.ptr.as_ptr(), render_device.raw()) }
    }

    /// Release the renderer's device-bound resources.
    pub fn shutdown(&mut self) {
        unsafe { dm_noesis_renderer_shutdown(self.ptr.as_ptr()) }
    }

    /// Grab the most recent snapshot captured by [`View::update`]. Returns
    /// `false` when no new snapshot was available.
    pub fn update_render_tree(&mut self) -> bool {
        unsafe { dm_noesis_renderer_update_render_tree(self.ptr.as_ptr()) }
    }

    /// Populate offscreen textures the next [`Self::render`] may sample.
    /// Returns `false` when nothing was rendered (safe to skip GPU state
    /// restore in that case).
    pub fn render_offscreen(&mut self) -> bool {
        unsafe { dm_noesis_renderer_render_offscreen(self.ptr.as_ptr()) }
    }

    /// Render the UI into the currently-bound "onscreen" target (from the
    /// render device's perspective).
    pub fn render(&mut self, flip_y: bool, clear: bool) {
        unsafe { dm_noesis_renderer_render(self.ptr.as_ptr(), flip_y, clear) }
    }
}
