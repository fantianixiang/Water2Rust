"""生成 hydro 阶段 6c 河流外层循环（solve_laplace_per_polygon）对拍夹具。

合成 transform + DEM + 若干河流多边形，复刻外层循环：
逐多边形 window_from_geometry_bounds(真实 rasterio) → rasterize(all_touched) →
solve_core(固定种子, dump tiebreaker) → 缝合到 float32 surface。

运行：
  & <myproject_py310>\python.exe scripts\parity\gen_stage6c_river_pipeline_fixtures.py
"""

from __future__ import annotations

import json
import math
import sys
from pathlib import Path

import numpy as np
import rasterio
from rasterio.features import rasterize

sys.path.insert(0, r"E:\Projects\MyProject\modules")
sys.path.insert(0, str(Path(__file__).resolve().parent))
from shapely.geometry import Polygon  # noqa: E402

from gen_stage6c_river_solve_fixtures import solve_core  # noqa: E402

OUT = (
    Path(__file__).resolve().parents[2]
    / "crates" / "water-hydro" / "tests" / "fixtures" / "stage6c_river_pipeline_cases.json"
)


def _jnum(v):
    v = float(v)
    return None if math.isnan(v) else v


def window_from_bounds_pure(poly, a, c, e, f, h, w, pad=2):
    """纯逆仿射窗口（与 Rust window_from_geometry_bounds 同式，避开本机崩溃的 rasterio.from_bounds）。"""
    minx, miny, maxx, maxy = poly.bounds
    col_start = (minx - c) / a
    row_start = (maxy - f) / e
    col_stop = (maxx - c) / a
    row_stop = (miny - f) / e
    row_off = max(0, int(math.floor(row_start)) - pad)
    col_off = max(0, int(math.floor(col_start)) - pad)
    row_end = min(h, int(math.ceil(row_stop)) + pad)
    col_end = min(w, int(math.ceil(col_stop)) + pad)
    lh = max(0, row_end - row_off)
    lw = max(0, col_end - col_off)
    win_c = c + col_off * a
    win_f = f + row_off * e
    win_t = rasterio.Affine(a, 0.0, win_c, 0.0, e, win_f)
    return row_off, col_off, lh, lw, win_t


def rect_world(col0, col1, row0, row1, a, c, e, f):
    """像素范围 → 世界坐标矩形环（north-up）。"""
    x0 = c + col0 * a
    x1 = c + col1 * a
    y0 = f + row0 * e
    y1 = f + row1 * e
    return [(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)]


def main():
    h, w = 20, 40
    a, e = 10.0, -10.0
    c, f = 0.0, 0.0
    transform = rasterio.Affine(a, 0.0, c, 0.0, e, f)

    dem = np.zeros((h, w), dtype=np.float64)
    for r in range(h):
        for col in range(w):
            dem[r, col] = 100.0 + col * 1.5 + abs(r - 10) * 4.0

    # 两条河流多边形（矩形，细长 → 主路径）
    polys_px = [
        (3, 30, 6, 12),   # col0,col1,row0,row1
        (5, 22, 13, 18),
    ]
    polygons = [Polygon(rect_world(*p, a, c, e, f)) for p in polys_px]
    fclass = ["river", "river"]

    surface = np.full((h, w), np.nan, dtype=np.float32)
    tiebreakers = []
    for idx, poly in enumerate(polygons):
        r0, c0, lh, lw, win_t = window_from_bounds_pure(poly, a, c, e, f, h, w, pad=2)
        if lh <= 0 or lw <= 0:
            tiebreakers.append([])
            continue
        dem_loc = dem[r0:r0 + lh, c0:c0 + lw]
        poly_mask = rasterize([(poly, 1)], out_shape=(lh, lw), transform=win_t, fill=0, all_touched=True, dtype="uint8").astype(bool)
        n = int(poly_mask.sum())
        if n == 0:
            tiebreakers.append([])
            continue
        pixel_m = abs(win_t[0])
        tb = []
        z_local = solve_core(poly_mask, dem_loc, pixel_m, tb)
        tiebreakers.append([int(v) for v in tb[0].tolist()])
        _water = poly_mask & np.isfinite(z_local)
        surface[r0:r0 + lh, c0:c0 + lw][_water] = z_local[_water].astype(np.float32)

    case = {
        "h": h, "w": w,
        "transform": [a, 0.0, c, 0.0, e, f],
        "dem": [float(v) for v in dem.flatten()],
        "polygons": [[[float(x), float(y)] for (x, y) in list(p.exterior.coords)] for p in polygons],
        "fclass": fclass,
        "tiebreakers": tiebreakers,
        "surface": [_jnum(v) for v in surface.flatten()],
    }

    OUT.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as fp:
        json.dump({"cases": [case]}, fp)
    nfin = sum(1 for v in case["surface"] if v is not None)
    print(f"  river_pipeline: {h}x{w} polys={len(polygons)} surface有限={nfin}")
    print(f"written {OUT}")


if __name__ == "__main__":
    main()
