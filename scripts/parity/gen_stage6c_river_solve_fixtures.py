"""生成 hydro 阶段 6c 河流水面数值核对拍夹具。

忠实复刻 `hydro_laplace.py::solve_laplace_per_polygon` 的**单多边形内层块**
（调用真实子函数），对给定 (poly_mask, dem_loc, pixel_m) 产出 z_local_field。
medial_axis 用固定种子 SEED，并复现其 tiebreaker 一并 dump（供 Rust 注入同序列）。

运行：
  & <myproject_py310>\python.exe scripts\parity\gen_stage6c_river_solve_fixtures.py
"""

from __future__ import annotations

import json
import math
import sys
from pathlib import Path

import numpy as np
import pandas as pd
from scipy.ndimage import (
    binary_erosion,
    distance_transform_edt,
    gaussian_filter,
    percentile_filter,
)
from scipy.spatial import cKDTree
from skimage.morphology import medial_axis

sys.path.insert(0, r"E:\Projects\MyProject\modules")
from waters.hydro.hydro_laplace import solve_laplace_dirichlet  # noqa: E402
from waters.hydro.hydro_skeleton_zloc import (  # noqa: E402
    _compute_skeleton_tangents,
    _cross_section_z_at_skeleton_pixels,
    _detect_junction_stations,
    _isotonic_multi_peak,
    _order_skeleton_pixels_along_flow,
)

OUT = (
    Path(__file__).resolve().parents[2]
    / "crates" / "water-hydro" / "tests" / "fixtures" / "stage6c_river_solve_cases.json"
)
SEED = 12345


def _jnum(v):
    v = float(v)
    return None if math.isnan(v) else v


def solve_core(poly_mask, dem_loc, pixel_m, tiebreaker_out):
    """复刻 solve_laplace_per_polygon 单多边形内层块。返回 z_local_field。"""
    lh, lw = poly_mask.shape
    n_px = int(poly_mask.sum())
    tiebreaker_out.append(np.random.default_rng(SEED).permutation(np.arange(n_px)))
    precomputed_skeleton = medial_axis(poly_mask, rng=SEED)

    _corr_edt = distance_transform_edt(poly_mask)
    _ordered = _order_skeleton_pixels_along_flow(precomputed_skeleton, dem_loc, _corr_edt)

    if len(_ordered) < 5:
        _, _nn = distance_transform_edt(~precomputed_skeleton, return_indices=True)
        z_local_field = dem_loc[_nn[0], _nn[1]].astype(np.float64)
        _skel_widths = _corr_edt[precomputed_skeleton]
        _med_hw = float(np.median(_skel_widths)) if _skel_widths.size else 3.0
        _depth_m = np.clip(_med_hw * pixel_m * 0.1, 1.0, 10.0)
        z_local_field = z_local_field + _depth_m
        z_local_field[~poly_mask] = np.nan
        return z_local_field

    _smoothed = [(max(0, min(lh - 1, r)), max(0, min(lw - 1, c))) for r, c in _ordered]
    _tangents = _compute_skeleton_tangents(precomputed_skeleton, _smoothed)
    _is_junction = _detect_junction_stations(_smoothed, _tangents, radius_px=4.0, angle_cos_threshold=0.5)

    _eroded = binary_erosion(poly_mask, iterations=1)
    _boundary_mask = poly_mask & ~_eroded

    _skel_rows, _skel_cols = np.nonzero(precomputed_skeleton)
    if _skel_rows.size > 0:
        _skel_tree = cKDTree(np.column_stack((_skel_rows, _skel_cols)))
        _smooth_arr = np.array(_smoothed, dtype=np.float64)
        _, _nn_idx = _skel_tree.query(_smooth_arr)
        _edt_at_skel = np.array([_corr_edt[_skel_rows[j], _skel_cols[j]] for j in _nn_idx], dtype=np.float64)
    else:
        _edt_at_skel = np.ones(len(_smoothed), dtype=np.float64)
    np.maximum(_edt_at_skel, 2.0, out=_edt_at_skel)

    _z_raw = _cross_section_z_at_skeleton_pixels(
        _smoothed, _tangents, _boundary_mask, dem_loc, edt_half_widths=_edt_at_skel,
    )

    _z_smooth = _isotonic_multi_peak(_z_raw)
    _finite = np.isfinite(_z_smooth)
    if np.any(_finite) and not np.all(_finite):
        _z_smooth = pd.Series(_z_smooth).ffill().bfill().values

    # 空间 P30
    _med_hw_skel = float(np.median(_corr_edt[precomputed_skeleton])) if _corr_edt[precomputed_skeleton].size > 0 else 3.0
    _pct_radius = max(2, int(round(_med_hw_skel)))
    _pct_size = 2 * _pct_radius + 1
    _zf_pct = np.full((lh, lw), np.nan, dtype=np.float64)
    for _k in range(len(_smoothed)):
        if not np.isfinite(_z_smooth[_k]):
            continue
        _sr = int(round(_smoothed[_k][0])); _sc = int(round(_smoothed[_k][1]))
        if 0 <= _sr < lh and 0 <= _sc < lw:
            _zf_pct[_sr, _sc] = _z_smooth[_k]
    _zf_for_pct = np.where(np.isnan(_zf_pct), np.inf, _zf_pct)
    _zf_low = percentile_filter(_zf_for_pct, percentile=30, size=_pct_size)
    _zf_low = np.where(np.isfinite(_zf_low) & (_zf_low < np.inf), _zf_low, np.nan)
    _z_smooth = _z_smooth.astype(np.float64).copy()
    for _k in range(len(_smoothed)):
        _sr = int(round(_smoothed[_k][0])); _sc = int(round(_smoothed[_k][1]))
        if 0 <= _sr < lh and 0 <= _sc < lw:
            _v = _zf_low[_sr, _sc]
            if np.isfinite(_v):
                _z_smooth[_k] = min(_z_smooth[_k], _v)

    # 2D mask-aware 高斯
    _spatial_sigma_px = 3.0
    _z_smooth_pre = _z_smooth.astype(np.float64).copy()
    _z_smooth = _z_smooth.astype(np.float64).copy()
    _z_field = np.zeros((lh, lw), dtype=np.float64)
    _mask_field = np.zeros((lh, lw), dtype=np.float32)
    _finite_zs = np.isfinite(_z_smooth)
    for _k in range(len(_smoothed)):
        if not _finite_zs[_k]:
            continue
        _sr = int(round(_smoothed[_k][0])); _sc = int(round(_smoothed[_k][1]))
        if 0 <= _sr < lh and 0 <= _sc < lw:
            _z_field[_sr, _sc] = _z_smooth[_k]; _mask_field[_sr, _sc] = 1.0
    _zg = gaussian_filter(_z_field, sigma=_spatial_sigma_px)
    _mg = gaussian_filter(_mask_field, sigma=_spatial_sigma_px)
    _z_field_smoothed = np.where(_mg > 1e-3, _zg / _mg, np.nan)
    for _k in range(len(_smoothed)):
        _sr = int(round(_smoothed[_k][0])); _sc = int(round(_smoothed[_k][1]))
        if 0 <= _sr < lh and 0 <= _sc < lw:
            _v = _z_field_smoothed[_sr, _sc]
            if np.isfinite(_v):
                _z_smooth[_k] = _v
    _z_smooth = np.minimum(_z_smooth, _z_smooth_pre)

    # Dirichlet + 求解
    _keep_bc = ~_is_junction
    _dir_mask = np.zeros((lh, lw), dtype=bool)
    _dir_z = np.full((lh, lw), np.nan, dtype=np.float64)
    for _k in range(len(_smoothed)):
        if not _keep_bc[_k]:
            continue
        _sr = int(round(_smoothed[_k][0])); _sc = int(round(_smoothed[_k][1]))
        if 0 <= _sr < lh and 0 <= _sc < lw and poly_mask[_sr, _sc]:
            _dir_mask[_sr, _sc] = True; _dir_z[_sr, _sc] = _z_smooth[_k]

    if int(np.count_nonzero(_dir_mask)) == 0:
        z_local_field = np.full((lh, lw), np.nan, dtype=np.float64)
        z_local_field[poly_mask] = float(np.nanmean(_z_smooth))
    else:
        z_local_field = solve_laplace_dirichlet(poly_mask, _dir_mask, _dir_z)
    return z_local_field


def case(name, poly_mask, dem, pixel_m=10.0):
    poly_mask = np.asarray(poly_mask, dtype=bool)
    dem = np.asarray(dem, dtype=np.float64)
    tb = []
    z = solve_core(poly_mask, dem, pixel_m, tb)
    h, w = poly_mask.shape
    return {
        "name": name, "h": int(h), "w": int(w), "pixel_m": float(pixel_m),
        "poly_mask": [int(v) for v in poly_mask.flatten()],
        "dem": [float(v) for v in dem.flatten()],
        "tiebreaker": [int(v) for v in tb[0].tolist()],
        "z_local": [_jnum(v) for v in np.asarray(z, dtype=np.float64).flatten()],
    }


def main():
    rng = np.random.default_rng(7)
    cases = []

    # 直河道矩形（skeleton 长 → 主路径）
    h, w = 14, 30
    pm = np.zeros((h, w), bool); pm[4:10, 2:28] = True
    dem = np.zeros((h, w), float)
    for r in range(h):
        for c in range(w):
            dem[r, c] = 100.0 + c * 1.5 + abs(r - 7) * 4.0
    cases.append(case("river_rect", pm, dem))

    # L 形河道
    h, w = 22, 22
    pm2 = np.zeros((h, w), bool); pm2[3:9, 3:19] = True; pm2[3:19, 13:19] = True
    dem2 = np.zeros((h, w), float)
    for r in range(h):
        for c in range(w):
            dem2[r, c] = 100.0 + (r + c) * 1.2 + rng.standard_normal() * 0.5
    cases.append(case("river_L", pm2, dem2))

    # 小 blob（ordered<5 → 回退路径）
    h, w = 8, 8
    pm3 = np.zeros((h, w), bool); pm3[3:5, 3:6] = True
    dem3 = np.zeros((h, w), float)
    for r in range(h):
        for c in range(w):
            dem3[r, c] = 200.0 + c + r
    cases.append(case("small_blob", pm3, dem3))

    OUT.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump({"cases": cases}, f)
    for c in cases:
        nfin = sum(1 for v in c["z_local"] if v is not None)
        print(f"  {c['name']}: {c['h']}x{c['w']} z_local有限={nfin}")
    print(f"written {OUT}")


if __name__ == "__main__":
    main()
