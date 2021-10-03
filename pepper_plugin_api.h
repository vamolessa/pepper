#ifndef PEPPER_PLUGIN_API_H
#define PEPPER_PLUGIN_API_H

typedef void *PepperPluginUserData;

typedef void (*PepperPluginDeinitFn)(PepperPluginUserData);

typedef void (*PepperPluginEventHandlerFn)(const struct PepperPluginApi*, PepperPluginUserData);

typedef struct PepperByteSlice {
    const char *bytes;
    unsigned int len;
} PepperByteSlice;

typedef const char *(*PepperPluginCommandFn)(const struct PepperPluginApi *api, PepperPluginUserData userdata);

typedef struct PepperPluginApi {
    void (*set_deinit_fn)(PepperPluginDeinitFn deinit_fn);
    void (*set_event_handler_fn)(PepperPluginEventHandlerFn event_handler_fn);
    void (*register_command)(struct PepperByteSlice name, PepperPluginCommandFn command_fn);
    void (*write_to_statusbar)(unsigned int level, struct PepperByteSlice message);
} PepperPluginApi;

#endif /* PEPPER_PLUGIN_API_H */
