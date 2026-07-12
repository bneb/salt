/**
 * Runtime Hardening Tests — Phase 1: Security & Soundness
 *
 * Tests for CVE-grade fixes in runtime.c:
 *   1. salt_get_argv OOB bounds check
 *   2. salt_get_argv_len OOB bounds check
 *   3. salt_memcpy_impl memmove semantics (overlap protection)
 *   4. salt_process_exec_capture snprintf truncation guard
 */
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

// Extern declarations for runtime functions under test
extern const char *salt_get_argv(int32_t idx);
extern int64_t salt_get_argv_len(int32_t idx);
// salt_memcpy_impl is exported under the asm label '_memcpy', so we
// call it through a tiny wrapper that invokes the standard memcpy symbol.
// When linked with runtime.c, this resolves to salt_memcpy_impl.
static int64_t test_salt_memcpy(int64_t dst, int64_t src, int64_t len) {
  // Use string.h memcpy — runtime.c overrides this symbol
  memcpy((void *)(uintptr_t)dst, (const void *)(uintptr_t)src, (size_t)len);
  return dst;
}
extern int64_t salt_process_exec_capture(const char *program, const char *arg1,
                                         char *out_buf, int64_t buf_size);

static int tests_passed = 0;
static int tests_failed = 0;

#define TEST(name)                                                             \
  do {                                                                         \
    printf("  TEST: %-50s", name);                                             \
  } while (0)
#define PASS()                                                                 \
  do {                                                                         \
    printf("PASS\n");                                                          \
    tests_passed++;                                                            \
  } while (0)
#define FAIL(msg)                                                              \
  do {                                                                         \
    printf("FAIL: %s\n", msg);                                                 \
    tests_failed++;                                                            \
  } while (0)

// ============================================================================
// 1. salt_get_argv OOB bounds check
// ============================================================================

void test_get_argv_oob_positive() {
  TEST("salt_get_argv(999) returns NULL");
  const char *result = salt_get_argv(999);
  if (result == NULL) {
    PASS();
  } else {
    FAIL("Expected NULL for OOB index");
  }
}

void test_get_argv_oob_negative() {
  TEST("salt_get_argv(-1) returns NULL");
  const char *result = salt_get_argv(-1);
  if (result == NULL) {
    PASS();
  } else {
    FAIL("Expected NULL for negative index");
  }
}

void test_get_argv_valid() {
  TEST("salt_get_argv(0) returns non-NULL");
  const char *result = salt_get_argv(0);
  if (result != NULL) {
    PASS();
  } else {
    FAIL("Expected non-NULL for argv[0]");
  }
}

// ============================================================================
// 2. salt_get_argv_len OOB bounds check
// ============================================================================

void test_get_argv_len_oob() {
  TEST("salt_get_argv_len(999) returns 0");
  int64_t result = salt_get_argv_len(999);
  if (result == 0) {
    PASS();
  } else {
    FAIL("Expected 0 for OOB index");
  }
}

void test_get_argv_len_negative() {
  TEST("salt_get_argv_len(-1) returns 0");
  int64_t result = salt_get_argv_len(-1);
  if (result == 0) {
    PASS();
  } else {
    FAIL("Expected 0 for negative index");
  }
}

void test_get_argv_len_valid() {
  TEST("salt_get_argv_len(0) returns > 0");
  int64_t result = salt_get_argv_len(0);
  if (result > 0) {
    PASS();
  } else {
    FAIL("Expected positive length for argv[0]");
  }
}

// ============================================================================
// 3. salt_memcpy_impl memmove semantics
// ============================================================================

void test_memcpy_non_overlapping() {
  TEST("memcpy: non-overlapping copy");
  char src[16] = "Hello, World!!!\0";
  char dst[16] = {0};
  test_salt_memcpy((int64_t)(uintptr_t)dst, (int64_t)(uintptr_t)src, 15);
  if (memcmp(dst, "Hello, World!!!", 15) == 0) {
    PASS();
  } else {
    FAIL("Non-overlapping copy produced wrong result");
  }
}

void test_memcpy_overlapping_forward() {
  TEST("memcpy: overlapping forward (dst > src)");
  // Simulates: memmove within a buffer where dst overlaps src ahead
  // Buffer: [A B C D E F G H _ _ _ _]
  // Copy 8 bytes from offset 0 to offset 4 — overlap of 4 bytes
  char buf[16] = "ABCDEFGH\0\0\0\0\0\0\0";
  char *src = buf;
  char *dst = buf + 4;
  test_salt_memcpy((int64_t)(uintptr_t)dst, (int64_t)(uintptr_t)src, 8);
  // Expected: buf = "ABCDABCDEFGH\0..."
  if (memcmp(buf + 4, "ABCDEFGH", 8) == 0) {
    PASS();
  } else {
    printf("\n    Got: ");
    for (int i = 0; i < 12; i++)
      printf("%c", buf[i]);
    printf("\n");
    FAIL("Overlapping forward copy produced wrong result");
  }
}

void test_memcpy_overlapping_backward() {
  TEST("memcpy: overlapping backward (dst < src)");
  // Copy from offset 4 to offset 0 — dst < src
  char buf[16] = "ABCDEFGH\0\0\0\0\0\0\0";
  char *dst = buf;
  char *src = buf + 4;
  test_salt_memcpy((int64_t)(uintptr_t)dst, (int64_t)(uintptr_t)src, 4);
  // Expected: buf = "EFGHEFGH\0..."
  if (memcmp(buf, "EFGH", 4) == 0) {
    PASS();
  } else {
    FAIL("Overlapping backward copy produced wrong result");
  }
}

void test_memcpy_zero_length() {
  TEST("memcpy: zero length does nothing");
  char buf[4] = "ABC";
  test_salt_memcpy((int64_t)(uintptr_t)buf, (int64_t)(uintptr_t)buf, 0);
  if (memcmp(buf, "ABC", 3) == 0) {
    PASS();
  } else {
    FAIL("Zero-length copy modified buffer");
  }
}

void test_memcpy_negative_length() {
  TEST("memcpy: negative length does nothing");
  char buf[4] = "ABC";
  test_salt_memcpy((int64_t)(uintptr_t)buf, (int64_t)(uintptr_t)buf, -5);
  if (memcmp(buf, "ABC", 3) == 0) {
    PASS();
  } else {
    FAIL("Negative-length copy modified buffer");
  }
}

// ============================================================================
// 4. salt_process_exec_capture snprintf truncation guard
// ============================================================================

void test_exec_capture_truncation() {
  TEST("exec_capture: 2000-char arg returns -1 (truncated)");
  // Create an argument string that exceeds the 1024-byte cmd buffer
  char long_arg[2048];
  memset(long_arg, 'A', sizeof(long_arg) - 1);
  long_arg[sizeof(long_arg) - 1] = '\0';

  char out_buf[256];
  int64_t result = salt_process_exec_capture("/bin/echo", long_arg, out_buf,
                                             sizeof(out_buf));
  if (result == -1) {
    PASS();
  } else {
    FAIL("Expected -1 for truncated command");
  }
}

void test_exec_capture_normal() {
  TEST("exec_capture: normal short command works");
  char out_buf[256] = {0};
  int64_t result =
      salt_process_exec_capture("/bin/echo", "hello", out_buf, sizeof(out_buf));
  if (result > 0) {
    PASS();
  } else {
    FAIL("Expected positive result for short command");
  }
}

// ============================================================================
// Main
// ============================================================================

int main() {
  printf("=== Runtime Hardening Tests ===\n\n");

  printf("[argv bounds]\n");
  test_get_argv_oob_positive();
  test_get_argv_oob_negative();
  test_get_argv_valid();

  printf("\n[argv_len bounds]\n");
  test_get_argv_len_oob();
  test_get_argv_len_negative();
  test_get_argv_len_valid();

  printf("\n[memcpy/memmove semantics]\n");
  test_memcpy_non_overlapping();
  test_memcpy_overlapping_forward();
  test_memcpy_overlapping_backward();
  test_memcpy_zero_length();
  test_memcpy_negative_length();

  printf("\n[exec_capture truncation]\n");
  test_exec_capture_truncation();
  test_exec_capture_normal();

  printf("\n=== Results: %d passed, %d failed ===\n", tests_passed,
         tests_failed);
  return tests_failed > 0 ? 1 : 0;
}
