use super::{Blend, Style};

// A texel with the style's flash mixed in and its alpha faded.
#[inline(always)]
pub(super) fn styled(p: u32, st: &Style) -> u32 {
    let a = ((p >> 24) * st.opacity as u32 + 127) / 255;
    let rgb = if st.flash_amount == 0 { p & 0xFF_FFFF } else { blend(st.flash, p & 0xFF_FFFF, st.flash_amount as u32) };
    a << 24 | rgb
}

#[inline(always)]
pub(super) fn put_with(d: &mut u32, p: u32, st: &Style) {
    let p = styled(p, st);
    match st.blend {
        Blend::Alpha => put(d, p),
        Blend::Add => *d = add_flat(*d, p & 0xFF_FFFF, p >> 24),
        Blend::Multiply => *d = mul_flat(*d, p & 0xFF_FFFF, p >> 24),
    }
}

// `col` scaled by `a` out of 255 and added to `dst`, each channel stopping at 255.
#[inline(always)]
pub(super) fn add_flat(dst: u32, col: u32, a: u32) -> u32 {
    let ch = |shift: u32| (((dst >> shift & 255) + ((col >> shift & 255) * a + 127) / 255).min(255)) << shift;
    ch(16) | ch(8) | ch(0)
}

// `dst` times `col` (each channel out of 255), laid over `dst` by `a` out of 255.
#[inline(always)]
pub(super) fn mul_flat(dst: u32, col: u32, a: u32) -> u32 {
    let ch = |shift: u32| {
        let (d, c) = (dst >> shift & 255, col >> shift & 255);
        ((d * (255 - a) + (d * c + 127) / 255 * a + 127) / 255) << shift
    };
    ch(16) | ch(8) | ch(0)
}

#[inline(always)]
pub(super) fn put(d: &mut u32, p: u32) {
    match p >> 24 {
        255 => *d = p,
        0 => {}
        a => *d = blend(p, *d, a),
    }
}

#[inline(always)]
pub(super) fn blend_row_plain(row: &mut [u32], src: u32, a: u32) {
    for px in row {
        *px = blend(src, *px, a);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
pub(super) fn blend_row_avx2(row: &mut [u32], src: u32, a: u32) {
    blend_row_plain(row, src, a);
}

pub(super) fn blend_row(row: &mut [u32], src: u32, a: u32) {
    #[cfg(target_arch = "x86_64")]
    if std::is_x86_feature_detected!("avx2") {
        // SAFETY: the CPU just reported AVX2 support.
        return unsafe { blend_row_avx2(row, src, a) };
    }
    blend_row_plain(row, src, a);
}

pub(super) fn upscale(src: &[u32], w: usize, scale: usize, dst: &mut [u32]) {
    let out_w = w * scale;
    for (y, row) in src.chunks_exact(w).enumerate() {
        let first = y * scale * out_w;
        for (px, run) in row.iter().zip(dst[first..first + out_w].chunks_exact_mut(scale)) {
            run.fill(*px);
        }
        for k in 1..scale {
            dst.copy_within(first..first + out_w, first + k * out_w);
        }
    }
}

#[inline(always)]
pub(super) fn blend(src: u32, dst: u32, a: u32) -> u32 {
    let a = a + (a >> 7);
    let inv = 256 - a;
    let rb = ((src & 0x00ff_00ff) * a + (dst & 0x00ff_00ff) * inv) >> 8 & 0x00ff_00ff;
    let g = ((src & 0x0000_ff00) * a + (dst & 0x0000_ff00) * inv) >> 8 & 0x0000_ff00;
    rb | g
}
