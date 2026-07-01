"""生成 hydro 阶段 5b 对拍夹具：多边形内部/岸线环 DEM 中位数（端到端）。

导入 MyProject 中**真实**的 Python 函数：
  - `_sample_polygon_interior_dem_median`
  - `_sample_polygon_boundary_ring_dem_median`

用整数像素对齐的多边形（避免栅格化压边 tie-break），DEM 含梯度 + 高脊 + NaN，
以同时考验窗口计算、栅格化、腐蚀取环、（截尾）中位数与有限值过滤。

运行：
  & <myproject_py310>\python.exe scripts\parity\gen_stage5b_fixtures.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np
import rasterio
from shapely.geometry import Polygon

sys.path.insert(0, r"E:\Projects\MyProject\modules")
from waters.hydro.hydro_lake_flatten import (  # noqa: E402
    _sample_polygon_interior_dem_median,
    _sample_polygon_boundary_ring_dem_median,
)

OUT = Path(__file__).resolve().parents[2] / "crates" / "water-hydro" / "tests" / "fixtures" / "stage5b_cases.json"

A, B, C, D, E, F = 0.001, 0.0, 100.0, 0.0, -0.001, 30.0
TRANSFORM = rasterio.Affine(A, B, C, D, E, F)


def px_to_world(col: float, row: float) -> tuple[float, float]:
    return C + col * A, F + row * E


def poly(ring_px, holes_px=None) -> Polygon:
    ext = [px_to_world(cx, ry) for cx, ry in ring_px]
    holes = [[px_to_world(cx, ry) for cx, ry in h] for h in (holes_px or [])]
    return Polygon(ext, holes)


def enc(arr) -> list:
    return [None if not np.isfinite(v) else float(v) for v in np.asarray(arr, dtype=np.float64).flatten()]


def build_dem(h: int, w: int) -> np.ndarray:
    yy, xx = np.mgrid[0:h, 0:w]
    dem = (1000.0 + 2.0 * yy + 1.0 * xx).astype(np.float32)
    # 高脊（模拟悬崖侵入，考验岸线环截尾中位数）
    dem[:, w // 2] += 400.0
    dem[h // 3, :] += 300.0
    # 少量 NaN（nodata）
    dem[2, 2] = np.nan
    dem[h - 3, w - 3] = np.nan
    return dem


def main() -> None:
    h, w = 40, 50
    dem = build_dem(h, w)

    polys = {
        "lake_box": poly([(5, 5), (20, 5), (20, 22), (5, 22), (5, 5)]),
        "cross_ridge": poly([(20, 8), (40, 8), (40, 26), (20, 26), (20, 8)]),
        "small": poly([(30, 30), (36, 30), (36, 36), (30, 36), (30, 30)]),
        "with_hole": poly(
            [(3, 26), (18, 26), (18, 38), (3, 38), (3, 26)],
            holes_px=[[(8, 30), (13, 30), (13, 34), (8, 34), (8, 30)]],
        ),
        "thin": poly([(42, 3), (48, 3), (48, 4), (42, 4), (42, 3)]),  # 极扁 → 腐蚀后环为空回退整掩膜
    }

    cases = []
    for name, p in polys.items():
        im, ic = _sample_polygon_interior_dem_median(p, dem, TRANSFORM)
        rm, rc = _sample_polygon_boundary_ring_dem_median(p, dem, TRANSFORM)
        ext = [[float(x), float(y)] for x, y in p.exterior.coords]
        interiors = [[[float(x), float(y)] for x, y in r.coords] for r in p.interiors]
        cases.append({
            "name": name, "h": h, "w": w, "transform": [A, B, C, D, E, F],
            "exterior": ext, "interiors": interiors,
            "interior_median": (None if not np.isfinite(im) else float(im)),
            "interior_count": int(ic),
            "ring_median": (None if not np.isfinite(rm) else float(rm)),
            "ring_count": int(rc),
        })
        print(f"  {name}: interior=({im:.4f},{ic}) ring=({rm:.4f},{rc})")

    OUT.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump({"dem": enc(dem), "h": h, "w": w, "cases": cases}, f)
    print(f"written {OUT}")


if __name__ == "__main__":
    main()
