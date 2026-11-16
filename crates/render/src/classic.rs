//! Classical enlargement in premultiplied linear light. Catmull–Rom reconstruction (16 taps),
//! followed by a mild residual sharpen. Clamp to the surrounding four source pixels prevents
//! overshoot and ringing on high-contrast edges. The GPU implements the same equations.
use crate::Image;
use filmcraft_geom::{Affine, Rect};
use rayon::prelude::*;

fn cubic(x: f32) -> f32 {
    let x = x.abs();
    if x < 1.0 {
        1.5 * x * x * x - 2.5 * x * x + 1.0
    } else if x < 2.0 {
        -0.5 * x * x * x + 2.5 * x * x - 4.0 * x + 2.0
    } else {
        0.0
    }
}
pub fn sample(src: &Image, x: f32, y: f32, sharpness: f32) -> [f32; 4] {
    let (qx, qy) = (x - 0.5, y - 0.5);
    let (ix, iy) = (qx.floor() as isize, qy.floor() as isize);
    let mut acc = [0.0f32; 4];
    for j in -1..=2 {
        for i in -1..=2 {
            let weight = cubic(qx - (ix + i) as f32) * cubic(qy - (iy + j) as f32);
            let p = src.get_clamped(ix + i, iy + j);
            for k in 0..4 {
                acc[k] += p[k] * weight;
            }
        }
    }
    let mut lo = [f32::INFINITY; 4];
    let mut hi = [f32::NEG_INFINITY; 4];
    for j in 0..=1 {
        for i in 0..=1 {
            let p = src.get_clamped(ix + i, iy + j);
            for k in 0..4 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
    }
    let smooth = src.sample_bilinear_clamped(x, y);
    let amount = sharpness.clamp(0.0, 100.0) * 0.0025;
    for k in 0..4 {
        acc[k] = (acc[k] + amount * (acc[k] - smooth[k])).clamp(lo[k], hi[k]);
    }
    acc
}
pub fn transformed(src: &Image, w: usize, h: usize, m: &Affine, sharpness: f32) -> Image {
    let mut out = Image::new(w, h);
    if src.w == 0 || src.h == 0 || w == 0 || h == 0 {
        return out;
    }
    let Some(inv) = m.inverse() else { return out };
    let b = m.bounds(&Rect::new(0.0, 0.0, src.w as f64, src.h as f64));
    let (x0, x1) = ((b.x.floor().max(0.0) as usize).min(w), (b.right().ceil().max(0.0) as usize).min(w));
    let (y0, y1) = ((b.y.floor().max(0.0) as usize).min(h), (b.bottom().ceil().max(0.0) as usize).min(h));
    out.px.par_chunks_mut(w * 4).enumerate().skip(y0).take(y1.saturating_sub(y0)).for_each(|(y, row)| {
        for x in x0..x1 {
            let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
            let (u, v) = (inv.a * px + inv.c * py + inv.e, inv.b * px + inv.d * py + inv.f);
            if u >= 0.0 && v >= 0.0 && u < src.w as f64 && v < src.h as f64 {
                row[x * 4..x * 4 + 4].copy_from_slice(&sample(src, u as f32, v as f32, sharpness));
            }
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flat_fields_edges_and_sharpen_have_no_overshoot() {
        let flat = Image::filled(5, 3, [0.3, 0.4, 0.5, 1.0]);
        for sharpness in [0.0, 50.0, 100.0] {
            let large = transformed(&flat, 10, 6, &Affine::scale(2.0, 2.0), sharpness);
            assert!(large.px.chunks_exact(4).all(|p| p.iter().zip([0.3, 0.4, 0.5, 1.0]).all(|(a, b)| (a - b).abs() < 1e-5)));
        }
        let mut edge = Image::filled(8, 4, [0.1, 0.1, 0.1, 1.0]);
        for y in 0..4 {
            for x in 4..8 {
                edge.px[(y * 8 + x) * 4..(y * 8 + x) * 4 + 4].copy_from_slice(&[0.8, 0.8, 0.8, 1.0]);
            }
        }
        let a = transformed(&edge, 16, 8, &Affine::scale(2.0, 2.0), 0.0);
        let b = transformed(&edge, 16, 8, &Affine::scale(2.0, 2.0), 100.0);
        assert!(a.px.chunks_exact(4).chain(b.px.chunks_exact(4)).all(|p| p[..3].iter().all(|v| *v >= 0.1 && *v <= 0.8)));
        assert_ne!(a.px, b.px);
    }
}
