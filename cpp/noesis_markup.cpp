// Custom MarkupExtension registration FFI (Phase 5.D).
//
// Same architectural pattern as noesis_classes.cpp: a per-base C++
// trampoline subclass + synthetic per-name TypeClassBuilder + Factory
// creator + Symbol → ClassData side table. The trampoline's virtual
// override is `ProvideValue` (rather than `OnPropertyChanged`), and it
// dispatches to the Rust callback with the current `Key` value the XAML
// parser set via the ContentProperty mechanism.
//
// v1 scope: a single positional `Key` string argument. Returns either a
// borrowed C string (most common — wrapped into a BoxedValue<String>) or
// a borrowed BaseComponent* (for value types that can't be expressed as
// text, e.g. an existing resource lookup). Reactive bindings (locale
// switch updates UI in place) are deferred to a later PR — they need a
// LocalizationManager-style indexer + Binding, which is its own design.

#include "noesis_shim.h"

#include <NsCore/Boxing.h>
#include <NsCore/Factory.h>
#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsCore/Reflection.h>
#include <NsCore/ReflectionImplement.h>
#include <NsCore/String.h>
#include <NsCore/Symbol.h>
#include <NsCore/TypeClassBuilder.h>
#include <NsCore/TypeClassCreator.h>
#include <NsCore/TypeOf.h>
#include <NsGui/ContentPropertyMetaData.h>
#include <NsGui/MarkupExtension.h>
#include <NsGui/ValueTargetProvider.h>

#include <mutex>
#include <unordered_map>

namespace {

// ── ClassData + registry ───────────────────────────────────────────────────

struct MarkupClassData {
    Noesis::String                 name;
    Noesis::Symbol                 sym;
    Noesis::TypeClassBuilder*      typeClass; // owned by Reflection registry
    dm_noesis_markup_provide_fn    cb;
    void*                          userdata;
};

std::mutex                                              g_markup_registry_mutex;
std::unordered_map<uint32_t, MarkupClassData*>          g_markup_registry;

MarkupClassData* markup_registry_find(Noesis::Symbol sym) {
    std::lock_guard<std::mutex> lock(g_markup_registry_mutex);
    auto it = g_markup_registry.find((uint32_t)sym);
    return it == g_markup_registry.end() ? nullptr : it->second;
}

bool markup_registry_insert(Noesis::Symbol sym, MarkupClassData* cd) {
    std::lock_guard<std::mutex> lock(g_markup_registry_mutex);
    return g_markup_registry.emplace((uint32_t)sym, cd).second;
}

void markup_registry_erase(Noesis::Symbol sym) {
    std::lock_guard<std::mutex> lock(g_markup_registry_mutex);
    g_markup_registry.erase((uint32_t)sym);
}

// ── Trampoline subclass: MarkupExtension ───────────────────────────────────
//
// Same hand-rolled-reflection pattern as noesis_classes.cpp's
// RustContentControl: NS_DECLARE_REFLECTION's macros generate a
// GetClassType() that always returns the static type, but we need our
// override to report the synthetic per-name class so XAML's parser
// finds the right factory creator.

class RustMarkupExtension: public Noesis::MarkupExtension {
public:
    Noesis::String Key; // ContentProperty — populated by XAML parser

    RustMarkupExtension() = default;

    void BindClassData(MarkupClassData* cd) { mClassData = cd; }
    MarkupClassData* GetClassData() const { return mClassData; }

    Noesis::Ptr<Noesis::BaseComponent>
    ProvideValue(const Noesis::ValueTargetProvider* /*provider*/) override;

    // Hand-rolled reflection — see noesis_classes.cpp::RustContentControl
    // for the rationale.
    static const Noesis::TypeClass*
    StaticGetClassType(Noesis::TypeTag<RustMarkupExtension>*);
    const Noesis::TypeClass* GetClassType() const override;

private:
    MarkupClassData* mClassData = nullptr;

    typedef RustMarkupExtension SelfClass;
    typedef Noesis::MarkupExtension ParentClass;
    friend class Noesis::TypeClassCreator;

    static void StaticFillClassType(Noesis::TypeClassCreator& helper) {
        // Register `Key` as a reflection property so XAML's parser can
        // populate it from `{aor:Localize SOME_KEY}`. Marking it as the
        // ContentProperty makes the positional argument syntax work
        // without callers having to write `Key=...` explicitly.
        helper.Prop("Key", &RustMarkupExtension::Key);
        helper.Meta<Noesis::ContentPropertyMetaData>("Key");
    }
};

const Noesis::TypeClass*
RustMarkupExtension::StaticGetClassType(Noesis::TypeTag<RustMarkupExtension>*) {
    static const Noesis::TypeClass* type;
    if (NS_UNLIKELY(type == 0)) {
        type = static_cast<const Noesis::TypeClass*>(Noesis::Reflection::RegisterType(
            "DmNoesis.RustMarkupExtension",
            Noesis::TypeClassCreator::Create<RustMarkupExtension>,
            Noesis::TypeClassCreator::Fill<RustMarkupExtension, Noesis::MarkupExtension>));
    }
    return type;
}

const Noesis::TypeClass* RustMarkupExtension::GetClassType() const {
    if (mClassData && mClassData->typeClass) {
        return static_cast<const Noesis::TypeClass*>(mClassData->typeClass);
    }
    return StaticGetClassType((Noesis::TypeTag<RustMarkupExtension>*)nullptr);
}

Noesis::Ptr<Noesis::BaseComponent>
RustMarkupExtension::ProvideValue(const Noesis::ValueTargetProvider* /*provider*/) {
    if (!mClassData || !mClassData->cb) {
        return nullptr;
    }

    const char* out_string = nullptr;
    void* out_component = nullptr;
    bool produced = mClassData->cb(
        mClassData->userdata, Key.Str(), &out_string, &out_component);

    if (!produced) {
        // Returning a null Ptr signals UnsetValue to Noesis's parser.
        return nullptr;
    }

    if (out_string) {
        // Box the C string into a BoxedValue<String>. Boxing copies the
        // bytes; the caller's pointer can go away after this call.
        return Noesis::Boxing::Box(out_string);
    }
    if (out_component) {
        // Borrowed BaseComponent*; increment the ref count for the
        // returned Ptr (Noesis::Ptr's adopt-from-raw form would consume
        // the caller's ref, which contract-wise we don't have).
        auto* obj = static_cast<Noesis::BaseComponent*>(out_component);
        return Noesis::Ptr<Noesis::BaseComponent>(obj);
    }
    return nullptr;
}

// ── Factory creator ────────────────────────────────────────────────────────

Noesis::BaseComponent* markup_creator(Noesis::Symbol name) {
    MarkupClassData* cd = markup_registry_find(name);
    if (!cd) return nullptr;
    auto* ext = new RustMarkupExtension();
    ext->BindClassData(cd);
    return ext;
}

}  // namespace

// ── C ABI surface ──────────────────────────────────────────────────────────

extern "C" void* dm_noesis_markup_extension_register(
    const char* name,
    dm_noesis_markup_provide_fn cb,
    void* userdata) {
    if (!name || !cb) return nullptr;

    Noesis::Symbol sym = Noesis::Symbol(name);
    if (Noesis::Reflection::IsTypeRegistered(sym)) {
        return nullptr;
    }

    auto* cd = new MarkupClassData();
    cd->name = name;
    cd->sym = sym;
    cd->cb = cb;
    cd->userdata = userdata;

    cd->typeClass = new Noesis::TypeClassBuilder(sym, /*isInterface*/ false);
    cd->typeClass->AddBase(Noesis::TypeOf<RustMarkupExtension>());

    Noesis::Reflection::RegisterType(cd->typeClass);
    Noesis::Factory::RegisterComponent(sym, Noesis::Symbol(""), markup_creator);

    if (!markup_registry_insert(sym, cd)) {
        Noesis::Factory::UnregisterComponent(sym);
        Noesis::Reflection::Unregister(cd->typeClass);
        delete cd;
        return nullptr;
    }

    return cd;
}

extern "C" void dm_noesis_markup_extension_unregister(void* token) {
    if (!token) return;
    auto* cd = static_cast<MarkupClassData*>(token);

    Noesis::Factory::UnregisterComponent(cd->sym);
    markup_registry_erase(cd->sym);
    Noesis::Reflection::Unregister(cd->typeClass);

    delete cd;
}
