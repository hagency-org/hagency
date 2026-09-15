#include <errno.h>
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/types.h>
#include <time.h>

#ifdef __APPLE__
#include <libproc.h>
#include <sys/proc_info.h>
#include <sys/sysctl.h>
#endif

#define ARGUMENT_DATA_CAP (4U * 1024U * 1024U)
#define ARGUMENT_COUNT_CAP 16384
#define XNU_EXEC_PATH_PREFIX_SIZE 16U
#define BUDGET_SECONDS 1
#define BUDGET_NANOSECONDS 500000000L

enum metadata_error {
    METADATA_OK = 0,
    METADATA_INVALID_COMMAND,
    METADATA_UNSUPPORTED_PLATFORM,
    METADATA_ARGUMENT_DATA,
    METADATA_PROCESS_ARGUMENTS,
    METADATA_CWD,
    METADATA_PROCESS_IDENTITY,
    METADATA_DEADLINE_EXCEEDED,
    METADATA_OUTPUT
};

struct arguments {
    size_t count;
    char **values;
};

struct output_buffer {
    char *bytes;
    size_t length;
    size_t capacity;
};

_Static_assert(sizeof(int) == 4, "KERN_PROCARGS2 argc must be four bytes");

static int valid_utf8(const unsigned char *value, size_t length) {
    size_t offset = 0;
    while (offset < length) {
        unsigned char first = value[offset];
        if (first <= 0x7fU) {
            ++offset;
        } else if (first >= 0xc2U && first <= 0xdfU) {
            if (offset + 1 >= length || value[offset + 1] < 0x80U ||
                value[offset + 1] > 0xbfU) return 0;
            offset += 2;
        } else if (first == 0xe0U) {
            if (offset + 2 >= length || value[offset + 1] < 0xa0U ||
                value[offset + 1] > 0xbfU || value[offset + 2] < 0x80U ||
                value[offset + 2] > 0xbfU) return 0;
            offset += 3;
        } else if ((first >= 0xe1U && first <= 0xecU) ||
                   (first >= 0xeeU && first <= 0xefU)) {
            if (offset + 2 >= length || value[offset + 1] < 0x80U ||
                value[offset + 1] > 0xbfU || value[offset + 2] < 0x80U ||
                value[offset + 2] > 0xbfU) return 0;
            offset += 3;
        } else if (first == 0xedU) {
            if (offset + 2 >= length || value[offset + 1] < 0x80U ||
                value[offset + 1] > 0x9fU || value[offset + 2] < 0x80U ||
                value[offset + 2] > 0xbfU) return 0;
            offset += 3;
        } else if (first == 0xf0U) {
            if (offset + 3 >= length || value[offset + 1] < 0x90U ||
                value[offset + 1] > 0xbfU || value[offset + 2] < 0x80U ||
                value[offset + 2] > 0xbfU || value[offset + 3] < 0x80U ||
                value[offset + 3] > 0xbfU) return 0;
            offset += 4;
        } else if (first >= 0xf1U && first <= 0xf3U) {
            if (offset + 3 >= length || value[offset + 1] < 0x80U ||
                value[offset + 1] > 0xbfU || value[offset + 2] < 0x80U ||
                value[offset + 2] > 0xbfU || value[offset + 3] < 0x80U ||
                value[offset + 3] > 0xbfU) return 0;
            offset += 4;
        } else if (first == 0xf4U) {
            if (offset + 3 >= length || value[offset + 1] < 0x80U ||
                value[offset + 1] > 0x8fU || value[offset + 2] < 0x80U ||
                value[offset + 2] > 0xbfU || value[offset + 3] < 0x80U ||
                value[offset + 3] > 0xbfU) return 0;
            offset += 4;
        } else {
            return 0;
        }
    }
    return 1;
}

static int parse_arguments(const unsigned char *raw, size_t length,
                           size_t pointer_width, struct arguments *out) {
    int native_count;
    size_t offset;
    const unsigned char *terminator;
    size_t executable_size;
    size_t padding;
    size_t index;
    char **values;

    if (out == NULL) return METADATA_ARGUMENT_DATA;
    out->count = 0;
    out->values = NULL;
    if (raw == NULL || length > ARGUMENT_DATA_CAP || length <= sizeof(native_count) ||
        (pointer_width != 4U && pointer_width != 8U)) {
        return METADATA_ARGUMENT_DATA;
    }
    memcpy(&native_count, raw, sizeof(native_count));
    if (native_count < 1 || native_count > ARGUMENT_COUNT_CAP) {
        return METADATA_ARGUMENT_DATA;
    }

    offset = sizeof(native_count);
    terminator = memchr(raw + offset, 0, length - offset);
    if (terminator == NULL || terminator == raw + offset ||
        !valid_utf8(raw + offset, (size_t)(terminator - (raw + offset)))) {
        return METADATA_ARGUMENT_DATA;
    }
    executable_size = (size_t)(terminator - (raw + offset)) + 1U;
    offset += executable_size;
    padding = (pointer_width -
               ((XNU_EXEC_PATH_PREFIX_SIZE + executable_size) % pointer_width)) %
              pointer_width;
    if (padding > length - offset) return METADATA_ARGUMENT_DATA;
    for (index = 0; index < padding; ++index) {
        if (raw[offset + index] != 0) return METADATA_ARGUMENT_DATA;
    }
    offset += padding;
    if (offset >= length || raw[offset] == 0) return METADATA_ARGUMENT_DATA;

    values = calloc((size_t)native_count, sizeof(*values));
    if (values == NULL) return METADATA_ARGUMENT_DATA;
    for (index = 0; index < (size_t)native_count; ++index) {
        size_t token_size;
        terminator = memchr(raw + offset, 0, length - offset);
        if (terminator == NULL) {
            free(values);
            return METADATA_ARGUMENT_DATA;
        }
        token_size = (size_t)(terminator - (raw + offset));
        if ((index == 0 && token_size == 0) ||
            !valid_utf8(raw + offset, token_size)) {
            free(values);
            return METADATA_ARGUMENT_DATA;
        }
        values[index] = (char *)(raw + offset);
        offset += token_size + 1U;
    }
    out->count = (size_t)native_count;
    out->values = values;
    return METADATA_OK;
}

static void free_arguments(struct arguments *arguments) {
    if (arguments == NULL) return;
    free(arguments->values);
    arguments->values = NULL;
    arguments->count = 0;
}

static int reserve_output(struct output_buffer *output, size_t extra) {
    size_t required;
    size_t next_capacity;
    char *next;
    if (extra > SIZE_MAX - output->length) return METADATA_OUTPUT;
    required = output->length + extra;
    if (required <= output->capacity) return METADATA_OK;
    next_capacity = output->capacity == 0 ? 256U : output->capacity;
    while (next_capacity < required) {
        if (next_capacity > SIZE_MAX / 2U) {
            next_capacity = required;
            break;
        }
        next_capacity *= 2U;
    }
    next = realloc(output->bytes, next_capacity);
    if (next == NULL) return METADATA_OUTPUT;
    output->bytes = next;
    output->capacity = next_capacity;
    return METADATA_OK;
}

static int append_output(struct output_buffer *output, const char *value,
                         size_t length) {
    int result = reserve_output(output, length);
    if (result != METADATA_OK) return result;
    memcpy(output->bytes + output->length, value, length);
    output->length += length;
    return METADATA_OK;
}

static int append_json_string(struct output_buffer *output, const char *value) {
    static const char hex[] = "0123456789abcdef";
    const unsigned char *bytes = (const unsigned char *)value;
    size_t length = strlen(value);
    size_t offset;
    int result;
    if (!valid_utf8(bytes, length)) return METADATA_OUTPUT;
    result = append_output(output, "\"", 1);
    if (result != METADATA_OK) return result;
    for (offset = 0; offset < length; ++offset) {
        unsigned char byte = bytes[offset];
        char escaped[6];
        if (byte == '"' || byte == '\\') {
            escaped[0] = '\\';
            escaped[1] = (char)byte;
            result = append_output(output, escaped, 2);
        } else if (byte == '\b' || byte == '\f' || byte == '\n' ||
                   byte == '\r' || byte == '\t') {
            escaped[0] = '\\';
            escaped[1] = byte == '\b' ? 'b' : byte == '\f' ? 'f' :
                         byte == '\n' ? 'n' : byte == '\r' ? 'r' : 't';
            result = append_output(output, escaped, 2);
        } else if (byte < 0x20U) {
            escaped[0] = '\\';
            escaped[1] = 'u';
            escaped[2] = '0';
            escaped[3] = '0';
            escaped[4] = hex[byte >> 4];
            escaped[5] = hex[byte & 0x0fU];
            result = append_output(output, escaped, sizeof(escaped));
        } else {
            result = append_output(output, (const char *)&bytes[offset], 1);
        }
        if (result != METADATA_OK) return result;
    }
    return append_output(output, "\"", 1);
}

static int emit_metadata(pid_t pid, const char *cwd,
                         const struct arguments *arguments) {
    struct output_buffer output = { NULL, 0, 0 };
    char pid_text[32];
    int pid_length;
    size_t index;
    int result;
    if (cwd == NULL || arguments == NULL || arguments->count < 1 ||
        arguments->values == NULL) return METADATA_OUTPUT;
    pid_length = snprintf(pid_text, sizeof(pid_text), "%d", (int)pid);
    if (pid_length <= 0 || (size_t)pid_length >= sizeof(pid_text)) return METADATA_OUTPUT;
    result = append_output(&output, "{\"version\":1,\"pid\":", 19);
    if (result == METADATA_OK) {
        result = append_output(&output, pid_text, (size_t)pid_length);
    }
    if (result == METADATA_OK) result = append_output(&output, ",\"cwd\":", 7);
    if (result == METADATA_OK) result = append_json_string(&output, cwd);
    if (result == METADATA_OK) result = append_output(&output, ",\"argv\":[", 9);
    for (index = 0; result == METADATA_OK && index < arguments->count; ++index) {
        if (index != 0) result = append_output(&output, ",", 1);
        if (result == METADATA_OK) {
            result = append_json_string(&output, arguments->values[index]);
        }
    }
    if (result == METADATA_OK) result = append_output(&output, "]}\n", 3);
    if (result == METADATA_OK &&
        (fwrite(output.bytes, output.length, 1, stdout) != 1 || fflush(stdout) != 0)) {
        result = METADATA_OUTPUT;
    }
    free(output.bytes);
    return result;
}

static const char *error_name(int category) {
    switch (category) {
        case METADATA_INVALID_COMMAND: return "invalid_command";
        case METADATA_UNSUPPORTED_PLATFORM: return "unsupported_platform";
        case METADATA_ARGUMENT_DATA: return "argument_data";
        case METADATA_PROCESS_ARGUMENTS: return "process_arguments";
        case METADATA_CWD: return "cwd";
        case METADATA_PROCESS_IDENTITY: return "process_identity";
        case METADATA_DEADLINE_EXCEEDED: return "deadline_exceeded";
        case METADATA_OUTPUT: return "output";
        default: return "internal";
    }
}

static void write_error(int category) {
    const char *name = error_name(category);
    (void)fprintf(stderr, "{\"category\":\"%s\"}\n", name);
}

static int parse_pid(int argc, char **argv, pid_t *pid) {
    const char *text;
    unsigned long value = 0;
    size_t offset;
    if (argc != 3 || strcmp(argv[1], "--pid") != 0) return 0;
    text = argv[2];
    if (text[0] == '\0' || (text[0] == '0' && text[1] != '\0')) return 0;
    for (offset = 0; text[offset] != '\0'; ++offset) {
        unsigned int digit;
        if (text[offset] < '0' || text[offset] > '9') return 0;
        digit = (unsigned int)(text[offset] - '0');
        if (value > ((unsigned long)INT_MAX - digit) / 10U) return 0;
        value = value * 10U + digit;
    }
    if (value == 0 || value > (unsigned long)INT_MAX) return 0;
    *pid = (pid_t)value;
    return 1;
}

#ifdef __APPLE__
static int within_budget(const struct timespec *started) {
    struct timespec now;
    time_t seconds;
    long nanoseconds;
    if (clock_gettime(CLOCK_MONOTONIC, &now) != 0) return 0;
    if (now.tv_sec < started->tv_sec ||
        (now.tv_sec == started->tv_sec && now.tv_nsec < started->tv_nsec)) return 0;
    seconds = now.tv_sec - started->tv_sec;
    nanoseconds = now.tv_nsec - started->tv_nsec;
    if (nanoseconds < 0) {
        --seconds;
        nanoseconds += 1000000000L;
    }
    return seconds < BUDGET_SECONDS ||
           (seconds == BUDGET_SECONDS && nanoseconds <= BUDGET_NANOSECONDS);
}

static int read_bsd_info(pid_t pid, const struct timespec *started,
                         struct proc_bsdinfo *info) {
    int received;
    if (!within_budget(started)) return METADATA_DEADLINE_EXCEEDED;
    received = proc_pidinfo((int)pid, PROC_PIDTBSDINFO, 0, info, (int)sizeof(*info));
    if (!within_budget(started)) return METADATA_DEADLINE_EXCEEDED;
    if (received != (int)sizeof(*info) || info->pbi_pid != (uint32_t)pid) {
        return METADATA_PROCESS_IDENTITY;
    }
    return METADATA_OK;
}

static int read_process_arguments(pid_t pid, const struct timespec *started,
                                  unsigned char **raw, size_t *length) {
    int mib[3] = { CTL_KERN, KERN_PROCARGS2, (int)pid };
    size_t sized = 0;
    size_t fetched;
    unsigned char *bytes;
    int syscall_result;
    if (!within_budget(started)) return METADATA_DEADLINE_EXCEEDED;
    syscall_result = sysctl(mib, 3, NULL, &sized, NULL, 0);
    if (!within_budget(started)) return METADATA_DEADLINE_EXCEEDED;
    if (syscall_result != 0) return METADATA_PROCESS_ARGUMENTS;
    if (sized <= sizeof(int) || sized > ARGUMENT_DATA_CAP) {
        return METADATA_PROCESS_ARGUMENTS;
    }
    bytes = malloc(sized);
    if (bytes == NULL) return METADATA_PROCESS_ARGUMENTS;
    fetched = sized;
    if (!within_budget(started)) {
        free(bytes);
        return METADATA_DEADLINE_EXCEEDED;
    }
    syscall_result = sysctl(mib, 3, bytes, &fetched, NULL, 0);
    if (!within_budget(started)) {
        free(bytes);
        return METADATA_DEADLINE_EXCEEDED;
    }
    if (syscall_result != 0) {
        free(bytes);
        return METADATA_PROCESS_ARGUMENTS;
    }
    if (fetched <= sizeof(int) || fetched > sized || fetched > ARGUMENT_DATA_CAP) {
        free(bytes);
        return METADATA_PROCESS_ARGUMENTS;
    }
    *raw = bytes;
    *length = fetched;
    return METADATA_OK;
}

static int read_cwd(pid_t pid, const struct timespec *started, char **cwd) {
    struct proc_vnodepathinfo path_info;
    char *terminator;
    size_t length;
    int received;
    if (!within_budget(started)) return METADATA_DEADLINE_EXCEEDED;
    received = proc_pidinfo((int)pid, PROC_PIDVNODEPATHINFO, 0,
                            &path_info, (int)sizeof(path_info));
    if (!within_budget(started)) return METADATA_DEADLINE_EXCEEDED;
    if (received != (int)sizeof(path_info)) return METADATA_CWD;
    terminator = memchr(path_info.pvi_cdir.vip_path, 0,
                        sizeof(path_info.pvi_cdir.vip_path));
    if (terminator == NULL || path_info.pvi_cdir.vip_path[0] != '/') {
        return METADATA_CWD;
    }
    length = (size_t)(terminator - path_info.pvi_cdir.vip_path);
    if (!valid_utf8((const unsigned char *)path_info.pvi_cdir.vip_path, length)) {
        return METADATA_CWD;
    }
    *cwd = malloc(length + 1U);
    if (*cwd == NULL) return METADATA_CWD;
    memcpy(*cwd, path_info.pvi_cdir.vip_path, length + 1U);
    return METADATA_OK;
}

static int collect_and_emit(pid_t pid) {
    struct timespec started;
    struct proc_bsdinfo before;
    struct proc_bsdinfo after;
    unsigned char *raw = NULL;
    size_t raw_length = 0;
    size_t pointer_width;
    struct arguments arguments = { 0, NULL };
    char *cwd = NULL;
    int result;

    if (clock_gettime(CLOCK_MONOTONIC, &started) != 0) {
        return METADATA_DEADLINE_EXCEEDED;
    }
    result = read_bsd_info(pid, &started, &before);
    if (result != METADATA_OK) goto done;
    pointer_width = (before.pbi_flags & PROC_FLAG_LP64) != 0 ? 8U : 4U;
    result = read_process_arguments(pid, &started, &raw, &raw_length);
    if (result != METADATA_OK) goto done;
    result = parse_arguments(raw, raw_length, pointer_width, &arguments);
    if (result != METADATA_OK) goto done;
    result = read_cwd(pid, &started, &cwd);
    if (result != METADATA_OK) goto done;
    result = read_bsd_info(pid, &started, &after);
    if (result != METADATA_OK) goto done;
    if (((before.pbi_flags ^ after.pbi_flags) & PROC_FLAG_LP64) != 0 ||
        before.pbi_start_tvsec != after.pbi_start_tvsec ||
        before.pbi_start_tvusec != after.pbi_start_tvusec) {
        result = METADATA_PROCESS_IDENTITY;
        goto done;
    }
    result = emit_metadata(pid, cwd, &arguments);

done:
    free(cwd);
    free_arguments(&arguments);
    free(raw);
    return result;
}
#endif

int main(int argc, char **argv) {
    pid_t pid;
    int result;
    (void)&parse_arguments;
    (void)&free_arguments;
    (void)&emit_metadata;
    if (!parse_pid(argc, argv, &pid)) {
        write_error(METADATA_INVALID_COMMAND);
        return EXIT_FAILURE;
    }
#ifdef __APPLE__
    result = collect_and_emit(pid);
#else
    (void)pid;
    result = METADATA_UNSUPPORTED_PLATFORM;
#endif
    if (result != METADATA_OK) {
        write_error(result);
        return EXIT_FAILURE;
    }
    return EXIT_SUCCESS;
}
