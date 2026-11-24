#ifndef FORGE_REFLECT_H
#define FORGE_REFLECT_H
#include <stdint.h>
#include <stddef.h>
#ifdef __cplusplus
extern "C" {
#endif

#define REFLECT_ABI_VERSION 1
#define REFLECT_OK 0
#define REFLECT_ERR_NULL_HANDLE -1
#define REFLECT_ERR_STALE_HANDLE -2
#define REFLECT_ERR_NULL_POINTER -3
#define REFLECT_ERR_BAD_UTF8 -4
#define REFLECT_ERR_BUFFER_TOO_SMALL -5
#define REFLECT_ERR_NO_TYPE -6
#define REFLECT_ERR_INDEX -7
#define REFLECT_ERR_PATH -8
#define REFLECT_ERR_REFUSED -9
#define REFLECT_ERR_VALUE -10
#define REFLECT_ERR_PANIC -11
#define REFLECT_ERR_POISONED -12
#define REFLECT_ERR_LIMIT -13
#define REFLECT_ERR_INTERNAL -14

/* Handles are thread-bound table ids, never addresses. Single-threaded use
 * per process is assumed (one lock, no other guarantee). Every call may
 * return REFLECT_ERR_PANIC. A destroyed handle never revives.
 * The caller owns every buffer. The library owns objects, freed only by
 * reflect_destroy. At most 4096 objects may be live.
 * Text is length-delimited UTF-8, never NUL-terminated; paths are object-relative.
 * Non-null pointers must cover their byte lengths; scalar outputs must be
 * aligned and writable. Outputs must not overlap inputs or each other.
 * Null inputs fail even at length zero. Lengths above isize::MAX fail before
 * reading. Buffer calls check out_len non-null and write the answer's byte
 * length first. If cap is too small, REFLECT_ERR_BUFFER_TOO_SMALL is returned
 * without copying, and buf may be null for a size query. Otherwise buf must
 * be non-null and writable for cap bytes. No terminator is written.
 * Other outputs are written only on REFLECT_OK. Successful calls keep the
 * thread's last error. reflect_last_error never changes it, even on failure;
 * before the first failure it is empty.
 */
int32_t reflect_abi_version(uint32_t *out_version);
int32_t reflect_type_count(size_t *out_count);
int32_t reflect_type_name(size_t index, uint8_t *buf, size_t cap, size_t *out_len);
int32_t reflect_field_count(const uint8_t *type_name, size_t type_len, size_t *out_count);
int32_t reflect_field_name(const uint8_t *type_name, size_t type_len, size_t index, uint8_t *buf, size_t cap, size_t *out_len);
int32_t reflect_create(const uint8_t *type_name, size_t type_len, uint64_t *out_handle);
int32_t reflect_destroy(uint64_t handle);
int32_t reflect_get(uint64_t handle, const uint8_t *path, size_t path_len, uint8_t *buf, size_t cap, size_t *out_len);
int32_t reflect_set(uint64_t handle, const uint8_t *path, size_t path_len, const uint8_t *value, size_t value_len);
int32_t reflect_last_error(uint8_t *buf, size_t cap, size_t *out_len);
int32_t reflect_selftest_panic(void);

#ifdef __cplusplus
}
#endif
#endif
