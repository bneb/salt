#include <stdio.h>
#include <stdlib.h>

#define N 200000L
#define QUERIES 500

static long long lcg(long long x) {
  return (1103515245LL * x + 12345LL) % 2147483648LL;
}

static void fill_sorted(int *a, long long n) {
  long long s = 42;
  int prev = 0;
  for (long long i = 0; i < n; i++) {
    a[i] = prev + (int)(s % 8);
    prev = a[i];
    s = lcg(s);
  }
}

static long long two_sum_search(const int *a, long long n, long long target) {
  long long lo = 0, hi = n - 1;
  while (lo < hi) {
    long long sum2 = (long long)a[lo] + a[hi];
    if (sum2 == target)
      return lo * n + hi;
    if (sum2 < target)
      lo++;
    else
      hi--;
  }
  return -1;
}

static long long run_queries(const int *a, long long n, int qcount) {
  long long s = 7777, checksum = 0;
  for (int q = 0; q < qcount; q++) {
    s = lcg(s);
    long long i1 = s % n;
    s = lcg(s);
    long long i2 = s % n;
    s = lcg(s);
    long long t = (long long)a[i1] + a[i2] + s % 3;
    checksum += two_sum_search(a, n, t);
  }
  return checksum;
}

int main(void) {
  int *a = malloc(N * sizeof(int));
  if (!a)
    return 1;
  fill_sorted(a, N);
  printf("checksum=%lld\n", run_queries(a, N, QUERIES));
  free(a);
  return 0;
}
