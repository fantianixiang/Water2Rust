"""生成 hydro 阶段 6c find_peaks(prominence) 对拍夹具。

直接调用 scipy.signal.find_peaks(x, prominence=thr)，dump 过滤后的峰下标与显著度。

运行：
  & <myproject_py310>\python.exe scripts\parity\gen_stage6c_findpeaks_fixtures.py
"""

from __future__ import annotations

import json
import math
import sys
from pathlib import Path

import numpy as np
from scipy.signal import find_peaks

sys.path.insert(0, r"E:\Projects\MyProject\modules")

OUT = (
    Path(__file__).resolve().parents[2]
    / "crates" / "water-hydro" / "tests" / "fixtures" / "stage6c_findpeaks_cases.json"
)


def _jnum(v):
    v = float(v)
    return None if math.isnan(v) else v


def case(name, data, prom):
    x = np.asarray(data, dtype=np.float64)
    peaks, props = find_peaks(x, prominence=prom)
    return {
        "name": name,
        "prom": float(prom),
        "x": [_jnum(v) for v in x.tolist()],
        "peaks": [int(p) for p in peaks.tolist()],
        "prominences": [float(v) for v in props["prominences"].tolist()],
    }


def main():
    rng = np.random.default_rng(63)
    nan = float("nan")
    cases = []
    cases.append(case("simple", [0, 1, 0, 2, 0, 3, 0], 0.5))
    cases.append(case("two_peaks", [1, 3, 1, 1, 5, 1], 1.0))
    cases.append(case("plateau", [0, 2, 2, 2, 0, 1, 3, 3, 0], 0.5))
    cases.append(case("high_thr", [0, 1, 0, 2, 0, 5, 0], 3.0))  # 只留大峰
    cases.append(case("monotonic", [1, 2, 3, 4, 5], 0.5))       # 无峰
    cases.append(case("valley_between", [0, 5, 3, 6, 0], 1.0))
    cases.append(case("with_nan", [0, 3, nan, 4, 0, 2, 0], 0.5))
    cases.append(case("flat", [2, 2, 2, 2], 0.5))
    # 随机 + 自适应阈值（模拟 multi_peak 的 prom_thr = max(5, 0.05*ptp)）
    r = rng.standard_normal(60) * 20 + 100
    ptp = float(np.ptp(r))
    cases.append(case("rand_adaptive", r.tolist(), max(5.0, 0.05 * ptp)))
    r2 = rng.standard_normal(120) * 50 + 2000
    ptp2 = float(np.ptp(r2))
    cases.append(case("rand_large", r2.tolist(), max(5.0, 0.05 * ptp2)))

    OUT.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump({"cases": cases}, f)
    for c in cases:
        print(f"  {c['name']}: peaks={c['peaks']}")
    print(f"written {OUT}")


if __name__ == "__main__":
    main()
