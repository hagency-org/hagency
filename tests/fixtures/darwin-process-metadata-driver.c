#ifndef __APPLE__
#define _POSIX_C_SOURCE 200809L
#endif

#include <errno.h>
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#ifdef __APPLE__
#include <libproc.h>
#include <sys/proc_info.h>
#include <sys/sysctl.h>

static const char *test_fault;
static int test_bsd_calls;
static int test_clock_calls;

static int __attribute__((unused)) metadata_test_sysctl(
    int *name, u_int namelen, void *oldp, size_t *oldlenp, void *newp, size_t newlen);
static int __attribute__((unused)) metadata_test_proc_pidinfo(
    int pid, int flavor, uint64_t arg, void *buffer, int buffersize);
static int __attribute__((unused)) metadata_test_clock_gettime(
    clockid_t clock_id, struct timespec *value);

#define sysctl metadata_test_sysctl
#define proc_pidinfo metadata_test_proc_pidinfo
#define clock_gettime metadata_test_clock_gettime
#endif

#define main darwin_process_metadata_main
#include "../../skills/hagency-inner-loop/native/darwin-process-metadata.c"
#undef main

#ifdef __APPLE__
#undef sysctl
#undef proc_pidinfo
#undef clock_gettime

static size_t make_mock_arguments(unsigned char *buffer, size_t capacity) {
    const char executable[] = "/mock/bin";
    const char argv0[] = "mock argv0";
    const char argv1[] = "value";
    const int count = 2;
    const size_t executable_size = sizeof(executable);
    const size_t padding = (8U - ((16U + executable_size) % 8U)) % 8U;
    const size_t needed = sizeof(count) + executable_size + padding +
                          sizeof(argv0) + sizeof(argv1);
    size_t offset = 0;
    if (capacity < needed) return 0;
    memcpy(buffer + offset, &count, sizeof(count));
    offset += sizeof(count);
    memcpy(buffer + offset, executable, executable_size);
    offset += executable_size;
    memset(buffer + offset, 0, padding);
    offset += padding;
    memcpy(buffer + offset, argv0, sizeof(argv0));
    offset += sizeof(argv0);
    memcpy(buffer + offset, argv1, sizeof(argv1));
    offset += sizeof(argv1);
    return offset;
}

static int metadata_test_sysctl(int *name, u_int namelen, void *oldp,
                                size_t *oldlenp, void *newp, size_t newlen) {
    unsigned char raw[128];
    size_t raw_size;
    (void)name;
    (void)namelen;
    (void)newp;
    (void)newlen;
    raw_size = make_mock_arguments(raw, sizeof(raw));
    if (oldp == NULL) {
        if (strcmp(test_fault, "sizing-error") == 0 ||
            strcmp(test_fault, "budget-after-sizing-error") == 0) return -1;
        *oldlenp = strcmp(test_fault, "sizing-oversize") == 0
            ? (4U * 1024U * 1024U) + 1U
            : raw_size;
        return 0;
    }
    if (strcmp(test_fault, "fetch-error") == 0) return -1;
    if (strcmp(test_fault, "fetch-short") == 0) {
        *oldlenp = 3;
        return 0;
    }
    if (strcmp(test_fault, "fetch-oversize") == 0) {
        *oldlenp = (4U * 1024U * 1024U) + 1U;
        return 0;
    }
    if (*oldlenp < raw_size) return -1;
    memcpy(oldp, raw, raw_size);
    *oldlenp = raw_size;
    return 0;
}

static int metadata_test_proc_pidinfo(int pid, int flavor, uint64_t arg,
                                      void *buffer, int buffersize) {
    (void)arg;
    if (flavor == PROC_PIDTBSDINFO) {
        struct proc_bsdinfo *info = buffer;
        ++test_bsd_calls;
        if ((strcmp(test_fault, "bsd-short") == 0 && test_bsd_calls == 1) ||
            (strcmp(test_fault, "bsd-after-short") == 0 && test_bsd_calls == 2)) {
            return buffersize - 1;
        }
        memset(info, 0, sizeof(*info));
        info->pbi_pid = ((strcmp(test_fault, "bsd-pid") == 0 && test_bsd_calls == 1) ||
                         (strcmp(test_fault, "bsd-after-pid") == 0 && test_bsd_calls == 2))
            ? (uint32_t)(pid + 1)
            : (uint32_t)pid;
        info->pbi_flags = PROC_FLAG_LP64;
        if (strcmp(test_fault, "bsd-lp64-change") == 0 && test_bsd_calls == 2) {
            info->pbi_flags = 0;
        }
        info->pbi_start_tvsec = 100;
        info->pbi_start_tvusec = 200;
        if (strcmp(test_fault, "bsd-birth-change") == 0 && test_bsd_calls == 2) {
            info->pbi_start_tvusec = 201;
        }
        return (int)sizeof(*info);
    }
    if (flavor == PROC_PIDVNODEPATHINFO) {
        struct proc_vnodepathinfo *info = buffer;
        if (strcmp(test_fault, "cwd-error") == 0) return -1;
        if (strcmp(test_fault, "cwd-short") == 0) return buffersize - 1;
        memset(info, 0, sizeof(*info));
        if (strcmp(test_fault, "cwd-no-nul") == 0) {
            memset(info->pvi_cdir.vip_path, 'x', sizeof(info->pvi_cdir.vip_path));
            info->pvi_cdir.vip_path[0] = '/';
        } else if (strcmp(test_fault, "cwd-relative") == 0) {
            memcpy(info->pvi_cdir.vip_path, "relative", sizeof("relative"));
        } else if (strcmp(test_fault, "cwd-invalid-utf8") == 0) {
            const unsigned char invalid[] = { '/', 0xed, 0xa0, 0x80, 0 };
            memcpy(info->pvi_cdir.vip_path, invalid, sizeof(invalid));
        } else {
            memcpy(info->pvi_cdir.vip_path, "/mock cwd", sizeof("/mock cwd"));
        }
        return (int)sizeof(*info);
    }
    return -1;
}

static int metadata_test_clock_gettime(clockid_t clock_id, struct timespec *value) {
    (void)clock_id;
    value->tv_sec = 10;
    value->tv_nsec = 0;
    if (strcmp(test_fault, "budget") == 0 && ++test_clock_calls >= 3) {
        value->tv_sec = 12;
    } else if (strcmp(test_fault, "budget-after-sizing-error") == 0 &&
               ++test_clock_calls >= 5) {
        value->tv_sec = 12;
    }
    return 0;
}
#endif

static int drain_stdin(void) {
    unsigned char buffer[256];
    while (read(STDIN_FILENO, buffer, sizeof(buffer)) > 0) {}
    return 0;
}

static int hold_fixture(void) {
    printf("{\"pid\":%d}\n", (int)getpid());
    fflush(stdout);
    return drain_stdin();
}

static int hold_empty_argv0_child(void) {
    const char *ready_text = getenv("READY_FD");
    char *end = NULL;
    long ready_fd;
    if (ready_text == NULL) return 31;
    errno = 0;
    ready_fd = strtol(ready_text, &end, 10);
    if (errno != 0 || end == ready_text || *end != '\0' || ready_fd < 0 || ready_fd > INT_MAX) {
        return 32;
    }
    if (write((int)ready_fd, "R", 1) != 1) return 33;
    close((int)ready_fd);
    return drain_stdin();
}

static int launch_empty_argv0(const char *self, const char *sentinel) {
    int ready_pipe[2];
    pid_t child;
    char ready_environment[64];
    char sentinel_environment[256];
    char ready_byte;
    int status;
    if (pipe(ready_pipe) != 0) return 40;
    child = fork();
    if (child < 0) return 41;
    if (child == 0) {
        char *child_argv[] = { (char *)"", (char *)"--empty-hold", (char *)"AFTER_EMPTY_ARGV0", NULL };
        char *child_environment[] = { ready_environment, sentinel_environment, NULL };
        close(ready_pipe[0]);
        snprintf(ready_environment, sizeof(ready_environment), "READY_FD=%d", ready_pipe[1]);
        snprintf(sentinel_environment, sizeof(sentinel_environment), "EMPTY_ARGV0_SENTINEL=%s", sentinel);
        execve(self, child_argv, child_environment);
        _exit(42);
    }
    close(ready_pipe[1]);
    if (read(ready_pipe[0], &ready_byte, 1) != 1 || ready_byte != 'R') return 43;
    close(ready_pipe[0]);
    printf("{\"pid\":%d}\n", (int)child);
    fflush(stdout);
    if (waitpid(child, &status, 0) != child) return 44;
    return WIFEXITED(status) ? WEXITSTATUS(status) : 45;
}

static int parse_stdin(const char *width_text) {
    const size_t limit = (4U * 1024U * 1024U) + 1U;
    unsigned char *raw = malloc(limit);
    size_t length = 0;
    size_t width;
    struct arguments parsed;
    int result;
    if (raw == NULL) return 50;
    width = (size_t)strtoul(width_text, NULL, 10);
    while (length < limit) {
        size_t got = fread(raw + length, 1, limit - length, stdin);
        length += got;
        if (got == 0) break;
    }
    result = parse_arguments(raw, length, width, &parsed);
    if (result != METADATA_OK) {
        free(raw);
        write_error(result);
        return 51;
    }
    result = emit_metadata(123, "/fixture", &parsed);
    free_arguments(&parsed);
    free(raw);
    if (result != METADATA_OK) {
        write_error(result);
        return 52;
    }
    return 0;
}

int main(int argc, char **argv) {
    if (argc >= 2 && strcmp(argv[1], "--hold") == 0) return hold_fixture();
    if (argc >= 2 && argv[0][0] == '\0' && strcmp(argv[1], "--empty-hold") == 0) {
        return hold_empty_argv0_child();
    }
    if (argc == 3 && strcmp(argv[1], "launch-empty") == 0) {
        return launch_empty_argv0(argv[0], argv[2]);
    }
    if (argc == 3 && strcmp(argv[1], "parse") == 0) return parse_stdin(argv[2]);
#ifdef __APPLE__
    if (argc == 3 && strcmp(argv[1], "mock") == 0) {
        char *collector_argv[] = { (char *)"darwin-process-metadata", (char *)"--pid", (char *)"321", NULL };
        test_fault = argv[2];
        test_bsd_calls = 0;
        test_clock_calls = 0;
        return darwin_process_metadata_main(3, collector_argv);
    }
#endif
    return 64;
}
