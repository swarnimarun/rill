#ifndef RILL_SLANG_H
#define RILL_SLANG_H
#include <stdint.h>
#if defined(_WIN32)
# if defined(RILL_SLANG_BUILD)
#  define RILL_SLANG_API __declspec(dllexport)
# else
#  define RILL_SLANG_API __declspec(dllimport)
# endif
#else
# define RILL_SLANG_API __attribute__((visibility("default")))
#endif
#ifdef __cplusplus
extern "C" {
#endif
/* Stages: 0 vertex, 1 fragment, 2 compute. Return 0 on success; 1 invalid
   arguments/stage, 2 compiler failure, 3 output I/O failure. Diagnostics go to
   stderr. The bridge owns and releases all temporary Slang allocations.
   SPIR-V is compiler output, not independently spirv-val validated here.
   Use `rill shader build` for validated, snapshotted project artifacts. */
RILL_SLANG_API int32_t RillSlang_compile_spirv(const char *source,
    const char *entry, int32_t stage, const char *output);
RILL_SLANG_API int32_t RillSlang_compile_msl(const char *source,
    const char *entry, int32_t stage, const char *output);
#ifdef __cplusplus
}
#endif
#endif
