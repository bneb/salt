#include <stdio.h>
#include <stdlib.h>

#define N 1000000L
#define ROUNDS 200

static long long lcg(long long x) {
  return (1103515245LL * x + 12345LL) % 2147483648LL;
}

static void fill(int *a, long long n) {
  long long s = 42;
  for (long long i = 0; i < n; i++) {
    a[i] = (int)(s % 101) - 50;
    s = lcg(s);
  }
}

static long long kadane(const int *a, long long n) {
  long long best = -1000000000LL, cur = 0;
  for (long long i = 0; i < n; i++) {
    long long v = a[i];
    cur = (cur > 0) ? cur + v : v;
    if (cur > best)
      best = cur;
  }
  return best;
}

static long long solve(const int *a, long long n, int rounds) {
  long long checksum = 0;
  for (int r = 0; r < rounds; r++)
    checksum += kadane(a, n);
  return checksum;
}

int main(void) {
  int *a = malloc(N * sizeof(int));
  if (!a)
    return 1;
  fill(a, N);
  long long checksum = solve(a, N, ROUNDS);
  printf("checksum=%lld\n", checksum);
  free(a);
  return 0;
}
