"""生成 hydro 阶段 6c 横断面水位采样对拍夹具。

直接调用原 Python `_cross_section_z_at_skeleton_pixels`，覆盖 boundary-only 模式
（管线路径，含 EDT 半宽射线截断）与 contour 模式（z_ref/epsilon）。

运行：
  & <myproject_py310>\python.exe scripts\parity\gen_stage6c_xsec_fixtures.py
"""

from __future__ import annotations

import json
import math
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, r"E:\Projects\MyProject\modules")
from waters.hydro.hydro_skeleton_zloc import (  # noqa: E402
    _compute_skeleton_tangents,
    _cross_section_z_at_skeleton_pixels,
)

OUT = (
    Path(__file__).resolve().parents[2]
    / "crates" / "water-hydro" / "tests" / "fixtures" / "stage6c_xsec_cases.json"
)


def _jnum(v):
    v = float(v)
    return None if math.isnan(v) else v


def _hits(lst):
    return [None if p is None else [int(p[0]), int(p[1])] for p in lst]


def case(name, ordered, boundary, dem, *, edt=None, z_ref=None, epsilon=None, max_ray=500):
    ordered = [(int(r), int(c)) for (r, c) in ordered]
    boundary = np.asarray(boundary, dtype=bool)
    dem = np.asarray(dem, dtype=np.float64)
    dummy = np.zeros((1, 1), dtype=bool)
    tang = _compute_skeleton_tangents(dummy, ordered, context_pixels=3)
    edt_arr = None if edt is None else np.asarray(edt, dtype=np.float64)
    zc, lh, rh = _cross_section_z_at_skeleton_pixels(
        ordered, tang, boundary, dem, max_ray_steps=max_ray,
        return_hits=True, z_ref=z_ref, epsilon=epsilon, edt_half_widths=edt_arr,
    )
    h, w = dem.shape
    return {
        "name": name, "h": int(h), "w": int(w),
        "ordered": [[int(r), int(c)] for (r, c) in ordered],
        "tangents": [[float(t[0]), float(t[1])] for t in np.asarray(tang)],
        "boundary": [int(v) for v in boundary.flatten()],
        "dem": [_jnum(v) for v in dem.flatten()],
        "edt": None if edt is None else [float(v) for v in edt_arr],
        "z_ref": z_ref, "epsilon": epsilon, "max_ray": int(max_ray),
        "z_cross": [_jnum(v) for v in np.asarray(zc)],
        "left_hits": _hits(lh), "right_hits": _hits(rh),
    }


def channel(h, w, chan_row, half):
    """水平河道：boundary 为上下两条岸，DEM 中间低两侧高。"""
    boundary = np.zeros((h, w), bool)
    dem = np.zeros((h, w), float)
    for r in range(h):
        for c in range(w):
            d = abs(r - chan_row)
            dem[r, c] = 100.0 + d * 5.0  # 距河心越远越高
    # 岸 = 距河心恰为 half 的行
    for c in range(w):
        if chan_row - half >= 0:
            boundary[chan_row - half, c] = True
        if chan_row + half < h:
            boundary[chan_row + half, c] = True
    skel = [(chan_row, c) for c in range(1, w - 1)]
    return skel, boundary, dem


def main():
    cases = []

    # 水平河道，boundary-only，无 EDT
    skel, bnd, dem = channel(11, 12, 5, 3)
    cases.append(case("channel_boundary", skel, bnd, dem))

    # 同河道 + EDT 半宽（大部分 =3，射线截断到 ceil(4.5)=5）
    edt = [3.0] * len(skel)
    cases.append(case("channel_edt3", skel, bnd, dem, edt=edt))

    # EDT 太小（<=1）→ 用骨架 DEM
    edt_small = [0.5] * len(skel)
    cases.append(case("channel_edt_small", skel, bnd, dem, edt=edt_small))

    # 部分站点无命中（boundary 很远超 max_ray=2）
    cases.append(case("short_ray", skel, bnd, dem, max_ray=2))

    # contour 模式：z_ref/epsilon
    skel2, bnd2, dem2 = channel(11, 12, 5, 4)
    cases.append(case("channel_contour", skel2, bnd2, dem2, z_ref=100.0, epsilon=8.0))

    OUT.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump({"cases": cases}, f)
    for c in cases:
        nfin = sum(1 for v in c["z_cross"] if v is not None)
        print(f"  {c['name']}: n={len(c['ordered'])} z_cross有限={nfin}")
    print(f"written {OUT}")


if __name__ == "__main__":
    main()
