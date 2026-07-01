"""生成 hydro CRS 解析对拍夹具（局地 UTM 估计）。

- utm_epsg_from_center：纯带号公式（zone=floor((lon+180)/6)+1，北 32600+zone / 南 32700+zone）。
- estimate_local_utm：调原 Python `estimate_local_utm_crs_from_bounds`（4326 源 + UTM 源）。

运行：
  & <myproject_py310>\python.exe scripts\parity\gen_crs_fixtures.py
"""

from __future__ import annotations

import json
import math
import sys
from pathlib import Path

sys.path.insert(0, r"E:\Projects\MyProject\modules")

OUT = (
    Path(__file__).resolve().parents[2]
    / "crates" / "water-hydro" / "tests" / "fixtures" / "crs_cases.json"
)


def utm_epsg(lon_c, lat_c):
    zone = int(math.floor((lon_c + 180.0) / 6.0)) + 1
    zone = max(1, min(zone, 60))
    return (32600 + zone) if lat_c >= 0.0 else (32700 + zone)


def main():
    centers = []
    for lon in [-179.0, -123.4, -75.0, -0.5, 0.0, 5.0, 100.5, 113.9, 120.0, 179.9]:
        for lat in [-60.0, -1.0, 0.0, 1.0, 45.0, 83.0]:
            centers.append({"lon": lon, "lat": lat, "epsg": int(utm_epsg(lon, lat))})

    estimates = []
    try:
        from waters.settings import estimate_local_utm_crs_from_bounds
        from pyproj import CRS

        # 4326 源（bounds 即经纬度）
        b1 = [113.8, 22.4, 114.1, 22.7]  # 深圳附近 → UTM 49N (326 49)
        e1 = estimate_local_utm_crs_from_bounds(bounds=b1, source_crs=CRS.from_epsg(4326))
        estimates.append({"bounds": b1, "source_epsg": 4326, "epsg": int(e1.to_epsg())})

        b2 = [-70.0, -33.6, -70.5, -33.3]  # 圣地亚哥附近（南半球）
        b2s = [min(b2[0], b2[2]), min(b2[1], b2[3]), max(b2[0], b2[2]), max(b2[1], b2[3])]
        e2 = estimate_local_utm_crs_from_bounds(bounds=b2s, source_crs=CRS.from_epsg(4326))
        estimates.append({"bounds": b2s, "source_epsg": 4326, "epsg": int(e2.to_epsg())})

        # UTM 源 → 转 4326 → 估计（应回到同带）
        b3 = [500000.0, 2478000.0, 520000.0, 2500000.0]  # 32649 (49N) 中部
        e3 = estimate_local_utm_crs_from_bounds(bounds=b3, source_crs=CRS.from_epsg(32649))
        estimates.append({"bounds": b3, "source_epsg": 32649, "epsg": int(e3.to_epsg())})
        print("estimate via real Python OK")
    except Exception as exc:  # pragma: no cover
        print(f"[warn] estimate 部分跳过：{exc}")

    OUT.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump({"centers": centers, "estimates": estimates}, f)
    print(f"  centers={len(centers)} estimates={len(estimates)}")
    print(f"written {OUT}")


if __name__ == "__main__":
    main()
