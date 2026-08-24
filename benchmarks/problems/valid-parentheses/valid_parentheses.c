#include <stdio.h>
#include <stdlib.h>

#define LEN 4096L
#define STRINGS 20000

/* Bracket types: 0='(' 1='[' 2='{' and 3=')' 4=']' 5='}'. Strings are
 * generated as an opener prefix plus a mirrored closer suffix, so they
 * are balanced by construction; every few strings get one character
 * flipped, which always breaks validity. Returns matched pairs if
 * valid, -1 if not. */
static long long lcg(long long x) {
  return (1103515245LL * x + 12345LL) % 2147483648LL;
}

static long long gen_string(int *buf, long long len, long long seed_in,
                            int corrupt) {
  long long s = seed_in;
  long long half = len / 2;
  for (long long j = 0; j < half; j++) {
    s = lcg(s);
    int t = (int)(s % 3);
    buf[j] = t;
    buf[len - 1 - j] = t + 3;
  }
  if (corrupt) {
    s = lcg(s);
    long long idx = s % len;
    buf[idx] = (int)(((buf[idx] + 3) % 6));
  }
  return s;
}

static long long validate(const int *buf, int *stack, long long len) {
  long long depth = 0, pairs = 0;
  int ok = 1;
  for (long long j = 0; j < len; j++) {
    int t = buf[j];
    if (t < 3)
      stack[depth++] = t;
    if (t >= 3 && depth == 0)
      ok = 0;
    if (t >= 3 && depth > 0) {
      depth--;
      if (stack[depth] == t - 3)
        pairs++;
      else
        ok = 0;
    }
  }
  return (ok && depth == 0) ? pairs : -1;
}

int main(void) {
  int *buf = malloc(LEN * sizeof(int));
  int *stack = malloc(LEN * sizeof(int));
  if (!buf || !stack)
    return 1;
  long long s = 12345, checksum = 0;
  for (int k = 0; k < STRINGS; k++) {
    s = gen_string(buf, LEN, s, k % 4 == 0);
    long long r = validate(buf, stack, LEN);
    checksum += (r >= 0) ? r + 1 : -1;
  }
  printf("checksum=%lld\n", checksum);
  free(buf);
  free(stack);
  return 0;
}
