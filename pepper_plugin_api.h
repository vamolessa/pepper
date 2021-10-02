#ifndef PEPPER_PLUGIN_API_H
#define PEPPER_PLUGIN_API_H

typedef struct PepperCommandContext PepperCommandContext;

typedef struct PepperStringSlice {
    const char *bytes;
    unsigned int len;
} PepperStringSlice;

typedef void *PepperPluginUserData;

typedef const char *(*PepperPluginCommandFn)(const struct PepperPluginApi *api, struct PepperCommandContext *ctx, PepperPluginUserData userdata);

typedef struct PepperPluginApi {
    void (*register_command)(struct PepperCommandContext *ctx, struct PepperStringSlice name, PepperPluginCommandFn command_fn);
    void (*write_to_statusbar)(struct PepperCommandContext *ctx, unsigned int level, struct PepperStringSlice message);
} PepperPluginApi;

#endif /* PEPPER_PLUGIN_API_H */
