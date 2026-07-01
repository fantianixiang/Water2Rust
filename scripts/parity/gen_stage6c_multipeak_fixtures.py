"""生成 hydro 阶段 6c _isotonic_multi_peak 对拍夹具。

直接调用原 Python `waters.hydro.hydro_skeleton_zloc._isotonic_multi_peak`，
覆盖多峰 / 单调 / 含 NaN 的河流横断面 z 剖面。

运行：
  & <myproject_py310>\python.exe scripts\parity\gen_stage6c_multipeak_fixtures.py
"""

from __future__ import annotations

import json
import math
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, r"E:\Projects\MyProject\modules")
from waters.hydro.hydro_skeleton_zloc import _isotonic_multi_peak  # noqa: E402

OUT = (
    Path(__file__).resolve().parents[2]
    / "crates" / "water-hydro" / "tests" / "fixtures" / "stage6c_multipeak_cases.json"
)


def _jnum(v):
    v = float(v)
    return None if math.isnan(v) else v


def case(name, data):
    z = np.asarray(data, dtype=np.float64)
    out = _isotonic_multi_peak(z)
    return {
        "name": name,
        "z": [_jnum(v) for v in z.tolist()],
        "out": [_jnum(v) for v in np.asarray(out, dtype=np.float64).tolist()],
    }


def main():
    rng = np.random.default_rng(63)
    nan = float("nan")
    cases = []
    cases.append(case("short", [10.0, 5.0]))                       # n<3 原样
    cases.append(case("monotonic_dec", [100.0, 90.0, 80.0, 70.0, 60.0]))
    cases.append(case("single_peak_desc", [50, 80, 120, 90, 60, 40, 20]))
    cases.append(case("two_peaks",
                      [100, 130, 90, 70, 110, 150, 80, 60, 40]))
    cases.append(case("valley_climb", [200, 150, 100, 120, 180, 250]))
    cases.append(case("with_nan", [100, nan, 130, 90, nan, 110, 150, 80, 60]))
    cases.append(case("all_finite_flat", [50, 50, 50, 50, 50]))
    # 真实感河流横断面：多段起伏 + 少量 NaN
    base = np.linspace(300, 100, 40) + 30 * np.sin(np.linspace(0, 6, 40))
    base = base + rng.standard_normal(40) * 3
    base[7] = nan
    base[23] = nan
    cases.append(case("river_like", base.tolist()))
    cases.append(case("rand", (rng.standard_normal(50) * 40 + 500).tolist()))

    OUT.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump({"cases": cases}, f)
    for c in cases:
        print(f"  {c['name']}: n={len(c['z'])}")
    print(f"written {OUT}")


if __name__ == "__main__":
    main()
