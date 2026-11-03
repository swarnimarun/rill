#ifndef RILL_SELECTED_UNSUPPORTED_H
#define RILL_SELECTED_UNSUPPORTED_H
#include <stdint.h>
typedef struct SelectedRecord { int32_t value; } SelectedRecord;
struct SelectedTag;
typedef int32_t (*SelectedCallback)(int32_t);
typedef enum SelectedEnum { SELECTED_MEMBER = 0 } SelectedEnum;
int32_t selected_callback(SelectedCallback callback);
int32_t selected_nested_pointer(int32_t **value);
int32_t selected_tag_only(struct SelectedTag *value);
SelectedRecord selected_aggregate(SelectedRecord value);
int32_t selected_variadic(int32_t count, ...);
int32_t selected_no_prototype();
int32_t selected_volatile(volatile int32_t *value);
int32_t selected_restrict(int32_t *restrict value);
int32_t selected_double(double value);
int32_t selected_signed_byte(signed char value);
int32_t selected_signed_wide(int64_t value);
int32_t selected_enum(SelectedEnum value);
const char *selected_return_string(void);
char *selected_mutable_string(void);
static int32_t selected_internal(int32_t value) { return value; }
#if defined(__x86_64__) && !defined(_WIN32)
__attribute__((ms_abi)) int32_t selected_calling_convention(int32_t value);
#endif
#define SELECTED_FUNCTION_MACRO(x) ((x) + 1)
#define SELECTED_FLOAT_CONSTANT 1.5f
#define SELECTED_STRING_CONSTANT "text"
#define SELECTED_RUNTIME_CONSTANT selected_double(1.0)
extern const int32_t SELECTED_RUNTIME_VARIABLE;
#endif
