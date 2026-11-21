// POSIX byte transport for the Rill modal editor.
#define _POSIX_C_SOURCE 200809L
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <termios.h>
#include <unistd.h>

enum { capacity = 256 };
static unsigned char initial[capacity + 1], staged[capacity];
static const char *path;
static struct stat original;
static bool existing, screen, raw;
static struct termios previous;
static volatile sig_atomic_t interrupted;
static const char *notice = "";

static void restore(void) {
    if (raw) {
        tcsetattr(STDIN_FILENO, TCSANOW, &previous);
        fputs("\033[0m\033[?25h\033[?1049l", stderr);
        raw = false;
    }
}
static void fail(const char *message) {
    restore();
    fprintf(stderr, "editor: %s\n", message);
    exit(EXIT_FAILURE);
}
static void interrupt(int signal_number) { (void)signal_number; interrupted = 1; }

int32_t ed_open(void) {
    path = getenv("RILL_EDITOR_FILE");
    if (!path || !*path) fail("set RILL_EDITOR_FILE to the document path");
    size_t count = 0;
    int fd = open(path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK);
    if (fd < 0) {
        if (errno != ENOENT) fail(strerror(errno));
    } else {
        existing = true;
        if (fstat(fd, &original) || !S_ISREG(original.st_mode)) fail("expected a regular file");
        FILE *input = fdopen(fd, "rb");
        if (!input) fail(strerror(errno));
        count = fread(initial, 1, sizeof initial, input);
        if (ferror(input) || fclose(input)) fail("document read failed");
        if (count > capacity) fail("document exceeds 256 bytes");
        for (size_t i = 0; i < count; ++i) {
            if (initial[i] != '\n' && (initial[i] < 32 || initial[i] > 126))
                fail("expected printable ASCII and LF newlines");
        }
    }
    screen = isatty(STDIN_FILENO) && isatty(STDERR_FILENO);
    if (screen) {
        if (tcgetattr(STDIN_FILENO, &previous)) fail(strerror(errno));
        struct termios mode = previous;
        mode.c_lflag &= ~(tcflag_t)(ICANON | ECHO | ISIG | IEXTEN);
        mode.c_iflag &= ~(tcflag_t)(IXON | ICRNL);
        mode.c_cc[VMIN] = 1;
        mode.c_cc[VTIME] = 0;
        struct sigaction action = {0};
        action.sa_handler = interrupt;
        sigemptyset(&action.sa_mask);
        if (sigaction(SIGTERM, &action, NULL) || sigaction(SIGHUP, &action, NULL)
            || sigaction(SIGINT, &action, NULL) || atexit(restore)) fail("terminal setup failed");
        if (tcsetattr(STDIN_FILENO, TCSANOW, &mode)) fail(strerror(errno));
        raw = true;
        fputs("\033[?1049h\033[?25l", stderr);
    }
    return (int32_t)count;
}
int32_t ed_initial_byte(int32_t index) {
    if (index < 0 || index >= capacity) fail("invalid document offset");
    return initial[index];
}
int32_t ed_key(void) {
    if (interrupted) return -1;
    int c = getchar();
    if (ferror(stdin) && !interrupted) fail("key input failed");
    if (c == 3 || c == EOF) return -1;
    if (c == '\r') return '\n';
    return c == 127 ? 8 : c;
}
void ed_frame(bool insert, bool dirty, bool full) {
    if (!screen) return;
    fprintf(stderr, "\033[H\033[2J%s%s%s   i insert   Esc normal   hjkl move   x delete   w save   q quit   Q discard\n%s\n",
            insert ? "INSERT" : "NORMAL", dirty ? " *" : "", full ? " [full]" : "", notice);
}
void ed_draw(int32_t byte, bool cursor) {
    if (!screen) return;
    if (cursor) fputs("\033[7m", stderr);
    if (cursor && byte == '\n') fputc(' ', stderr);
    fputc(byte, stderr);
    if (cursor) fputs("\033[0m", stderr);
}
void ed_present(void) { if (screen) fflush(stderr); }
void ed_stage(int32_t index, int32_t byte) {
    if (index < 0 || index >= capacity || byte < 0 || byte > 127) fail("invalid save byte");
    staged[index] = (unsigned char)byte;
}

static struct timespec modified(struct stat status) {
#ifdef __APPLE__
    return status.st_mtimespec;
#else
    return status.st_mtim;
#endif
}

bool ed_save(int32_t length) {
    if (length < 0 || length > capacity) fail("invalid save length");
    // Keep the old path intact until every byte is written and flushed.
    char *temporary = malloc(strlen(path) + sizeof ".rill-XXXXXX");
    if (!temporary) fail("out of memory");
    sprintf(temporary, "%s.rill-XXXXXX", path);
    int fd = mkstemp(temporary);
    bool created = fd >= 0;
    bool ok = created;
    size_t sent = 0;
    while (ok && sent < (size_t)length) {
        ssize_t n = write(fd, staged + sent, (size_t)length - sent);
        if (n < 0 && errno == EINTR) continue;
        if (n <= 0) ok = false;
        else sent += (size_t)n;
    }
    if (ok && existing) ok = fchmod(fd, original.st_mode & 0777) == 0;
    if (ok) ok = fsync(fd) == 0;
    struct stat replacement;
    if (ok) ok = fstat(fd, &replacement) == 0;
    if (created && close(fd)) ok = false;
    if (ok && existing) {
        struct stat current;
        ok = lstat(path, &current) == 0 && current.st_dev == original.st_dev
            && current.st_ino == original.st_ino && current.st_size == original.st_size
            && modified(current).tv_sec == modified(original).tv_sec
            && modified(current).tv_nsec == modified(original).tv_nsec;
        if (ok && rename(temporary, path) == 0) created = false;
        else ok = false;
    } else if (ok) {
        // A new document may only claim an unused name.
        ok = link(temporary, path) == 0;
    }
    if (created) unlink(temporary);
    free(temporary);
    if (ok) { existing = true; original = replacement; notice = "Saved"; }
    else { notice = "Save failed; check path, external changes and permissions"; fprintf(stderr, "editor: %s\n", notice); }
    return ok;
}
void ed_finish(bool unsaved) {
    if (unsaved) fail("input ended with unsaved changes");
    exit(EXIT_SUCCESS);
}
