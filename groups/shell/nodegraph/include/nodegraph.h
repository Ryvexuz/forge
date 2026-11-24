/* Reserved for the nodegraph C interface. No declarations in this skeleton. */
/* The marker above describes the original skeleton; the ABI follows. */
#ifndef NODEGRAPH_H
#define NODEGRAPH_H
#include <stdint.h>
#include <stddef.h>
#ifdef __cplusplus
extern "C" {
#endif

#define NODEGRAPH_ABI_VERSION 1
#define NG_OK 0
#define NG_ERR_NULL_HANDLE -1
#define NG_ERR_STALE_HANDLE -2
#define NG_ERR_NULL_POINTER -3
#define NG_ERR_BAD_UTF8 -4
#define NG_ERR_BUFFER_TOO_SMALL -5
#define NG_ERR_GRAPH -6
#define NG_ERR_EVAL -7
#define NG_ERR_FORMAT -8
#define NG_ERR_BAD_VALUE -9
#define NG_ERR_NO_VALUE -10
#define NG_ERR_PANIC -11
#define NG_ERR_POISONED -12
#define NG_ERR_LIMIT -13
#define NG_TYPE_BOOL 1
#define NG_TYPE_INT 2
#define NG_TYPE_F64 3
#define NG_TYPE_TEXT 4
#define NG_TYPE_IMAGE 5

/* The API has no aggregate types. Handles are thread-bound table identities,
 * never addresses. All calls may return NG_ERR_PANIC. Handle calls may return
 * NG_ERR_NULL_HANDLE, NG_ERR_STALE_HANDLE, NG_ERR_POISONED.
 * Non-null pointers must be valid for their lengths, aligned as appropriate,
 * and outputs must not overlap inputs or one another.
 * Text is length-delimited UTF-8, not NUL-terminated.
 * NULL is NG_ERR_NULL_POINTER even at zero length, except a buffer with cap
 * below the required length. Buffer calls write out_len before testing cap;
 * insufficient cap returns NG_ERR_BUFFER_TOO_SMALL without copying anything.
 * Other outputs (including image dimensions) are written only on NG_OK.
 */

/* NG_OK, NG_ERR_NULL_POINTER. */
int32_t ng_abi_version(uint32_t *out_version);
/* NG_OK, NG_ERR_NULL_POINTER, NG_ERR_LIMIT, NG_ERR_POISONED. */
int32_t ng_graph_create(uint64_t *out_handle);
/* NG_OK, handle errors. Invalidates every copy; stale handles never revive. */
int32_t ng_graph_free(uint64_t handle);
/* NG_OK, handle errors, NG_ERR_NULL_POINTER, NG_ERR_BAD_UTF8, NG_ERR_LIMIT, NG_ERR_GRAPH, NG_ERR_BAD_VALUE. */
int32_t ng_add_node(uint64_t handle, const uint8_t *type_name, size_t type_len, float x, float y, uint64_t *out_node);
/* NG_OK, handle errors, NG_ERR_NULL_POINTER, NG_ERR_BAD_UTF8, NG_ERR_LIMIT, NG_ERR_GRAPH. */
int32_t ng_set_param_f64(uint64_t handle, uint64_t node, const uint8_t *name, size_t name_len, double value);
/* NG_OK, handle errors, NG_ERR_NULL_POINTER, NG_ERR_BAD_UTF8, NG_ERR_LIMIT, NG_ERR_GRAPH. */
int32_t ng_set_param_i64(uint64_t handle, uint64_t node, const uint8_t *name, size_t name_len, int64_t value);
/* NG_OK, handle errors, NG_ERR_NULL_POINTER, NG_ERR_BAD_UTF8, NG_ERR_LIMIT, NG_ERR_GRAPH. */
int32_t ng_connect(uint64_t handle, uint64_t from, const uint8_t *from_port, size_t from_len, uint64_t to, const uint8_t *to_port, size_t to_len);
/* NG_OK, handle errors, NG_ERR_NULL_POINTER, NG_ERR_BAD_UTF8, NG_ERR_LIMIT, NG_ERR_EVAL, NG_ERR_GRAPH (registry initialization). */
int32_t ng_evaluate(uint64_t handle, uint64_t node, const uint8_t *port, size_t port_len, int32_t *out_type);
/* NG_OK, handle errors, NG_ERR_NULL_POINTER, NG_ERR_NO_VALUE, NG_ERR_BAD_VALUE. */
int32_t ng_read_f64(uint64_t handle, double *out_value);
/* NG_OK, handle errors, NG_ERR_NULL_POINTER, NG_ERR_NO_VALUE, NG_ERR_BAD_VALUE, NG_ERR_BUFFER_TOO_SMALL. out_len counts floats. */
int32_t ng_read_image(uint64_t handle, float *buf, size_t cap, size_t *out_len, uint32_t *out_width, uint32_t *out_height);
/* NG_OK, handle errors, NG_ERR_NULL_POINTER, NG_ERR_FORMAT, NG_ERR_BUFFER_TOO_SMALL. out_len counts bytes. */
int32_t ng_save(uint64_t handle, uint8_t *buf, size_t cap, size_t *out_len);
/* NG_OK, handle errors, NG_ERR_NULL_POINTER, NG_ERR_BAD_UTF8, NG_ERR_LIMIT, NG_ERR_FORMAT, NG_ERR_GRAPH (registry initialization). */
int32_t ng_load(uint64_t handle, const uint8_t *buf, size_t len);
/* NG_OK, NG_ERR_NULL_POINTER, NG_ERR_BUFFER_TOO_SMALL. Size queries retain the diagnostic; empty before any failure. */
int32_t ng_last_error(uint8_t *buf, size_t cap, size_t *out_len);
/* NG_ERR_PANIC, with a diagnostic; never takes the table lock. */
int32_t ng_selftest_panic(void);

#ifdef __cplusplus
}
#endif
#endif
