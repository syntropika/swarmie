#include "CSwarmie.h"
#include <dlfcn.h>
#include <pthread.h>
#include <stddef.h>

static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static void *library;
static uint32_t (*version)(void);
static char *(*execute)(const char *);
static void (*release)(char *);

int swarmie_binding_load(const char *path) {
    pthread_mutex_lock(&lock);
    if (library) { pthread_mutex_unlock(&lock); return 1; }
    void *candidate = dlopen(path, RTLD_NOW | RTLD_LOCAL);
    if (!candidate) { pthread_mutex_unlock(&lock); return 0; }
    version = dlsym(candidate, "swarmie_client_abi_version");
    execute = dlsym(candidate, "swarmie_client_execute");
    release = dlsym(candidate, "swarmie_client_free");
    if (!version || !execute || !release || version() != 1) {
        version = NULL; execute = NULL; release = NULL;
        dlclose(candidate); pthread_mutex_unlock(&lock); return 0;
    }
    /* Keep the library loaded while any native handles or allocations exist. */
    library = candidate;
    pthread_mutex_unlock(&lock);
    return 1;
}
uint32_t swarmie_binding_version(void) { return version ? version() : 0; }
char *swarmie_binding_execute(const char *command) { return execute ? execute(command) : NULL; }
void swarmie_binding_free(char *result) { if (release) release(result); }
