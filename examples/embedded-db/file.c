// Host adapter for the single-writer Rill revision log.
#include <ctype.h>
#include <errno.h>
#include <inttypes.h>
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static FILE *store;
static int appending;
static int32_t row_count;
static const unsigned char magic[] = { 'R', 'D', 'B', '1' };
static unsigned char row[8];
static int32_t args[3];

void db_error(const char *reason) {
    fprintf(stderr, "embedded-db: %s\n", reason);
    exit(EXIT_FAILURE);
}

int32_t db_open(void) {
    if (store) db_error("database is already open");
    const char *path = getenv("RILL_DB_PATH");
    if (!path || !*path) db_error("set RILL_DB_PATH to a database file");
    // Only a newly created file receives a header. An empty existing file is corrupt.
    store = fopen(path, "r+b");
    if (!store && errno == ENOENT) {
        store = fopen(path, "w+bx");
        if (store && fwrite(magic, 1, sizeof magic, store) != sizeof magic)
            db_error("cannot initialize database");
    }
    if (!store || fseek(store, 0, SEEK_END)) db_error("cannot open database");
    long size = ftell(store);
    if (size < 4 || (size - 4) % 8 != 0 || (size - 4) / 8 > INT32_MAX)
        db_error("invalid or unsupported database length");
    unsigned char header[sizeof magic];
    if (fseek(store, 0, SEEK_SET) || fread(header, 1, sizeof header, store) != sizeof header ||
        memcmp(header, magic, sizeof magic)) db_error("invalid database header");
    row_count = (int32_t)((size - 4) / 8);
    return row_count;
}

void db_seek(int32_t skipped) {
    int64_t offset = 4 + (int64_t)skipped * 8;
    if (!store || skipped < 0 || skipped > row_count || offset > LONG_MAX ||
        fseek(store, (long)offset, SEEK_SET))
        db_error("cannot seek database");
    appending = 0;
}

void db_read(void) {
    if (!store || appending || fread(row, 1, sizeof row, store) != sizeof row) db_error("cannot read complete row");
}

static int32_t decode(const unsigned char *bytes) {
    uint32_t n = (uint32_t)bytes[0] | ((uint32_t)bytes[1] << 8) |
        ((uint32_t)bytes[2] << 16) | ((uint32_t)bytes[3] << 24);
    return n <= INT32_MAX ? (int32_t)n : -1 - (int32_t)(UINT32_MAX - n);
}

int32_t db_key(void) { return decode(row); }
int32_t db_value(void) { return decode(row + 4); }

static void encode(unsigned char *out, uint32_t n) {
    for (int i = 0; i < 4; ++i) out[i] = (unsigned char)(n >> (8 * i));
}

void db_append(int32_t key, int32_t value) {
    if (!store || row_count == INT32_MAX || 4 + ((int64_t)row_count + 1) * 8 > LONG_MAX)
        db_error("cannot extend database");
    if (!appending && fseek(store, 0, SEEK_END)) db_error("cannot seek for append");
    appending = 1;
    encode(row, (uint32_t)key);
    encode(row + 4, (uint32_t)value);
    if (fwrite(row, 1, sizeof row, store) != sizeof row) db_error("cannot append row");
    row_count++;
}

int32_t db_command(void) {
    static const struct { const char *name; int arity; } commands[] = {
        {"put", 2}, {"get", 1}, {"changes", 1}, {"apply", 3}, {"count_ge", 2}, {"head", 0}
    };
    // Read a bounded line. Reject embedded NUL, overlong lines and partial I/O.
    char line[128];
    size_t used = 0;
    int c;
    while ((c = fgetc(stdin)) != EOF && c != '\n') {
        if (!c || used == sizeof line - 1) db_error("invalid command line");
        line[used++] = (char)c;
    }
    if (ferror(stdin)) db_error("cannot read command");
    if (!used && c == EOF) return 0;
    line[used] = 0;
    const char *spaces = " \t\r";
    char *name = strtok(line, spaces);
    for (size_t i = 0; name && i < sizeof commands / sizeof commands[0]; ++i) {
        if (strcmp(name, commands[i].name) != 0) continue;
        for (int j = 0; j < commands[i].arity; ++j) {
            char *arg = strtok(NULL, spaces), *end;
            if (!arg || isspace((unsigned char)*arg)) db_error("expected i32 argument");
            errno = 0;
            intmax_t value = strtoimax(arg, &end, 10);
            if (errno || end == arg || *end || value < INT32_MIN || value > INT32_MAX)
                db_error("expected i32 argument");
            args[j] = (int32_t)value;
        }
        if (strtok(NULL, spaces)) db_error("extra command argument");
        return (int32_t)i + 1;
    }
    db_error("unknown command");
    return 0;
}

int32_t db_argument(int32_t index) {
    if (index < 0 || index > 2) db_error("invalid argument index");
    return args[index];
}
void db_number(int32_t value) { printf("%" PRId32 "\n", value); }
void db_missing(void) { puts("missing"); }
void db_change(int32_t revision, int32_t key, int32_t value) {
    printf("apply %" PRId32 " %" PRId32 " %" PRId32 "\n", revision, key, value);
}

void db_close(void) {
    if (!store || fclose(store)) db_error("cannot close database");
    store = NULL;
    appending = 0;
}

void db_finish(void) {
    if (fflush(stdout) || ferror(stdout)) db_error("cannot finish output");
    // The normal Rill executable prints its main value. Here stdout is a replay stream.
    exit(EXIT_SUCCESS);
}
