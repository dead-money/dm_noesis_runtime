//! Rust-side machinery for implementing `Noesis::RenderDevice`.
//!
//! Phase plan in `../../docs/PHASE_1_PLAN.md`. Layers, in dependency order:
//!
//! - [`types`] — `#[repr(C)]` mirrors of the public Noesis types in
//!   `Include/NsRender/RenderDevice.h`. ABI surface; layout-checked at
//!   compile time.
//! - [`device`] — the [`RenderDevice`] trait that Rust-side device impls
//!   satisfy, plus its handle / desc / binding plain-data types.
//! - `vtable` (Phase 1.5) — extern "C" trampolines that dispatch from the
//!   C++ `RustRenderDevice` subclass into a boxed [`RenderDevice`] impl.

pub mod device;
pub mod types;

pub use device::{
    RenderDevice, RenderTargetBinding, RenderTargetDesc, RenderTargetHandle, TextureBinding,
    TextureDesc, TextureHandle, TextureRect,
};
