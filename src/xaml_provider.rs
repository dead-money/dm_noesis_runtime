//! Rust-side [`XamlProvider`] trait + the [`set_xaml_provider`] registration
//! entrypoint. Mirrors `crate::render_device::vtable::register` — a boxed
//! trait object is handed to the C++ `RustXamlProvider` subclass via a vtable
//! of trampolines; the returned [`Registered`] guard owns both the boxed impl
//! and the C++ provider handle.
//!
//! # Lifetime
//!
//! The `Registered` guard must outlive every Noesis-internal reference that
//! might call back into [`XamlProvider::load_xaml`]. In practice that means
//! keeping it alive until after `dm_noesis::shutdown()` returns — the latter
//! releases Noesis's internal `Ptr<XamlProvider>`, after which the C++
//! wrapper's refcount drops to 1 (ours). Dropping the guard then releases the
//! final ref, fires the C++ destructor, and frees the boxed Rust impl.

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface — explicit blocks add noise

use core::ptr::NonNull;
use std::ffi::{c_void, CStr};
use std::os::raw::c_char;

use crate::ffi::{
    dm_noesis_set_xaml_provider, dm_noesis_xaml_provider_create,
    dm_noesis_xaml_provider_destroy, XamlProviderVTable,
};

/// Rust-side XAML provider. The bytes returned from [`load_xaml`] are wrapped
/// in a Noesis `MemoryStream` *without copying* and must stay valid until the
/// XAML parse that triggered the lookup returns. Since Noesis parses
/// synchronously inside `GUI::LoadXaml`, storing the bytes in `&self` (e.g.
/// a `HashMap<String, Vec<u8>>`) and returning a borrow is sufficient.
///
/// [`load_xaml`]: Self::load_xaml
pub trait XamlProvider {
    /// Return the XAML bytes for `uri`, or `None` if the URI is unknown.
    fn load_xaml(&mut self, uri: &str) -> Option<&[u8]>;
}

// ────────────────────────────────────────────────────────────────────────────
// Trampoline
// ────────────────────────────────────────────────────────────────────────────

/// SAFETY: `userdata` must be a pointer produced by `register_xaml_provider`
/// and still alive (the [`Registered`] guard hasn't been dropped).
unsafe fn provider<'a>(userdata: *mut c_void) -> &'a mut Box<dyn XamlProvider> {
    &mut *userdata.cast::<Box<dyn XamlProvider>>()
}

unsafe extern "C" fn t_load_xaml(
    userdata: *mut c_void,
    uri: *const c_char,
    out_data: *mut *const u8,
    out_len: *mut u32,
) -> bool {
    let uri_str = if uri.is_null() {
        ""
    } else {
        // Noesis URIs are always ASCII / UTF-8; a non-UTF-8 URI is a bug on
        // their end that should surface loudly.
        CStr::from_ptr(uri)
            .to_str()
            .expect("noesis passed non-UTF-8 URI to XamlProvider")
    };
    let Some(bytes) = provider(userdata).load_xaml(uri_str) else {
        return false;
    };
    out_data.write(bytes.as_ptr());
    out_len.write(u32::try_from(bytes.len()).expect("XAML > 4 GiB"));
    true
}

static VTABLE: XamlProviderVTable = XamlProviderVTable {
    load_xaml: t_load_xaml,
};

// ────────────────────────────────────────────────────────────────────────────
// Registered — RAII wrapper holding the boxed impl and the C++ provider
// ────────────────────────────────────────────────────────────────────────────

/// Owns a Rust [`XamlProvider`] impl together with its C++ `RustXamlProvider`
/// instance. Dropping releases the +1 ref we hold on the C++ side and frees
/// the boxed impl. The caller is responsible for having called
/// [`crate::shutdown`] before this drop so Noesis's own `Ptr<XamlProvider>`
/// is already released; otherwise the final destructor fires later than
/// expected and the boxed impl outlives its C++ wrapper briefly (still
/// safe — no further callbacks are possible after `Shutdown`).
pub struct Registered {
    handle: NonNull<c_void>,
    userdata: NonNull<Box<dyn XamlProvider>>,
}

impl Registered {
    /// Raw `Noesis::XamlProvider*` — useful for passing to other Noesis APIs
    /// that take a provider. Borrowed for the lifetime of this `Registered`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.handle.as_ptr()
    }
}

impl Drop for Registered {
    fn drop(&mut self) {
        // SAFETY: handle + userdata produced together by register(); both
        // freed exactly once here.
        unsafe {
            dm_noesis_xaml_provider_destroy(self.handle.as_ptr());
            drop(Box::from_raw(self.userdata.as_ptr()));
        }
    }
}

/// Install `provider` as the global Noesis XAML provider. Holds both the
/// boxed trait object and the C++ wrapper; drop the returned [`Registered`]
/// guard to tear everything down (after [`crate::shutdown`]).
///
/// # Panics
///
/// Panics if the C++ factory returns null (only possible on internal logic
/// errors).
pub fn set_xaml_provider<P: XamlProvider + 'static>(provider: P) -> Registered {
    // Double-Box gives a stable thin pointer for the C ABI userdata.
    let outer: Box<Box<dyn XamlProvider>> = Box::new(Box::new(provider));
    let userdata = Box::into_raw(outer);
    // SAFETY: VTABLE is 'static; userdata is freshly leaked.
    let handle = unsafe { dm_noesis_xaml_provider_create(&raw const VTABLE, userdata.cast()) };
    let handle = NonNull::new(handle).expect("dm_noesis_xaml_provider_create returned null");
    // Install globally. Noesis retains its own +1; we keep ours until the
    // Registered is dropped.
    unsafe { dm_noesis_set_xaml_provider(handle.as_ptr()) };

    Registered {
        handle,
        userdata: NonNull::new(userdata).expect("Box::into_raw returned null"),
    }
}
