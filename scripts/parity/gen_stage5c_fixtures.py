"""生成 hydro 阶段 5c 对拍夹具：湖泊压平（常数水位盖回求解面）端到端。

导入 MyProject 中**真实**的 `_flatten_lake_polygons_on_surface`，覆盖两种场景：
  - 孤立湖（component_of_polygon=None）：每个湖多边形各自算常数水位。
  - component 分组：同 component 的多个湖多边形共享一个常数水位。

含一个非湖(river)多边形，应被忽略；DEM 含高脊以考验岸线环截尾中位数。

运行：
  & <myproject_py310>\python.exe scripts\parity\gen_stage5c_fixtures.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np
import rasterio
from shapely.geometry import box

sys.path.insert(0, r"E:\Projects\MyProject\modules")
from waters.hydro.hydro_lake_flatten import _flatten_lake_polygons_on_surface  # noqa: E402

OUT = Path(__file__).resolve().parents[2] / "crates" / "water-hydro" / "tests" / "fixtures" / "stage5c_cases.json"

A, B, C, D, E, F = 0.001, 0.0, 100.0, 0.0, -0.001, 30.0
TRANSFORM = rasterio.Affine(A, B, C, D, E, F)


def pw(col, row):
    return C + col * A, F + row * E


def bx(c0, r0, c1, r1):
    return box(*pw(c0, r0), *pw(c1, r1))


def enc(arr):
    return [None if not np.isfinite(v) else float(v) for v in np.asarray(arr, dtype=np.float64).flatten()]


def poly_json(p):
    ext = [[float(x), float(y)] for x, y in p.exterior.coords]
    interiors = [[[float(x), float(y)] for x, y in r.coords] for r in p.interiors]
    return {"exterior": ext, "interiors": interiors}


def build_dem(h, w):
    yy, xx = np.mgrid[0:h, 0:w]
    dem = (1000.0 + 2.0 * yy + 1.0 * xx).astype(np.float32)
    dem[:, w // 2] += 350.0   # 竖直高脊
    dem[h // 2, :] += 250.0   # 水平高脊
    dem[3, 3] = np.nan
    return dem


def run_case(name, dem, h, w, polys, fclass, comp_map):
    surface = np.full((h, w), np.nan, dtype=np.float32)
    summary = _flatten_lake_polygons_on_surface(
        surface=surface,
        transform=TRANSFORM,
        water_polygons=polys,
        water_fclass=fclass,
        profile_records=[],
        solved_nodes=None,
        dem=dem,
        all_touched=True,
        verbose=False,
        component_of_polygon=comp_map,
    )
    pcz = {str(k): float(v) for k, v in summary["polygon_constant_z"].items()}
    return {
        "name": name,
        "polygons": [poly_json(p) for p in polys],
        "fclass": fclass,
        "component_map": ({str(k): int(v) for k, v in comp_map.items()} if comp_map else None),
        "surface_after": enc(surface),
        "summary": {
            "lake_polygon_count": int(summary["lake_polygon_count"]),
            "filled_polygon_count": int(summary["filled_polygon_count"]),
            "skipped_no_constant": int(summary["skipped_no_constant"]),
            "filled_pixel_count": int(summary["filled_pixel_count"]),
        },
        "polygon_constant_z": pcz,
    }


def main():
    h, w = 40, 50
    dem = build_dem(h, w)

    # 多边形：0 lake, 1 reservoir, 2 water, 3 river(非湖,忽略), 4/5 同 component 的水库碎片
    polys = [
        bx(4, 4, 18, 20),      # 0 lake
        bx(22, 5, 36, 18),     # 1 reservoir
        bx(6, 24, 20, 36),     # 2 water
        bx(24, 24, 34, 34),    # 3 river (忽略)
        bx(38, 4, 44, 14),     # 4 reservoir 碎片
        bx(44, 4, 48, 14),     # 5 reservoir 碎片 (与4同component)
    ]
    fclass = ["lake", "reservoir", "water", "river", "reservoir", "reservoir"]

    cases = []
    # 场景1：全孤立（无 component）
    cases.append(run_case("orphans", dem, h, w, polys, fclass, None))
    # 场景2：4 与 5 同 component（共享常数水位）
    cases.append(run_case("component_group", dem, h, w, polys, fclass, {4: 100, 5: 100}))

    OUT.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump({"dem": enc(dem), "h": h, "w": w, "transform": [A, B, C, D, E, F], "cases": cases}, f)
    for c in cases:
        print(f"  {c['name']}: summary={c['summary']} z={c['polygon_constant_z']}")
    print(f"written {OUT}")


if __name__ == "__main__":
    main()
