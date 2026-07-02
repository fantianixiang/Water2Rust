"""固定 medial_axis 种子跑 Python hydro，生成可复现参照。

两处猴子补丁：
1. medial_axis 固定 rng（消除随机 tiebreak，给确定性骨架）。
2. rasterio.windows.from_bounds 在本机会触发 PROJ 原生崩溃（0xC06D007F），
   用纯 numpy 仿射实现替换各模块命名空间里的 from_bounds（纯像素窗口计算，无需 PROJ）。

用法：W2R_SEED 选种子（默认 12345）。
"""
import faulthandler
faulthandler.enable()
import importlib
import os
from pathlib import Path

from rasterio.windows import Window
from skimage.morphology import medial_axis as _ma

SEED = os.environ.get("W2R_SEED", "12345")


def fixed_medial_axis(image, mask=None, return_distance=False, *, rng=None):
    return _ma(image, mask=mask, return_distance=return_distance, rng=int(SEED))


def deterministic_medial_axis(image, mask=None, return_distance=False, *, rng=None):
    """skimage.medial_axis 的确定性版本：tiebreaker = arange(N)（identity，行主序 fg 秩），
    与 Rust water_core::medial_axis 的 (0..n) tiebreaker 一致，从而可 bit 级对齐。"""
    import numpy as np
    from scipy import ndimage as ndi
    from skimage.morphology._skeletonize import (
        _pattern_of, _table_lookup, _skeletonize_loop, _eight_connect,
    )
    if mask is None:
        masked_image = image.astype(bool)
    else:
        masked_image = image.astype(bool).copy()
        masked_image[~mask] = False
    center_is_foreground = (np.arange(512) & 2 ** 4).astype(bool)
    table = center_is_foreground & (
        np.array([
            ndi.label(_pattern_of(idx), _eight_connect)[1]
            != ndi.label(_pattern_of(idx & ~(2 ** 4)), _eight_connect)[1]
            for idx in range(512)
        ])
        | np.array([np.sum(_pattern_of(idx)) < 3 for idx in range(512)])
    )
    distance = ndi.distance_transform_edt(masked_image)
    store_distance = distance.copy() if return_distance else None
    cornerness_table = np.array([9 - np.sum(_pattern_of(idx)) for idx in range(512)])
    corner_score = _table_lookup(masked_image, cornerness_table)
    i, j = np.mgrid[0:image.shape[0], 0:image.shape[1]]
    result = masked_image.copy()
    distance = distance[result]
    i = np.ascontiguousarray(i[result], dtype=np.intp)
    j = np.ascontiguousarray(j[result], dtype=np.intp)
    result = np.ascontiguousarray(result, np.uint8)
    tiebreaker = np.arange(int(masked_image.sum()))  # identity（确定性）
    order = np.lexsort((tiebreaker, corner_score[masked_image], distance))
    order = np.ascontiguousarray(order, dtype=np.int32)
    table = np.ascontiguousarray(table, dtype=np.uint8)
    _skeletonize_loop(result, i, j, order, table)
    result = result.astype(bool)
    if mask is not None:
        result[~mask] = image[~mask]
    return (result, store_distance) if return_distance else result


def pure_from_bounds(left, bottom, right, top, transform):
    """纯仿射 from_bounds（north-up / 一般旋转均支持），避开 rasterio 原生 PROJ 崩溃。"""
    a, b, c = transform.a, transform.b, transform.c
    d, e, f = transform.d, transform.e, transform.f
    det = a * e - b * d
    cols = []
    rows = []
    for x in (left, right):
        for y in (top, bottom):
            col = (e * (x - c) - b * (y - f)) / det
            row = (-d * (x - c) + a * (y - f)) / det
            cols.append(col)
            rows.append(row)
    col_off = min(cols)
    row_off = min(rows)
    return Window(col_off, row_off, max(cols) - col_off, max(rows) - row_off)


import modules.waters.hydro.hydro_laplace as hl
if SEED == "identity":
    hl.medial_axis = deterministic_medial_axis
    print("[patch] medial_axis -> deterministic identity tiebreaker", flush=True)
else:
    hl.medial_axis = fixed_medial_axis
    print(f"[patch] medial_axis rng fixed to {SEED}", flush=True)

for modname, attr in [
    ("modules.waters.hydro.hydro_geom_utils", "from_bounds"),
    ("modules.waters.hydro.hydro_io", "from_bounds"),
    ("modules.waters.hydro.hydro_window", "window_from_bounds"),
    ("modules.waters.hydro.hydro_observations", "from_bounds"),
    ("modules.waters.hydro.hydro_qa", "from_bounds"),
]:
    try:
        m = importlib.import_module(modname)
        if hasattr(m, attr):
            setattr(m, attr, pure_from_bounds)
            print(f"[patch] {modname}.{attr} -> pure_from_bounds", flush=True)
    except Exception as exc:
        print(f"[patch] skip {modname}: {exc}", flush=True)

import rasterio.windows as _rw
_rw.from_bounds = pure_from_bounds

# dump 管线内工作网格 DEM，用于与 Rust dem_work 直接对比。
import numpy as _np
import modules.waters.hydro.hydro_pipeline as _hp
_orig_read_win = _hp._read_dem_to_crs_windowed
def _wrap_read_win(*a, **k):
    res = _orig_read_win(*a, **k)
    try:
        _np.save("E:/Projects/Water2Rust/data/tmp/py_dem_work.npy", res[0])
        print(f"[dump] py_dem_work {res[0].shape}", flush=True)
    except Exception as exc:
        print(f"[dump] fail {exc}", flush=True)
    return res
_hp._read_dem_to_crs_windowed = _wrap_read_win

from modules.waters import generate_hydro_water_dem

water = os.environ.get("W2R_WATER", "E:/Projects/Water2Rust/data/tmp/single8.shp")
out_env = os.environ.get("W2R_OUT")
out = Path(out_env) if out_env else Path(f"E:/Projects/Water2Rust/data/tmp/single8_pyseed{SEED}.tif")
r = generate_hydro_water_dem(
    dem_path=Path("E:/Projects/Water2Rust/data/linzhi/dem.tif"),
    water_path=Path(water),
    output_path=out,
    output_mode="water_surface_with_dem",
    all_touched=True,
    debug=False,
)
print("OUT", r, flush=True)
