#ifndef PEPPER_PLUGIN_API_H
#define PEPPER_PLUGIN_API_H

struct PepperCommandContext;

typedef void* PepperPluginUserData;

typedef const char* (* PepperPluginCommandFn)(
    const struct PepperPluginApi* api,
    struct PepperCommandContext* ctx,
    PepperPluginUserData userdata
);

struct PepperPluginApi {
    void (* register_command)(struct PepperCommandContext* ctx, const char *name, PepperPluginCommandFn command_fn);
    void (* write_to_statusbar)(struct PepperCommandContext* ctx, int level, const char *message);
};

#endif
