# dm_noesis

FFI bindings to the [Noesis GUI Native SDK](https://www.noesisengine.com/) plus a narrow C++ shim that exposes a `Noesis::RenderDevice` C++ subclass (and the providers/view it drives) back to Rust. Renderer-agnostic — Bevy integration lives in the sibling crate [`dm_noesis_bevy`](https://github.com/dead-money/dm_noesis_bevy).

**Status:** Phases 0, 1, 4.C, 4.E (texture provider), 4.F.1 (font provider), and 5 (input) of the surface are shipped. That's lifecycle, the full `RenderDevice` C++ subclass + Rust vtable, `XamlProvider` + `FontProvider` + `TextureProvider` FFI shims, `FrameworkElement` / `View` / `Renderer` safe wrappers, and the full View input pump (mouse / key / char / touch / focus). Everything the Bevy plugin needs for Phase 5 is in place.

The full cross-crate phase plan lives in [`../dm_noesis_bevy/CLAUDE.md`](https://github.com/dead-money/dm_noesis_bevy/blob/main/CLAUDE.md); the Phase 1 render-device design is in [`docs/PHASE_1_PLAN.md`](./docs/PHASE_1_PLAN.md).

## FFI surface roadmap

What this crate exposes, layered by phase. Each layer ships when its sibling crate's milestone needs it.

- [x] **0 — Lifecycle.** `dm_noesis::{init, shutdown, set_license, version}`. C++ shim is `cpp/noesis_shim.{h,cpp}`. Verified by `tests/lifecycle.rs`.
- [x] **1 — Render device.** `RenderDevice` trait (`src/render_device/device.rs`) + C++ `RustRenderDevice` / `RustTexture` / `RustRenderTarget` subclasses that trampoline every Noesis pure virtual into Rust. `register()` returns a `Registered` guard that owns the boxed impl + the C++ device handle, with a `device_mut::<D>()` downcast accessor. Verified by `tests/render_device.rs --features test-utils`.
- [x] **4.C — View + XamlProvider.** `XamlProvider` trait + `set_xaml_provider` mirror the render-device registration pattern. `RustXamlProvider` C++ subclass trampolines `LoadXaml(Uri)` into the Rust vtable and wraps the returned bytes in a zero-copy `MemoryStream`. `FrameworkElement`, `View`, and `Renderer` safe wrappers over the opaque handles live in `src/view.rs`. `GUI::LoadApplicationResources(Uri)` for theme loading.
- [x] **4.E — Texture provider.** `TextureProvider` trait + `set_texture_provider` (`src/texture_provider.rs`) following the same pattern. `RustTextureProvider` C++ subclass handles `GetTextureInfo` (metadata-only) and `LoadTexture` (decoded RGBA8 bytes); the shim immediately hands the pixels to the `RenderDevice` Noesis passed in, so the resulting `Ptr<Texture>` is a real GPU texture that plugs into `Batch.pattern` through the `RustTexture` handle round-trip. Enables `<ImageBrush>` / `<Image>`.
- [x] **4.F.1 — Font provider.** `FontProvider` trait + `set_font_provider` (`src/font_provider.rs`). `RustFontProvider` subclasses `Noesis::CachedFontProvider`, so matching / weight-stretch-style selection stays inside Noesis — Rust only provides `ScanFolder(folder)` and `OpenFont(folder, filename)`. `set_font_fallbacks(&[...])` + `set_font_default_properties(...)` for the unstyled-element fallback chain.
- [x] **5 — View input pump.** `mouse_move`, `mouse_button_{down,up}`, `mouse_double_click`, `mouse_wheel`, `scroll` / `hscroll`, `touch_{down,move,up}`, `key_{down,up}`, `char_input`, `activate` / `deactivate`. `MouseButton` + `Key` enum subsets have C++ `static_assert` ordinal checks against `NsGui/InputEnums.h`, so a Noesis header shift in a future SDK would fail compile rather than silently drift.
- [ ] **6 — Effects** — custom pixel-shader registration through `Batch.pixelShader`; registering Noesis's built-in `DOWNSAMPLE` / `BLUR` / `SHADOW` / `OPACITY` / `UPSAMPLE` pipelines.

The render-graph optimization (Phase 9 in `dm_noesis_bevy`) needs no new FFI — it reuses the Phase 1 device interface against a Bevy-supplied wgpu render target.

## Setup

You need the Noesis Native SDK 3.2.12 (Indie tier or higher). Extract it and point `NOESIS_SDK_DIR` at the root (the directory containing `Include/` and `Bin/`):

```sh
unzip ~/Downloads/NoesisGUI-NativeSDK-linux-3.2.12-Indie.zip -d ~/deadmoney/sdk/noesis-3.2.12
export NOESIS_SDK_DIR=~/deadmoney/sdk/noesis-3.2.12
cargo test
```

The `tests/lifecycle.rs` integration test calls `Noesis::Init` → `GetBuildVersion` → `Noesis::Shutdown` and asserts a non-empty version string. `--features test-utils` unlocks `tests/render_device.rs` (full `RenderDevice` trampoline regression).

Optional — suppress the trial watermark with your Indie credentials:

```sh
export NOESIS_LICENSE_NAME=...
export NOESIS_LICENSE_KEY=...
```

## Layout

- `cpp/noesis_shim.{h,cpp}` — narrow C ABI over Noesis. Hand-written.
- `cpp/noesis_render_device.cpp` — `RustRenderDevice` / `RustTexture` / `RustRenderTarget` subclasses trampolining Noesis virtuals into the Rust vtable (Phase 1).
- `cpp/noesis_view.cpp` — `RustXamlProvider` subclass + `IView` / `IRenderer` forwarders + the View input trampolines (Phase 4.C + Phase 5).
- `cpp/noesis_font_provider.cpp` — `RustFontProvider` subclass of `Noesis::CachedFontProvider` + `SetFontFallbacks` / `SetFontDefaultProperties` (Phase 4.F.1).
- `cpp/noesis_texture_provider.cpp` — `RustTextureProvider` subclass; `LoadTexture` turns around and calls `device->CreateTexture(...)` with the Rust-supplied RGBA8 bytes (Phase 4.E).
- `src/ffi.rs` — Rust declarations mirroring the shim header.
- `src/lib.rs` — top-level safe wrappers (lifecycle).
- `src/render_device/` — `RenderDevice` trait + `register()` / `Registered` guard + type mirrors of Noesis's `Batch` / `DeviceCaps` / `Tile` / `UniformData` (Phase 1).
- `src/xaml_provider.rs`, `src/font_provider.rs`, `src/texture_provider.rs` — provider traits + `Registered` guards (Phase 4.C / 4.F.1 / 4.E).
- `src/view.rs` — `FrameworkElement` + `View` + `Renderer` wrappers; input pump methods (Phase 4.C + 5).
- `src/gui.rs` — global `GUI::*` bindings (`LoadApplicationResources`).
- `build.rs` — resolves `NOESIS_SDK_DIR`, compiles the shim TUs with `cc`, links `libNoesis`, bakes `Bin/<platform>/` into `rpath` on Linux.

## Why not bindgen?

Noesis's public C++ surface (`NsCore`, `NsGui`) leans heavily on templates, intrusive `Ptr<T>` smart pointers, and pure-virtual class hierarchies. Bindgen handles those poorly. We hand-write a narrow C ABI in `cpp/noesis_shim.{h,cpp}` and mirror it in `src/ffi.rs`. The underlying `NsCore` / `NsGui` types stay opaque; Rust only touches C-layout POD mirrors (`Batch`, `Tile`, `DeviceCaps`, `TextureInfo`, …) whose field layouts are asserted at compile time via `const _: () = assert!(size_of::<T>() == …)`.

## Licensing

Code in this repository is © 2026 Dead Money, all rights reserved (private repo).

The Noesis Native SDK is **not redistributed** here. You must obtain it from Noesis Technologies under their EULA and extract it yourself; `build.rs` reads it from `NOESIS_SDK_DIR` at compile time and links `libNoesis.{so,dll}` from `Bin/<platform>/`. Per-developer license, no checked-in binaries.
