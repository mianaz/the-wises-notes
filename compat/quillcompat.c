// SPDX-License-Identifier: MIT
// Part of sage-notes; see LICENSE.
// quillcompat: run quill (built for reMarkable OS <= 3.2x) on OS 3.29+,
// and make its refreshes cheaper on the way.
//
// 1. ABI bridge. Older libqsgepaper exported
//      EPFramebuffer::swapBuffers(QRect, EPContentType, EPScreenMode, QFlags<UpdateFlag>)
//    OS 3.29 removed EPContentType and exports instead
//      EPFramebuffer::swapBuffers(QRect, EPScreenMode, QFlags<UpdateFlag>)
//    with a merged 0..6 screen-mode enum (decoded from
//    EPFramebufferAcep2::swapBuffers_impl):
//      0 pen, 1 mono, 2 animation, 3 ui/default, 4 content (color), 5 ?, 6 overlay
//    libquill resolves the OLD mangled name with dlsym(RTLD_DEFAULT, ...); this
//    library exports it (load with LD_PRELOAD) and forwards to the new one.
//
// 2. Damage tracking. A shadow copy of what was last pushed to the panel lets
//    every swap be cut down to the rows/columns that actually changed (merged
//    into a few bands). Unchanged swaps are dropped entirely.
//
// 3. Mode policy. Color requests go out as QUILLCOMPAT_COLOR_MODE (default 1 =
//    mono, much faster than the color waveform); full/cleaning refreshes as
//    QUILLCOMPAT_FULL_MODE (default 4 = color content).
//
// Env: QUILLCOMPAT_COLOR_MODE, QUILLCOMPAT_FULL_MODE (0-6),
//      QUILLCOMPAT_DAMAGE=0 to disable damage tracking, QUILLCOMPAT_DEBUG=1.
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

typedef struct { int x1, y1, x2, y2; } QRect;   // same layout/ABI as Qt's QRect (inclusive)
typedef void (*NewSwap)(void *self, QRect rect, int mode, unsigned flags);

#define OLD_SYM "_ZN13EPFramebuffer11swapBuffersE5QRect13EPContentType12EPScreenMode6QFlagsINS_10UpdateFlagEE"
#define NEW_SYM "_ZN13EPFramebuffer11swapBuffersE5QRect12EPScreenMode6QFlagsINS_10UpdateFlagEE"

unsigned long quillcompat_swap(void *self, QRect rect, int content_type, int mode,
                               unsigned flags) __asm__(OLD_SYM);

static int env_mode(const char *name, int dflt) {
    const char *e = getenv(name);
    return (e && *e >= '0' && *e <= '6' && !e[1]) ? *e - '0' : dflt;
}
static int env_flag(const char *name, int dflt) {
    const char *e = getenv(name);
    return e && *e ? *e != '0' : dflt;
}

static int map_mode(int content_type, int mode) {
    static int color = -1;
    if (color < 0) color = env_mode("QUILLCOMPAT_COLOR_MODE", 1);
    if (content_type == 1) return color;
    switch (mode) {
    case 0: return 0;                        // fastest mono -> pen (low latency)
    case 1: return 1;                        // fast mono    -> mono
    case 2: return 2;
    case 3: return 3;                        // quality mono -> ui/default
    default: return 4;
    }
}

// ---- framebuffer access through libquill's C ABI ----
typedef unsigned char *(*BufFn)(void);
typedef int (*IntFn)(void);
static unsigned char *fb, *shadow;
static int fbw, fbh, fbstride, damage_on = -1, dbg = -1;

static int fb_ready(void) {
    if (shadow) return 1;
    BufFn buf = (BufFn)dlsym(RTLD_DEFAULT, "quill_buffer");
    IntFn w = (IntFn)dlsym(RTLD_DEFAULT, "quill_width");
    IntFn h = (IntFn)dlsym(RTLD_DEFAULT, "quill_height");
    IntFn st = (IntFn)dlsym(RTLD_DEFAULT, "quill_stride");
    if (!buf || !w || !h || !st) return 0;
    fb = buf(); fbw = w(); fbh = h(); fbstride = st();
    if (!fb || fbw <= 0 || fbh <= 0 || fbstride < fbw * 4) return 0;
    shadow = malloc((size_t)fbh * fbstride);
    if (!shadow) return 0;
    memcpy(shadow, fb, (size_t)fbh * fbstride);
    return 1;
}

static double now_ms(void) {
    struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec * 1e3 + t.tv_nsec / 1e6;
}

#define MAX_BANDS 6
#define MERGE_GAP 48   // rows of "unchanged" allowed inside one band

typedef struct { int y0, y1, x0, x1; } Band;   // inclusive

// Find changed bands inside rect r (inclusive), update shadow for them.
static int diff_rect(QRect r, Band *bands) {
    int n = 0;
    int in_band = 0, gap = 0;
    Band cur = {0, 0, 0, 0};
    size_t xoff = (size_t)r.x1 * 4, xlen = (size_t)(r.x2 - r.x1 + 1) * 4;
    for (int y = r.y1; y <= r.y2; y++) {
        const uint32_t *a = (const uint32_t *)(fb + (size_t)y * fbstride + xoff);
        uint32_t *b = (uint32_t *)(shadow + (size_t)y * fbstride + xoff);
        if (memcmp(a, b, xlen) == 0) {
            if (in_band && ++gap > MERGE_GAP) {
                bands[n++] = cur; in_band = 0;
                if (n == MAX_BANDS) break;
            }
            continue;
        }
        int cols = r.x2 - r.x1 + 1, lo = 0, hi = cols - 1;
        while (lo < cols && a[lo] == b[lo]) lo++;
        while (hi > lo && a[hi] == b[hi]) hi--;
        memcpy(b, a, xlen);
        int x0 = r.x1 + lo, x1 = r.x1 + hi;
        if (!in_band) { cur = (Band){y, y, x0, x1}; in_band = 1; }
        else { cur.y1 = y; if (x0 < cur.x0) cur.x0 = x0; if (x1 > cur.x1) cur.x1 = x1; }
        gap = 0;
    }
    if (in_band && n < MAX_BANDS) bands[n++] = cur;
    if (n == MAX_BANDS) {
        // Too fragmented: one bounding band over the rest, shadow fully synced.
        Band all = bands[0];
        for (int i = 1; i < n; i++) {
            if (bands[i].y1 > all.y1) all.y1 = bands[i].y1;
            if (bands[i].x0 < all.x0) all.x0 = bands[i].x0;
            if (bands[i].x1 > all.x1) all.x1 = bands[i].x1;
        }
        all.y1 = r.y2; all.x0 = r.x1; all.x1 = r.x2;
        for (int y = all.y0; y <= r.y2; y++)
            memcpy(shadow + (size_t)y * fbstride + xoff, fb + (size_t)y * fbstride + xoff, xlen);
        bands[0] = all; n = 1;
    }
    return n;
}

unsigned long quillcompat_swap(void *self, QRect rect, int content_type, int mode,
                               unsigned flags) {
    static NewSwap fn;
    static int warned;
    if (!fn) fn = (NewSwap)dlsym(RTLD_DEFAULT, NEW_SYM);
    if (!fn) {
        if (!warned++) fputs("quillcompat: new swapBuffers not found\n", stderr);
        return 0;
    }
    if (damage_on < 0) damage_on = env_flag("QUILLCOMPAT_DAMAGE", 1);
    if (dbg < 0) dbg = env_flag("QUILLCOMPAT_DEBUG", 0);

    int full = flags & 1;
    if (full) {
        static int full_mode = -1;
        if (full_mode < 0) full_mode = env_mode("QUILLCOMPAT_FULL_MODE", 4);
        if (damage_on && fb_ready())
            memcpy(shadow, fb, (size_t)fbh * fbstride);
        fn(self, rect, full_mode, flags);
        if (dbg) fprintf(stderr, "quillcompat: full %dx%d mode=%d\n",
                         rect.x2 - rect.x1 + 1, rect.y2 - rect.y1 + 1, full_mode);
        return 1;
    }

    int m = map_mode(content_type, mode);
    if (!damage_on || !fb_ready()) { fn(self, rect, m, flags); return 1; }

    QRect r = rect;
    if (r.x1 < 0) r.x1 = 0;
    if (r.y1 < 0) r.y1 = 0;
    if (r.x2 >= fbw) r.x2 = fbw - 1;
    if (r.y2 >= fbh) r.y2 = fbh - 1;
    if (r.x2 < r.x1 || r.y2 < r.y1) return 1;

    double t0 = dbg ? now_ms() : 0;
    Band bands[MAX_BANDS];
    int n = diff_rect(r, bands);
    long px = 0;
    for (int i = 0; i < n; i++) {
        QRect b = {bands[i].x0, bands[i].y0, bands[i].x1, bands[i].y1};
        px += (long)(b.x2 - b.x1 + 1) * (b.y2 - b.y1 + 1);
        fn(self, b, m, flags);
    }
    if (dbg)
        fprintf(stderr, "quillcompat: req %dx%d -> %d band(s), %ld px (%.0f%%), mode=%d, diff %.1fms\n",
                r.x2 - r.x1 + 1, r.y2 - r.y1 + 1, n, px,
                100.0 * px / ((double)(r.x2 - r.x1 + 1) * (r.y2 - r.y1 + 1)), m, now_ms() - t0);
    return 1;
}

__attribute__((constructor)) static void quillcompat_hello(void) {
    fputs("quillcompat: OS 3.29 swapBuffers shim loaded\n", stderr);
}
