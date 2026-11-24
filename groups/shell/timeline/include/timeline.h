#ifndef TIMELINE_H
#define TIMELINE_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* No C-layout aggregate types are exposed. Handles are generation-tagged table
 * indices, never pointers. All functions return TL_OK or a named negative code.
 * API capacities: 4096 handle slots; 1048576 samples per audio block.
 * Non-null pointers must be live, aligned, and valid for the declared extent;
 * strings are NUL-terminated UTF-8. Input/output regions must not overlap or be
 * concurrently modified. Output is unchanged on errors except save's out_len.
 * Times are signed ticks (254016000000 ticks per second).
 * Audio is synthetic mono 440 Hz, amplitude 0.5 at 48000 Hz, with no fades.
 * Video is a synthetic frame-index source at 24 fps. No media files are opened.
 * Every handle-taking call can return NULL_HANDLE, STALE_HANDLE, POISONED;
 * every entry can return PANIC (all with the TL_ERR_ prefix).
 */

#define TL_OK 0
#define TL_ERR_NULL_HANDLE -1
#define TL_ERR_STALE_HANDLE -2
#define TL_ERR_NULL_POINTER -3
#define TL_ERR_BUFFER_TOO_SMALL -4
#define TL_ERR_BAD_TRACK -5
#define TL_ERR_BAD_TIME -6
#define TL_ERR_BAD_VALUE -7
#define TL_ERR_BAD_TEXT -8
#define TL_ERR_EDIT_REFUSED -9
#define TL_ERR_NOT_IN_CLIP -10
#define TL_ERR_FORMAT -11
#define TL_ERR_EVAL -12
#define TL_ERR_PANIC -13
#define TL_ERR_POISONED -14
#define TL_ERR_LIMIT -15
#define TL_INTERP_STEP 0
#define TL_INTERP_LINEAR 1

/* TL_OK; TL_ERR_NULL_POINTER, TL_ERR_LIMIT, TL_ERR_POISONED, TL_ERR_PANIC. */
int32_t tl_create(uint64_t *out_handle);
/* TL_OK; TL_ERR_NULL_HANDLE, TL_ERR_STALE_HANDLE, TL_ERR_POISONED, TL_ERR_PANIC.
 * Free retires exhausted generations. */
int32_t tl_free(uint64_t handle);
/* TL_OK; TL_ERR_NULL_POINTER, TL_ERR_BAD_TEXT, TL_ERR_BAD_VALUE, TL_ERR_EDIT_REFUSED;
 * TL_ERR_NULL_HANDLE, TL_ERR_STALE_HANDLE, TL_ERR_POISONED, TL_ERR_PANIC.
 * Animation needs a target; audio/video require NULL. */
int32_t tl_add_track(uint64_t handle, uint64_t track_id, const char *kind, const char *name, const char *channel_target);
/* TL_OK; TL_ERR_NULL_POINTER, TL_ERR_BAD_TEXT, TL_ERR_BAD_TIME, TL_ERR_BAD_TRACK, TL_ERR_EDIT_REFUSED;
 * TL_ERR_NULL_HANDLE, TL_ERR_STALE_HANDLE, TL_ERR_POISONED, TL_ERR_PANIC. */
int32_t tl_add_clip(uint64_t handle, uint64_t track_id, int64_t start, int64_t len, int64_t source_offset, const char *source, const char *name);
/* TL_OK; TL_ERR_BAD_TIME, TL_ERR_BAD_TRACK, TL_ERR_BAD_VALUE, TL_ERR_EDIT_REFUSED;
 * TL_ERR_NULL_HANDLE, TL_ERR_STALE_HANDLE, TL_ERR_POISONED, TL_ERR_PANIC. */
int32_t tl_add_key(uint64_t handle, uint64_t track_id, uint32_t channel, int64_t time, double value, uint32_t interp);
/* TL_OK; TL_ERR_BAD_TIME;
 * TL_ERR_NULL_HANDLE, TL_ERR_STALE_HANDLE, TL_ERR_POISONED, TL_ERR_PANIC. */
int32_t tl_scrub(uint64_t handle, int64_t time);
/* TL_OK; TL_ERR_NULL_POINTER, TL_ERR_BAD_TRACK, TL_ERR_BAD_VALUE, TL_ERR_NOT_IN_CLIP, TL_ERR_EVAL;
 * TL_ERR_NULL_HANDLE, TL_ERR_STALE_HANDLE, TL_ERR_POISONED, TL_ERR_PANIC. */
int32_t tl_read_value(uint64_t handle, uint64_t track_id, uint32_t channel, double *out_value);
/* TL_OK; TL_ERR_NULL_POINTER, TL_ERR_BAD_VALUE, TL_ERR_BAD_TRACK, TL_ERR_EVAL;
 * TL_ERR_NULL_HANDLE, TL_ERR_STALE_HANDLE, TL_ERR_POISONED, TL_ERR_PANIC.
 * Zero len still needs a non-null pointer; silence outside clips. */
int32_t tl_read_audio(uint64_t handle, uint64_t track_id, double *out_samples, size_t len);
/* TL_OK; TL_ERR_NULL_POINTER, TL_ERR_BAD_TRACK, TL_ERR_NOT_IN_CLIP, TL_ERR_EVAL;
 * TL_ERR_NULL_HANDLE, TL_ERR_STALE_HANDLE, TL_ERR_POISONED, TL_ERR_PANIC. */
int32_t tl_read_video_frame(uint64_t handle, uint64_t track_id, uint64_t *out_frame);
/* TL_OK (out_done is 0 or 1); TL_ERR_NULL_POINTER, TL_ERR_EDIT_REFUSED;
 * TL_ERR_NULL_HANDLE, TL_ERR_STALE_HANDLE, TL_ERR_POISONED, TL_ERR_PANIC. */
int32_t tl_undo(uint64_t handle, int32_t *out_done);
/* TL_OK (out_done is 0 or 1); TL_ERR_NULL_POINTER, TL_ERR_EDIT_REFUSED;
 * TL_ERR_NULL_HANDLE, TL_ERR_STALE_HANDLE, TL_ERR_POISONED, TL_ERR_PANIC. */
int32_t tl_redo(uint64_t handle, int32_t *out_done);
/* TL_OK; TL_ERR_NULL_POINTER, TL_ERR_BUFFER_TOO_SMALL;
 * TL_ERR_NULL_HANDLE, TL_ERR_STALE_HANDLE, TL_ERR_POISONED, TL_ERR_PANIC.
 * No NUL. A NULL buf with cap 0 queries out_len; insufficient cap never writes buf. */
int32_t tl_save(uint64_t handle, uint8_t *buf, size_t cap, size_t *out_len);
/* TL_OK; TL_ERR_NULL_POINTER, TL_ERR_FORMAT;
 * TL_ERR_NULL_HANDLE, TL_ERR_STALE_HANDLE, TL_ERR_POISONED, TL_ERR_PANIC.
 * Atomic; success clears history and resets playhead to 0. */
int32_t tl_load(uint64_t handle, const uint8_t *buf, size_t len);
/* Boundary self-test: always TL_ERR_PANIC, without taking the table lock. */
int32_t tl_selftest_panic(void);

#ifdef __cplusplus
}
#endif
#endif
