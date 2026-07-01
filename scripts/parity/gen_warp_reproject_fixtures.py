"""生成 warp reproject 对拍夹具（rasterio/GDAL 参照，bilinear + nearest + nodata）。

运行：
  & <myproject_py310>\python.exe scripts\parity\gen_warp_reproject_fixtures.py
"""

from __future__ import annotations

import json
import math
import sys
from pathlib import Path

import numpy as np
import rasterio
from rasterio.warp import calculate_default_transform, reproject, Resampling
from rasterio.crs import CRS

OUT = (
    Path(__file__).resolve().parents[2]
    / "crates" / "water-io" / "tests" / "fixtures" / "warp_reproject_cases.json"
)


def _jnum(v):
    v = float(v)
    return None if math.isnan(v) else v


def case(name, src_epsg, dst_epsg, a, e, c, f, w, h, src, method, nodata=None):
    src = np.asarray(src, dtype=np.float32)
    src_t = rasterio.Affine(a, 0.0, c, 0.0, e, f)
    bounds = (c, f + h * e, c + w * a, f)
    dt, dw, dh = calculate_default_transform(
        CRS.from_epsg(src_epsg), CRS.from_epsg(dst_epsg), w, h, *bounds
    )
    dst = np.full((dh, dw), np.nan, dtype=np.float32)
    rs = Resampling.bilinear if method == "bilinear" else Resampling.nearest
    reproject(
        source=src, destination=dst,
        src_transform=src_t, src_crs=CRS.from_epsg(src_epsg), src_nodata=nodata,
        dst_transform=dt, dst_crs=CRS.from_epsg(dst_epsg), dst_nodata=np.nan,
        resampling=rs,
    )
    return {
        "name": name, "method": method,
        "src_epsg": src_epsg, "dst_epsg": dst_epsg,
        "src_transform": [a, 0.0, c, 0.0, e, f], "src_w": w, "src_h": h,
        "src": [_jnum(v) for v in src.flatten()],
        "nodata": None if nodata is None else float(nodata),
        "dst_transform": [dt.a, dt.b, dt.c, dt.d, dt.e, dt.f],
        "dst_w": int(dw), "dst_h": int(dh),
        "dst": [_jnum(v) for v in np.asarray(dst, dtype=np.float32).flatten()],
    }


def ramp(w, h, base=100.0):
    a = np.zeros((h, w), dtype=np.float32)
    for r in range(h):
        for cc in range(w):
            a[r, cc] = base + cc * 1.5 + r * 0.7
    return a


def main():
    rng = np.random.default_rng(11)
    cases = []

    w, h = 24, 18
    dem = ramp(w, h, 500.0)
    cases.append(case("wgs84_utm49_bilin", 4326, 32649, 0.001, -0.001, 113.90, 22.60, w, h, dem, "bilinear"))
    cases.append(case("wgs84_utm49_near", 4326, 32649, 0.001, -0.001, 113.90, 22.60, w, h, dem, "nearest"))

    # 平滑 DEM，UTM→4326（用平滑坡面隔离重采样算法；随机 DEM 会放大 proj 库亚厘米差）
    w2, h2 = 20, 16
    dem2 = ramp(w2, h2, 1500.0)
    cases.append(case("utm49_wgs84_bilin", 32649, 4326, 30.0, -30.0, 500000.0, 2500000.0, w2, h2, dem2, "bilinear"))

    # 含 nodata（-9999），部分像素无效
    dem3 = ramp(22, 16, 800.0)
    dem3[5:8, 6:10] = -9999.0  # 一块 nodata
    dem3[0, :] = -9999.0       # 顶边 nodata
    cases.append(case("wgs84_utm48_nodata_bilin", 4326, 32648, 0.0009, -0.0009, 104.0, 30.5, 22, 16, dem3, "bilinear", nodata=-9999.0))
    cases.append(case("wgs84_utm48_nodata_near", 4326, 32648, 0.0009, -0.0009, 104.0, 30.5, 22, 16, dem3, "nearest", nodata=-9999.0))

    OUT.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as fp:
        json.dump({"cases": cases}, fp)
    for c in cases:
        nfin = sum(1 for v in c["dst"] if v is not None)
        print(f"  {c['name']}({c['method']}): {c['dst_w']}x{c['dst_h']} 有限={nfin}/{c['dst_w']*c['dst_h']}")
    print(f"written {OUT}")


if __name__ == "__main__":
    main()
