// v1: one pass over the bytes — framing validation + rolling checksum.
use std::fs;

fn checksum(data: &[u8]) -> i64 {
    let mut sum: i64 = 0;
    let mut depth: i64 = 0;
    let mut in_string = false;
    let mut escaped = false;
    for &b in data {
        sum = sum.wrapping_add(b as i64);
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
        } else if b == b'"' {
            in_string = true;
        } else if b == b'{' || b == b'[' {
            depth += 1;
        } else if b == b'}' || b == b']' {
            depth -= 1;
        }
    }
    sum + depth
}

fn main() {
    let data = fs::read("/tmp/bench.json").expect("read input");
    let mut total: i64 = 0;
    for _ in 0..200 {
        total = total.wrapping_add(checksum(&data));
    }
    println!("rust   bytes={} total={}", data.len(), total);
}
