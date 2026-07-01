"""生成 hydro 阶段 6 EDT 对拍夹具：scipy.ndimage.distance_transform_edt（含 return_indices）。

运行：
  & <myproject_py310>\python.exe scripts\parity\gen_stage6_edt_fixtures.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np
from scipy.ndimage import distance_transform_edt

sys.path.insert(0, r"E:\Projects\MyProject\modules")

OUT = Path(__file__).resolve().parents[2] / "crates" / "water-hydro" / "tests" / "fixtures" / "stage6_edt_cases.json"


def case(name, mask):
    mask = np.asarray(mask, dtype=bool)
    dist, (ir, ic) = distance_transform_edt(mask, return_indices=True)
    h, w = mask.shape
    return {
        "name": name, "h": int(h), "w": int(w),
        "mask": [int(v) for v in mask.flatten()],
        "dist": [float(v) for v in np.asarray(dist, dtype=np.float64).flatten()],
        "ir": [int(v) for v in np.asarray(ir).flatten()],
        "ic": [int(v) for v in np.asarray(ic).flatten()],
    }


def main():
    rng = np.random.default_rng(6)
    cases = []

    # 矩形前景
    m = np.zeros((14, 18), bool); m[3:11, 4:14] = True
    cases.append(case("rect", m))
    # 圆盘
    yy, xx = np.mgrid[0:24, 0:24]
    cases.append(case("disk", ((xx - 11.5) ** 2 + (yy - 11.5) ** 2) <= 8.0 ** 2))
    # 带洞环
    ring = ((xx - 11.5) ** 2 + (yy - 11.5) ** 2) <= 10.0 ** 2
    ring &= ((xx - 11.5) ** 2 + (yy - 11.5) ** 2) >= 5.0 ** 2
    cases.append(case("ring", ring))
    # 随机
    cases.append(case("random", rng.random((20, 26)) > 0.4))
    # 细线前景
    ln = np.zeros((12, 20), bool); ln[6, 2:18] = True
    cases.append(case("thin_line", ln))
    # 单个背景点（其余全前景）→ 距离场为到该点的距离
    one = np.ones((15, 15), bool); one[7, 7] = False
    cases.append(case("single_bg", one))

    OUT.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump({"cases": cases}, f)
    for c in cases:
        print(f"  {c['name']}: {c['h']}x{c['w']} maxdist={max(c['dist']):.3f}")
    print(f"written {OUT}")


if __name__ == "__main__":
    main()
