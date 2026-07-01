"""生成 hydro 阶段 2/3/4 数值对拍夹具。

导入 MyProject 中**真实**的 Python 函数：
  - `_iterative_trimmed_median`（湖泊常数水位内核）
  - `apply_river_dem_floor_lift`（河床抬升 / 漫滩分类）
  - `compose_water_output_array`（输出组合）

对 floor_lift：原函数内部按 all_touched 栅格化河流/湖泊多边形；本脚本用**完全相同**的
rasterize 调用复算 river_mask / lake_mask 并一并 dump，作为 Rust 端输入（栅格化本身留待阶段 5）。

运行：
  & <myproject_py310>\python.exe scripts\parity\gen_hydro_stage234_fixtures.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np
import rasterio
from rasterio.features import rasterize
from shapely.geometry import box

sys.path.insert(0, r"E:\Projects\MyProject\modules")
from waters.hydro.hydro_lake_flatten import _iterative_trimmed_median  # noqa: E402
from waters.hydro.hydro_raster_postprocess import apply_river_dem_floor_lift  # noqa: E402
from waters.output import compose_water_output_array  # noqa: E402

OUT = Path(__file__).resolve().parents[2] / "crates" / "water-hydro" / "tests" / "fixtures" / "hydro_stage234_cases.json"

IDENT = rasterio.Affine(1.0, 0.0, 0.0, 0.0, 1.0, 0.0)


def enc(arr: np.ndarray) -> list:
    return [None if not np.isfinite(v) else float(v) for v in np.asarray(arr, dtype=np.float64).flatten()]


def bmask(arr: np.ndarray) -> list:
    return [int(bool(v)) for v in np.asarray(arr, dtype=bool).flatten()]


def trimmed_median_cases() -> list[dict]:
    rng = np.random.default_rng(7)
    out = []

    def c(name, values, max_iter=5):
        values = np.asarray(values, dtype=np.float64)
        z = _iterative_trimmed_median(values, max_iter=max_iter)
        out.append({"name": name, "values": enc(values),
                    "max_iter": int(max_iter),
                    "expected": (None if not np.isfinite(z) else float(z))})

    c("odd_simple", [1.0, 2.0, 3.0, 4.0, 5.0])
    c("even_simple", [1.0, 2.0, 3.0, 4.0])
    c("with_high_outliers", [10.0, 10.1, 9.9, 10.2, 50.0, 60.0, 10.05])
    c("cliff_intrusion", [100.0, 101.0, 99.5, 100.5, 500.0, 480.0, 100.2, 100.8, 99.0])
    c("random_gauss", (200.0 + 5.0 * rng.standard_normal(51)).tolist())
    c("with_nan", [5.0, float("nan"), 6.0, 7.0, float("nan"), 6.5])
    return out


def floor_lift_cases() -> list[dict]:
    out = []

    def c(name, h, w, dem, surface, write, rivers, lakes):
        polys = [box(*b) for b in rivers] + [box(*b) for b in lakes]
        fclass = ["river"] * len(rivers) + ["lake"] * len(lakes)
        # 复刻原函数内部栅格化以 dump 掩膜
        river_mask = np.zeros((h, w), bool)
        lake_mask = np.zeros((h, w), bool)
        for b in rivers:
            river_mask |= rasterize([box(*b)], out_shape=(h, w), transform=IDENT,
                                    fill=0, all_touched=True, dtype="uint8").astype(bool)
        for b in lakes:
            lake_mask |= rasterize([box(*b)], out_shape=(h, w), transform=IDENT,
                                   fill=0, all_touched=True, dtype="uint8").astype(bool)
        surf = np.array(surface, dtype=np.float32).copy()
        n_lifted, n_lake_excluded, n_overbank = apply_river_dem_floor_lift(
            surf,
            write_mask=np.asarray(write, dtype=bool),
            dem=np.asarray(dem, dtype=np.float32),
            water_polygons=polys,
            water_fclass=fclass,
            transform=IDENT,
            out_shape=(h, w),
            all_touched=True,
        )
        out.append({
            "name": name, "h": h, "w": w,
            "dem": enc(dem), "surface": enc(surface), "write": bmask(write),
            "river": bmask(river_mask), "lake": bmask(lake_mask),
            "after": enc(surf), "counts": [int(n_lifted), int(n_lake_excluded), int(n_overbank)],
        })

    h, w = 10, 14
    yy, xx = np.mgrid[0:h, 0:w]
    dem = (100.0 + yy + 0.5 * xx).astype(np.float32)
    # 水面：整体略低于 dem（河流像素→漫滩；额外非多边形像素→抬升）
    surface = (dem - 3.0).astype(np.float32)
    write = np.zeros((h, w), bool)
    write[1:8, 1:12] = True  # 覆盖多边形与部分背景
    c("mixed_river_lake", h, w, dem, surface, write,
      rivers=[(1, 1, 6, 5)], lakes=[(2, 6, 12, 9)])

    # 全部水面高于 dem（无抬升、无漫滩）
    surface2 = (dem + 5.0).astype(np.float32)
    c("all_above_dem", h, w, dem, surface2, write,
      rivers=[(1, 1, 6, 5)], lakes=[(2, 6, 12, 9)])

    # 含 NaN 水面
    surface3 = surface.copy()
    surface3[3, 3] = np.nan
    surface3[5, 8] = np.nan
    c("with_nan_surface", h, w, dem, surface3, write,
      rivers=[(1, 1, 6, 5)], lakes=[(2, 6, 12, 9)])
    return out


def compose_cases() -> list[dict]:
    rng = np.random.default_rng(11)
    out = []

    def c(name, h, w, dem, surface, mask, mode):
        dem = np.asarray(dem, dtype=np.float32)
        surface = np.asarray(surface, dtype=np.float32)
        mask = np.asarray(mask, dtype=bool)
        arr, metrics = compose_water_output_array(
            dem=dem, water_surface=surface, water_mask=mask, output_mode=mode)
        out.append({
            "name": name, "h": h, "w": w, "mode": mode,
            "dem": enc(dem), "surface": enc(surface), "mask": bmask(mask),
            "output": enc(arr),
            "metrics": {k: int(v) for k, v in metrics.items() if isinstance(v, (int, np.integer))},
        })

    h, w = 8, 10
    dem = (50.0 + rng.standard_normal((h, w))).astype(np.float32)
    surface = np.full((h, w), np.nan, dtype=np.float32)
    mask = np.zeros((h, w), bool)
    mask[2:6, 3:7] = True
    surface[3:5, 4:6] = 48.0  # 部分水体有解，部分为空洞
    c("with_dem_partial", h, w, dem, surface, mask, "water_surface_with_dem")
    c("only_partial", h, w, dem, surface, mask, "water_surface_only")

    # DEM 含 NaN
    dem2 = dem.copy()
    dem2[0, 0] = np.nan
    dem2[7, 9] = np.nan
    c("with_dem_nan", h, w, dem2, surface, mask, "water_surface_with_dem")

    # 全水体有解
    surface_full = np.where(mask, 45.0, np.nan).astype(np.float32)
    c("full_surface", h, w, dem, surface_full, mask, "water_surface_with_dem")
    return out


def main() -> None:
    data = {
        "trimmed_median": trimmed_median_cases(),
        "floor_lift": floor_lift_cases(),
        "compose": compose_cases(),
    }
    OUT.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump(data, f)
    print(f"trimmed_median: {len(data['trimmed_median'])} 例")
    print(f"floor_lift: {len(data['floor_lift'])} 例")
    print(f"compose: {len(data['compose'])} 例")
    for c in data["floor_lift"]:
        print(f"  floor_lift {c['name']}: counts(lift,lake,overbank)={c['counts']}")
    print(f"written {OUT}")


if __name__ == "__main__":
    main()
