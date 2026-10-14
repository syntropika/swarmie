#ifndef SWARMIE_CLIENT_H
#define SWARMIE_CLIENT_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Version 1 lowers swarmie:client/native@0.1.0 using UTF-8 JSON, snake_case
   fields, and an action tag. Each reply has either ok or a WIT client-error.
   Input remains owned by the caller. Output must be freed exactly once. */
uint32_t swarmie_client_abi_version(void);
char *swarmie_client_execute(const char *command);
void swarmie_client_free(char *result);
#ifdef __cplusplus
}
#endif
#endif
