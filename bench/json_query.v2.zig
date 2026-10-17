// v2: zero-copy JSON query engine — same algorithm as the Rill and Rust versions.
const std = @import("std");

const CAP: usize = 16384;

const Node = struct {
    kind: u8 = 0,
    start: usize = 0,
    end: usize = 0,
    first: usize = 0,
    next: usize = 0,
    count: usize = 0,
};

const Pool = struct {
    nodes: [CAP]Node = undefined,
    used: usize = 0,
    failed: bool = false,

    fn push(self: *Pool, kind: u8, start: usize, end: usize) usize {
        if (self.used >= CAP) {
            self.failed = true;
            return 0;
        }
        const idx = self.used;
        self.nodes[idx] = .{ .kind = kind, .start = start, .end = end };
        self.used = idx + 1;
        return idx;
    }
};

fn isWs(b: u8) bool {
    return b == ' ' or b == '\n' or b == '\r' or b == '\t';
}

fn skipWs(s: []const u8, i: *usize, n: usize) void {
    while (i.* < n and isWs(s[i.*])) : (i.* += 1) {}
}

fn scanString(s: []const u8, i: *usize, n: usize) void {
    i.* += 1;
    while (i.* < n) {
        const b = s[i.*];
        i.* += 1;
        if (b == '\\') {
            i.* += 1;
        } else if (b == '"') {
            return;
        }
    }
}

fn scanBare(s: []const u8, i: *usize, n: usize) void {
    while (i.* < n) {
        const b = s[i.*];
        if (b == ',' or b == '}' or b == ']' or isWs(b)) return;
        i.* += 1;
    }
}

fn numberValue(s: []const u8, start: usize, end: usize) i64 {
    var value: i64 = 0;
    var negative = false;
    var k = start;
    while (k < end) : (k += 1) {
        const b = s[k];
        if (b == '-') {
            negative = true;
        } else if (b >= '0' and b <= '9') {
            value = value * 10 + @as(i64, b - '0');
        }
    }
    return if (negative) -value else value;
}

fn textEq(s: []const u8, start: usize, end: usize, lit: []const u8) bool {
    return end >= start and std.mem.eql(u8, s[start..end], lit);
}

fn parseValue(s: []const u8, i: *usize, n: usize, p: *Pool) usize {
    skipWs(s, i, n);
    if (i.* >= n) return 0;
    const start = i.*;
    const b = s[i.*];
    if (b == '{') {
        const node = p.push(0, start, start);
        i.* += 1;
        var last: usize = 0;
        var count: usize = 0;
        while (i.* < n) {
            skipWs(s, i, n);
            if (i.* >= n) break;
            if (s[i.*] == '}') {
                i.* += 1;
                break;
            }
            const key = p.push(2, i.*, i.*);
            scanString(s, i, n);
            p.nodes[key].end = i.*;
            if (count == 0) {
                p.nodes[node].first = key;
            } else {
                p.nodes[last].next = key;
            }
            last = key;
            skipWs(s, i, n);
            if (i.* < n and s[i.*] == ':') i.* += 1;
            const value = parseValue(s, i, n, p);
            p.nodes[key].next = value;
            last = value;
            count += 1;
            p.nodes[node].count = count;
            skipWs(s, i, n);
            if (i.* < n and s[i.*] == ',') i.* += 1;
        }
        p.nodes[node].end = i.*;
        return node;
    } else if (b == '[') {
        const node = p.push(1, start, start);
        i.* += 1;
        var last: usize = 0;
        var count: usize = 0;
        while (i.* < n) {
            skipWs(s, i, n);
            if (i.* >= n) break;
            if (s[i.*] == ']') {
                i.* += 1;
                break;
            }
            const value = parseValue(s, i, n, p);
            if (count == 0) {
                p.nodes[node].first = value;
            } else {
                p.nodes[last].next = value;
            }
            last = value;
            count += 1;
            p.nodes[node].count = count;
            skipWs(s, i, n);
            if (i.* < n and s[i.*] == ',') i.* += 1;
        }
        p.nodes[node].end = i.*;
        return node;
    } else if (b == '"') {
        const node = p.push(2, start, start);
        scanString(s, i, n);
        p.nodes[node].end = i.*;
        return node;
    } else {
        const node = p.push(3, start, start);
        scanBare(s, i, n);
        p.nodes[node].end = i.*;
        return node;
    }
}

fn memberFrom(s: []const u8, p: *const Pool, child0: usize, key: []const u8) usize {
    var child = child0;
    while (child != 0) {
        const c = p.nodes[child];
        if (textEq(s, c.start + 1, c.end - 1, key)) return c.next;
        child = p.nodes[c.next].next;
    }
    return 0;
}

fn member(s: []const u8, p: *const Pool, object: usize, key: []const u8) usize {
    return memberFrom(s, p, p.nodes[object].first, key);
}

fn queryRows(s: []const u8, p: *const Pool, row0: usize, threshold: i64) i64 {
    var sum: i64 = 0;
    var row = row0;
    while (row != 0) {
        const score = member(s, p, row, "score");
        if (score != 0 and numberValue(s, p.nodes[score].start, p.nodes[score].end) > threshold) {
            const id = member(s, p, row, "id");
            if (id != 0) sum += numberValue(s, p.nodes[id].start, p.nodes[id].end);
        }
        row = p.nodes[row].next;
    }
    return sum;
}

fn serialise(sum: i64, count: i64, out: *[64]u8) i64 {
    var k: usize = 0;
    for (0..8) |byte| {
        out[k] = @intCast((sum >> @intCast(byte * 8)) & 0xff);
        k += 1;
    }
    for (0..8) |byte| {
        out[k] = @intCast((count >> @intCast(byte * 8)) & 0xff);
        k += 1;
    }
    var read_back: i64 = 0;
    for (0..8) |byte| {
        read_back += @as(i64, out[byte]) << @intCast(byte * 8);
    }
    return read_back + count;
}

var pool_buffer: Pool = .{};

fn runOnce(s: []const u8, n: usize, threshold: i64) i64 {
    const p = &pool_buffer;
    p.used = 0;
    p.failed = false;
    var i: usize = 0;
    const root = parseValue(s, &i, n, p);
    const rows = member(s, p, root, "rows");
    const sum = queryRows(s, p, p.nodes[rows].first, threshold);
    var buffer: [64]u8 = undefined;
    _ = serialise(sum, @intCast(p.used), &buffer);
    return if (p.failed) -1 else sum;
}

pub fn main() !void {
    var buffer: [1 << 20]u8 = undefined;
    const fd = try std.posix.openat(std.posix.AT.FDCWD, "/tmp/bench_small.json", .{ .ACCMODE = .RDONLY }, 0);
    defer _ = std.c.close(fd);
    const n = try std.posix.read(fd, buffer[0..]);
    const s = buffer[0..n];
    var total: i64 = 0;
    var rep: usize = 0;
    while (rep < 200) : (rep += 1) {
        total = total +% runOnce(s, n, 500);
    }
    std.debug.print("zig    v2 total={d}\n", .{total});
}
