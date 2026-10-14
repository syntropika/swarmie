#ifndef C_SWARMIE_H
#define C_SWARMIE_H
#include <stdint.h>
/* Load one process-wide native library. Repeated calls reuse the loaded module. */
int swarmie_binding_load(const char *path);
uint32_t swarmie_binding_version(void);
char *swarmie_binding_execute(const char *command);
void swarmie_binding_free(char *result);
#endif
