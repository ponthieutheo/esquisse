//! The LZ77 variant of R2004+ pages (spec §4.7), greedy with hash chains. Back-references
//! reach 0x4000 bytes (opcodes 0x20–0xFF); a back-reference distance is the coded offset
//! plus one, as AutoCAD's files have it. Streams end with 0x11 0x00 0x00, like AutoCAD's.

const WINDOW: usize = 0x4000;
const MIN_MATCH: usize = 3;
const MAX_CHAIN: usize = 48;

/// A literal length (≥ 4 bytes): `n - 3` in one byte up to 18, else 0x00, zero bytes
/// worth 0xFF each and a last non-zero byte.
fn literal_length(n: usize, out: &mut Vec<u8>) {
    debug_assert!(n >= 4);
    if n - 3 <= 0x0F {
        out.push((n - 3) as u8);
        return;
    }
    out.push(0);
    long_count(n - 3 - 0x0F, out);
}

/// The tail of a long count (≥ 1): zero bytes worth 0xFF each, then the rest.
fn long_count(mut r: usize, out: &mut Vec<u8>) {
    while r > 0xFF {
        out.push(0);
        r -= 0xFF;
    }
    out.push(r as u8);
}

/// A back-reference of `len` bytes `dist` bytes back, followed by `lit` (0–3) literals.
fn back_reference(len: usize, dist: usize, lit: u8, out: &mut Vec<u8>) {
    let off = dist - 1;
    if len <= 14 && dist <= 0x400 {
        out.push(((len as u8 + 1) << 4) | (((off & 3) as u8) << 2) | lit);
        out.push((off >> 2) as u8);
    } else {
        if len - 2 <= 0x1F {
            out.push(0x20 | (len - 2) as u8);
        } else {
            out.push(0x20);
            long_count(len - 2 - 0x1F, out);
        }
        out.push((((off & 0x3F) as u8) << 2) | lit);
        out.push((off >> 6) as u8);
    }
}

fn hash(d: &[u8], i: usize) -> usize {
    ((usize::from(d[i]) << 10) ^ (usize::from(d[i + 1]) << 5) ^ usize::from(d[i + 2])) & 0x7FFF
}

/// Compresses one page.
pub(super) fn compress(data: &[u8]) -> Vec<u8> {
    let n = data.len();
    let mut out = Vec::with_capacity(n / 2 + 16);
    if n < 4 {
        // Too short for a literal run: zeros complete it (the section size cuts them off).
        let mut d = data.to_vec();
        d.resize(4, 0);
        return compress(&d);
    }
    let mut head = vec![usize::MAX; 0x8000];
    let mut prev = vec![usize::MAX; n];
    let insert = |head: &mut Vec<usize>, prev: &mut Vec<usize>, i: usize| {
        if i + MIN_MATCH <= n {
            let k = hash(data, i);
            prev[i] = head[k];
            head[k] = i;
        }
    };
    // Tokens: (literal start, literal end, match length, distance).
    let mut tokens: Vec<(usize, usize, usize, usize)> = Vec::new();
    let mut lit_start = 0;
    let mut i = 0;
    while i < n {
        let mut best = (0, 0);
        // The first literal run is at least 4 bytes; a match needs something to copy.
        if i >= 4 && i + MIN_MATCH <= n {
            let mut j = head[hash(data, i)];
            let mut chain = 0;
            while j != usize::MAX && i - j <= WINDOW && chain < MAX_CHAIN {
                let mut l = 0;
                while i + l < n && data[j + l] == data[i + l] && l < 0x7400 {
                    l += 1;
                }
                if l > best.0 {
                    best = (l, i - j);
                }
                j = prev[j];
                chain += 1;
            }
        }
        if best.0 >= MIN_MATCH {
            tokens.push((lit_start, i, best.0, best.1));
            for k in i..i + best.0 {
                insert(&mut head, &mut prev, k);
            }
            i += best.0;
            lit_start = i;
        } else {
            insert(&mut head, &mut prev, i);
            i += 1;
        }
    }
    // The first token's literals open the stream; each match carries the count of the
    // literals that follow it.
    let first_end = tokens.first().map_or(n, |t| t.1);
    literal_length(first_end, &mut out);
    out.extend_from_slice(&data[..first_end]);
    for (k, &(_, _, len, dist)) in tokens.iter().enumerate() {
        let (ls, le) = match tokens.get(k + 1) {
            Some(t) => (t.0, t.1),
            None => (lit_start, n),
        };
        let lits = le - ls;
        let small = if (1..=3).contains(&lits) { lits as u8 } else { 0 };
        back_reference(len, dist, small, &mut out);
        if lits >= 4 {
            literal_length(lits, &mut out);
        }
        out.extend_from_slice(&data[ls..le]);
    }
    out.extend([0x11, 0x00, 0x00]);
    out
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// The decoder of §4.7 as AutoCAD's files need it (distance = offset + 1).
    pub fn decompress(src: &[u8]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        let mut p = 0usize;
        let mut next = || {
            p += 1;
            src[p - 1]
        };
        fn lit_len(code: u8, next: &mut impl FnMut() -> u8) -> usize {
            let mut t = usize::from(code & 0x0F);
            if t == 0 {
                loop {
                    let b = next();
                    if b == 0 {
                        t += 0xFF;
                    } else {
                        t += 0x0F + usize::from(b);
                        break;
                    }
                }
            }
            t + 3
        }
        fn count(op: u8, mask: u8, next: &mut impl FnMut() -> u8) -> usize {
            let mut t = usize::from(op & mask);
            if t == 0 {
                loop {
                    let b = next();
                    if b == 0 {
                        t += 0xFF;
                    } else {
                        t += usize::from(b) + usize::from(mask);
                        break;
                    }
                }
            }
            t + 2
        }
        let mut op = next();
        if op & 0xF0 == 0 {
            let l = lit_len(op, &mut next);
            for _ in 0..l {
                out.push(next());
            }
            op = next();
        }
        while op != 0x11 {
            let (c, d, lit) = match op {
                0x10..=0x1F => {
                    let c = count(op, 7, &mut next);
                    let (f, s) = (next(), next());
                    (c, ((usize::from(op & 8) << 11) | (usize::from(f) >> 2) | (usize::from(s) << 6)) + 0x4000, f)
                }
                0x20..=0x3F => {
                    let c = count(op, 0x1F, &mut next);
                    let (f, s) = (next(), next());
                    (c, ((usize::from(f) >> 2) | (usize::from(s) << 6)) + 1, f)
                }
                _ => {
                    let s = next();
                    (usize::from(op >> 4) - 1, (usize::from((op >> 2) & 3) | (usize::from(s) << 2)) + 1, op)
                }
            };
            let start = out.len() - d;
            for k in 0..c {
                out.push(out[start + k]);
            }
            let mut l = usize::from(lit & 3);
            if l == 0 {
                op = next();
                if op & 0xF0 == 0 {
                    l = lit_len(op, &mut next);
                }
            }
            if l > 0 {
                for _ in 0..l {
                    out.push(next());
                }
                op = next();
            }
        }
        out
    }

    #[test]
    fn round_trips() {
        let mut x: u32 = 7;
        let mut rnd = || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x
        };
        let mut cases: Vec<Vec<u8>> = vec![
            b"abcd".to_vec(),
            b"abcdabcdabcdabcdabcd".to_vec(),
            vec![0; 1000],
            (0..=255u8).collect(),
            b"x".to_vec(),
        ];
        // Text-like data with repeats at every distance, and runs of every length.
        let mut t = Vec::new();
        for i in 0..0x7400usize {
            let r = rnd();
            t.push(if r % 5 == 0 {
                (r >> 8) as u8
            } else {
                t.get(i.saturating_sub((r as usize >> 12) % 0x5000 + 1)).copied().unwrap_or(b'a')
            });
        }
        cases.push(t);
        for len in [3, 4, 5, 14, 15, 18, 19, 33, 34, 290, 600] {
            let mut d: Vec<u8> = (0..len as u8).collect();
            d.extend(std::iter::repeat_n(9u8, len));
            d.extend((0..len).map(|k| (k * 7) as u8));
            d.extend((0..len).map(|k| (k * 7) as u8));
            cases.push(d);
        }
        for d in cases {
            let c = compress(&d);
            let back = decompress(&c);
            assert_eq!(&back[..d.len()], &d[..], "{} bytes", d.len());
            assert!(c.ends_with(&[0x11, 0, 0]));
        }
        assert!(compress(&vec![0; 0x7400]).len() < 200);
    }
}
