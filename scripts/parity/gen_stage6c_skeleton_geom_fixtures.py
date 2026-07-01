"""生成 hydro 阶段 6c 骨架切线 + 汇流站点对拍夹具。

直接调用原 Python `_compute_skeleton_tangents` / `_detect_junction_stations`，
以注入的 ordered_pixels（含分支拼接跳变）驱动。tangents 的 `skel` 参数实际未用，传 dummy。

运行：
  & <myproject_py310>\python.exe scripts\parity\gen_stage6c_skeleton_geom_fixtures.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, r"E:\Projects\MyProject\modules")
from waters.hydro.hydro_skeleton_zloc import (  # noqa: E402
    _compute_skeleton_tangents,
    _detect_junction_stations,
)

OUT = (
    Path(__file__).resolve().parents[2]
    / "crates" / "water-hydro" / "tests" / "fixtures" / "stage6c_skeleton_geom_cases.json"
)


def case(name, ordered, context=3, radius=4.0, thr=0.5):
    ordered = [(int(r), int(c)) for (r, c) in ordered]
    dummy = np.zeros((1, 1), dtype=bool)
    tang = _compute_skeleton_tangents(dummy, ordered, context_pixels=context)
    junc = _detect_junction_stations(ordered, tang, radius_px=radius, angle_cos_threshold=thr)
    return {
        "name": name,
        "context": int(context),
        "radius": float(radius),
        "thr": float(thr),
        "ordered": [[int(r), int(c)] for (r, c) in ordered],
        "tangents": [[float(t[0]), float(t[1])] for t in np.asarray(tang)],
        "junction": [bool(b) for b in np.asarray(junc)],
    }


def main():
    cases = []
    # 直线水平骨架
    cases.append(case("horiz_line", [(5, c) for c in range(0, 12)]))
    # 直线竖直骨架
    cases.append(case("vert_line", [(r, 5) for r in range(0, 10)]))
    # 45° 对角
    cases.append(case("diag", [(i, i) for i in range(0, 10)]))
    # 折线（L 形拐弯）
    cases.append(case("elbow", [(2, c) for c in range(0, 6)] + [(r, 5) for r in range(3, 9)]))
    # 分支拼接跳变（两段相距很远，中间有大跳变）
    seg_a = [(3, c) for c in range(0, 5)]
    seg_b = [(20, c) for c in range(10, 15)]
    cases.append(case("branch_jump", seg_a + seg_b))
    # T 形汇流：主干 + 一条垂直支流，交汇处切线交叉
    trunk = [(10, c) for c in range(0, 11)]
    branch = [(r, 5) for r in range(0, 10)]
    cases.append(case("t_junction", trunk + branch))
    # 短路径（n<3 边界）
    cases.append(case("two_px", [(0, 0), (0, 1)]))
    cases.append(case("one_px", [(3, 3)]))

    OUT.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump({"cases": cases}, f)
    for c in cases:
        nj = sum(c["junction"])
        print(f"  {c['name']}: n={len(c['ordered'])} 汇流站点={nj}")
    print(f"written {OUT}")


if __name__ == "__main__":
    main()
