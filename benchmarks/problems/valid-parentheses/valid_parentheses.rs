// Bracket types: 0='(' 1='[' 2='{' and 3=')' 4=']' 5='}'. Strings are
// generated as an opener prefix plus a mirrored closer suffix, so they
// are balanced by construction; every few strings get one character
// flipped, which always breaks validity. Returns matched pairs if
// valid, -1 if not.
fn lcg(x: i64) -> i64 {
    (1103515245i64.wrapping_mul(x) + 12345) % 2147483648
}

fn gen_string(buf: &mut [i32], len: usize, seed_in: i64, corrupt: bool) -> i64 {
    let mut s = seed_in;
    let half = len / 2;
    for j in 0..half {
        s = lcg(s);
        let t = (s % 3) as i32;
        buf[j] = t;
        buf[len - 1 - j] = t + 3;
    }
    if corrupt {
        s = lcg(s);
        let idx = (s % len as i64) as usize;
        buf[idx] = (buf[idx] + 3) % 6;
    }
    s
}

fn validate(buf: &[i32], stack: &mut [i32], len: usize) -> i64 {
    let mut depth: usize = 0;
    let mut pairs: i64 = 0;
    let mut ok = true;
    for j in 0..len {
        let t = buf[j];
        if t < 3 {
            stack[depth] = t;
            depth += 1;
        }
        if t >= 3 && depth == 0 {
            ok = false;
        }
        if t >= 3 && depth > 0 {
            depth -= 1;
            if stack[depth] == t - 3 {
                pairs += 1;
            } else {
                ok = false;
            }
        }
    }
    if ok && depth == 0 { pairs } else { -1 }
}

fn main() {
    let len: usize = 4096;
    let strings: usize = 20000;
    let mut buf = vec![0i32; len];
    let mut stack = vec![0i32; len];
    let mut s: i64 = 12345;
    let mut checksum: i64 = 0;
    for k in 0..strings {
        s = gen_string(&mut buf, len, s, k % 4 == 0);
        let r = validate(&buf, &mut stack, len);
        checksum += if r >= 0 { r + 1 } else { -1 };
    }
    println!("checksum={}", checksum);
}
