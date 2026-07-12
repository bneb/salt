// =============================================================================
// Salt TLS Bridge — C bridge wrapping OpenSSL 3.x
// =============================================================================
// Provides TLS client/server operations via OpenSSL.
// Compile with: cc -c tls_bridge.c -I/opt/homebrew/opt/openssl/include
// Link with: -lssl -lcrypto -L/opt/homebrew/opt/openssl/lib
//
// Functions:
//   salt_tls_ctx_new() → ctx handle
//   salt_tls_ctx_new_server(cert_file, key_file) → ctx handle
//   salt_tls_connect(ctx, fd) → ssl handle
//   salt_tls_accept(ctx, fd) → ssl handle
//   salt_tls_read(ssl, buf, len) → bytes read
//   salt_tls_write(ssl, buf, len) → bytes written
//   salt_tls_shutdown(ssl)
//   salt_tls_free(ssl)
//   salt_tls_ctx_free(ctx)
// =============================================================================

#include <openssl/err.h>
#include <openssl/ssl.h>
#include <stdint.h>

// --- Initialization (auto-init on first ctx creation) ---

static int _ssl_initialized = 0;

static void ensure_ssl_init(void) {
  if (!_ssl_initialized) {
    OPENSSL_init_ssl(
        OPENSSL_INIT_LOAD_SSL_STRINGS | OPENSSL_INIT_LOAD_CRYPTO_STRINGS, NULL);
    _ssl_initialized = 1;
  }
}

// --- Context Creation ---

// Create a TLS client context using TLS_client_method().
// Returns opaque handle (SSL_CTX* cast to i64), or 0 on failure.
int64_t salt_tls_ctx_new(void) {
  ensure_ssl_init();
  SSL_CTX *ctx = SSL_CTX_new(TLS_client_method());
  if (!ctx)
    return 0;

  // Enable certificate verification with system store
  SSL_CTX_set_default_verify_paths(ctx);
  SSL_CTX_set_verify(ctx, SSL_VERIFY_PEER, NULL);

  return (int64_t)ctx;
}

// Create a TLS server context with cert/key files.
// Returns opaque handle, or 0 on failure.
int64_t salt_tls_ctx_new_server(const char *cert_file, const char *key_file) {
  ensure_ssl_init();
  SSL_CTX *ctx = SSL_CTX_new(TLS_server_method());
  if (!ctx)
    return 0;

  if (SSL_CTX_use_certificate_file(ctx, cert_file, SSL_FILETYPE_PEM) <= 0) {
    SSL_CTX_free(ctx);
    return 0;
  }
  if (SSL_CTX_use_PrivateKey_file(ctx, key_file, SSL_FILETYPE_PEM) <= 0) {
    SSL_CTX_free(ctx);
    return 0;
  }
  if (!SSL_CTX_check_private_key(ctx)) {
    SSL_CTX_free(ctx);
    return 0;
  }

  return (int64_t)ctx;
}

// --- Connection Operations ---

// Create an SSL connection on an existing socket fd.
// Performs TLS handshake as client. Returns ssl handle or 0.
int64_t salt_tls_connect(int64_t ctx_handle, int32_t fd) {
  if (ctx_handle == 0)
    return 0;
  SSL_CTX *ctx = (SSL_CTX *)ctx_handle;

  SSL *ssl = SSL_new(ctx);
  if (!ssl)
    return 0;

  SSL_set_fd(ssl, fd);

  if (SSL_connect(ssl) <= 0) {
    SSL_free(ssl);
    return 0;
  }

  return (int64_t)ssl;
}

// Accept a TLS connection on an existing socket fd (server-side).
// Returns ssl handle or 0.
int64_t salt_tls_accept(int64_t ctx_handle, int32_t fd) {
  if (ctx_handle == 0)
    return 0;
  SSL_CTX *ctx = (SSL_CTX *)ctx_handle;

  SSL *ssl = SSL_new(ctx);
  if (!ssl)
    return 0;

  SSL_set_fd(ssl, fd);

  if (SSL_accept(ssl) <= 0) {
    SSL_free(ssl);
    return 0;
  }

  return (int64_t)ssl;
}

// --- I/O Operations ---

// Read up to len bytes into buf. Returns bytes read, or -1 on error.
int64_t salt_tls_read(int64_t ssl_handle, void *buf, int64_t len) {
  if (ssl_handle == 0)
    return -1;
  SSL *ssl = (SSL *)ssl_handle;
  int n = SSL_read(ssl, buf, (int)len);
  return (int64_t)n;
}

// Write len bytes from buf. Returns bytes written, or -1 on error.
int64_t salt_tls_write(int64_t ssl_handle, const void *buf, int64_t len) {
  if (ssl_handle == 0)
    return -1;
  SSL *ssl = (SSL *)ssl_handle;
  int n = SSL_write(ssl, buf, (int)len);
  return (int64_t)n;
}

// --- Cleanup ---

// Gracefully shut down the TLS session.
void salt_tls_shutdown(int64_t ssl_handle) {
  if (ssl_handle == 0)
    return;
  SSL *ssl = (SSL *)ssl_handle;
  SSL_shutdown(ssl);
}

// Free an SSL connection handle.
void salt_tls_free(int64_t ssl_handle) {
  if (ssl_handle == 0)
    return;
  SSL *ssl = (SSL *)ssl_handle;
  SSL_free(ssl);
}

// Free an SSL_CTX handle.
void salt_tls_ctx_free(int64_t ctx_handle) {
  if (ctx_handle == 0)
    return;
  SSL_CTX *ctx = (SSL_CTX *)ctx_handle;
  SSL_CTX_free(ctx);
}

// --- Utility ---

// Get the last OpenSSL error as a human-readable string.
// Returns pointer to static buffer (valid until next OpenSSL call).
const char *salt_tls_error_string(void) {
  unsigned long err = ERR_peek_last_error();
  if (err == 0)
    return "no error";
  return ERR_reason_error_string(err);
}
