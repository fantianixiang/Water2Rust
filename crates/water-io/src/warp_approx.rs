//! GDAL 近似坐标变换器（`GDALApproxTransform`）的忠实复刻。
//!
//! GDAL warp 默认对**每个目标行**用一个「近似变换器」代替逐像素精确 PROJ：把该行递归
//! 细分为若干段，每段用两端点的精确变换做**线性插值**，当段中点的线性预测与精确值的
//! 曼哈顿误差 ≤ `max_error`（默认 0.125 像素）时即接受，否则继续细分。这带来两点：
//! 1）比逐像素 PROJ **更快**；2）引入 ≤0.125px 的采样偏移——本仓库为**与原 Python(GDAL)
//! 参照 bit 级一致**而复刻之（见 docs/HYDRO.md 与仓库记忆）。
//!
//! 算法逐行对齐 GDAL `alg/gdaltransformer.cpp` 的 `GDALApproxTransform` /
//! `GDALApproxTransformInternal`（目标行内 y 恒定、x 单调，故走近似分支）。

/// 逐目标行近似变换的上下文：`base` 为精确基变换 `(dst_col, dst_row) → Some((src_col, src_row))`。
pub(crate) struct RowApprox<'a, F>
where
    F: Fn(f64, f64) -> Option<(f64, f64)>,
{
    /// 目标像素中心行坐标（`row + 0.5`，行内恒定）。
    pub py: f64,
    /// 误差阈值（像素，曼哈顿）。`<= 0` 表示逐点精确（不近似）。
    pub max_error: f64,
    pub base: &'a F,
}

impl<'a, F> RowApprox<'a, F>
where
    F: Fn(f64, f64) -> Option<(f64, f64)>,
{
    #[inline]
    fn dx(j: usize) -> f64 {
        j as f64 + 0.5
    }

    #[inline]
    fn base_one(&self, j: usize) -> Option<(f64, f64)> {
        (self.base)(Self::dx(j), self.py)
    }

    fn exact(&self, j: usize, sx: &mut [f64], sy: &mut [f64], ok: &mut [bool]) {
        match self.base_one(j) {
            Some((a, b)) => {
                sx[j] = a;
                sy[j] = b;
                ok[j] = true;
            }
            None => ok[j] = false,
        }
    }

    fn exact_range(&self, lo: usize, hi: usize, sx: &mut [f64], sy: &mut [f64], ok: &mut [bool]) {
        for j in lo..=hi {
            self.exact(j, sx, sy, ok);
        }
    }

    /// 递归近似 `[lo, hi]`（含端点），已知端点/中点精确变换 `s_lo / s_mid / s_hi`。
    #[allow(clippy::too_many_arguments)]
    fn internal(
        &self,
        lo: usize,
        hi: usize,
        s_lo: (f64, f64),
        s_mid: (f64, f64),
        s_hi: (f64, f64),
        sx: &mut [f64],
        sy: &mut [f64],
        ok: &mut [bool],
    ) {
        let n = hi - lo + 1;
        let n_middle = (n - 1) / 2;
        let mid_abs = lo + n_middle;

        let denom = Self::dx(hi) - Self::dx(lo);
        let ddx = (s_hi.0 - s_lo.0) / denom;
        let ddy = (s_hi.1 - s_lo.1) / denom;

        // 段中点线性预测与精确值的曼哈顿误差。
        let off_mid = Self::dx(mid_abs) - Self::dx(lo);
        let err = (s_lo.0 + ddx * off_mid - s_mid.0).abs()
            + (s_lo.1 + ddy * off_mid - s_mid.1).abs();

        if err <= self.max_error {
            for j in lo..=hi {
                let d = Self::dx(j) - Self::dx(lo);
                sx[j] = s_lo.0 + ddx * d;
                sy[j] = s_lo.1 + ddy * d;
                ok[j] = true;
            }
            return;
        }

        // 细分：half1=[lo..=mid_abs-1]（n_middle 点），half2=[mid_abs..=hi]（n-n_middle 点）。
        let h1_end = mid_abs - 1;
        let use_base1 = n_middle <= 5;
        let use_base2 = (n - n_middle) <= 5;

        if use_base1 {
            self.exact_range(lo, h1_end, sx, sy, ok);
        } else {
            let h1_mid = lo + (n_middle - 1) / 2;
            match (self.base_one(h1_mid), self.base_one(h1_end)) {
                (Some(m), Some(e)) => self.internal(lo, h1_end, s_lo, m, e, sx, sy, ok),
                _ => self.exact_range(lo, h1_end, sx, sy, ok),
            }
        }

        if use_base2 {
            self.exact_range(mid_abs, hi, sx, sy, ok);
        } else {
            let h2_mid = mid_abs + (n - n_middle - 1) / 2;
            match self.base_one(h2_mid) {
                Some(m) => self.internal(mid_abs, hi, s_mid, m, s_hi, sx, sy, ok),
                None => self.exact_range(mid_abs, hi, sx, sy, ok),
            }
        }
    }

    /// 计算整行 `width` 个目标像素的源像素坐标 `(sx, sy)` 与有效标志 `ok`。
    pub fn transform_row(&self, width: usize) -> (Vec<f64>, Vec<f64>, Vec<bool>) {
        let mut sx = vec![f64::NAN; width];
        let mut sy = vec![f64::NAN; width];
        let mut ok = vec![false; width];
        if width == 0 {
            return (sx, sy, ok);
        }

        // 近似关闭或点数过少 → 逐点精确（对齐 GDAL bail 分支）。
        if self.max_error <= 0.0 || width <= 5 {
            self.exact_range(0, width - 1, &mut sx, &mut sy, &mut ok);
            return (sx, sy, ok);
        }

        let n_middle = (width - 1) / 2;
        match (self.base_one(0), self.base_one(n_middle), self.base_one(width - 1)) {
            (Some(s0), Some(sm), Some(se)) => {
                self.internal(0, width - 1, s0, sm, se, &mut sx, &mut sy, &mut ok);
            }
            _ => self.exact_range(0, width - 1, &mut sx, &mut sy, &mut ok),
        }
        (sx, sy, ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 轻度非线性基变换：sx = px + k·px²（模拟投影曲率），sy = py。
    fn nonlinear_base(k: f64) -> impl Fn(f64, f64) -> Option<(f64, f64)> {
        move |px: f64, py: f64| Some((px + k * px * px, py))
    }

    #[test]
    fn max_error_zero_is_exact() {
        let base = nonlinear_base(1e-3);
        let a = RowApprox { py: 0.5, max_error: 0.0, base: &base };
        let (sx, _sy, ok) = a.transform_row(200);
        for j in 0..200 {
            assert!(ok[j]);
            let px = j as f64 + 0.5;
            assert!((sx[j] - (px + 1e-3 * px * px)).abs() < 1e-12, "j={j} 应精确");
        }
    }

    #[test]
    fn approx_linearizes_within_tolerance() {
        let k = 1e-3;
        let base = nonlinear_base(k);
        let a = RowApprox { py: 0.5, max_error: 0.125, base: &base };
        let (sx, _sy, ok) = a.transform_row(400);
        // 近似应发生（结果不逐点等于精确），但每点误差受阈值量级约束。
        let mut worst = 0.0f64;
        let mut any_diff = false;
        for j in 0..400 {
            assert!(ok[j]);
            let px = j as f64 + 0.5;
            let exact = px + k * px * px;
            let d = (sx[j] - exact).abs();
            worst = worst.max(d);
            if d > 1e-9 {
                any_diff = true;
            }
        }
        assert!(any_diff, "max_error=0.125 时应产生线性近似（与精确不同）");
        // 曼哈顿阈值 0.125 下单点偏差不应超过若干倍阈值。
        assert!(worst < 1.0, "近似偏差过大: {worst}");
    }
}

