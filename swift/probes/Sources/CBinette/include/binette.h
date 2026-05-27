#ifndef BINETTE_H
#define BINETTE_H

#pragma once

/* Generated with cbindgen:0.29.2 */

#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>

#define BINETTE_STATUS_OK 0

#define BINETTE_STATUS_NULL_POINTER 1

#define BINETTE_STATUS_DESCRIPTOR 2

#define BINETTE_STATUS_PLAN 3

#define BINETTE_STATUS_STENCIL 4

typedef struct BinetteLocalDescriptorHandle BinetteLocalDescriptorHandle;

typedef uint32_t BinetteLocalSchemaRefTag;

typedef struct BinetteLocalStrAbi {
  const uint8_t *ptr;
  size_t len;
} BinetteLocalStrAbi;

typedef struct BinetteLocalSchemaRefAbi {
  BinetteLocalSchemaRefTag tag;
  uint64_t type_id;
  uint64_t owner_type_id;
  struct BinetteLocalStrAbi path;
} BinetteLocalSchemaRefAbi;

typedef uint32_t BinetteLocalBackendAbi;

typedef struct BinetteLocalLayoutAbi {
  size_t size;
  size_t align;
  size_t stride;
} BinetteLocalLayoutAbi;

typedef uint32_t BinetteLocalKindTag;

typedef uint32_t BinetteLocalScalarTag;

typedef uint32_t BinetteLocalSequenceStorageTag;

typedef size_t (*BinetteLocalSequenceLenThunk)(const uint8_t *value, void *context);

typedef uint8_t (*BinetteLocalSequenceU8Thunk)(const uint8_t *value, size_t index, void *context);

typedef const uint8_t *(*BinetteLocalSequenceElementPtrThunk)(const uint8_t *value,
                                                              size_t index,
                                                              void *context);

typedef bool (*BinetteLocalSequenceWriteBytesThunk)(uint8_t *value,
                                                    const uint8_t *ptr,
                                                    size_t len,
                                                    void *context);

typedef bool (*BinetteLocalSequenceWriteFixedElementsThunk)(uint8_t *value,
                                                            const uint8_t *ptr,
                                                            size_t count,
                                                            size_t element_stride,
                                                            void *context);

typedef struct BinetteLocalSequenceThunksAbi {
  BinetteLocalSequenceLenThunk len;
  BinetteLocalSequenceU8Thunk element_u8;
  BinetteLocalSequenceElementPtrThunk element_ptr;
  BinetteLocalSequenceWriteBytesThunk write_bytes;
  BinetteLocalSequenceWriteFixedElementsThunk write_fixed_elements;
  void *context;
} BinetteLocalSequenceThunksAbi;

typedef struct BinetteLocalSequenceStorageAbi {
  BinetteLocalSequenceStorageTag tag;
  size_t offset;
  size_t element_count;
  size_t pointer_offset;
  size_t length_offset;
  uint8_t has_capacity;
  size_t capacity_offset;
  size_t element_stride;
  struct BinetteLocalSequenceThunksAbi thunks;
} BinetteLocalSequenceStorageAbi;

typedef struct BinetteLocalScalarAbi {
  BinetteLocalScalarTag tag;
  struct BinetteLocalSequenceStorageAbi storage;
} BinetteLocalScalarAbi;

typedef struct BinetteLocalFieldAbi {
  struct BinetteLocalStrAbi name;
  size_t offset;
  const struct BinetteLocalDescriptorAbi *descriptor;
} BinetteLocalFieldAbi;

typedef struct BinetteLocalStructAbi {
  const struct BinetteLocalFieldAbi *fields;
  size_t field_count;
} BinetteLocalStructAbi;

typedef uint32_t BinetteLocalAccessTag;

typedef uint32_t (*BinetteLocalEnumTagThunk)(const uint8_t *value, void *context);

typedef struct BinetteLocalEnumTagThunkAbi {
  BinetteLocalEnumTagThunk call;
  void *context;
} BinetteLocalEnumTagThunkAbi;

typedef struct BinetteLocalEnumTagAccessAbi {
  BinetteLocalAccessTag tag;
  size_t direct_offset;
  struct BinetteLocalEnumTagThunkAbi thunk;
} BinetteLocalEnumTagAccessAbi;

typedef const uint8_t *(*BinetteLocalVariantProjectThunk)(const uint8_t *value, void *context);

typedef struct BinetteLocalVariantProjectThunkAbi {
  BinetteLocalVariantProjectThunk call;
  void *context;
} BinetteLocalVariantProjectThunkAbi;

typedef struct BinetteLocalVariantProjectAccessAbi {
  BinetteLocalAccessTag tag;
  size_t direct_offset;
  struct BinetteLocalVariantProjectThunkAbi thunk;
} BinetteLocalVariantProjectAccessAbi;

typedef bool (*BinetteLocalVariantProjectIntoThunk)(const uint8_t *value,
                                                    uint8_t *out,
                                                    size_t out_len,
                                                    void *context);

typedef struct BinetteLocalVariantProjectIntoAbi {
  BinetteLocalVariantProjectIntoThunk call;
  void *context;
} BinetteLocalVariantProjectIntoAbi;

typedef void (*BinetteLocalVariantDropProjectedThunk)(uint8_t *value, void *context);

typedef struct BinetteLocalVariantDropAbi {
  BinetteLocalVariantDropProjectedThunk call;
  void *context;
} BinetteLocalVariantDropAbi;

typedef bool (*BinetteLocalVariantConstructThunk)(uint8_t *value,
                                                  const uint8_t *payload,
                                                  size_t payload_len,
                                                  void *context);

typedef struct BinetteLocalVariantConstructAbi {
  BinetteLocalVariantConstructThunk call;
  void *context;
} BinetteLocalVariantConstructAbi;

typedef struct BinetteLocalVariantAbi {
  struct BinetteLocalStrAbi name;
  uint32_t index;
  struct BinetteLocalVariantProjectAccessAbi project;
  struct BinetteLocalVariantProjectIntoAbi project_into;
  struct BinetteLocalVariantDropAbi drop_projected;
  struct BinetteLocalVariantConstructAbi construct;
  const struct BinetteLocalDescriptorAbi *payload;
} BinetteLocalVariantAbi;

typedef struct BinetteLocalEnumAbi {
  struct BinetteLocalEnumTagAccessAbi tag;
  const struct BinetteLocalVariantAbi *variants;
  size_t variant_count;
} BinetteLocalEnumAbi;

typedef struct BinetteLocalSequenceAbi {
  const struct BinetteLocalDescriptorAbi *element;
  struct BinetteLocalSequenceStorageAbi storage;
} BinetteLocalSequenceAbi;

typedef uint32_t BinetteLocalOptionRepresentationTag;

typedef bool (*BinetteLocalOptionIsSomeThunk)(const uint8_t *value, void *context);

typedef const uint8_t *(*BinetteLocalOptionSomeThunk)(const uint8_t *value, void *context);

typedef bool (*BinetteLocalOptionWriteNoneThunk)(uint8_t *value, void *context);

typedef bool (*BinetteLocalOptionWriteSomeBytesThunk)(uint8_t *value,
                                                      const uint8_t *ptr,
                                                      size_t len,
                                                      void *context);

typedef struct BinetteLocalOptionThunksAbi {
  BinetteLocalOptionIsSomeThunk is_some;
  BinetteLocalOptionSomeThunk some;
  BinetteLocalOptionWriteNoneThunk write_none;
  BinetteLocalOptionWriteSomeBytesThunk write_some_bytes;
  void *context;
} BinetteLocalOptionThunksAbi;

typedef struct BinetteLocalOptionRepresentationAbi {
  BinetteLocalOptionRepresentationTag tag;
  size_t tag_offset;
  size_t tag_width;
  size_t none_value;
  size_t some_value;
  size_t some_offset;
  const uint8_t *none_bytes;
  size_t none_bytes_len;
  struct BinetteLocalOptionThunksAbi thunks;
} BinetteLocalOptionRepresentationAbi;

typedef struct BinetteLocalOptionAbi {
  const struct BinetteLocalDescriptorAbi *some;
  struct BinetteLocalOptionRepresentationAbi representation;
} BinetteLocalOptionAbi;

typedef struct BinetteLocalKindAbi {
  BinetteLocalKindTag tag;
  struct BinetteLocalScalarAbi scalar;
  struct BinetteLocalStructAbi structure;
  struct BinetteLocalEnumAbi enumeration;
  struct BinetteLocalSequenceAbi sequence;
  struct BinetteLocalOptionAbi option;
  struct BinetteLocalStrAbi text;
} BinetteLocalKindAbi;

typedef struct BinetteLocalDescriptorAbi {
  struct BinetteLocalSchemaRefAbi schema;
  BinetteLocalBackendAbi backend;
  struct BinetteLocalLayoutAbi layout;
  struct BinetteLocalKindAbi kind;
} BinetteLocalDescriptorAbi;

typedef struct BinetteByteBuffer {
  uint8_t *ptr;
  size_t len;
  size_t cap;
} BinetteByteBuffer;

#define BINETTE_LOCAL_BACKEND_RUST_FACET 1

#define BINETTE_LOCAL_BACKEND_SWIFT 2

#define BINETTE_LOCAL_SCHEMA_REF_TYPE 1

#define BINETTE_LOCAL_SCHEMA_REF_POSITION 2

#define BINETTE_LOCAL_KIND_SCALAR 1

#define BINETTE_LOCAL_KIND_STRUCT 2

#define BINETTE_LOCAL_KIND_ENUM 3

#define BINETTE_LOCAL_KIND_SEQUENCE 4

#define BINETTE_LOCAL_KIND_OPTION 5

#define BINETTE_LOCAL_KIND_EXTERNAL_ATTACHMENT 6

#define BINETTE_LOCAL_KIND_OPAQUE 7

#define BINETTE_LOCAL_SCALAR_PLAIN 1

#define BINETTE_LOCAL_SCALAR_STRING 2

#define BINETTE_LOCAL_SCALAR_BYTES 3

#define BINETTE_LOCAL_ACCESS_DIRECT 1

#define BINETTE_LOCAL_ACCESS_THUNK 2

#define BINETTE_LOCAL_SEQUENCE_INLINE_FIXED 1

#define BINETTE_LOCAL_SEQUENCE_DIRECT_CONTIGUOUS 2

#define BINETTE_LOCAL_SEQUENCE_THUNK 3

#define BINETTE_LOCAL_OPTION_DIRECT_TAG 1

#define BINETTE_LOCAL_OPTION_NICHE 2

#define BINETTE_LOCAL_OPTION_THUNK 3

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

int32_t binette_local_descriptor_import(const struct BinetteLocalDescriptorAbi *descriptor,
                                        struct BinetteLocalDescriptorHandle **out);

void binette_local_descriptor_free(struct BinetteLocalDescriptorHandle *handle);

void binette_byte_buffer_free(struct BinetteByteBuffer buffer);

uint64_t binette_primitive_string_type_id(void);

uint64_t binette_primitive_u32_type_id(void);

uint64_t binette_canary_message_type_id(void);

int32_t binette_canary_message_encode(const struct BinetteLocalDescriptorHandle *handle,
                                      const uint8_t *value,
                                      struct BinetteByteBuffer *out);

int32_t binette_canary_message_decode(const struct BinetteLocalDescriptorHandle *handle,
                                      const uint8_t *bytes,
                                      size_t len,
                                      uint8_t *out_value);

int32_t binette_canary_message_rust_encode_hi(struct BinetteByteBuffer *out);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* BINETTE_H */
