fn lcg(x: i64) -> i64 {
    (1103515245i64.wrapping_mul(x) + 12345) % 2147483648
}

fn fill(a: &mut [i32], n: usize) {
    let mut s: i64 = 42;
    for i in 0..n {
        a[i] = (s % 101 - 50) as i32;
        s = lcg(s);
    }
}

fn kadane(a: &[i32], n: usize) -> i64 {
    let mut best: i64 = -1000000000;
    let mut cur: i64 = 0;
    for i in 0..n {
        let v = a[i] as i64;
        cur = if cur > 0 { cur + v } else { v };
        if cur > best {
            best = cur;
        }
    }
    best
}

fn solve(a: &[i32], n: usize, rounds: usize) -> i64 {
    let mut checksum: i64 = 0;
    for _ in 0..rounds {
        checksum += kadane(a, n);
    }
    checksum
}

fn main() {
    let n: usize = 1000000;
    let rounds: usize = 200;
    let mut a = vec![0i32; n];
    fill(&mut a, n);
    let checksum = solve(&a, n, rounds);
    println!("checksum={}", checksum);
}
