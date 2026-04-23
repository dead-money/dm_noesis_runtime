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
use std::ffi::{c_void, CString};

use crate::ffi::{
    dm_noesis_base_component_release, dm_noesis_gui_load_xaml, dm_noesis_renderer_init,
    dm_noesis_renderer_render, dm_noesis_renderer_render_offscreen, dm_noesis_renderer_shutdown,
    dm_noesis_renderer_update_render_tree, dm_noesis_view_create, dm_noesis_view_destroy,
    dm_noesis_view_get_renderer, dm_noesis_view_set_flags, dm_noesis_view_set_projection_matrix,
    dm_noesis_view_set_size, dm_noesis_view_update,
};
use crate::render_device::Registered as RegisteredDevice;

/// A loaded XAML root. Holds a +1 refcount on the underlying
/// `Noesis::FrameworkElement`; [`View::create`] consumes it and forwards the
/// ownership to the View.
pub struct FrameworkElement {
    ptr: NonNull<c_void>,
}

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
}

impl Drop for View {
    fn drop(&mut self) {
        // SAFETY: produced by dm_noesis_view_create which returns +1 ref.
        unsafe { dm_noesis_view_destroy(self.ptr.as_ptr()) }
    }
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
