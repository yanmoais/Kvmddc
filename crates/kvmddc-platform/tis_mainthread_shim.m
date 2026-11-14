#import <Carbon/Carbon.h>
#import <CoreGraphics/CoreGraphics.h>
#import <Foundation/Foundation.h>
#include <dispatch/dispatch.h>
#include <errno.h>
#include <netinet/in.h>
#include <netinet/tcp.h>
#include <os/lock.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

#ifndef SO_NET_SERVICE_TYPE
#define SO_NET_SERVICE_TYPE 0x1116
#endif
#ifndef NET_SERVICE_TYPE_VO
#define NET_SERVICE_TYPE_VO 6
#endif

static atomic_bool g_main_ready = false;
static id g_latency_activity = nil;

static os_unfair_lock g_cache_lock = OS_UNFAIR_LOCK_INIT;
static uint64_t g_last_cache_ns = 0;
static bool g_cached_confined = false;
static bool g_cached_has_builtin = false;
static CGDirectDisplayID g_cached_builtin_id = 0;
static CGRect g_cached_builtin_bounds = {{0.0, 0.0}, {0.0, 0.0}};

__attribute__((constructor))
static void init_tis_shim(void) {
    @autoreleasepool {
        // Prevent macOS App Nap & timer coalescing on headless/lid-closed deskflow-core
        // while still allowing normal idle system sleep.
        NSActivityOptions opts =
            NSActivityUserInitiatedAllowingIdleSystemSleep | NSActivityLatencyCritical;
        g_latency_activity = [[[NSProcessInfo processInfo]
            beginActivityWithOptions:opts
                              reason:@"Kvmddc low-latency HID stream"] retain];
    }
    (void)pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0);
    dispatch_async(dispatch_get_main_queue(), ^{
        (void)pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0);
        atomic_store(&g_main_ready, true);
    });
}

static bool compute_confined_flag_raw(void) {
    const char *flag_file = getenv("KVMDDC_CONFINED_BUILTIN_FLAG_FILE");
    if (flag_file != NULL && flag_file[0] != '\0') {
        return access(flag_file, F_OK) == 0;
    }
    const char *v = getenv("KVMDDC_CONFINED_BUILTIN_ONLY");
    return v != NULL && strcmp(v, "1") == 0;
}

static void refresh_builtin_cache_if_needed(
    bool *out_confined,
    bool *out_has_builtin,
    CGDirectDisplayID *out_id,
    CGRect *out_bounds
) {
    uint64_t now_ns = clock_gettime_nsec_np(CLOCK_UPTIME_RAW);
    os_unfair_lock_lock(&g_cache_lock);
    if (g_last_cache_ns != 0 && (now_ns - g_last_cache_ns) < 350000000ULL) {
        if (out_confined) *out_confined = g_cached_confined;
        if (out_has_builtin) *out_has_builtin = g_cached_has_builtin;
        if (out_id) *out_id = g_cached_builtin_id;
        if (out_bounds) *out_bounds = g_cached_builtin_bounds;
        os_unfair_lock_unlock(&g_cache_lock);
        return;
    }
    os_unfair_lock_unlock(&g_cache_lock);

    bool confined = compute_confined_flag_raw();
    bool has_builtin = false;
    CGDirectDisplayID builtin_id = 0;
    CGRect bounds = CGRectZero;

    if (confined) {
        CGDirectDisplayID all_ids[32];
        uint32_t total = 0;
        if (CGGetOnlineDisplayList(32, all_ids, &total) == kCGErrorSuccess && total > 0) {
            for (uint32_t i = 0; i < total; i++) {
                if (CGDisplayIsBuiltin(all_ids[i]) != 0) {
                    builtin_id = all_ids[i];
                    bounds = CGDisplayBounds(builtin_id);
                    has_builtin = true;
                    break;
                }
            }
        }
    }

    os_unfair_lock_lock(&g_cache_lock);
    g_last_cache_ns = now_ns;
    g_cached_confined = confined;
    g_cached_has_builtin = has_builtin;
    g_cached_builtin_id = builtin_id;
    g_cached_builtin_bounds = bounds;
    os_unfair_lock_unlock(&g_cache_lock);

    if (out_confined) *out_confined = confined;
    if (out_has_builtin) *out_has_builtin = has_builtin;
    if (out_id) *out_id = builtin_id;
    if (out_bounds) *out_bounds = bounds;
}

static void run_on_main_sync(dispatch_block_t block) {
    if (pthread_main_np() != 0) {
        block();
        return;
    }
    for (int i = 0; i < 500 && !atomic_load(&g_main_ready); i++) {
        usleep(1000);
    }
    if (atomic_load(&g_main_ready)) {
        dispatch_sync(dispatch_get_main_queue(), block);
    } else {
        block();
    }
}

static TISInputSourceRef shim_TISCopyCurrentKeyboardInputSource(void) {
    __block TISInputSourceRef res = NULL;
    run_on_main_sync(^{
        res = TISCopyCurrentKeyboardInputSource();
    });
    return res;
}

static TISInputSourceRef shim_TISCopyCurrentKeyboardLayoutInputSource(void) {
    __block TISInputSourceRef res = NULL;
    run_on_main_sync(^{
        res = TISCopyCurrentKeyboardLayoutInputSource();
    });
    return res;
}

static TISInputSourceRef shim_TISCopyCurrentASCIICapableKeyboardLayoutInputSource(void) {
    __block TISInputSourceRef res = NULL;
    run_on_main_sync(^{
        res = TISCopyCurrentASCIICapableKeyboardLayoutInputSource();
    });
    return res;
}

static CFArrayRef shim_TISCreateInputSourceList(CFDictionaryRef properties, Boolean includeAllInstalled) {
    __block CFArrayRef res = NULL;
    run_on_main_sync(^{
        res = TISCreateInputSourceList(properties, includeAllInstalled);
    });
    return res;
}

static void *shim_TISGetInputSourceProperty(TISInputSourceRef inputSource, CFStringRef propertyKey) {
    __block void *res = NULL;
    run_on_main_sync(^{
        res = TISGetInputSourceProperty(inputSource, propertyKey);
    });
    return res;
}

static OSStatus shim_TISSelectInputSource(TISInputSourceRef inputSource) {
    __block OSStatus res = noErr;
    run_on_main_sync(^{
        res = TISSelectInputSource(inputSource);
    });
    return res;
}

static UInt8 shim_LMGetKbdType(void) {
    __block UInt8 res = 0;
    run_on_main_sync(^{
        res = LMGetKbdType();
    });
    return res;
}

static CGError shim_CGGetActiveDisplayList(
    uint32_t maxDisplays,
    CGDirectDisplayID *activeDisplays,
    uint32_t *displayCount
) {
    bool confined = false;
    refresh_builtin_cache_if_needed(&confined, NULL, NULL, NULL);
    if (!confined) {
        return CGGetActiveDisplayList(maxDisplays, activeDisplays, displayCount);
    }

    CGDirectDisplayID all_ids[32];
    uint32_t total = 0;
    CGError rc = CGGetActiveDisplayList(32, all_ids, &total);
    if (rc != kCGErrorSuccess || total == 0) {
        return CGGetActiveDisplayList(maxDisplays, activeDisplays, displayCount);
    }

    CGDirectDisplayID builtins[32];
    uint32_t n_builtin = 0;
    for (uint32_t i = 0; i < total; i++) {
        if (CGDisplayIsBuiltin(all_ids[i]) != 0) {
            builtins[n_builtin++] = all_ids[i];
        }
    }
    if (n_builtin == 0) {
        return CGGetActiveDisplayList(maxDisplays, activeDisplays, displayCount);
    }

    if (activeDisplays == NULL || maxDisplays == 0) {
        if (displayCount != NULL) {
            *displayCount = n_builtin;
        }
        return kCGErrorSuccess;
    }

    uint32_t n_copy = n_builtin < maxDisplays ? n_builtin : maxDisplays;
    for (uint32_t i = 0; i < n_copy; i++) {
        activeDisplays[i] = builtins[i];
    }
    if (displayCount != NULL) {
        *displayCount = n_copy;
    }
    return kCGErrorSuccess;
}

static CGError shim_CGGetDisplaysWithPoint(
    CGPoint point,
    uint32_t maxDisplays,
    CGDirectDisplayID *displays,
    uint32_t *matchingDisplayCount
) {
    bool confined = false;
    bool has_builtin = false;
    CGDirectDisplayID builtin_id = 0;
    refresh_builtin_cache_if_needed(&confined, &has_builtin, &builtin_id, NULL);
    if (!confined || !has_builtin) {
        return CGGetDisplaysWithPoint(point, maxDisplays, displays, matchingDisplayCount);
    }

    CGDirectDisplayID all_matches[32];
    uint32_t total = 0;
    CGError rc = CGGetDisplaysWithPoint(point, 32, all_matches, &total);
    if (rc != kCGErrorSuccess) {
        return rc;
    }

    CGDirectDisplayID builtin_matches[32];
    uint32_t n_match = 0;
    for (uint32_t i = 0; i < total; i++) {
        if (all_matches[i] == builtin_id) {
            builtin_matches[n_match++] = all_matches[i];
        }
    }

    if (displays == NULL || maxDisplays == 0) {
        if (matchingDisplayCount != NULL) {
            *matchingDisplayCount = n_match;
        }
        return kCGErrorSuccess;
    }

    uint32_t n_copy = n_match < maxDisplays ? n_match : maxDisplays;
    for (uint32_t i = 0; i < n_copy; i++) {
        displays[i] = builtin_matches[i];
    }
    if (matchingDisplayCount != NULL) {
        *matchingDisplayCount = n_copy;
    }
    return kCGErrorSuccess;
}

static CGError shim_CGWarpMouseCursorPosition(CGPoint newCursorPosition) {
    bool confined = false;
    bool has_builtin = false;
    CGRect b = CGRectZero;
    refresh_builtin_cache_if_needed(&confined, &has_builtin, NULL, &b);
    if (confined && has_builtin && b.size.width > 4.0 && b.size.height > 4.0) {
        double min_x = b.origin.x;
        double max_x = b.origin.x + b.size.width - 1.0;
        double min_y = b.origin.y;
        double max_y = b.origin.y + b.size.height - 1.0;
        if (newCursorPosition.x < min_x) newCursorPosition.x = min_x;
        if (newCursorPosition.x > max_x) newCursorPosition.x = max_x;
        if (newCursorPosition.y < min_y) newCursorPosition.y = min_y;
        if (newCursorPosition.y > max_y) newCursorPosition.y = max_y;
    }
    return CGWarpMouseCursorPosition(newCursorPosition);
}

static void tune_low_latency_tcp_socket(int fd) {
    if (fd < 0) {
        return;
    }
    int saved_errno = errno;
    int sock_type = 0;
    socklen_t opt_len = sizeof(sock_type);
    if (getsockopt(fd, SOL_SOCKET, SO_TYPE, &sock_type, &opt_len) == 0 && sock_type == SOCK_STREAM) {
        struct sockaddr_storage ss;
        socklen_t ss_len = sizeof(ss);
        if (getsockname(fd, (struct sockaddr *)&ss, &ss_len) == 0) {
            if (ss.ss_family == AF_INET || ss.ss_family == AF_INET6) {
                int one = 1;
                (void)setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &one, sizeof(one));
                int vo = NET_SERVICE_TYPE_VO;
                (void)setsockopt(fd, SOL_SOCKET, SO_NET_SERVICE_TYPE, &vo, sizeof(vo));
            }
        }
    }
    errno = saved_errno;
}

static int shim_socket(int domain, int type, int protocol) {
    int fd = socket(domain, type, protocol);
    if (fd >= 0 && (domain == AF_INET || domain == AF_INET6) && (type & SOCK_STREAM) != 0) {
        int saved_errno = errno;
        int one = 1;
        (void)setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &one, sizeof(one));
        int vo = NET_SERVICE_TYPE_VO;
        (void)setsockopt(fd, SOL_SOCKET, SO_NET_SERVICE_TYPE, &vo, sizeof(vo));
        errno = saved_errno;
    }
    return fd;
}

static int shim_setsockopt(
    int socket_fd,
    int level,
    int option_name,
    const void *option_value,
    socklen_t option_len
) {
    if (level == IPPROTO_TCP && option_name == TCP_NODELAY && option_value != NULL
        && option_len >= (socklen_t)sizeof(int)) {
        int val = *(const int *)option_value;
        if (val == 0) {
            int one = 1;
            return setsockopt(socket_fd, level, option_name, &one, sizeof(one));
        }
    }
    return setsockopt(socket_fd, level, option_name, option_value, option_len);
}

static int shim_accept(int socket, struct sockaddr *address, socklen_t *address_len) {
    int fd = accept(socket, address, address_len);
    if (fd >= 0) {
        tune_low_latency_tcp_socket(fd);
    }
    return fd;
}

static int shim_connect(int socket, const struct sockaddr *address, socklen_t address_len) {
    if (socket >= 0 && address != NULL && (address->sa_family == AF_INET || address->sa_family == AF_INET6)) {
        int saved_errno = errno;
        int sock_type = 0;
        socklen_t opt_len = sizeof(sock_type);
        if (getsockopt(socket, SOL_SOCKET, SO_TYPE, &sock_type, &opt_len) == 0 && sock_type == SOCK_STREAM) {
            int one = 1;
            (void)setsockopt(socket, IPPROTO_TCP, TCP_NODELAY, &one, sizeof(one));
            int vo = NET_SERVICE_TYPE_VO;
            (void)setsockopt(socket, SOL_SOCKET, SO_NET_SERVICE_TYPE, &vo, sizeof(vo));
        }
        errno = saved_errno;
    }
    return connect(socket, address, address_len);
}

typedef struct {
    const void *replacement;
    const void *replacee;
} dyld_interpose_tuple;

__attribute__((used, section("__DATA,__interpose")))
static const dyld_interpose_tuple interposers[] = {
    { (const void *)shim_TISCopyCurrentKeyboardInputSource, (const void *)TISCopyCurrentKeyboardInputSource },
    { (const void *)shim_TISCopyCurrentKeyboardLayoutInputSource, (const void *)TISCopyCurrentKeyboardLayoutInputSource },
    { (const void *)shim_TISCopyCurrentASCIICapableKeyboardLayoutInputSource, (const void *)TISCopyCurrentASCIICapableKeyboardLayoutInputSource },
    { (const void *)shim_TISCreateInputSourceList, (const void *)TISCreateInputSourceList },
    { (const void *)shim_TISGetInputSourceProperty, (const void *)TISGetInputSourceProperty },
    { (const void *)shim_TISSelectInputSource, (const void *)TISSelectInputSource },
    { (const void *)shim_LMGetKbdType, (const void *)LMGetKbdType },
    { (const void *)shim_CGGetActiveDisplayList, (const void *)CGGetActiveDisplayList },
    { (const void *)shim_CGGetDisplaysWithPoint, (const void *)CGGetDisplaysWithPoint },
    { (const void *)shim_CGWarpMouseCursorPosition, (const void *)CGWarpMouseCursorPosition },
    { (const void *)shim_socket, (const void *)socket },
    { (const void *)shim_setsockopt, (const void *)setsockopt },
    { (const void *)shim_accept, (const void *)accept },
    { (const void *)shim_connect, (const void *)connect },
};
