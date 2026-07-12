// =============================================================================
// Salt Regex Bridge — C bridge wrapping POSIX regex.h
// =============================================================================
// Provides compiled regex matching via POSIX Extended Regular Expressions.
//
// Functions:
//   salt_regex_compile(pattern) → handle (0 on error)
//   salt_regex_match(handle, text) → 1/0
//   salt_regex_find(handle, text, start_out, end_out) → 1/0
//   salt_regex_free(handle)
// =============================================================================

#include <regex.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

// Compile a regex pattern with POSIX Extended syntax.
// Returns an opaque handle (pointer cast to i64), or 0 on failure.
int64_t salt_regex_compile(const char *pattern) {
  regex_t *re = (regex_t *)malloc(sizeof(regex_t));
  if (!re)
    return 0;

  int err = regcomp(re, pattern, REG_EXTENDED | REG_NOSUB);
  if (err != 0) {
    free(re);
    return 0;
  }
  return (int64_t)re;
}

// Compile with sub-match support (for find/groups).
int64_t salt_regex_compile_match(const char *pattern) {
  regex_t *re = (regex_t *)malloc(sizeof(regex_t));
  if (!re)
    return 0;

  int err = regcomp(re, pattern, REG_EXTENDED);
  if (err != 0) {
    free(re);
    return 0;
  }
  return (int64_t)re;
}

// Test if text matches the compiled pattern. Returns 1 (match) or 0 (no match).
int32_t salt_regex_match(int64_t handle, const char *text) {
  if (handle == 0)
    return 0;
  regex_t *re = (regex_t *)handle;
  return regexec(re, text, 0, NULL, 0) == 0 ? 1 : 0;
}

// Find the first match in text. Writes start/end byte offsets.
// Returns 1 if found, 0 if not.
int32_t salt_regex_find(int64_t handle, const char *text, int64_t *start_out,
                        int64_t *end_out) {
  if (handle == 0)
    return 0;
  regex_t *re = (regex_t *)handle;
  regmatch_t match;

  if (regexec(re, text, 1, &match, 0) == 0) {
    *start_out = (int64_t)match.rm_so;
    *end_out = (int64_t)match.rm_eo;
    return 1;
  }
  return 0;
}

// Find match with capture groups.
// groups_out is an array of [start, end] pairs (2 * max_groups entries).
// Returns number of groups matched (including group 0 = whole match).
int32_t salt_regex_find_groups(int64_t handle, const char *text,
                               int64_t *groups_out, int32_t max_groups) {
  if (handle == 0 || max_groups <= 0)
    return 0;
  regex_t *re = (regex_t *)handle;

  regmatch_t *matches = (regmatch_t *)malloc(sizeof(regmatch_t) * max_groups);
  if (!matches)
    return 0;

  int result = regexec(re, text, max_groups, matches, 0);
  if (result != 0) {
    free(matches);
    return 0;
  }

  int count = 0;
  for (int i = 0; i < max_groups; i++) {
    if (matches[i].rm_so == -1)
      break;
    groups_out[i * 2] = (int64_t)matches[i].rm_so;
    groups_out[i * 2 + 1] = (int64_t)matches[i].rm_eo;
    count++;
  }

  free(matches);
  return count;
}

// Free a compiled regex handle.
void salt_regex_free(int64_t handle) {
  if (handle == 0)
    return;
  regex_t *re = (regex_t *)handle;
  regfree(re);
  free(re);
}
