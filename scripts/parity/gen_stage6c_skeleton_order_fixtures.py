"""生成 hydro 阶段 6c 骨架沿流排序对拍夹具。

直接调用原 Python `_order_skeleton_pixels_along_flow(skel, dem_window, distance)`
（distance 实际未用，传 dummy），以注入的骨架布尔阵 + DEM 窗口驱动。

运行：
  & <myproject_py310>\python.exe scripts\parity\gen_stage6c_skeleton_order_fixtures.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, r"E:\Projects\MyProject\modules")
from waters.hydro.hydro_skeleton_zloc import _order_skeleton_pixels_along_flow  # noqa: E402

OUT = (
    Path(__file__).resolve().parents[2]
    / "crates" / "water-hydro" / "tests" / "fixtures" / "stage6c_skeleton_order_cases.json"
)


def case(name, skel, dem):
    skel = np.asarray(skel, dtype=bool)
    dem = np.asarray(dem, dtype=np.float64)
    dummy_dist = np.zeros_like(dem)
    ordered = _order_skeleton_pixels_along_flow(skel, dem, dummy_dist)
    h, w = skel.shape
    return {
        "name": name, "h": int(h), "w": int(w),
        "skel": [int(v) for v in skel.flatten()],
        "dem": [float(v) for v in dem.flatten()],
        "ordered": [[int(r), int(c)] for (r, c) in ordered],
    }


def straight_h(h, w, row):
    skel = np.zeros((h, w), bool)
    skel[row, :] = True
    # DEM 从左到右递增 → 出水口在左端
    dem = np.tile(np.arange(w, dtype=float), (h, 1)) * 1.0 + 100.0
    return skel, dem


def main():
    cases = []

    # 水平直线，左端最低（出水口在左）
    skel, dem = straight_h(3, 10, 1)
    cases.append(case("horiz_outlet_left", skel, dem))

    # 水平直线，右端最低（出水口在右，应从右向左排）
    skel2 = np.zeros((3, 10), bool); skel2[1, :] = True
    dem2 = np.tile(np.arange(9, -1, -1, dtype=float), (3, 1)) + 100.0
    cases.append(case("horiz_outlet_right", skel2, dem2))

    # Y 形分叉：主干 + 两条上游支
    h, w = 11, 11
    skel3 = np.zeros((h, w), bool)
    for c in range(0, 6):
        skel3[5, c] = True            # 主干水平段 (row5, col0..5)
    for k in range(1, 5):
        skel3[5 - k, 5 + k] = True    # 上支斜向右上
        skel3[5 + k, 5 + k] = True    # 下支斜向右下
    dem3 = np.zeros((h, w), float)
    for r in range(h):
        for c in range(w):
            dem3[r, c] = 100.0 + c * 2.0 + abs(r - 5) * 1.0  # 左低右高
    cases.append(case("y_fork", skel3, dem3))

    # 折线 L 形
    h, w = 9, 9
    skel4 = np.zeros((h, w), bool)
    for c in range(0, 6):
        skel4[2, c] = True
    for r in range(2, 8):
        skel4[r, 5] = True
    dem4 = np.zeros((h, w), float)
    for r in range(h):
        for c in range(w):
            dem4[r, c] = 100.0 + c + r
    cases.append(case("elbow", skel4, dem4))

    # 短骨架（两像素）
    skel5 = np.zeros((3, 4), bool); skel5[1, 1] = True; skel5[1, 2] = True
    dem5 = np.zeros((3, 4), float); dem5[1, 1] = 100.0; dem5[1, 2] = 105.0
    cases.append(case("two_px", skel5, dem5))

    OUT.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump({"cases": cases}, f)
    for c in cases:
        print(f"  {c['name']}: {c['h']}x{c['w']} n_ordered={len(c['ordered'])}")
    print(f"written {OUT}")


if __name__ == "__main__":
    main()
