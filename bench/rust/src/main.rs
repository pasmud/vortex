//! The reference workload, written in Rust.
//!
//! The same arithmetic in the same order as `bench/c/sieve.c`, so the two rows
//! of the comparison table are comparable. If a future Vortex implementation
//! does not print the same checksum, the implementations have drifted apart
//! and the comparison is invalid.

const LIMIT: u32 = 2_000_000;
const N: usize = 256;

/// The sieve of Eratosthenes, summed.
fn sieve_sum(limit: u32) -> u32 {
    if limit < 2 {
        return 0;
    }
    let mut composite = vec![false; limit as usize + 1];
    let mut total: u32 = 0;
    for i in 2..=limit {
        if composite[i as usize] {
            continue;
        }
        total = total.wrapping_add(i);
        let start = i as u64 * i as u64;
        if start <= limit as u64 {
            let mut j = start;
            while j <= limit as u64 {
                composite[j as usize] = true;
                j += i as u64;
            }
        }
    }
    total
}

fn matrix_work() -> f64 {
    let mut a = vec![0.0f64; N * N];
    let mut b = vec![0.0f64; N * N];
    let mut c = vec![0.0f64; N * N];

    for i in 0..N {
        for j in 0..N {
            a[i * N + j] = ((i + j) % 17) as f64;
            b[i * N + j] = ((i * 3 + j * 5) % 23) as f64;
        }
    }

    for _ in 0..8 {
        for i in 0..N {
            for j in 0..N {
                c[i * N + j] = a[i * N + j] + b[i * N + j];
            }
        }
        for i in 0..N {
            for j in 0..N {
                a[i * N + j] = c[i * N + j] * 0.5 + 1.0;
            }
        }
    }

    let mut total = 0.0f64;
    for i in 0..N {
        total += a[i * N + i];
    }
    total
}

fn main() {
    let s = sieve_sum(LIMIT);
    let m = matrix_work();
    println!("checksum {} {:.6}", s, m);
}