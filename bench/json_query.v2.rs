// v2: zero-copy JSON query engine — same algorithm as bench/json_query.v2.rill:
// one node pool of (kind, span, first, next, count) records pointing into the
// input, sibling-linked as key -> value -> next key, then a query over the
// "rows" array plus the same flatbuffer-style serialisation.
use std::fs;

const CAP: usize = 16384;

#[derive(Clone, Copy, Default)]
struct Node {
    kind: u8,
    start: usize,
    end: usize,
    first: usize,
    next: usize,
    count: usize,
}

struct Pool {
    nodes: Vec<Node>,
    used: usize,
    failed: bool,
}

impl Pool {
    fn new() -> Self {
        Pool { nodes: vec![Node::default(); CAP], used: 0, failed: false }
    }
    fn push(&mut self, kind: u8, start: usize, end: usize) -> usize {
        if self.used >= CAP {
            self.failed = true;
            return 0;
        }
        let idx = self.used;
        self.nodes[idx] = Node { kind, start, end, first: 0, next: 0, count: 0 };
        self.used = idx + 1;
        idx
    }
}

fn is_ws(b: u8) -> bool {
    b == b' ' || b == b'\n' || b == b'\r' || b == b'\t'
}

fn skip_ws(s: &[u8], i: &mut usize, n: usize) {
    while *i < n && is_ws(s[*i]) {
        *i += 1;
    }
}

fn scan_string(s: &[u8], i: &mut usize, n: usize) {
    *i += 1;
    while *i < n {
        let b = s[*i];
        *i += 1;
        if b == b'\\' {
            *i += 1;
        } else if b == b'"' {
            return;
        }
    }
}

fn scan_bare(s: &[u8], i: &mut usize, n: usize) {
    while *i < n {
        let b = s[*i];
        if b == b',' || b == b'}' || b == b']' || is_ws(b) {
            return;
        }
        *i += 1;
    }
}

fn number_value(s: &[u8], start: usize, end: usize) -> i64 {
    let mut value: i64 = 0;
    let mut negative = false;
    for k in start..end.max(start) {
        let b = s[k];
        if b == b'-' {
            negative = true;
        } else if b.is_ascii_digit() {
            value = value * 10 + (b - b'0') as i64;
        }
    }
    if negative { -value } else { value }
}

fn text_eq(s: &[u8], start: usize, end: usize, lit: &[u8]) -> bool {
    end >= start && &s[start..end] == lit
}

fn parse_value(s: &[u8], i: &mut usize, n: usize, p: &mut Pool) -> usize {
    skip_ws(s, i, n);
    if *i >= n {
        return 0;
    }
    let start = *i;
    let b = s[*i];
    if b == b'{' {
        let node = p.push(0, start, start);
        *i += 1;
        let mut last = 0usize;
        let mut count = 0usize;
        while *i < n {
            skip_ws(s, i, n);
            if *i >= n {
                break;
            }
            if s[*i] == b'}' {
                *i += 1;
                break;
            }
            let key = p.push(2, *i, *i);
            scan_string(s, i, n);
            p.nodes[key].end = *i;
            if count == 0 {
                p.nodes[node].first = key;
            } else {
                p.nodes[last].next = key;
            }
            last = key;
            skip_ws(s, i, n);
            if *i < n && s[*i] == b':' {
                *i += 1;
            }
            let value = parse_value(s, i, n, p);
            p.nodes[key].next = value;
            last = value;
            count += 1;
            p.nodes[node].count = count;
            skip_ws(s, i, n);
            if *i < n && s[*i] == b',' {
                *i += 1;
            }
        }
        p.nodes[node].end = *i;
        node
    } else if b == b'[' {
        let node = p.push(1, start, start);
        *i += 1;
        let mut last = 0usize;
        let mut count = 0usize;
        while *i < n {
            skip_ws(s, i, n);
            if *i >= n {
                break;
            }
            if s[*i] == b']' {
                *i += 1;
                break;
            }
            let value = parse_value(s, i, n, p);
            if count == 0 {
                p.nodes[node].first = value;
            } else {
                p.nodes[last].next = value;
            }
            last = value;
            count += 1;
            p.nodes[node].count = count;
            skip_ws(s, i, n);
            if *i < n && s[*i] == b',' {
                *i += 1;
            }
        }
        p.nodes[node].end = *i;
        node
    } else if b == b'"' {
        let node = p.push(2, start, start);
        scan_string(s, i, n);
        p.nodes[node].end = *i;
        node
    } else {
        let node = p.push(3, start, start);
        scan_bare(s, i, n);
        p.nodes[node].end = *i;
        node
    }
}

/// Object members are key -> value -> next key.
fn member_from(s: &[u8], p: &Pool, mut child: usize, key: &[u8]) -> usize {
    while child != 0 {
        let c = &p.nodes[child];
        if text_eq(s, c.start + 1, c.end - 1, key) {
            return c.next;
        }
        child = p.nodes[c.next].next;
    }
    0
}

fn member(s: &[u8], p: &Pool, object: usize, key: &[u8]) -> usize {
    member_from(s, p, p.nodes[object].first, key)
}

fn query_rows(s: &[u8], p: &Pool, mut row: usize, threshold: i64) -> i64 {
    let mut sum = 0;
    while row != 0 {
        let score = member(s, p, row, b"score");
        if score != 0 && number_value(s, p.nodes[score].start, p.nodes[score].end) > threshold {
            let id = member(s, p, row, b"id");
            if id != 0 {
                sum += number_value(s, p.nodes[id].start, p.nodes[id].end);
            }
        }
        row = p.nodes[row].next;
    }
    sum
}

fn serialise(sum: i64, count: i64, out: &mut [u8; 64]) -> i64 {
    let mut k = 0usize;
    for byte in 0..8 {
        out[k] = (sum >> (byte * 8)) as u8;
        k += 1;
    }
    for byte in 0..8 {
        out[k] = (count >> (byte * 8)) as u8;
        k += 1;
    }
    let mut read_back: i64 = 0;
    for byte in 0..8 {
        read_back += (out[byte] as i64) << (byte * 8);
    }
    read_back + count
}

fn run_once(s: &[u8], n: usize, threshold: i64, pool: &mut Pool) -> i64 {
    pool.used = 0;
    pool.failed = false;
    let mut i = 0usize;
    let root = parse_value(s, &mut i, n, pool);
    let rows = member(s, pool, root, b"rows");
    let sum = query_rows(s, pool, pool.nodes[rows].first, threshold);
    let mut buffer = [0u8; 64];
    serialise(sum, pool.used as i64, &mut buffer);
    if pool.failed { -1 } else { sum }
}

fn main() {
    let s = fs::read("/tmp/bench_small.json").expect("read input");
    let n = s.len();
    let mut pool = Pool::new();
    let mut total: i64 = 0;
    for _ in 0..200 {
        total = total.wrapping_add(run_once(&s, n, 500, &mut pool));
    }
    println!("rust v2 total={total}");
}
