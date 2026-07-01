"""生成 warp 的 calculate_default_transform 对拍夹具（rasterio/GDAL 参照）。

本机 rasterio 的 calculate_default_transform / reproject 正常（仅 from_bounds 崩溃）。

运行：
  & <myproject_py310>\python.exe scripts\parity\gen_warp_suggest_fixtures.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import rasterio
from rasterio.warp import calculate_default_transform
from rasterio.crs import CRS

OUT = (
    Path(__file__).resolve().parents[2]
    / "crates" / "water-io" / "tests" / "fixtures" / "warp_suggest_cases.json"
)


def case(name, src_epsg, dst_epsg, a, e, c, f, w, h):
    src_t = rasterio.Affine(a, 0.0, c, 0.0, e, f)
    bounds = (c, f + h * e, c + w * a, f)  # left, bottom, right, top
    dt, dw, dh = calculate_default_transform(
        CRS.from_epsg(src_epsg), CRS.from_epsg(dst_epsg), w, h, *bounds
    )
    return {
        "name": name,
        "src_epsg": src_epsg, "dst_epsg": dst_epsg,
        "src_transform": [a, 0.0, c, 0.0, e, f],
        "src_w": w, "src_h": h,
        "dst_transform": [dt.a, dt.b, dt.c, dt.d, dt.e, dt.f],
        "dst_w": int(dw), "dst_h": int(dh),
    }


def main():
    cases = []
    # 4326 → UTM 49N（深圳附近）
    cases.append(case("wgs84_to_utm49_sz", 4326, 32649, 0.001, -0.001, 113.90, 22.60, 200, 160))
    # 4326 → UTM 48N（更西，含更多样本）
    cases.append(case("wgs84_to_utm48", 4326, 32648, 0.0008, -0.0008, 104.0, 30.5, 320, 240))
    # UTM 49N → 4326（回投）
    cases.append(case("utm49_to_wgs84", 32649, 4326, 30.0, -30.0, 500000.0, 2500000.0, 256, 200))
    # 同 CRS（identity 情形，UTM→UTM）
    cases.append(case("utm_same", 32649, 32649, 30.0, -30.0, 500000.0, 2500000.0, 128, 96))
    # 4326 → 3857（Web Mercator）
    cases.append(case("wgs84_to_3857", 4326, 3857, 0.005, -0.005, 8.0, 47.0, 180, 140))

    OUT.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as fp:
        json.dump({"cases": cases}, fp)
    for c in cases:
        print(f"  {c['name']}: {c['src_w']}x{c['src_h']} -> {c['dst_w']}x{c['dst_h']} px={c['dst_transform'][0]:.4f}")
    print(f"written {OUT}")


if __name__ == "__main__":
    main()
