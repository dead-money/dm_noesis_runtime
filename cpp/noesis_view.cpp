// C++ wrappers for the XamlProvider / IView / IRenderer surface (Phase 4.C).
//
// Mirrors the RustRenderDevice pattern in noesis_render_device.cpp:
//   * `RustXamlProvider` subclasses `Noesis::XamlProvider` and trampolines
//     `LoadXaml` into a Rust vtable. The Rust side owns the bytes; this shim
//     wraps them in a `Noesis::MemoryStream` whose `const void*` buffer is
//     the Rust-owned storage.
//   * Thin extern "C" entrypoints over `GUI::LoadXaml`, `GUI::CreateView`,
//     and the `IView` / `IRenderer` methods Phase 4.D will drive.
//
// No Rust callback fires on XamlProvider teardown — the Rust side manages
// the boxed trait object's lifetime via its `Drop` impl (mirrors the
// `Registered` pattern for RenderDevice).

#include "noesis_shim.h"

#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsCore/DynamicCast.h>
#include <NsGui/FrameworkElement.h>
#include <NsGui/IntegrationAPI.h>
#include <NsGui/IRenderer.h>
#include <NsGui/IView.h>
#include <NsGui/MemoryStream.h>
#include <NsGui/Stream.h>
#include <NsGui/Uri.h>
#include <NsGui/XamlProvider.h>
#include <NsMath/Matrix.h>
#include <NsRender/RenderDevice.h>

#include <cstdint>
#include <cstring>

namespace {

// ── RustXamlProvider ───────────────────────────────────────────────────────

class RustXamlProvider final : public Noesis::XamlProvider {
public:
    RustXamlProvider(const dm_noesis_xaml_provider_vtable* vtable, void* userdata)
        : mVtable(*vtable), mUserdata(userdata)
    {}

    Noesis::Ptr<Noesis::Stream> LoadXaml(const Noesis::Uri& uri) override {
        const char* uriChars = uri.Str();
        const uint8_t* data = nullptr;
        uint32_t len = 0;
        bool ok = mVtable.load_xaml(mUserdata, uriChars ? uriChars : "", &data, &len);
        if (!ok || data == nullptr) {
            return nullptr;
        }
        // MemoryStream stores the buffer pointer without copying. The Rust
        // side guarantees the bytes stay valid until parsing completes (which
        // is synchronous with this call's return).
        return Noesis::MakePtr<Noesis::MemoryStream>(data, len);
    }

private:
    dm_noesis_xaml_provider_vtable mVtable;
    void* mUserdata;
};

}  // namespace

// ── XamlProvider C ABI ─────────────────────────────────────────────────────

extern "C" void* dm_noesis_xaml_provider_create(
    const dm_noesis_xaml_provider_vtable* vtable, void* userdata)
{
    if (!vtable) return nullptr;
    Noesis::Ptr<RustXamlProvider> p =
        Noesis::MakePtr<RustXamlProvider>(vtable, userdata);
    return p.GiveOwnership();
}

extern "C" void dm_noesis_xaml_provider_destroy(void* provider) {
    if (!provider) return;
    static_cast<Noesis::XamlProvider*>(provider)->Release();
}

extern "C" void dm_noesis_set_xaml_provider(void* provider) {
    Noesis::GUI::SetXamlProvider(static_cast<Noesis::XamlProvider*>(provider));
}

// ── XAML load + generic release ────────────────────────────────────────────

extern "C" void* dm_noesis_gui_load_xaml(const char* uri) {
    if (!uri) return nullptr;
    Noesis::Ptr<Noesis::BaseComponent> component =
        Noesis::GUI::LoadXaml(Noesis::Uri(uri));
    if (!component) return nullptr;
    // GUI::CreateView wants a FrameworkElement*. DynamicPtrCast fails
    // predictably if the loaded root isn't one (e.g. a ResourceDictionary).
    Noesis::Ptr<Noesis::FrameworkElement> element =
        Noesis::DynamicPtrCast<Noesis::FrameworkElement>(component);
    if (!element) return nullptr;
    return element.GiveOwnership();
}

extern "C" void dm_noesis_base_component_release(void* obj) {
    if (!obj) return;
    static_cast<Noesis::BaseComponent*>(obj)->Release();
}

// ── View lifecycle ─────────────────────────────────────────────────────────

extern "C" void* dm_noesis_view_create(void* framework_element) {
    if (!framework_element) return nullptr;
    Noesis::Ptr<Noesis::IView> view = Noesis::GUI::CreateView(
        static_cast<Noesis::FrameworkElement*>(framework_element));
    if (!view) return nullptr;
    return view.GiveOwnership();
}

extern "C" void dm_noesis_view_destroy(void* view) {
    if (!view) return;
    static_cast<Noesis::IView*>(view)->Release();
}

// ── View setters ───────────────────────────────────────────────────────────

extern "C" void dm_noesis_view_set_size(void* view, uint32_t width, uint32_t height) {
    static_cast<Noesis::IView*>(view)->SetSize(width, height);
}

extern "C" void dm_noesis_view_set_projection_matrix(void* view, const float* matrix) {
    // Matrix4(const float*) reads 16 floats; the native GetData() layout is
    // row-major (Vector4 mVal[4] holding rows), so we pass the Rust array
    // through untouched.
    Noesis::Matrix4 m(matrix);
    static_cast<Noesis::IView*>(view)->SetProjectionMatrix(m);
}

extern "C" bool dm_noesis_view_update(void* view, double time_seconds) {
    return static_cast<Noesis::IView*>(view)->Update(time_seconds);
}

extern "C" void dm_noesis_view_set_flags(void* view, uint32_t flags) {
    static_cast<Noesis::IView*>(view)->SetFlags(flags);
}

extern "C" void* dm_noesis_view_get_renderer(void* view) {
    return static_cast<Noesis::IView*>(view)->GetRenderer();
}

// ── Renderer ───────────────────────────────────────────────────────────────

extern "C" void dm_noesis_renderer_init(void* renderer, void* render_device) {
    static_cast<Noesis::IRenderer*>(renderer)->Init(
        static_cast<Noesis::RenderDevice*>(render_device));
}

extern "C" void dm_noesis_renderer_shutdown(void* renderer) {
    static_cast<Noesis::IRenderer*>(renderer)->Shutdown();
}

extern "C" bool dm_noesis_renderer_update_render_tree(void* renderer) {
    return static_cast<Noesis::IRenderer*>(renderer)->UpdateRenderTree();
}

extern "C" bool dm_noesis_renderer_render_offscreen(void* renderer) {
    return static_cast<Noesis::IRenderer*>(renderer)->RenderOffscreen();
}

extern "C" void dm_noesis_renderer_render(void* renderer, bool flip_y, bool clear) {
    static_cast<Noesis::IRenderer*>(renderer)->Render(flip_y, clear);
}
