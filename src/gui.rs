//! Thin wrappers around the top-level `Noesis::GUI::*` helpers that don't
//! fit into the provider / view / render-device modules.

use std::ffi::CString;

use crate::ffi::dm_noesis_gui_load_application_resources;

/// Load a [`ResourceDictionary`] XAML via the installed XAML provider and
/// install it as the process-global application resources — every
/// [`crate::view::View`] created afterwards inherits these styles and
/// brushes. Replaces any previously-installed dictionary.
///
/// Returns `true` when the URI resolved to a valid
/// `ResourceDictionary`; `false` when the provider didn't serve bytes or
/// when the XAML parsed to a different root element.
///
/// [`ResourceDictionary`]: https://docs.noesisengine.com/gui/ResourceDictionary.html
///
/// # Panics
///
/// Panics if `uri` contains an interior NUL byte.
pub fn load_application_resources(uri: &str) -> bool {
    let c = CString::new(uri).expect("uri contained NUL");
    // SAFETY: c.as_ptr() lives for the duration of the call; the shim
    // only reads it.
    unsafe { dm_noesis_gui_load_application_resources(c.as_ptr()) }
}
