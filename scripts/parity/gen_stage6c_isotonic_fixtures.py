"""生成 hydro 阶段 6c 等渗回归（PAVA）对拍夹具。

直接调用原 Python `waters.hydro.hydro_skeleton_zloc._isotonic_non_increasing`
（非增）及其非减派生形式，覆盖含 NaN 的序列。

运行：
  & <myproject_py310>\python.exe scripts\parity\gen_stage6c_isotonic_fixtures.py
"""

from __future__ import annotations

import json
import math
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, r"E:\Projects\MyProject\modules")
from waters.hydro.hydro_skeleton_zloc import _isotonic_non_increasing  # noqa: E402

OUT = (
    Path(__file__).resolve().parents[2]
    / "crates" / "water-hydro" / "tests" / "fixtures" / "stage6c_isotonic_cases.json"
)


def _jnum(v):
    # Python json.dump 写 NaN 为非法 token，统一转 null。
    return None if (v is None or (isinstance(v, float) and math.isnan(v))) else float(v)


def case(name, values):
    a = np.asarray(values, dtype=np.float64)
    non_inc = _isotonic_non_increasing(a)
    # 非减 = reverse(non_increasing(reverse))（与 Python 用法一致）。
    non_dec = _isotonic_non_increasing(a[::-1])[::-1]
    return {
        "name": name,
        "n": int(a.size),
        "values": [_jnum(v) for v in a.tolist()],
        "non_increasing": [_jnum(v) for v in np.asarray(non_inc).tolist()],
        "non_decreasing": [_jnum(v) for v in np.asarray(non_dec).tolist()],
    }


def main():
    rng = np.random.default_rng(63)
    nan = float("nan")
    cases = []
    cases.append(case("empty", []))
    cases.append(case("single", [5.0]))
    cases.append(case("all_nan", [nan, nan, nan]))
    cases.append(case("already_dec", [9.0, 7.0, 5.0, 3.0, 1.0]))
    cases.append(case("increasing", [1.0, 2.0, 3.0, 4.0, 5.0]))  # 全被压平为均值
    cases.append(case("violators", [3.0, 5.0, 2.0, 8.0, 1.0, 4.0]))
    cases.append(case("plateau", [5.0, 5.0, 5.0, 2.0, 2.0, 8.0]))
    cases.append(case("with_nan", [10.0, nan, 8.0, 12.0, nan, 3.0, 5.0]))
    cases.append(case("nan_ends", [nan, 4.0, 9.0, 2.0, 6.0, nan]))
    cases.append(case("rand", (rng.standard_normal(30) * 10 + 100).tolist()))
    cases.append(case("rand_large", (rng.standard_normal(200) * 50 + 2000).tolist()))

    OUT.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump({"cases": cases}, f)
    for c in cases:
        print(f"  {c['name']}: n={c['n']}")
    print(f"written {OUT}")


if __name__ == "__main__":
    main()
