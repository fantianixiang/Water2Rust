"""生成 hydro 阶段 5 数值对拍夹具：多边形栅格化(all_touched=False) 与 binary_erosion。

- rasterize：`rasterio.features.rasterize(all_touched=False)` 作为参考，验证 eci-gdal-alg 扫描线栅格化一致。
- erosion：`scipy.ndimage.binary_erosion`（默认十字结构、border_value=0）。

运行：
  & <myproject_py310>\python.exe scripts\parity\gen_stage5_fixtures.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np
import rasterio
from rasterio.features import rasterize
from scipy.ndimage import binary_erosion
from shapely.geometry import Polygon

sys.path.insert(0, r"E:\Projects\MyProject\modules")

OUT = Path(__file__).resolve().parents[2] / "crates" / "water-hydro" / "tests" / "fixtures" / "stage5_cases.json"

# 北向上仿射：像素 0.001 度，原点 (100, 30)
A, B, C, D, E, F = 0.001, 0.0, 100.0, 0.0, -0.001, 30.0
TRANSFORM = rasterio.Affine(A, B, C, D, E, F)


def px_to_world(col: float, row: float) -> tuple[float, float]:
    return C + col * A, F + row * E


def poly_from_pixel_ring(ring_px: list[tuple[float, float]], holes_px: list[list[tuple[float, float]]] | None = None) -> Polygon:
    ext = [px_to_world(cx, ry) for (cx, ry) in ring_px]
    holes = [[px_to_world(cx, ry) for (cx, ry) in hole] for hole in (holes_px or [])]
    return Polygon(ext, holes)


def rasterize_cases() -> list[dict]:
    out = []

    def c(name, h, w, poly: Polygon):
        mask = rasterize([(poly, 1)], out_shape=(h, w), transform=TRANSFORM,
                         fill=0, all_touched=False, dtype="uint8")
        ext = [[float(x), float(y)] for x, y in poly.exterior.coords]
        interiors = [[[float(x), float(y)] for x, y in ring.coords] for ring in poly.interiors]
        out.append({"name": name, "h": h, "w": w,
                    "transform": [A, B, C, D, E, F],
                    "exterior": ext, "interiors": interiors,
                    "mask": [int(v) for v in mask.flatten()]})

    # 矩形
    c("rect", 20, 24, poly_from_pixel_ring([(3, 3), (18, 3), (18, 15), (3, 15), (3, 3)]))
    # 三角形
    c("triangle", 22, 22, poly_from_pixel_ring([(2, 2), (19, 4), (10, 19), (2, 2)]))
    # 五边形(不规则)
    c("pentagon", 26, 30, poly_from_pixel_ring(
        [(4, 5), (24, 3), (27, 16), (14, 22), (2, 14), (4, 5)]))
    # 带洞矩形
    c("rect_with_hole", 24, 24, poly_from_pixel_ring(
        [(2, 2), (21, 2), (21, 21), (2, 21), (2, 2)],
        holes_px=[[(8, 8), (14, 8), (14, 15), (8, 15), (8, 8)]]))
    # 斜边多边形（考验边界像素）
    c("slanted", 18, 28, poly_from_pixel_ring(
        [(1.4, 1.6), (25.7, 4.3), (23.1, 15.8), (3.2, 13.1), (1.4, 1.6)]))
    return out


def erosion_cases() -> list[dict]:
    rng = np.random.default_rng(2026)
    out = []

    def c(name, mask, iters):
        mask = np.asarray(mask, dtype=bool)
        er = binary_erosion(mask, iterations=iters)
        h, w = mask.shape
        out.append({"name": name, "h": int(h), "w": int(w), "iterations": int(iters),
                    "mask": [int(v) for v in mask.flatten()],
                    "eroded": [int(v) for v in er.flatten()]})

    # 实心矩形
    m = np.zeros((14, 18), bool)
    m[2:12, 3:15] = True
    c("solid_rect_it1", m, 1)
    c("solid_rect_it2", m, 2)
    # 随机块
    m2 = rng.random((16, 20)) > 0.35
    c("random_it1", m2, 1)
    # 圆盘
    yy, xx = np.mgrid[0:24, 0:24]
    disk = ((xx - 11.5) ** 2 + (yy - 11.5) ** 2) <= 9.0 ** 2
    c("disk_it1", disk, 1)
    c("disk_it3", disk, 3)
    # 细条(单像素宽 → 完全腐蚀)
    m3 = np.zeros((10, 20), bool)
    m3[5, 2:18] = True
    c("thin_line", m3, 1)
    return out


def main() -> None:
    data = {"rasterize_false": rasterize_cases(), "erosion": erosion_cases()}
    OUT.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump(data, f)
    for c in data["rasterize_false"]:
        print(f"  rasterize {c['name']}: {c['h']}x{c['w']} on={sum(c['mask'])}")
    for c in data["erosion"]:
        print(f"  erosion {c['name']}: it={c['iterations']} on={sum(c['mask'])}->{sum(c['eroded'])}")
    print(f"written {OUT}")


if __name__ == "__main__":
    main()
