//! `HCOMPRESS_1` tile decoding: a port of cfitsio's `fits_hdecompress.c`
//! (R. White, STScI).
//!
//! A tile is a quadtree-coded set of bit planes of H-transform coefficients:
//! the coefficients are decoded, multiplied back by the scale factor, and
//! the H-transform inverted, optionally smoothing the coefficients first
//! (`SMOOTH = 1`). cfitsio does this arithmetic in 32-bit integers for
//! `ZBITPIX` 8 and 16 and in 64-bit integers otherwise, wrapping on
//! overflow; both are reproduced so lossy (scaled) images decode to the same
//! values. Unlike the C, reads past the end of the tile and dimensions that
//! don't match the tile are errors.

use alloc::vec;
use alloc::vec::Vec;

use crate::error::{Error, Result};

const MAGIC: [u8; 2] = [0xDD, 0x99];

/// Decode one `HCOMPRESS_1` tile of `pixels` pixels, in tile raster order.
///
/// `wide` selects cfitsio's 64-bit arithmetic (`ZBITPIX` 32, and quantized
/// floats); otherwise it is 32-bit (`ZBITPIX` 8 and 16).
pub(crate) fn decompress(
    input: &[u8],
    pixels: usize,
    smooth: bool,
    wide: bool,
) -> Result<Vec<i32>> {
    if wide {
        run::<i64>(input, pixels, smooth)
    } else {
        run::<i32>(input, pixels, smooth)
    }
}

fn corrupt(what: &'static str) -> Error {
    Error::DecompressionError(what)
}

/// The integer arithmetic of cfitsio's `int` or `LONGLONG` code, wrapping.
trait HInt: Copy + Ord + Default {
    const BITS: u32;
    fn from_i64(v: i64) -> Self;
    fn to_i32(self) -> i32;
    fn add(self, o: Self) -> Self;
    fn sub(self, o: Self) -> Self;
    fn mul(self, o: Self) -> Self;
    fn shl(self, n: u32) -> Self;
    /// Arithmetic shift right.
    fn shr(self, n: u32) -> Self;
    fn and(self, o: Self) -> Self;
    fn or(self, o: Self) -> Self;
    fn xor(self, o: Self) -> Self;
    fn neg(self) -> Self;
}

macro_rules! hint {
    ($t:ty) => {
        impl HInt for $t {
            const BITS: u32 = <$t>::BITS;
            fn from_i64(v: i64) -> Self {
                v as $t
            }
            fn to_i32(self) -> i32 {
                self as i32
            }
            fn add(self, o: Self) -> Self {
                self.wrapping_add(o)
            }
            fn sub(self, o: Self) -> Self {
                self.wrapping_sub(o)
            }
            fn mul(self, o: Self) -> Self {
                self.wrapping_mul(o)
            }
            fn shl(self, n: u32) -> Self {
                self.wrapping_shl(n)
            }
            fn shr(self, n: u32) -> Self {
                self.wrapping_shr(n)
            }
            fn and(self, o: Self) -> Self {
                self & o
            }
            fn or(self, o: Self) -> Self {
                self | o
            }
            fn xor(self, o: Self) -> Self {
                self ^ o
            }
            fn neg(self) -> Self {
                self.wrapping_neg()
            }
        }
    };
}

hint!(i32);
hint!(i64);

fn int<T: HInt>(v: i64) -> T {
    T::from_i64(v)
}

/// cfitsio's byte and bit input. Reading past the end yields zeros and is
/// reported once decoding is done.
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    buffer: u32,
    bits_to_go: u32,
    overrun: bool,
}

impl<'a> Reader<'a> {
    fn byte(&mut self) -> u8 {
        let b = self.data.get(self.pos).copied().unwrap_or_else(|| {
            self.overrun = true;
            0
        });
        self.pos += 1;
        b
    }

    fn bytes<const N: usize>(&mut self) -> [u8; N] {
        let mut out = [0u8; N];
        for b in &mut out {
            *b = self.byte();
        }
        out
    }

    fn start_bits(&mut self) {
        self.bits_to_go = 0;
    }

    fn bit(&mut self) -> u32 {
        if self.bits_to_go == 0 {
            self.buffer = u32::from(self.byte());
            self.bits_to_go = 8;
        }
        self.bits_to_go -= 1;
        (self.buffer >> self.bits_to_go) & 1
    }

    /// Up to 8 bits.
    fn bits(&mut self, n: u32) -> u32 {
        if self.bits_to_go < n {
            self.buffer = (self.buffer << 8) | u32::from(self.byte());
            self.bits_to_go += 8;
        }
        self.bits_to_go -= n;
        (self.buffer >> self.bits_to_go) & ((1 << n) - 1)
    }

    fn nybble(&mut self) -> u8 {
        self.bits(4) as u8
    }

    /// One of the fixed Huffman codes for the 4-bit quadtree values.
    fn huffman(&mut self) -> u8 {
        let mut c = self.bits(3);
        if c < 4 {
            return 1 << c;
        }
        c = self.bit() | (c << 1);
        match c {
            8 => return 3,
            9 => return 5,
            10 => return 10,
            11 => return 12,
            12 => return 15,
            _ => {}
        }
        c = self.bit() | (c << 1);
        match c {
            26 => return 6,
            27 => return 7,
            28 => return 9,
            29 => return 11,
            30 => return 13,
            _ => {}
        }
        c = self.bit() | (c << 1);
        if c == 62 {
            0
        } else {
            14
        }
    }
}

/// `fits_hdecompress`/`fits_hdecompress64`.
fn run<T: HInt>(input: &[u8], pixels: usize, smooth: bool) -> Result<Vec<i32>> {
    let mut r = Reader {
        data: input,
        pos: 0,
        buffer: 0,
        bits_to_go: 0,
        overrun: false,
    };
    if r.bytes::<2>() != MAGIC {
        return Err(corrupt("HCOMPRESS tile has no magic number"));
    }
    let nx = i32::from_be_bytes(r.bytes());
    let ny = i32::from_be_bytes(r.bytes());
    let scale = i32::from_be_bytes(r.bytes());
    let sumall = i64::from_be_bytes(r.bytes());
    let nbitplanes: [u8; 3] = r.bytes();
    let (nx, ny) = match (usize::try_from(nx), usize::try_from(ny)) {
        (Ok(nx), Ok(ny)) if nx > 0 && ny > 0 && nx.checked_mul(ny) == Some(pixels) => (nx, ny),
        _ => return Err(corrupt("HCOMPRESS tile dimensions don't match the tile")),
    };
    if nbitplanes.iter().any(|&n| u32::from(n) > T::BITS) {
        return Err(corrupt("HCOMPRESS tile has too many bit planes"));
    }

    let mut a = vec![T::default(); pixels];
    dodecode(&mut r, &mut a, nx, ny, nbitplanes)?;
    a[0] = int(sumall);
    if r.overrun {
        return Err(corrupt("HCOMPRESS tile is truncated"));
    }
    if scale > 1 {
        let scale = int::<T>(i64::from(scale));
        for v in &mut a {
            *v = v.mul(scale);
        }
    }
    hinv(&mut a, nx, ny, smooth, scale);
    Ok(a.into_iter().map(HInt::to_i32).collect())
}

/// Decode the four quadrants' bit planes, then the signs.
fn dodecode<T: HInt>(
    r: &mut Reader,
    a: &mut [T],
    nx: usize,
    ny: usize,
    nbitplanes: [u8; 3],
) -> Result<()> {
    let nx2 = nx.div_ceil(2);
    let ny2 = ny.div_ceil(2);
    r.start_bits();
    qtree_decode(r, a, 0, ny, nx2, ny2, nbitplanes[0])?;
    qtree_decode(r, a, ny2, ny, nx2, ny / 2, nbitplanes[1])?;
    qtree_decode(r, a, ny * nx2, ny, nx / 2, ny2, nbitplanes[1])?;
    qtree_decode(r, a, ny * nx2 + ny2, ny, nx / 2, ny / 2, nbitplanes[2])?;
    if r.nybble() != 0 {
        return Err(corrupt("HCOMPRESS tile has bad bit plane values"));
    }
    r.start_bits();
    for v in a.iter_mut() {
        if *v != T::default() && r.bit() != 0 {
            *v = v.neg();
        }
    }
    Ok(())
}

/// cfitsio's `log2n`: log2 of `n` rounded up.
fn log2_ceil(n: usize) -> u32 {
    if n <= 1 {
        0
    } else {
        usize::BITS - (n - 1).leading_zeros()
    }
}

/// Decode `nbitplanes` bit planes of the `nqx` × `nqy` quadrant at `base` in
/// `a`, whose rows are `n` long.
fn qtree_decode<T: HInt>(
    r: &mut Reader,
    a: &mut [T],
    base: usize,
    n: usize,
    nqx: usize,
    nqy: usize,
    nbitplanes: u8,
) -> Result<()> {
    let log2n = log2_ceil(nqx.max(nqy));
    let codes = nqx.div_ceil(2) * nqy.div_ceil(2);
    let mut scratch = vec![0u8; codes.max(1)];
    for bit in (0..u32::from(nbitplanes)).rev() {
        match r.nybble() {
            // Written directly, four pixels a nybble.
            0 => {
                for s in &mut scratch[..codes] {
                    *s = r.nybble();
                }
            }
            0xf => {
                scratch[0] = r.huffman();
                let (mut nx, mut ny) = (1usize, 1usize);
                let (mut nfx, mut nfy) = (nqx, nqy);
                let mut c = 1usize << log2n;
                for _ in 1..log2n {
                    c >>= 1;
                    nx <<= 1;
                    ny <<= 1;
                    if nfx <= c {
                        nx -= 1;
                    } else {
                        nfx -= c;
                    }
                    if nfy <= c {
                        ny -= 1;
                    } else {
                        nfy -= c;
                    }
                    qtree_expand(r, &mut scratch, nx, ny);
                }
            }
            _ => return Err(corrupt("HCOMPRESS tile has a bad quadtree code")),
        }
        qtree_bitins(&scratch, nqx, nqy, a, base, n, bit);
    }
    Ok(())
}

/// Expand the 4-bit codes in `b` to `nx` × `ny`, reading a new code for every
/// non-zero one.
fn qtree_expand(r: &mut Reader, b: &mut [u8], nx: usize, ny: usize) {
    qtree_copy(b, nx, ny, ny);
    for i in (0..nx * ny).rev() {
        if b[i] != 0 {
            b[i] = r.huffman();
        }
    }
}

/// Spread the `(nx+1)/2` × `(ny+1)/2` 4-bit codes at the start of `b` over
/// `nx` × `ny` (rows `n` long), one bit per pixel of each 2×2 block.
fn qtree_copy(b: &mut [u8], nx: usize, ny: usize, n: usize) {
    let nx2 = nx.div_ceil(2);
    let ny2 = ny.div_ceil(2);
    // Back to front, since the codes and the result share `b`.
    let mut k = ny2 * nx2;
    for i in (0..nx2).rev() {
        for j in (0..ny2).rev() {
            k -= 1;
            b[2 * (n * i + j)] = b[k];
        }
    }
    let mut i = 0;
    while i + 1 < nx {
        let s00 = n * i;
        let s10 = s00 + n;
        let mut j = 0;
        while j + 1 < ny {
            let v = b[s00 + j];
            b[s10 + j + 1] = v & 1;
            b[s10 + j] = (v >> 1) & 1;
            b[s00 + j + 1] = (v >> 2) & 1;
            b[s00 + j] = (v >> 3) & 1;
            j += 2;
        }
        if j < ny {
            let v = b[s00 + j];
            b[s10 + j] = (v >> 1) & 1;
            b[s00 + j] = (v >> 3) & 1;
        }
        i += 2;
    }
    if i < nx {
        let s00 = n * i;
        let mut j = 0;
        while j + 1 < ny {
            let v = b[s00 + j];
            b[s00 + j + 1] = (v >> 2) & 1;
            b[s00 + j] = (v >> 3) & 1;
            j += 2;
        }
        if j < ny {
            b[s00 + j] = (b[s00 + j] >> 3) & 1;
        }
    }
}

/// Set bit plane `bit` of the `nx` × `ny` block at `base` in `b` (rows `n`
/// long) from the 4-bit codes in `a`, one per 2×2 block.
fn qtree_bitins<T: HInt>(
    a: &[u8],
    nx: usize,
    ny: usize,
    b: &mut [T],
    base: usize,
    n: usize,
    bit: u32,
) {
    let plane = int::<T>(1).shl(bit);
    let mut set = |at: usize| b[base + at] = b[base + at].or(plane);
    let mut k = 0;
    let mut i = 0;
    while i + 1 < nx {
        let s00 = n * i;
        let mut j = 0;
        while j + 1 < ny {
            let v = a[k];
            if v & 1 != 0 {
                set(s00 + n + j + 1);
            }
            if v & 2 != 0 {
                set(s00 + n + j);
            }
            if v & 4 != 0 {
                set(s00 + j + 1);
            }
            if v & 8 != 0 {
                set(s00 + j);
            }
            k += 1;
            j += 2;
        }
        if j < ny {
            let v = a[k];
            if v & 2 != 0 {
                set(s00 + n + j);
            }
            if v & 8 != 0 {
                set(s00 + j);
            }
            k += 1;
        }
        i += 2;
    }
    if i < nx {
        let s00 = n * i;
        let mut j = 0;
        while j + 1 < ny {
            let v = a[k];
            if v & 4 != 0 {
                set(s00 + j + 1);
            }
            if v & 8 != 0 {
                set(s00 + j);
            }
            k += 1;
            j += 2;
        }
        if j < ny && a[k] & 8 != 0 {
            set(s00 + j);
        }
    }
}

/// Interleave the two halves of the `n` elements of `a` at stride `n2`
/// starting at `start`: the first half to the even places, the second half
/// to the odd ones.
fn unshuffle<T: HInt>(a: &mut [T], start: usize, n: usize, n2: usize, tmp: &mut [T]) {
    let nhalf = n.div_ceil(2);
    for (t, i) in tmp.iter_mut().zip(nhalf..n) {
        *t = a[start + n2 * i];
    }
    for i in (0..nhalf).rev() {
        a[start + 2 * n2 * i] = a[start + n2 * i];
    }
    for (t, i) in tmp.iter().zip((1..n).step_by(2)) {
        a[start + n2 * i] = *t;
    }
}

/// The inverse H-transform of the `nx` × `ny` coefficients in `a`.
fn hinv<T: HInt>(a: &mut [T], nx: usize, ny: usize, smooth: bool, scale: i32) {
    let nmax = nx.max(ny);
    let log2n = log2_ceil(nmax);
    let mut tmp = vec![T::default(); nmax.div_ceil(2)];

    let mut shift = 1;
    let mut bit0 = int::<T>(1).shl(log2n.saturating_sub(1));
    let mut bit1 = bit0.shl(1);
    let mut bit2 = bit0.shl(2);
    let mut mask0 = bit0.neg();
    let mut mask1 = mask0.shl(1);
    let mask2 = mask0.shl(2);
    let mut prnd0 = bit0.shr(1);
    let mut prnd1 = bit1.shr(1);
    let prnd2 = bit2.shr(1);
    let one = int::<T>(1);
    let zero = T::default();
    let mut nrnd0 = prnd0.sub(one);
    let mut nrnd1 = prnd1.sub(one);
    let nrnd2 = prnd2.sub(one);
    // Round to a multiple of `mask`'s low bit, as the transform rounded.
    let round =
        |v: T, prnd: T, nrnd: T, mask: T| v.add(if v >= zero { prnd } else { nrnd }).and(mask);

    a[0] = round(a[0], prnd2, nrnd2, mask2);

    let (mut nxtop, mut nytop) = (1usize, 1usize);
    let (mut nxf, mut nyf) = (nx, ny);
    let mut c = 1usize << log2n;
    for k in (0..log2n).rev() {
        c >>= 1;
        nxtop <<= 1;
        nytop <<= 1;
        if nxf <= c {
            nxtop -= 1;
        } else {
            nxf -= c;
        }
        if nyf <= c {
            nytop -= 1;
        } else {
            nyf -= c;
        }
        if k == 0 {
            nrnd0 = zero;
            shift = 2;
        }
        for i in 0..nxtop {
            unshuffle(a, ny * i, nytop, 1, &mut tmp);
        }
        for j in 0..nytop {
            unshuffle(a, j, nxtop, ny, &mut tmp);
        }
        if smooth {
            hsmooth(a, nxtop, nytop, ny, scale);
        }
        let oddx = nxtop % 2;
        let oddy = nytop % 2;
        let mut i = 0;
        while i < nxtop - oddx {
            let mut s00 = ny * i;
            let mut s10 = s00 + ny;
            let mut j = 0;
            while j < nytop - oddy {
                let mut h0 = a[s00];
                let hx = round(a[s10], prnd1, nrnd1, mask1);
                let hy = round(a[s00 + 1], prnd1, nrnd1, mask1);
                let hc = round(a[s10 + 1], prnd0, nrnd0, mask0);
                // Propagate bit 0 of hc to hx and hy, then bits 0 and 1 of
                // hc, hx, hy to h0.
                let lowbit0 = hc.and(bit0);
                let hx = if hx >= zero {
                    hx.sub(lowbit0)
                } else {
                    hx.add(lowbit0)
                };
                let hy = if hy >= zero {
                    hy.sub(lowbit0)
                } else {
                    hy.add(lowbit0)
                };
                let lowbit1 = hc.xor(hx).xor(hy).and(bit1);
                h0 = if h0 >= zero {
                    h0.add(lowbit0).sub(lowbit1)
                } else if lowbit0 == zero {
                    h0.add(lowbit1)
                } else {
                    h0.add(lowbit0.sub(lowbit1))
                };
                a[s10 + 1] = h0.add(hx).add(hy).add(hc).shr(shift);
                a[s10] = h0.add(hx).sub(hy).sub(hc).shr(shift);
                a[s00 + 1] = h0.sub(hx).add(hy).sub(hc).shr(shift);
                a[s00] = h0.sub(hx).sub(hy).add(hc).shr(shift);
                s00 += 2;
                s10 += 2;
                j += 2;
            }
            if oddy == 1 {
                let h0 = a[s00];
                let hx = round(a[s10], prnd1, nrnd1, mask1);
                let lowbit1 = hx.and(bit1);
                let h0 = if h0 >= zero {
                    h0.sub(lowbit1)
                } else {
                    h0.add(lowbit1)
                };
                a[s10] = h0.add(hx).shr(shift);
                a[s00] = h0.sub(hx).shr(shift);
            }
            i += 2;
        }
        if oddx == 1 {
            let mut s00 = ny * i;
            let mut j = 0;
            while j < nytop - oddy {
                let h0 = a[s00];
                let hy = round(a[s00 + 1], prnd1, nrnd1, mask1);
                let lowbit1 = hy.and(bit1);
                let h0 = if h0 >= zero {
                    h0.sub(lowbit1)
                } else {
                    h0.add(lowbit1)
                };
                a[s00 + 1] = h0.add(hy).shr(shift);
                a[s00] = h0.sub(hy).shr(shift);
                s00 += 2;
                j += 2;
            }
            if oddy == 1 {
                a[s00] = a[s00].shr(shift);
            }
        }
        bit2 = bit1;
        bit1 = bit0;
        bit0 = bit0.shr(1);
        mask1 = mask0;
        mask0 = mask0.shr(1);
        prnd1 = prnd0;
        prnd0 = prnd0.shr(1);
        nrnd1 = nrnd0;
        nrnd0 = prnd0.sub(one);
    }
    let _ = bit2;
}

/// Adjust the H-transform coefficients toward values interpolated from their
/// neighbours, by at most half the scale factor, as `SMOOTH = 1` asks.
fn hsmooth<T: HInt>(a: &mut [T], nxtop: usize, nytop: usize, ny: usize, scale: i32) {
    let smax = int::<T>(i64::from(scale >> 1));
    let zero = T::default();
    if smax <= zero {
        return;
    }
    let ny2 = ny << 1;
    let clamp = |s: T| s.min(smax).max(smax.neg());
    // Divide by 2^bits, rounding negative values as cfitsio does.
    let div = |s: T, bits: u32| {
        if s >= zero {
            s.shr(bits)
        } else {
            s.add(int::<T>((1 << bits) - 1)).shr(bits)
        }
    };

    // x differences
    let mut i = 2;
    while i + 2 < nxtop {
        let mut s00 = ny * i;
        let mut s10 = s00 + ny;
        let mut j = 0;
        while j < nytop {
            let hm = a[s00 - ny2];
            let h0 = a[s00];
            let hp = a[s00 + ny2];
            let dmax = hp.sub(h0).min(h0.sub(hm)).max(zero).shl(2);
            let dmin = hp.sub(h0).max(h0.sub(hm)).min(zero).shl(2);
            if dmin < dmax {
                let diff = hp.sub(hm).min(dmax).max(dmin);
                let s = clamp(div(diff.sub(a[s10].shl(3)), 3));
                a[s10] = a[s10].add(s);
            }
            s00 += 2;
            s10 += 2;
            j += 2;
        }
        i += 2;
    }
    // y differences
    let mut i = 0;
    while i < nxtop {
        let mut s00 = ny * i + 2;
        let mut j = 2;
        while j + 2 < nytop {
            let hm = a[s00 - 2];
            let h0 = a[s00];
            let hp = a[s00 + 2];
            let dmax = hp.sub(h0).min(h0.sub(hm)).max(zero).shl(2);
            let dmin = hp.sub(h0).max(h0.sub(hm)).min(zero).shl(2);
            if dmin < dmax {
                let diff = hp.sub(hm).min(dmax).max(dmin);
                let s = clamp(div(diff.sub(a[s00 + 1].shl(3)), 3));
                a[s00 + 1] = a[s00 + 1].add(s);
            }
            s00 += 2;
            j += 2;
        }
        i += 2;
    }
    // curvature differences
    let mut i = 2;
    while i + 2 < nxtop {
        let mut s00 = ny * i + 2;
        let mut s10 = s00 + ny;
        let mut j = 2;
        while j + 2 < nytop {
            let hmm = a[s00 - ny2 - 2];
            let hpm = a[s00 + ny2 - 2];
            let hmp = a[s00 - ny2 + 2];
            let hpp = a[s00 + ny2 + 2];
            let h0 = a[s00];
            let diff = hpp.add(hmm).sub(hmp).sub(hpm);
            let hx2 = a[s10].shl(1);
            let hy2 = a[s00 + 1].shl(1);
            let m1 = hpp
                .sub(h0)
                .max(zero)
                .sub(hx2)
                .sub(hy2)
                .min(h0.sub(hpm).max(zero).add(hx2).sub(hy2));
            let m2 = h0
                .sub(hmp)
                .max(zero)
                .sub(hx2)
                .add(hy2)
                .min(hmm.sub(h0).max(zero).add(hx2).add(hy2));
            let dmax = m1.min(m2).shl(4);
            let m1 = hpp
                .sub(h0)
                .min(zero)
                .sub(hx2)
                .sub(hy2)
                .max(h0.sub(hpm).min(zero).add(hx2).sub(hy2));
            let m2 = h0
                .sub(hmp)
                .min(zero)
                .sub(hx2)
                .add(hy2)
                .max(hmm.sub(h0).min(zero).add(hx2).add(hy2));
            let dmin = m1.max(m2).shl(4);
            if dmin < dmax {
                let diff = diff.min(dmax).max(dmin);
                let s = clamp(div(diff.sub(a[s10 + 1].shl(6)), 6));
                a[s10 + 1] = a[s10 + 1].add(s);
            }
            s00 += 2;
            s10 += 2;
            j += 2;
        }
        i += 2;
    }
}
