#include "noesis_shim.h"

#include <NsCore/Noesis.h>
#include <NsCore/Init.h>
#include <NsCore/Log.h>
#include <NsCore/Version.h>

namespace {

dm_noesis_log_fn g_log_cb       = nullptr;
void*            g_log_userdata = nullptr;

void log_trampoline(const char* file, uint32_t line, uint32_t level,
                    const char* channel, const char* message)
{
    if (g_log_cb) {
        g_log_cb(g_log_userdata, file, line,
                 static_cast<dm_noesis_log_level>(level),
                 channel ? channel : "",
                 message ? message : "");
    }
}

}  // namespace

extern "C" void dm_noesis_set_license(const char* name, const char* key)
{
    Noesis::SetLicense(name ? name : "", key ? key : "");
}

extern "C" void dm_noesis_set_log_handler(dm_noesis_log_fn cb, void* userdata)
{
    g_log_cb       = cb;
    g_log_userdata = userdata;
    Noesis::SetLogHandler(cb ? log_trampoline : nullptr);
}

extern "C" void dm_noesis_init(void)
{
    Noesis::Init();
}

extern "C" void dm_noesis_shutdown(void)
{
    Noesis::Shutdown();
}

extern "C" const char* dm_noesis_version(void)
{
    return Noesis::GetBuildVersion();
}
