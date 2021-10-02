#ifndef PEPPER_PLUGIN_API_H
#define PEPPER_PLUGIN_API_H

typedef void *PepperPluginUserData;

typedef void (*PepperPluginDeinitFn)(PepperPluginUserData);

typedef struct PepperStringSlice {
    const char *bytes;
    unsigned int len;
} PepperStringSlice;

typedef const char *(*PepperPluginCommandFn)(const struct PepperPluginApi *api, PepperPluginUserData userdata);

typedef struct PepperPluginApi {
    void (*set_deinit_fn)(PepperPluginDeinitFn deinit_fn);
    void (*register_command)(struct PepperStringSlice name, PepperPluginCommandFn command_fn);
    void (*write_to_statusbar)(unsigned int level, struct PepperStringSlice message);
} PepperPluginApi;

#endif /* PEPPER_PLUGIN_API_H */
