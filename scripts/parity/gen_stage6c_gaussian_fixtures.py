"""生成 hydro 阶段 6c gaussian_filter 对拍夹具（scipy.ndimage.gaussian_filter 默认参数）。

运行：
  & <myproject_py310>\python.exe scripts\parity\gen_stage6c_gaussian_fixtures.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np
from scipy.ndimage import gaussian_filter

sys.path.insert(0, r"E:\Projects\MyProject\modules")

OUT = Path(__file__).resolve().parents[2] / "crates" / "water-hydro" / "tests" / "fixtures" / "stage6c_gaussian_cases.json"


def case(name, data, sigma):
    data = np.asarray(data, dtype=np.float64)
    out = gaussian_filter(data, sigma=sigma)  # 默认 mode='reflect', truncate=4.0, order=0
    h, w = data.shape
    return {
        "name": name, "h": int(h), "w": int(w), "sigma": float(sigma),
        "data": [float(v) for v in data.flatten()],
        "out": [float(v) for v in np.asarray(out, dtype=np.float64).flatten()],
    }


def main():
    rng = np.random.default_rng(63)
    cases = []
    # 中心冲激（核形状 + 边界）
    d = np.zeros((11, 11)); d[5, 5] = 1.0
    cases.append(case("delta_s12", d, 1.2))
    cases.append(case("delta_s30", d, 3.0))
    # 随机
    cases.append(case("rand_s08", rng.standard_normal((14, 18)) * 10 + 100, 0.8))
    cases.append(case("rand_s12", rng.standard_normal((16, 20)) * 5 + 1000, 1.2))
    cases.append(case("rand_s30", rng.standard_normal((20, 26)) * 50 + 2000, 3.0))
    # 斜坡（考验边界 reflect）
    yy, xx = np.mgrid[0:12, 0:15]
    cases.append(case("ramp_s20", (yy * 3.0 + xx * 1.5).astype(float), 2.0))
    # 小数组
    cases.append(case("small_s10", rng.standard_normal((5, 5)), 1.0))

    OUT.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump({"cases": cases}, f)
    for c in cases:
        print(f"  {c['name']}: {c['h']}x{c['w']} sigma={c['sigma']}")
    print(f"written {OUT}")


if __name__ == "__main__":
    main()
