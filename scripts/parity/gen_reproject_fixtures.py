"""生成 hydro 几何重投影对拍夹具（pyproj，等价 geopandas to_crs）。

定义若干多边形（源 CRS 顶点），用 pyproj 逐顶点重投影到目标 CRS，dump 源/目标坐标。

运行：
  & <myproject_py310>\python.exe scripts\parity\gen_reproject_fixtures.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

from pyproj import Transformer

OUT = (
    Path(__file__).resolve().parents[2]
    / "crates" / "water-hydro" / "tests" / "fixtures" / "reproject_cases.json"
)


def reproj_ring(coords, tf):
    out = []
    for (x, y) in coords:
        px, py = tf.transform(x, y)
        out.append([float(px), float(py)])
    return out


def case(name, src_epsg, dst_epsg, exterior, interiors=None):
    interiors = interiors or []
    tf = Transformer.from_crs(src_epsg, dst_epsg, always_xy=True)
    return {
        "name": name,
        "src_epsg": int(src_epsg),
        "dst_epsg": int(dst_epsg),
        "src_exterior": [[float(x), float(y)] for (x, y) in exterior],
        "src_interiors": [[[float(x), float(y)] for (x, y) in r] for r in interiors],
        "dst_exterior": reproj_ring(exterior, tf),
        "dst_interiors": [reproj_ring(r, tf) for r in interiors],
    }


def main():
    cases = []

    # 深圳附近 4326 → 32649（UTM 49N）
    ext1 = [(113.90, 22.50), (113.95, 22.50), (113.95, 22.55), (113.90, 22.55), (113.90, 22.50)]
    cases.append(case("shenzhen_4326_to_49n", 4326, 32649, ext1))

    # 带内环的多边形
    ext2 = [(114.00, 22.60), (114.10, 22.60), (114.10, 22.70), (114.00, 22.70), (114.00, 22.60)]
    hole = [(114.03, 22.63), (114.07, 22.63), (114.07, 22.67), (114.03, 22.67), (114.03, 22.63)]
    cases.append(case("with_hole_4326_to_49n", 4326, 32649, ext2, [hole]))

    # 反向：UTM 49N → 4326
    ext3 = [(500000.0, 2488000.0), (510000.0, 2488000.0), (510000.0, 2498000.0),
            (500000.0, 2498000.0), (500000.0, 2488000.0)]
    cases.append(case("49n_to_4326", 32649, 4326, ext3))

    # 4326 → 3857（Web Mercator）
    ext4 = [(120.0, 30.0), (120.1, 30.0), (120.1, 30.1), (120.0, 30.1), (120.0, 30.0)]
    cases.append(case("hangzhou_4326_to_3857", 4326, 3857, ext4))

    OUT.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump({"cases": cases}, f)
    for c in cases:
        print(f"  {c['name']}: {c['src_epsg']}->{c['dst_epsg']} verts={len(c['src_exterior'])}")
    print(f"written {OUT}")


if __name__ == "__main__":
    main()
