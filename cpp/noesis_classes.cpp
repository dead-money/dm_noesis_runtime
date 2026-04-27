// Custom XAML class registration FFI (Phase 5.C).
//
// Architecture mirrors what Noesis's C# / Unity binding does for managed
// code — we cannot conjure C++ types from C-FFI, but we *can* synthesize
// a `TypeClassBuilder` per consumer-named class and register a Factory
// creator that returns a fresh instance of a per-base trampoline subclass
// whose `GetClassType()` reports the synthetic class.
//
// Components:
//   * RustContentControl — trampoline subclass of Noesis::ContentControl.
//     Holds a back-pointer to its owning ClassData; overrides GetClassType()
//     and OnPropertyChanged() to (a) report the synthetic TypeClass for XAML
//     style/binding lookups and (b) forward DP changes to the Rust callback.
//   * ClassData — per-registered-class state: synthetic TypeClassBuilder,
//     UIElementData metadata, dense list of registered DPs (with their FFI
//     value-type tags so we know how to marshal across the boundary), and
//     the Rust callback fn pointer + userdata.
//   * g_class_registry — Symbol → ClassData* lookup so the Factory creator
//     (which only receives a Symbol) can recover the right ClassData.
//
// Adding a new base type (Control, UserControl, FrameworkElement, Panel)
// is a uniform addition: derive another trampoline class with the same
// override pattern, branch on the `base` enum in dm_noesis_class_register.

#include "noesis_shim.h"

#include <NsCore/Noesis.h>
#include <NsCore/Boxing.h>
#include <NsCore/Factory.h>
#include <NsCore/HashMap.h>
#include <NsCore/Ptr.h>
#include <NsCore/Reflection.h>
#include <NsCore/ReflectionImplement.h>
#include <NsCore/String.h>
#include <NsCore/Symbol.h>
#include <NsCore/TypeClassBuilder.h>
#include <NsCore/TypeOf.h>
#include <NsDrawing/Color.h>
#include <NsDrawing/Rect.h>
#include <NsDrawing/Thickness.h>
#include <NsGui/ContentControl.h>
#include <NsGui/DependencyObject.h>
#include <NsGui/DependencyProperty.h>
#include <NsGui/ImageSource.h>
#include <NsGui/PropertyMetadata.h>
#include <NsGui/UIElementData.h>

#include <mutex>
#include <unordered_map>
#include <vector>

namespace {

struct PropEntry {
    const Noesis::DependencyProperty* dp;
    dm_noesis_prop_type type;
};

struct ClassData {
    Noesis::String                      name;
    Noesis::Symbol                      sym;
    // TypeClassBuilder isn't a BaseComponent — it inherits TypeMeta and is
    // owned by Noesis's Reflection registry (Reflection::RegisterType +
    // Reflection::Unregister handle the lifecycle).
    Noesis::TypeClassBuilder*           typeClass;
    Noesis::Ptr<Noesis::UIElementData>  uiData;
    std::vector<PropEntry>              properties;
    dm_noesis_prop_changed_fn           cb;
    void*                               userdata;
};

// Symbol-keyed registry. Symbols are 32-bit interned IDs so we can use a
// plain unordered_map keyed by their underlying integer.
std::mutex                                       g_registry_mutex;
std::unordered_map<uint32_t, ClassData*>         g_class_registry;

ClassData* registry_find(Noesis::Symbol sym) {
    std::lock_guard<std::mutex> lock(g_registry_mutex);
    auto it = g_class_registry.find((uint32_t)sym);
    return it == g_class_registry.end() ? nullptr : it->second;
}

bool registry_insert(Noesis::Symbol sym, ClassData* cd) {
    std::lock_guard<std::mutex> lock(g_registry_mutex);
    return g_class_registry.emplace((uint32_t)sym, cd).second;
}

void registry_erase(Noesis::Symbol sym) {
    std::lock_guard<std::mutex> lock(g_registry_mutex);
    g_class_registry.erase((uint32_t)sym);
}

// ── Trampoline subclass: ContentControl ────────────────────────────────────
//
// Hand-rolled reflection: we cannot use NS_DECLARE_REFLECTION /
// NS_IMPLEMENT_REFLECTION because they generate a `GetClassType()` body that
// always returns the static class type. We need a custom override so that
// instances created via the synthetic Factory creator report their per-name
// TypeClass instead — that's what makes XAML `Style TargetType="aor:Foo"`
// matching work. The hand-rolled version reuses Noesis's own
// `TypeClassCreator::Create` / `Fill` template machinery, so `TypeOf<>`,
// `RegisterType`, and `IsAssignableFrom` all behave normally.

class RustContentControl: public Noesis::ContentControl {
public:
    RustContentControl() = default;

    void BindClassData(ClassData* cd) { mClassData = cd; }
    ClassData* GetClassData() const { return mClassData; }

    // Custom reflection — see comment above.
    static const Noesis::TypeClass* StaticGetClassType(Noesis::TypeTag<RustContentControl>*);
    const Noesis::TypeClass* GetClassType() const override;

protected:
    bool OnPropertyChanged(const Noesis::DependencyPropertyChangedEventArgs& args) override {
        bool processed = ContentControl::OnPropertyChanged(args);
        ForwardChange(args);
        return processed;
    }

private:
    void ForwardChange(const Noesis::DependencyPropertyChangedEventArgs& args);

    ClassData* mClassData = nullptr;

    // Required by TypeClassCreator::Fill<SelfClass, ParentClass>.
    typedef RustContentControl SelfClass;
    typedef Noesis::ContentControl ParentClass;
    friend class Noesis::TypeClassCreator;
    static void StaticFillClassType(Noesis::TypeClassCreator& /*helper*/) {
        // No statically-declared DPs — every consumer adds their own via
        // dm_noesis_class_register_property against a synthetic TypeClass.
    }
};

const Noesis::TypeClass*
RustContentControl::StaticGetClassType(Noesis::TypeTag<RustContentControl>*) {
    static const Noesis::TypeClass* type;
    if (NS_UNLIKELY(type == 0)) {
        type = static_cast<const Noesis::TypeClass*>(Noesis::Reflection::RegisterType(
            "DmNoesis.RustContentControl",
            Noesis::TypeClassCreator::Create<RustContentControl>,
            Noesis::TypeClassCreator::Fill<RustContentControl, Noesis::ContentControl>));
    }
    return type;
}

const Noesis::TypeClass* RustContentControl::GetClassType() const {
    if (mClassData && mClassData->typeClass) {
        return static_cast<const Noesis::TypeClass*>(mClassData->typeClass);
    }
    return StaticGetClassType((Noesis::TypeTag<RustContentControl>*)nullptr);
}

// ── Marshaling helpers ─────────────────────────────────────────────────────

void invoke_cb(ClassData* cd, void* instance, uint32_t idx,
               dm_noesis_prop_type ty, const void* raw) {
    if (!cd->cb) return;

    switch (ty) {
        case DM_NOESIS_PROP_INT32:
        case DM_NOESIS_PROP_FLOAT:
        case DM_NOESIS_PROP_DOUBLE:
        case DM_NOESIS_PROP_BOOL:
        case DM_NOESIS_PROP_THICKNESS:
        case DM_NOESIS_PROP_COLOR:
        case DM_NOESIS_PROP_RECT:
            // Pass through directly — `raw` already points to the typed value.
            cd->cb(cd->userdata, instance, idx, raw);
            return;

        case DM_NOESIS_PROP_STRING: {
            // Noesis stores String values as Noesis::String (FixedString<24>);
            // expose as `const char*` to Rust via a borrowed pointer.
            const auto* s = static_cast<const Noesis::String*>(raw);
            const char* c = s ? s->Str() : nullptr;
            cd->cb(cd->userdata, instance, idx, &c);
            return;
        }

        case DM_NOESIS_PROP_IMAGE_SOURCE:
        case DM_NOESIS_PROP_BASE_COMPONENT: {
            // Noesis stores object values as Ptr<T> — the raw pointer is to a
            // Ptr<BaseComponent>. Unbox to a borrowed BaseComponent*.
            const auto* p = static_cast<const Noesis::Ptr<Noesis::BaseComponent>*>(raw);
            Noesis::BaseComponent* b = p ? p->GetPtr() : nullptr;
            cd->cb(cd->userdata, instance, idx, &b);
            return;
        }
    }
}

void RustContentControl::ForwardChange(
    const Noesis::DependencyPropertyChangedEventArgs& args) {
    if (!mClassData) return;
    for (uint32_t i = 0; i < mClassData->properties.size(); ++i) {
        const auto& pe = mClassData->properties[i];
        if (pe.dp == args.prop) {
            invoke_cb(mClassData, this, i, pe.type, args.newValue);
            return;
        }
    }
}

// ── Factory creator ────────────────────────────────────────────────────────

Noesis::BaseComponent* class_creator(Noesis::Symbol name) {
    ClassData* cd = registry_find(name);
    if (!cd) return nullptr;

    // Only ContentControl base for v1. Future bases dispatch on the base
    // tag we'd need to stash in ClassData.
    auto* instance = new RustContentControl();
    instance->BindClassData(cd);
    return instance;
}

// ── DP creation: dispatch by FFI type tag ──────────────────────────────────

Noesis::Ptr<Noesis::DependencyProperty> create_dp(
    const char* name,
    const Noesis::TypeClass* owner,
    dm_noesis_prop_type type,
    const void* default_ptr) {
    using namespace Noesis;
    switch (type) {
        case DM_NOESIS_PROP_INT32: {
            int32_t def = default_ptr ? *static_cast<const int32_t*>(default_ptr) : 0;
            return DependencyProperty::Create<int32_t>(
                name, owner, PropertyMetadata::Create(def), nullptr);
        }
        case DM_NOESIS_PROP_FLOAT: {
            float def = default_ptr ? *static_cast<const float*>(default_ptr) : 0.0f;
            return DependencyProperty::Create<float>(
                name, owner, PropertyMetadata::Create(def), nullptr);
        }
        case DM_NOESIS_PROP_DOUBLE: {
            double def = default_ptr ? *static_cast<const double*>(default_ptr) : 0.0;
            return DependencyProperty::Create<double>(
                name, owner, PropertyMetadata::Create(def), nullptr);
        }
        case DM_NOESIS_PROP_BOOL: {
            bool def = default_ptr ? *static_cast<const bool*>(default_ptr) : false;
            return DependencyProperty::Create<bool>(
                name, owner, PropertyMetadata::Create(def), nullptr);
        }
        case DM_NOESIS_PROP_STRING: {
            const char* def = default_ptr ? *static_cast<const char* const*>(default_ptr)
                                          : nullptr;
            String s = def ? String(def) : String();
            return DependencyProperty::Create<String>(
                name, owner, PropertyMetadata::Create(s), nullptr);
        }
        case DM_NOESIS_PROP_THICKNESS: {
            Thickness def;
            if (default_ptr) {
                const auto* f = static_cast<const float*>(default_ptr);
                def = Thickness(f[0], f[1], f[2], f[3]);
            }
            return DependencyProperty::Create<Thickness>(
                name, owner, PropertyMetadata::Create(def), nullptr);
        }
        case DM_NOESIS_PROP_COLOR: {
            Color def;
            if (default_ptr) {
                const auto* f = static_cast<const float*>(default_ptr);
                def = Color(f[0], f[1], f[2], f[3]);
            }
            return DependencyProperty::Create<Color>(
                name, owner, PropertyMetadata::Create(def), nullptr);
        }
        case DM_NOESIS_PROP_RECT: {
            Rect def;
            if (default_ptr) {
                const auto* f = static_cast<const float*>(default_ptr);
                def = Rect(f[0], f[1], f[0] + f[2], f[1] + f[3]);
            }
            return DependencyProperty::Create<Rect>(
                name, owner, PropertyMetadata::Create(def), nullptr);
        }
        case DM_NOESIS_PROP_IMAGE_SOURCE: {
            // Default is null — overriding non-null requires a Ptr we'd hold;
            // skip for v1 since AoR's slicers default Source to null anyway.
            return DependencyProperty::Create<Ptr<BaseComponent>>(
                name, TypeOf<ImageSource>(), owner, PropertyMetadata::Create(), nullptr);
        }
        case DM_NOESIS_PROP_BASE_COMPONENT: {
            return DependencyProperty::Create<Ptr<BaseComponent>>(
                name, TypeOf<BaseComponent>(), owner, PropertyMetadata::Create(), nullptr);
        }
    }
    return nullptr;
}

}  // namespace

// ── C ABI surface ──────────────────────────────────────────────────────────

extern "C" void* dm_noesis_class_register(
    const char* name,
    dm_noesis_class_base base,
    dm_noesis_prop_changed_fn cb,
    void* userdata) {
    if (!name) return nullptr;
    if (base != DM_NOESIS_BASE_CONTENT_CONTROL) return nullptr;

    Noesis::Symbol sym = Noesis::Symbol(name);

    // Reject duplicate names so callers see the failure rather than silently
    // shadowing an earlier registration with a stale ClassData* dangling
    // inside the Factory creator path.
    if (Noesis::Reflection::IsTypeRegistered(sym)) {
        return nullptr;
    }

    auto* cd = new ClassData();
    cd->name = name;
    cd->sym = sym;
    cd->cb = cb;
    cd->userdata = userdata;

    // Build the synthetic TypeClass. Reflection::RegisterType assumes
    // ownership and deletes it on Unregister / Shutdown.
    cd->typeClass = new Noesis::TypeClassBuilder(sym, /*isInterface*/ false);
    cd->typeClass->AddBase(Noesis::TypeOf<RustContentControl>());

    cd->uiData = Noesis::MakePtr<Noesis::UIElementData>(cd->typeClass);
    cd->typeClass->AddMeta(cd->uiData.GetPtr());

    Noesis::Reflection::RegisterType(cd->typeClass);
    Noesis::Factory::RegisterComponent(sym, Noesis::Symbol(""), class_creator);

    if (!registry_insert(sym, cd)) {
        // Symbol collision after the IsTypeRegistered check — extremely
        // unlikely, but unwind to keep the registry consistent.
        Noesis::Factory::UnregisterComponent(sym);
        Noesis::Reflection::Unregister(cd->typeClass);
        delete cd;
        return nullptr;
    }

    return cd;
}

extern "C" uint32_t dm_noesis_class_register_property(
    void* class_token,
    const char* prop_name,
    dm_noesis_prop_type prop_type,
    const void* default_ptr) {
    if (!class_token || !prop_name) return UINT32_MAX;
    auto* cd = static_cast<ClassData*>(class_token);

    auto dp = create_dp(prop_name, cd->typeClass, prop_type, default_ptr);
    if (!dp) return UINT32_MAX;

    const Noesis::DependencyProperty* installed = cd->uiData->InsertProperty(dp.GetPtr());
    if (!installed) return UINT32_MAX;

    cd->properties.push_back({installed, prop_type});
    return static_cast<uint32_t>(cd->properties.size() - 1);
}

extern "C" void dm_noesis_class_unregister(void* class_token) {
    if (!class_token) return;
    auto* cd = static_cast<ClassData*>(class_token);

    Noesis::Factory::UnregisterComponent(cd->sym);
    registry_erase(cd->sym);

    // Drop UIElementData first (it holds a Ptr<DependencyProperty> per DP),
    // then Reflection::Unregister deletes the synthetic TypeClass.
    cd->uiData.Reset();
    Noesis::Reflection::Unregister(cd->typeClass);

    delete cd;
}

namespace {

// Helper: locate the prop entry on an instance.
const PropEntry* instance_prop(void* instance, uint32_t prop_index, ClassData** out_cd) {
    if (!instance) return nullptr;
    auto* tramp = static_cast<RustContentControl*>(instance);
    ClassData* cd = tramp->GetClassData();
    if (!cd || prop_index >= cd->properties.size()) return nullptr;
    if (out_cd) *out_cd = cd;
    return &cd->properties[prop_index];
}

}  // namespace

extern "C" void dm_noesis_instance_set_property(
    void* instance,
    uint32_t prop_index,
    const void* value_ptr) {
    const PropEntry* pe = instance_prop(instance, prop_index, nullptr);
    if (!pe) return;
    auto* obj = static_cast<Noesis::DependencyObject*>(static_cast<RustContentControl*>(instance));

    using namespace Noesis;
    switch (pe->type) {
        case DM_NOESIS_PROP_INT32:
            obj->SetValue<int32_t>(pe->dp,
                value_ptr ? *static_cast<const int32_t*>(value_ptr) : 0);
            return;
        case DM_NOESIS_PROP_FLOAT:
            obj->SetValue<float>(pe->dp,
                value_ptr ? *static_cast<const float*>(value_ptr) : 0.0f);
            return;
        case DM_NOESIS_PROP_DOUBLE:
            obj->SetValue<double>(pe->dp,
                value_ptr ? *static_cast<const double*>(value_ptr) : 0.0);
            return;
        case DM_NOESIS_PROP_BOOL:
            obj->SetValue<bool>(pe->dp,
                value_ptr ? *static_cast<const bool*>(value_ptr) : false);
            return;
        case DM_NOESIS_PROP_STRING: {
            // SetValueType<String>::Type is `const char*` — pass the C string
            // directly. Noesis copies into its own String storage.
            const char* s = value_ptr ? *static_cast<const char* const*>(value_ptr)
                                      : nullptr;
            obj->SetValue<String>(pe->dp, s ? s : "");
            return;
        }
        case DM_NOESIS_PROP_THICKNESS: {
            Thickness t;
            if (value_ptr) {
                const auto* f = static_cast<const float*>(value_ptr);
                t = Thickness(f[0], f[1], f[2], f[3]);
            }
            obj->SetValue<Thickness>(pe->dp, t);
            return;
        }
        case DM_NOESIS_PROP_COLOR: {
            Color c;
            if (value_ptr) {
                const auto* f = static_cast<const float*>(value_ptr);
                c = Color(f[0], f[1], f[2], f[3]);
            }
            obj->SetValue<Color>(pe->dp, c);
            return;
        }
        case DM_NOESIS_PROP_RECT: {
            Rect r;
            if (value_ptr) {
                const auto* f = static_cast<const float*>(value_ptr);
                r = Rect(f[0], f[1], f[0] + f[2], f[1] + f[3]);
            }
            obj->SetValue<Rect>(pe->dp, r);
            return;
        }
        case DM_NOESIS_PROP_IMAGE_SOURCE:
        case DM_NOESIS_PROP_BASE_COMPONENT: {
            BaseComponent* b = value_ptr ? *static_cast<BaseComponent* const*>(value_ptr)
                                         : nullptr;
            obj->SetValueObject(pe->dp, b);
            return;
        }
    }
}

extern "C" bool dm_noesis_instance_get_property(
    void* instance,
    uint32_t prop_index,
    void* out_value) {
    const PropEntry* pe = instance_prop(instance, prop_index, nullptr);
    if (!pe || !out_value) return false;
    auto* obj = static_cast<Noesis::DependencyObject*>(static_cast<RustContentControl*>(instance));

    using namespace Noesis;
    switch (pe->type) {
        case DM_NOESIS_PROP_INT32:
            *static_cast<int32_t*>(out_value) = obj->GetValue<int32_t>(pe->dp);
            return true;
        case DM_NOESIS_PROP_FLOAT:
            *static_cast<float*>(out_value) = obj->GetValue<float>(pe->dp);
            return true;
        case DM_NOESIS_PROP_DOUBLE:
            *static_cast<double*>(out_value) = obj->GetValue<double>(pe->dp);
            return true;
        case DM_NOESIS_PROP_BOOL:
            *static_cast<bool*>(out_value) = obj->GetValue<bool>(pe->dp);
            return true;
        case DM_NOESIS_PROP_STRING: {
            const String& s = obj->GetValue<String>(pe->dp);
            *static_cast<const char**>(out_value) = s.Str();
            return true;
        }
        case DM_NOESIS_PROP_THICKNESS: {
            const Thickness& t = obj->GetValue<Thickness>(pe->dp);
            auto* f = static_cast<float*>(out_value);
            f[0] = t.left; f[1] = t.top; f[2] = t.right; f[3] = t.bottom;
            return true;
        }
        case DM_NOESIS_PROP_COLOR: {
            const Color& c = obj->GetValue<Color>(pe->dp);
            auto* f = static_cast<float*>(out_value);
            f[0] = c.r; f[1] = c.g; f[2] = c.b; f[3] = c.a;
            return true;
        }
        case DM_NOESIS_PROP_RECT: {
            const Rect& r = obj->GetValue<Rect>(pe->dp);
            auto* f = static_cast<float*>(out_value);
            f[0] = r.x; f[1] = r.y; f[2] = r.width; f[3] = r.height;
            return true;
        }
        case DM_NOESIS_PROP_IMAGE_SOURCE:
        case DM_NOESIS_PROP_BASE_COMPONENT: {
            Ptr<BaseComponent> v = obj->GetValueObject(pe->dp);
            *static_cast<BaseComponent**>(out_value) = v.GetPtr();
            return true;
        }
    }
    return false;
}
