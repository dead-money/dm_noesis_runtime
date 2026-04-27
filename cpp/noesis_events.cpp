// FrameworkElement traversal + event subscription FFI (Phase 5.B).
//
// Two pieces:
//   * `dm_noesis_framework_element_find_name` — wraps Noesis's `FindName`.
//     Returns an owning (+1 ref) `FrameworkElement*` so the Rust side
//     manages lifetime via the same release path as `GUI::LoadXaml`.
//   * `dm_noesis_subscribe_click` — installs a Rust callback on the
//     `BaseButton::Click` routed event. `dm_noesis_unsubscribe_click`
//     removes it. The token returned to Rust is a heap-allocated
//     `RustClickHandler` whose lifetime is tied 1:1 to the subscription;
//     it owns a +1 ref on the button so the subscription stays valid
//     even if the only other reference is the parent FrameworkElement
//     that the Rust caller dropped.
//
// Why a separate translation unit (rather than appending to noesis_view.cpp):
// the headers we pull in here (`BaseButton.h`, `RoutedEvent.h`, `Delegate.h`)
// are heavy enough that we'd rather not pay for them in the input-pump file
// every other FFI surface depends on.

#include "noesis_shim.h"

#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsCore/Delegate.h>
#include <NsCore/DynamicCast.h>
#include <NsGui/BaseButton.h>
#include <NsGui/FrameworkElement.h>
#include <NsGui/RoutedEvent.h>
#include <NsGui/UIElement.h>

namespace {

// Adapter between Noesis's `Delegate<void(BaseComponent*, const
// RoutedEventArgs&)>` and the C ABI callback. Stores the function pointer +
// userdata that the Rust trampoline registered, plus a +1 ref on the button
// so the subscription remains valid even if Rust drops every other handle to
// the element. A handler owns its subscription; pair construction with
// `Click() +=` and destruction with `Click() -=` so the reference symmetry
// between this object and the routed-event-handler list is exact.
class RustClickHandler {
public:
    RustClickHandler(dm_noesis_click_fn cb, void* userdata, Noesis::BaseButton* button)
        : mCb(cb), mUserdata(userdata), mButton(button)
    {
        if (mButton) {
            mButton->AddReference();
        }
    }

    ~RustClickHandler() {
        if (mButton) {
            mButton->Release();
        }
    }

    RustClickHandler(const RustClickHandler&) = delete;
    RustClickHandler& operator=(const RustClickHandler&) = delete;

    void OnClick(Noesis::BaseComponent* /*sender*/, const Noesis::RoutedEventArgs& /*args*/) {
        if (mCb) {
            mCb(mUserdata);
        }
    }

    Noesis::BaseButton* button() const { return mButton; }

private:
    dm_noesis_click_fn mCb;
    void* mUserdata;
    Noesis::BaseButton* mButton;  // raw + manual AddRef/Release — see ctor/dtor.
};

}  // namespace

extern "C" void* dm_noesis_framework_element_find_name(void* element, const char* name) {
    if (!element || !name) return nullptr;
    auto* fe = static_cast<Noesis::FrameworkElement*>(element);
    Noesis::BaseComponent* found = fe->FindName(name);
    if (!found) return nullptr;
    // FindName returns a non-owning raw pointer (the parent FE owns the named
    // child). We need an owning +1-ref `FrameworkElement*` so the Rust wrapper
    // can release it via `dm_noesis_base_component_release` like every other
    // FFI-provided component. Cast first; AddReference second.
    auto* result = Noesis::DynamicCast<Noesis::FrameworkElement*>(found);
    if (!result) return nullptr;
    result->AddReference();
    return result;
}

extern "C" const char* dm_noesis_framework_element_get_name(void* element) {
    if (!element) return nullptr;
    return static_cast<Noesis::FrameworkElement*>(element)->GetName();
}

extern "C" void dm_noesis_framework_element_set_visibility(void* element, bool visible) {
    if (!element) return;
    auto* fe = static_cast<Noesis::FrameworkElement*>(element);
    fe->SetVisibility(visible ? Noesis::Visibility_Visible : Noesis::Visibility_Collapsed);
}

extern "C" void* dm_noesis_subscribe_click(
    void* element, dm_noesis_click_fn cb, void* userdata)
{
    if (!element || !cb) return nullptr;
    auto* fe = static_cast<Noesis::FrameworkElement*>(element);
    auto* button = Noesis::DynamicCast<Noesis::BaseButton*>(fe);
    if (!button) return nullptr;

    auto* handler = new RustClickHandler(cb, userdata, button);
    button->Click() += Noesis::MakeDelegate(handler, &RustClickHandler::OnClick);
    return handler;
}

extern "C" void dm_noesis_unsubscribe_click(void* token) {
    if (!token) return;
    auto* handler = static_cast<RustClickHandler*>(token);
    if (auto* button = handler->button()) {
        button->Click() -= Noesis::MakeDelegate(handler, &RustClickHandler::OnClick);
    }
    delete handler;
}
