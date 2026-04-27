//! Subscribe Rust callbacks to Noesis routed events (Phase 5.B).
//!
//! Currently exposes [`subscribe_click`] for `BaseButton::Click`. The shape
//! generalizes — every routed event is a `Delegate<void(BaseComponent*, const
//! RoutedEventArgs&)>` on the C++ side, and the FFI pattern (heap-allocated
//! handler that owns its registration via RAII) repeats. Add sibling
//! functions when other events earn the surface.
//!
//! # Threading
//!
//! Click callbacks fire from inside Noesis's input pump (typically
//! `IView::MouseButtonUp` or `IView::Update`), on whatever thread is driving
//! the view. The callback signature has no `Send` bound at the FFI level —
//! the safe wrapper enforces it on the Rust side via the trait. Keep work
//! in the callback small: push to a queue / channel and process from a
//! regular Bevy system step if you need anything heavier than a flag flip.
//!
//! # Lifetime
//!
//! [`ClickSubscription`] is RAII: while alive, the registered handler stays
//! on the button's `Click` event. Drop the subscription to unsubscribe.
//! The subscription holds a `+1` ref on the button so the handler list
//! stays valid even if the only other reference to the element was the
//! [`crate::view::FrameworkElement`] you used to subscribe.

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface — explicit blocks add noise

use core::ptr::NonNull;
use std::ffi::c_void;

use crate::ffi::{dm_noesis_subscribe_click, dm_noesis_unsubscribe_click};
use crate::view::FrameworkElement;

/// Rust-side click handler. Implementors receive a single `()` notification
/// per fired click; if you need the sender / event args, extend the FFI
/// before adding a richer trait method here.
///
/// The `Send + 'static` bounds let the handler live inside a Bevy
/// `Resource` or be moved onto the render thread.
pub trait ClickHandler: Send + 'static {
    fn on_click(&mut self);
}

impl<F: FnMut() + Send + 'static> ClickHandler for F {
    fn on_click(&mut self) {
        self();
    }
}

/// SAFETY: `userdata` must be a pointer produced by [`subscribe_click`] and
/// still alive (the [`ClickSubscription`] hasn't been dropped).
unsafe extern "C" fn click_trampoline(userdata: *mut c_void) {
    let handler = &mut *userdata.cast::<Box<dyn ClickHandler>>();
    handler.on_click();
}

/// RAII subscription token. Drop to unsubscribe and free the boxed handler.
///
/// Holds a `+1` ref on the underlying button (managed C++-side); dropping
/// this releases that ref and removes the handler from the routed-event
/// list. Drop before [`crate::shutdown`] like every other owning handle in
/// this crate.
pub struct ClickSubscription {
    token: NonNull<c_void>,
    userdata: NonNull<Box<dyn ClickHandler>>,
}

// SAFETY: matches the Registered guards on the providers — every Box<dyn
// ClickHandler> is `Send`, and the C++ subscription is bound to a single
// button whose access is serialized by Noesis. Sync is safe for the same
// reason: there are no `&self` methods that touch Noesis state.
unsafe impl Send for ClickSubscription {}
unsafe impl Sync for ClickSubscription {}

impl Drop for ClickSubscription {
    fn drop(&mut self) {
        // SAFETY: token + userdata produced together by subscribe_click;
        // freed exactly once here.
        unsafe {
            dm_noesis_unsubscribe_click(self.token.as_ptr());
            drop(Box::from_raw(self.userdata.as_ptr()));
        }
    }
}

/// Subscribe `handler` to `BaseButton::Click` on `element`. Returns `None`
/// if the element is not castable to `BaseButton` (e.g. it's a plain
/// `ContentControl` or a `UserControl` whose root isn't a button).
///
/// The returned [`ClickSubscription`] keeps the handler installed for as
/// long as it lives; drop it (or replace it) to unsubscribe.
///
/// # Panics
///
/// Panics only on internal logic errors — specifically if `Box::into_raw`
/// returns null (it cannot, but the wrapper is `NonNull` to keep the
/// invariant explicit at the type level).
pub fn subscribe_click<H: ClickHandler>(
    element: &FrameworkElement,
    handler: H,
) -> Option<ClickSubscription> {
    // Double-Box gives a stable thin pointer for the C ABI userdata, same
    // pattern as the providers.
    let outer: Box<Box<dyn ClickHandler>> = Box::new(Box::new(handler));
    let userdata = Box::into_raw(outer);

    // SAFETY: trampoline is `extern "C"`; userdata is freshly leaked; the
    // element pointer is borrowed for the call duration only — Noesis copies
    // whatever it needs into the routed-event handler list.
    let token =
        unsafe { dm_noesis_subscribe_click(element.raw(), click_trampoline, userdata.cast()) };

    if let Some(token) = NonNull::new(token) {
        Some(ClickSubscription {
            token,
            userdata: NonNull::new(userdata).expect("Box::into_raw returned null"),
        })
    } else {
        // Subscription failed (e.g. element wasn't a button). Free the
        // userdata we leaked above so we don't leak the handler.
        // SAFETY: userdata came from Box::into_raw moments ago; nothing else
        // ever saw the pointer.
        unsafe { drop(Box::from_raw(userdata)) };
        None
    }
}
