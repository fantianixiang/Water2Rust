"""生成 hydro 阶段 6b medial_axis 对拍夹具（固定随机种子）。

skimage.morphology.medial_axis 用 PCG64 随机 permutation 作 tiebreaker（默认非确定性）。
本脚本用**固定种子** SEED 调用 medial_axis(rng=SEED)，并用同一种子复现其内部
tiebreaker（= default_rng(SEED).permutation(arange(n))）一并 dump，供 Rust 注入同一序列，
从而双方在同种子下逐像素一致。

运行：
  & <myproject_py310>\python.exe scripts\parity\gen_stage6b_medial_fixtures.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np
from skimage.morphology import medial_axis

sys.path.insert(0, r"E:\Projects\MyProject\modules")

OUT = Path(__file__).resolve().parents[2] / "crates" / "water-hydro" / "tests" / "fixtures" / "stage6b_medial_cases.json"
SEED = 12345


def case(name, mask):
    mask = np.asarray(mask, dtype=bool)
    n = int(mask.sum())
    # 复现 medial_axis 内部的 tiebreaker（同种子、同为首次抽样）
    tiebreaker = np.random.default_rng(SEED).permutation(np.arange(n))
    skel = medial_axis(mask, rng=SEED)
    h, w = mask.shape
    return {
        "name": name, "h": int(h), "w": int(w),
        "mask": [int(v) for v in mask.flatten()],
        "tiebreaker": [int(v) for v in tiebreaker.tolist()],
        "skel": [int(v) for v in np.asarray(skel, dtype=bool).flatten()],
    }


def main():
    rng = np.random.default_rng(99)
    cases = []

    # 文档示例方块
    sq = np.zeros((7, 7), bool); sq[1:-1, 2:-2] = True
    cases.append(case("doc_square", sq))
    # 实心矩形
    m = np.zeros((16, 22), bool); m[3:13, 4:18] = True
    cases.append(case("rect", m))
    # 圆盘
    yy, xx = np.mgrid[0:26, 0:26]
    cases.append(case("disk", ((xx - 12.5) ** 2 + (yy - 12.5) ** 2) <= 10.0 ** 2))
    # L 形
    lsh = np.zeros((24, 24), bool); lsh[3:20, 3:9] = True; lsh[14:20, 3:20] = True
    cases.append(case("lshape", lsh))
    # 细长河道状 blob（斜向加宽）
    riv = np.zeros((20, 40), bool)
    for c in range(2, 38):
        r0 = int(4 + 0.2 * c)
        half = 2 + (c % 5 == 0)
        riv[max(0, r0 - half):r0 + half + 1, c] = True
    cases.append(case("river_blob", riv))
    # 随机团块（closing 后取最大连通）
    from scipy.ndimage import binary_closing, label
    base = rng.random((22, 28)) > 0.35
    base = binary_closing(base, structure=np.ones((3, 3), bool))
    lbl, nlab = label(base, structure=np.ones((3, 3), bool))
    if nlab >= 1:
        sizes = [(lbl == i).sum() for i in range(1, nlab + 1)]
        base = lbl == (int(np.argmax(sizes)) + 1)
    cases.append(case("random_blob", base))

    OUT.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump({"seed": SEED, "cases": cases}, f)
    for c in cases:
        print(f"  {c['name']}: {c['h']}x{c['w']} fg={sum(c['mask'])} skel={sum(c['skel'])}")
    print(f"written {OUT}")


if __name__ == "__main__":
    main()
