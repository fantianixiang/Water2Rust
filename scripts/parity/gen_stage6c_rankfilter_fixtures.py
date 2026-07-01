"""生成 hydro 阶段 6c 秩滤波对拍夹具（scipy.ndimage percentile_filter / median_filter）。

- percentile_filter：2D 方框窗口，覆盖 laplace 中 P30 空间滤波的用法（含 +inf 填充）。
- median_filter：1D，覆盖 _isotonic_multi_peak 的轻度平滑用法。

运行：
  & <myproject_py310>\python.exe scripts\parity\gen_stage6c_rankfilter_fixtures.py
"""

from __future__ import annotations

import json
import math
import sys
from pathlib import Path

import numpy as np
from scipy.ndimage import median_filter, percentile_filter

sys.path.insert(0, r"E:\Projects\MyProject\modules")

OUT = (
    Path(__file__).resolve().parents[2]
    / "crates" / "water-hydro" / "tests" / "fixtures" / "stage6c_rankfilter_cases.json"
)


def _jnum(v):
    if v is None:
        return None
    v = float(v)
    if math.isnan(v):
        return None
    if math.isinf(v):
        return "inf" if v > 0 else "-inf"
    return v


def pct_case(name, data, percentile, size):
    data = np.asarray(data, dtype=np.float64)
    out = percentile_filter(data, percentile=percentile, size=size)  # mode='reflect'
    h, w = data.shape
    return {
        "kind": "pct2d", "name": name, "h": int(h), "w": int(w),
        "percentile": float(percentile), "size": int(size),
        "data": [_jnum(v) for v in data.flatten()],
        "out": [_jnum(v) for v in np.asarray(out, dtype=np.float64).flatten()],
    }


def med_case(name, data, size):
    data = np.asarray(data, dtype=np.float64)
    out = median_filter(data, size=size)  # 1D, mode='reflect'
    return {
        "kind": "med1d", "name": name, "n": int(data.size), "size": int(size),
        "data": [_jnum(v) for v in data.flatten()],
        "out": [_jnum(v) for v in np.asarray(out, dtype=np.float64).flatten()],
    }


def main():
    rng = np.random.default_rng(63)
    inf = float("inf")
    cases = []

    # 2D percentile P30（laplace 用法）：稀疏骨架值 + 其余 +inf 填充。
    d = np.full((9, 11), inf)
    for (r, c, v) in [(2, 3, 100.0), (2, 4, 105.0), (3, 4, 98.0), (4, 5, 110.0),
                      (5, 6, 95.0), (6, 6, 120.0)]:
        d[r, c] = v
    cases.append(pct_case("sparse_p30_s5", d, 30, 5))
    cases.append(pct_case("sparse_p30_s7", d, 30, 7))

    # 2D percentile 一般数据，多个 size / percentile。
    cases.append(pct_case("rand_p30_s3", rng.standard_normal((10, 12)) * 10 + 100, 30, 3))
    cases.append(pct_case("rand_p50_s5", rng.standard_normal((11, 13)) * 5 + 50, 50, 5))
    cases.append(pct_case("rand_p100_s4", rng.standard_normal((8, 9)), 100, 4))
    cases.append(pct_case("rand_p0_s3", rng.standard_normal((7, 7)), 0, 3))

    # 1D median（multi_peak 用法）。
    cases.append(med_case("med_s3", [3.0, 1.0, 4.0, 1.0, 5.0, 9.0, 2.0, 6.0], 3))
    cases.append(med_case("med_s5", [10.0, 8.0, 6.0, 20.0, 4.0, 2.0, 30.0, 1.0, 5.0], 5))
    cases.append(med_case("med_s2", [3.0, 1.0, 4.0, 1.0, 5.0], 2))
    cases.append(med_case("med_rand_s3", (rng.standard_normal(40) * 10 + 100).tolist(), 3))

    OUT.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump({"cases": cases}, f)
    for c in cases:
        print(f"  {c['kind']} {c['name']}")
    print(f"written {OUT}")


if __name__ == "__main__":
    main()
