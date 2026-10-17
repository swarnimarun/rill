// v1: one pass over the bytes — framing validation + rolling checksum.
const std = @import("std");

fn checksum(data: []const u8) i64 {
    var sum: i64 = 0;
    var depth: i64 = 0;
    var in_string = false;
    var escaped = false;
    for (data) |b| {
        sum = sum +% @as(i64, b);
        if (in_string) {
            if (escaped) {
                escaped = false;
            } else if (b == '\\') {
                escaped = true;
            } else if (b == '"') {
                in_string = false;
            }
        } else if (b == '"') {
            in_string = true;
        } else if (b == '{' or b == '[') {
            depth += 1;
        } else if (b == '}' or b == ']') {
            depth -= 1;
        }
    }
    return sum +% depth;
}

pub fn main() !void {
    // POSIX directly: one allocation-free read into a static buffer.
    const fd = try std.posix.openat(std.posix.AT.FDCWD, "/tmp/bench.json", .{ .ACCMODE = .RDONLY }, 0);
    defer _ = std.c.close(fd);
    var buffer: [1 << 21]u8 = undefined;
    const n = try std.posix.read(fd, buffer[0..]);
    const data = buffer[0..n];
    var total: i64 = 0;
    var rep: usize = 0;
    while (rep < 200) : (rep += 1) {
        total = total +% checksum(data);
    }
    std.debug.print("zig    bytes={d} total={d}\n", .{ data.len, total });
}
