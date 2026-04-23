# dm_noesis

FFI bindings to the [Noesis GUI Native SDK](https://www.noesisengine.com/) plus a narrow C++ shim that exposes a `Noesis::RenderDevice` C++ subclass back to Rust. Renderer-agnostic — Bevy integration lives in the sibling crate [`dm_noesis_bevy`](https://github.com/dead-money/dm_noesis_bevy).

**Status:** Phase 0 complete (init / shutdown / version). Phase 1 (render-device subclass + Rust vtable) in planning — see [`docs/PHASE_1_PLAN.md`](./docs/PHASE_1_PLAN.md). The full phase plan lives in [`../dm_noesis_bevy/CLAUDE.md`](https://github.com/dead-money/dm_noesis_bevy/blob/main/CLAUDE.md).

## FFI surface roadmap

What this crate exposes, layered by phase. Each layer ships only when its sibling crate's milestone needs it.

- [x] **0 — Lifecycle.** `dm_noesis::{init, shutdown, set_license, version}`. C++ shim is `cpp/noesis_shim.{h,cpp}`. Verified by `tests/lifecycle.rs`.
- [ ] **1 — Render device.** `RenderDevice` trait + C++ `RustRenderDevice` subclass that trampolines every Noesis pure virtual into Rust. Plus `RustTexture` / `RustRenderTarget` subclasses for the resources Noesis allocates through us. *(Plan: [`docs/PHASE_1_PLAN.md`](./docs/PHASE_1_PLAN.md).)*
- [ ] **4 — View + XAML.** `View` lifecycle, XAML loader / `XamlProvider`, input pump (`MouseMove`, `MouseButtonDown`, `KeyDown`, etc.). The Bevy plugin drives `View::Update` and `Renderer::Render` from this surface.
- [ ] **5 — Resource provider.** `FileTextureProvider`, `FontProvider`, custom resource lookup via Bevy's `AssetServer`.
- [ ] **6 — Effects** — custom pixel-shader registration through `Batch.pixelShader`.

The render-graph optimization (Phase 9 in `dm_noesis_bevy`) needs no new FFI — it reuses the Phase 1 device interface against a Bevy-supplied wgpu render target.

## Setup

You need the Noesis Native SDK 3.2.12 (Indie tier or higher). Extract it and point `NOESIS_SDK_DIR` at the root (the directory containing `Include/` and `Bin/`):

```sh
unzip ~/Downloads/NoesisGUI-NativeSDK-linux-3.2.12-Indie.zip -d ~/deadmoney/sdk/noesis-3.2.12
export NOESIS_SDK_DIR=~/deadmoney/sdk/noesis-3.2.12
cargo test
```

The integration test in `tests/lifecycle.rs` calls `Noesis::Init` → `GetBuildVersion` → `Noesis::Shutdown` and asserts a non-empty version string.

Optional — suppress the trial watermark with your Indie credentials:

```sh
export NOESIS_LICENSE_NAME=...
export NOESIS_LICENSE_KEY=...
```

## Layout

- `cpp/noesis_shim.{h,cpp}` — narrow C ABI over Noesis. Hand-written.
- `src/ffi.rs` — Rust declarations mirroring the shim header.
- `src/lib.rs` — safe-ish wrappers.
- `build.rs` — resolves `NOESIS_SDK_DIR`, compiles the shim with `cc`, links `libNoesis`, bakes `Bin/<platform>/` into `rpath` on Linux.

## Why not bindgen?

Noesis's public C++ surface (`NsCore`, `NsGui`) leans heavily on templates, intrusive `Ptr<T>` smart pointers, and pure-virtual class hierarchies. Bindgen handles those poorly. We hand-write a narrow C ABI in `cpp/noesis_shim.{h,cpp}` and mirror it in `src/ffi.rs`. If the surface grows past ~30 functions we'll switch to bindgen-on-the-shim-header, but the underlying NsCore/NsGui types stay opaque.

## Licensing

Code in this repository is © 2026 Dead Money, all rights reserved (private repo).

The Noesis Native SDK is **not redistributed** here. You must obtain it from Noesis Technologies under their EULA and extract it yourself; `build.rs` reads it from `NOESIS_SDK_DIR` at compile time and links `libNoesis.{so,dll}` from `Bin/<platform>/`. Per-developer license, no checked-in binaries.
