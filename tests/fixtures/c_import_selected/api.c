#include "api.h"
static SelectedItem item = { 42 };
SelectedI32 selected_add(SelectedI32 a, SelectedI32 b) { return a + b; }
SelectedU8 selected_byte(SelectedU8 value) { return value; }
SelectedU32 selected_unsigned(SelectedU32 value) { return value; }
SelectedU64 selected_wide(SelectedU64 value) { return value; }
SelectedFloat selected_float(SelectedFloat value) { return value + 0.5f; }
SelectedBool selected_bool(SelectedBool value) { return value; }
SelectedBool selected_text(const char *value) { return value && value[0] == 'R'; }
SelectedAlias *selected_item(void) { return &item; }
SelectedConstAlias *selected_const_item(void) { return &item; }
int32_t selected_inspect(const SelectedAlias *value) { return value ? value->value : 0; }
int32_t selected_inspect_alias(SelectedConstAlias *value) { return value ? value->value : 0; }
int32_t selected_handle(SelectedHandle value) { return value ? value->value : 0; }
int32_t selected_read_i32(const SelectedI32 *value) { return value ? *value : 42; }
void *selected_null(void) { return 0; }
void selected_discard(void *value) { (void)value; }
SelectedConditional selected_conditional(SelectedConditional value) { return value; }
