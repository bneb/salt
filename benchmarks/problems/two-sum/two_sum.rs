fn lcg(x: i64) -> i64 {
    (1103515245i64.wrapping_mul(x) + 12345) % 2147483648
}

fn fill_sorted(a: &mut [i32], n: usize) {
    let mut s: i64 = 42;
    let mut prev: i32 = 0;
    for i in 0..n {
        a[i] = prev + (s % 8) as i32;
        prev = a[i];
        s = lcg(s);
    }
}

fn two_sum_search(a: &[i32], n: usize, target: i64) -> i64 {
    let mut lo: i64 = 0;
    let mut hi: i64 = n as i64 - 1;
    while lo < hi {
        let sum2 = a[lo as usize] as i64 + a[hi as usize] as i64;
        if sum2 == target {
            return lo * n as i64 + hi;
        }
        if sum2 < target {
            lo += 1;
        } else {
            hi -= 1;
        }
    }
    -1
}

fn run_queries(a: &[i32], n: usize, qcount: usize) -> i64 {
    let mut s: i64 = 7777;
    let mut checksum: i64 = 0;
    let ni = n as i64;
    for _ in 0..qcount {
        s = lcg(s);
        let i1 = s % ni;
        s = lcg(s);
        let i2 = s % ni;
        s = lcg(s);
        let t = a[i1 as usize] as i64 + a[i2 as usize] as i64 + s % 3;
        checksum += two_sum_search(a, n, t);
    }
    checksum
}

fn main() {
    let n: usize = 200000;
    let qcount: usize = 500;
    let mut a = vec![0i32; n];
    fill_sorted(&mut a, n);
    println!("checksum={}", run_queries(&a, n, qcount));
}
