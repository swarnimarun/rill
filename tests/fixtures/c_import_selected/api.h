#ifndef RILL_SELECTED_API_H
#define RILL_SELECTED_API_H
#include <stdint.h>

typedef int32_t SelectedI32;
typedef uint8_t SelectedU8;
typedef uint32_t SelectedU32;
typedef uint64_t SelectedU64;
typedef float SelectedFloat;
typedef _Bool SelectedBool;
typedef struct SelectedItem { int32_t value; } SelectedItem;
typedef SelectedItem SelectedAlias;
typedef const SelectedAlias SelectedConstAlias;
typedef SelectedAlias *SelectedHandle;

#define SELECTED_LARGE UINT64_C(18446744073709551615)
#define SELECTED_BYTE ((uint8_t)255)
#define SELECTED_UNSIGNED UINT32_C(4294967295)
#define SELECTED_BOOL ((_Bool)1)
enum { SELECTED_ENUM = 42 };

SelectedI32 selected_add(SelectedI32 let, SelectedI32);
SelectedU8 selected_byte(SelectedU8 value);
SelectedU32 selected_unsigned(SelectedU32 value);
SelectedU64 selected_wide(SelectedU64 value);
SelectedFloat selected_float(SelectedFloat value);
SelectedBool selected_bool(SelectedBool value);
SelectedBool selected_text(const char *value);
SelectedAlias *selected_item(void);
SelectedConstAlias *selected_const_item(void);
int32_t selected_inspect(const SelectedAlias *value);
int32_t selected_inspect_alias(SelectedConstAlias *value);
int32_t selected_handle(SelectedHandle value);
int32_t selected_read_i32(const SelectedI32 *value);
void *selected_null(void);
void selected_discard(void *value);
/* Compatible repeated declarations must produce a single binding. */
SelectedI32 selected_add(SelectedI32, SelectedI32);

#ifdef SELECTED_ALTERNATE
typedef uint32_t SelectedConditional;
#define SELECTED_CONDITIONAL UINT32_C(42)
#else
typedef int32_t SelectedConditional;
#define SELECTED_CONDITIONAL INT32_C(41)
#endif
SelectedConditional selected_conditional(SelectedConditional value);
#endif
