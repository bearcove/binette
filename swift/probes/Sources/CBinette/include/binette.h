#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define BINETTE_STATUS_OK 0
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

typedef size_t (*BinetteLocalSequenceLenThunk)(const uint8_t *value, void *context);
typedef uint8_t (*BinetteLocalSequenceU8Thunk)(const uint8_t *value, size_t index, void *context);
typedef const uint8_t *(*BinetteLocalSequenceElementPtrThunk)(const uint8_t *value, size_t index, void *context);
typedef bool (*BinetteLocalSequenceWriteBytesThunk)(uint8_t *value, const uint8_t *ptr, size_t len, void *context);
typedef bool (*BinetteLocalSequenceWriteFixedElementsThunk)(uint8_t *value, const uint8_t *ptr, size_t count, size_t element_stride, void *context);
typedef bool (*BinetteLocalOptionIsSomeThunk)(const uint8_t *value, void *context);
typedef const uint8_t *(*BinetteLocalOptionSomeThunk)(const uint8_t *value, void *context);
typedef bool (*BinetteLocalOptionWriteNoneThunk)(uint8_t *value, void *context);
typedef bool (*BinetteLocalOptionWriteSomeBytesThunk)(uint8_t *value, const uint8_t *ptr, size_t len, void *context);
typedef uint32_t (*BinetteLocalEnumTagThunk)(const uint8_t *value, void *context);
typedef const uint8_t *(*BinetteLocalVariantProjectThunk)(const uint8_t *value, void *context);
typedef bool (*BinetteLocalVariantProjectIntoThunk)(const uint8_t *value, uint8_t *out, size_t out_len, void *context);
typedef void (*BinetteLocalVariantDropProjectedThunk)(uint8_t *value, void *context);
typedef bool (*BinetteLocalVariantConstructThunk)(uint8_t *value, const uint8_t *payload, size_t payload_len, void *context);

typedef struct {
    const uint8_t *ptr;
    size_t len;
} BinetteLocalStrAbi;

typedef struct {
    size_t size;
    size_t align;
    size_t stride;
} BinetteLocalLayoutAbi;

typedef struct {
    uint32_t tag;
    uint64_t type_id;
    uint64_t owner_type_id;
    BinetteLocalStrAbi path;
} BinetteLocalSchemaRefAbi;

typedef struct BinetteLocalDescriptorAbi BinetteLocalDescriptorAbi;

typedef struct {
    BinetteLocalSequenceLenThunk len;
    BinetteLocalSequenceU8Thunk element_u8;
    BinetteLocalSequenceElementPtrThunk element_ptr;
    BinetteLocalSequenceWriteBytesThunk write_bytes;
    BinetteLocalSequenceWriteFixedElementsThunk write_fixed_elements;
    void *context;
} BinetteLocalSequenceThunksAbi;

typedef struct {
    uint32_t tag;
    size_t offset;
    size_t element_count;
    size_t pointer_offset;
    size_t length_offset;
    uint8_t has_capacity;
    size_t capacity_offset;
    size_t element_stride;
    BinetteLocalSequenceThunksAbi thunks;
} BinetteLocalSequenceStorageAbi;

typedef struct {
    uint32_t tag;
    BinetteLocalSequenceStorageAbi storage;
} BinetteLocalScalarAbi;

typedef struct {
    bool (*is_some)(const uint8_t *value, void *context);
    const uint8_t *(*some)(const uint8_t *value, void *context);
    bool (*write_none)(uint8_t *value, void *context);
    bool (*write_some_bytes)(uint8_t *value, const uint8_t *ptr, size_t len, void *context);
    void *context;
} BinetteLocalOptionThunksAbi;

typedef struct {
    uint32_t tag;
    size_t tag_offset;
    size_t tag_width;
    size_t none_value;
    size_t some_value;
    size_t some_offset;
    const uint8_t *none_bytes;
    size_t none_bytes_len;
    BinetteLocalOptionThunksAbi thunks;
} BinetteLocalOptionRepresentationAbi;

typedef struct {
    const BinetteLocalDescriptorAbi *some;
    BinetteLocalOptionRepresentationAbi representation;
} BinetteLocalOptionAbi;

typedef struct {
    const BinetteLocalDescriptorAbi *element;
    BinetteLocalSequenceStorageAbi storage;
} BinetteLocalSequenceAbi;

typedef struct {
    BinetteLocalStrAbi name;
    size_t offset;
    const BinetteLocalDescriptorAbi *descriptor;
} BinetteLocalFieldAbi;

typedef struct {
    const BinetteLocalFieldAbi *fields;
    size_t field_count;
} BinetteLocalStructAbi;

typedef struct {
    BinetteLocalEnumTagThunk call;
    void *context;
} BinetteLocalEnumTagThunkAbi;

typedef struct {
    uint32_t tag;
    size_t direct_offset;
    BinetteLocalEnumTagThunkAbi thunk;
} BinetteLocalEnumTagAccessAbi;

typedef struct {
    BinetteLocalVariantProjectThunk call;
    void *context;
} BinetteLocalVariantProjectThunkAbi;

typedef struct {
    uint32_t tag;
    size_t direct_offset;
    BinetteLocalVariantProjectThunkAbi thunk;
} BinetteLocalVariantProjectAccessAbi;

typedef struct {
    BinetteLocalVariantProjectIntoThunk call;
    void *context;
} BinetteLocalVariantProjectIntoAbi;

typedef struct {
    BinetteLocalVariantDropProjectedThunk call;
    void *context;
} BinetteLocalVariantDropAbi;

typedef struct {
    BinetteLocalVariantConstructThunk call;
    void *context;
} BinetteLocalVariantConstructAbi;

typedef struct {
    BinetteLocalStrAbi name;
    uint32_t index;
    BinetteLocalVariantProjectAccessAbi project;
    BinetteLocalVariantProjectIntoAbi project_into;
    BinetteLocalVariantDropAbi drop_projected;
    BinetteLocalVariantConstructAbi construct;
    const BinetteLocalDescriptorAbi *payload;
} BinetteLocalVariantAbi;

typedef struct {
    BinetteLocalEnumTagAccessAbi tag;
    const BinetteLocalVariantAbi *variants;
    size_t variant_count;
} BinetteLocalEnumAbi;

typedef struct {
    uint32_t tag;
    BinetteLocalScalarAbi scalar;
    BinetteLocalStructAbi structure;
    BinetteLocalEnumAbi enumeration;
    BinetteLocalSequenceAbi sequence;
    BinetteLocalOptionAbi option;
    BinetteLocalStrAbi text;
} BinetteLocalKindAbi;

struct BinetteLocalDescriptorAbi {
    BinetteLocalSchemaRefAbi schema;
    uint32_t backend;
    BinetteLocalLayoutAbi layout;
    BinetteLocalKindAbi kind;
};

typedef struct {
    uint8_t *ptr;
    size_t len;
    size_t cap;
} BinetteByteBuffer;

typedef struct BinetteLocalDescriptorHandle BinetteLocalDescriptorHandle;

int32_t binette_local_descriptor_import(
    const BinetteLocalDescriptorAbi *descriptor,
    BinetteLocalDescriptorHandle **out
);
void binette_local_descriptor_free(BinetteLocalDescriptorHandle *handle);
void binette_byte_buffer_free(BinetteByteBuffer buffer);
uint64_t binette_primitive_string_type_id(void);
uint64_t binette_primitive_u32_type_id(void);
uint64_t binette_canary_message_type_id(void);
int32_t binette_canary_message_encode(
    const BinetteLocalDescriptorHandle *handle,
    const uint8_t *value,
    BinetteByteBuffer *out
);
int32_t binette_canary_message_decode(
    const BinetteLocalDescriptorHandle *handle,
    const uint8_t *bytes,
    size_t len,
    uint8_t *out_value
);
int32_t binette_canary_message_rust_encode_hi(BinetteByteBuffer *out);
