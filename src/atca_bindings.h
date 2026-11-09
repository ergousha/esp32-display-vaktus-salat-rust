/* Bindings for the CryptoAuthLib calls the firmware makes directly (chip
 * identity, lock state, key generation, CSR signing). TLS client signing goes
 * through esp-tls + atca_mbedtls instead. */
#include "cryptoauthlib.h"
