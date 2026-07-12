// =============================================================================
// Salt HTTP Server Bridge — Minimal C bridge for TCP + kqueue
// =============================================================================
// Provides the syscall-level operations that Salt's HTTP server needs.
// Each function is extern'd from Salt and called via the standard ABI.
//
// Functions:
//   http_tcp_listen(port) → listen_fd
//   http_kq_create() → kq_fd
//   http_kq_register(kq, fd, filter) → 0/-1
//   http_kq_wait(kq, events_out, max_events, timeout_ms) → n_events
//   http_accept(listen_fd) → client_fd
//   http_recv(fd, buf, len) → bytes_read
//   http_send(fd, buf, len) → bytes_sent
//   http_close(fd) → 0/-1
//   clock_gettime_ns() → nanoseconds
// =============================================================================

#include <errno.h>
#include <fcntl.h>
#include <mach/mach_time.h>
#include <netinet/in.h>
#include <netinet/tcp.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/event.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <time.h>
#include <unistd.h>

// --- TCP Operations ---

int32_t http_tcp_listen(int32_t port) {
  int fd = socket(AF_INET, SOCK_STREAM, 0);
  if (fd < 0)
    return -1;

  int opt = 1;
  setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt));
  setsockopt(fd, SOL_SOCKET, SO_REUSEPORT, &opt, sizeof(opt));

  // Disable Nagle for low-latency responses
  setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &opt, sizeof(opt));

  struct sockaddr_in addr;
  memset(&addr, 0, sizeof(addr));
  addr.sin_family = AF_INET;
  addr.sin_addr.s_addr = INADDR_ANY;
  addr.sin_port = htons(port);

  if (bind(fd, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
    close(fd);
    return -1;
  }
  if (listen(fd, 4096) < 0) {
    close(fd);
    return -1;
  }

  // Set non-blocking
  int flags = fcntl(fd, F_GETFL, 0);
  fcntl(fd, F_SETFL, flags | O_NONBLOCK);

  return fd;
}

int32_t http_accept(int32_t listen_fd) {
  int fd = accept(listen_fd, NULL, NULL);
  if (fd < 0)
    return -1;

  // Set non-blocking + TCP_NODELAY on accepted socket
  int flags = fcntl(fd, F_GETFL, 0);
  fcntl(fd, F_SETFL, flags | O_NONBLOCK);
  int opt = 1;
  setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &opt, sizeof(opt));

  return fd;
}

int64_t http_recv(int32_t fd, void *buf, int64_t len) {
  ssize_t n = read(fd, buf, (size_t)len);
  return (int64_t)n;
}

int64_t http_send(int32_t fd, const void *buf, int64_t len) {
  ssize_t n = write(fd, buf, (size_t)len);
  return (int64_t)n;
}

int32_t http_close(int32_t fd) { return close(fd); }

// --- kqueue Operations ---

int32_t http_kq_create() { return kqueue(); }

// filter: 0 = EVFILT_READ, 1 = EVFILT_WRITE
int32_t http_kq_register(int32_t kq, int32_t fd, int32_t filter) {
  struct kevent ev;
  int16_t evfilt = (filter == 0) ? EVFILT_READ : EVFILT_WRITE;
  EV_SET(&ev, fd, evfilt, EV_ADD | EV_ENABLE, 0, 0, NULL);
  return kevent(kq, &ev, 1, NULL, 0, NULL);
}

int32_t http_kq_deregister(int32_t kq, int32_t fd) {
  struct kevent ev;
  EV_SET(&ev, fd, EVFILT_READ, EV_DELETE, 0, 0, NULL);
  kevent(kq, &ev, 1, NULL, 0, NULL);
  EV_SET(&ev, fd, EVFILT_WRITE, EV_DELETE, 0, 0, NULL);
  return kevent(kq, &ev, 1, NULL, 0, NULL);
}

// Returns number of ready events. Each event is stored as:
//   events_out[i*2] = fd (ident)
//   events_out[i*2+1] = filter (0=read, 1=write)
int32_t http_kq_wait(int32_t kq, int64_t *events_out, int32_t max_events,
                     int32_t timeout_ms) {
  struct kevent events[64];
  int n = max_events;
  if (n > 64)
    n = 64;

  struct timespec ts;
  struct timespec *tsp = NULL;
  if (timeout_ms >= 0) {
    ts.tv_sec = timeout_ms / 1000;
    ts.tv_nsec = (timeout_ms % 1000) * 1000000L;
    tsp = &ts;
  }

  int ready = kevent(kq, NULL, 0, events, n, tsp);
  if (ready < 0)
    return -1;

  for (int i = 0; i < ready; i++) {
    events_out[i * 2] = (int64_t)events[i].ident;
    events_out[i * 2 + 1] = (events[i].filter == EVFILT_READ) ? 0 : 1;
  }

  return ready;
}

// --- Timing ---

int64_t clock_gettime_ns() {
  static mach_timebase_info_data_t info = {0};
  if (info.denom == 0)
    mach_timebase_info(&info);
  uint64_t ticks = mach_absolute_time();
  return (int64_t)(ticks * info.numer / info.denom);
}

// --- TCP Client (outbound connect) ---
#include <netdb.h>

// Connect to host:port (blocking). Returns fd or -1 on error.
int32_t http_tcp_connect(const char *host, int32_t port) {
  struct addrinfo hints, *result;
  memset(&hints, 0, sizeof(hints));
  hints.ai_family = AF_INET;
  hints.ai_socktype = SOCK_STREAM;

  char port_str[8];
  snprintf(port_str, sizeof(port_str), "%d", port);

  int ret = getaddrinfo(host, port_str, &hints, &result);
  if (ret != 0)
    return -1;

  int fd = socket(result->ai_family, result->ai_socktype, result->ai_protocol);
  if (fd < 0) {
    freeaddrinfo(result);
    return -1;
  }

  if (connect(fd, result->ai_addr, result->ai_addrlen) < 0) {
    close(fd);
    freeaddrinfo(result);
    return -1;
  }
  freeaddrinfo(result);

  // Disable Nagle
  int opt = 1;
  setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &opt, sizeof(opt));
  return fd;
}

// High-level HTTP GET: connect, send request, read response into buffer.
// Returns bytes read, or -1 on error.
int64_t salt_http_get(const char *host, int32_t port, const char *path,
                      char *out_buf, int64_t buf_size) {
  int fd = http_tcp_connect(host, port);
  if (fd < 0)
    return -1;

  // Build GET request
  char req[1024];
  int req_len = snprintf(
      req, sizeof(req),
      "GET %s HTTP/1.1\r\nHost: %s\r\nConnection: close\r\n\r\n", path, host);

  // Send
  ssize_t sent = write(fd, req, req_len);
  if (sent < 0) {
    close(fd);
    return -1;
  }

  // Receive
  int64_t total = 0;
  while (total < buf_size - 1) {
    ssize_t n = read(fd, out_buf + total, buf_size - total - 1);
    if (n <= 0)
      break;
    total += n;
  }
  out_buf[total] = '\0';
  close(fd);
  return total;
}
