"""生成 Laplace 求解器数值对拍夹具。

导入 MyProject 中**真实**的 `solve_laplace_dirichlet`，对若干构造用例求解，
将输入与期望输出写入 JSON，供 Rust 端 `laplace_parity` 集成测试比对。

运行（用带 GIS 依赖的环境）：
  & <myproject_py310>\python.exe scripts\parity\gen_laplace_fixtures.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np

# 让 `from waters.hydro...` 可解析（waters 包位于 MyProject/modules 下）
sys.path.insert(0, r"E:\Projects\MyProject\modules")
from waters.hydro.hydro_laplace import solve_laplace_dirichlet  # noqa: E402

OUT = Path(__file__).resolve().parents[2] / "crates" / "water-hydro" / "tests" / "fixtures" / "laplace_cases.json"


def _encode(arr: np.ndarray) -> list:
    """float 数组 → list，NaN 编码为 None（对应 Rust 的 null）。"""
    return [None if not np.isfinite(v) else float(v) for v in arr.flatten()]


def _case(name: str, poly: np.ndarray, dmask: np.ndarray, dz: np.ndarray) -> dict:
    poly = poly.astype(bool)
    dmask = dmask.astype(bool)
    dz = dz.astype(np.float64)
    expected = solve_laplace_dirichlet(poly, dmask, dz)
    h, w = poly.shape
    return {
        "name": name,
        "h": int(h),
        "w": int(w),
        "poly": poly.astype(np.uint8).flatten().tolist(),
        "dmask": dmask.astype(np.uint8).flatten().tolist(),
        "dz": [float(v) for v in dz.flatten()],
        "expected": _encode(expected),
    }


def build_cases() -> list[dict]:
    rng = np.random.default_rng(20260701)
    cases: list[dict] = []

    # 用例 1：矩形域，四边为 Dirichlet 环，dz 为线性梯度（经典调和延拓）
    h, w = 12, 16
    poly = np.ones((h, w), dtype=bool)
    dmask = np.zeros((h, w), dtype=bool)
    dmask[0, :] = dmask[-1, :] = dmask[:, 0] = dmask[:, -1] = True
    dz = np.zeros((h, w), dtype=np.float64)
    for r in range(h):
        for c in range(w):
            dz[r, c] = 100.0 + 0.5 * c + 0.2 * r  # 平面 → 解应精确等于该平面
    cases.append(_case("rect_linear", poly, dmask, dz))

    # 用例 2：矩形域，仅左右两列为 Dirichlet，中间插值（一维调和 = 线性）
    h, w = 10, 20
    poly = np.ones((h, w), dtype=bool)
    dmask = np.zeros((h, w), dtype=bool)
    dmask[:, 0] = True
    dmask[:, -1] = True
    dz = np.zeros((h, w), dtype=np.float64)
    dz[:, 0] = 50.0
    dz[:, -1] = 80.0
    cases.append(_case("rect_lr", poly, dmask, dz))

    # 用例 3：圆形 blob 域，边界环为 Dirichlet，dz 随机
    h, w = 24, 24
    yy, xx = np.mgrid[0:h, 0:w]
    poly = ((xx - 11.5) ** 2 + (yy - 11.5) ** 2) <= 10.0 ** 2
    from scipy.ndimage import binary_erosion

    eroded = binary_erosion(poly, iterations=1)
    dmask = poly & ~eroded
    dz = np.zeros((h, w), dtype=np.float64)
    dz[dmask] = 200.0 + 20.0 * rng.standard_normal(int(dmask.sum()))
    cases.append(_case("blob_ring_random", poly, dmask, dz))

    # 用例 4：内部含一条 Dirichlet 线（模拟 centerline pin）
    h, w = 18, 30
    poly = np.ones((h, w), dtype=bool)
    dmask = np.zeros((h, w), dtype=bool)
    dmask[:, 0] = dmask[:, -1] = dmask[0, :] = dmask[-1, :] = True
    dmask[9, 5:25] = True  # 内部一条水平线
    dz = np.zeros((h, w), dtype=np.float64)
    dz[:, 0] = 10.0
    dz[:, -1] = 10.0
    dz[0, :] = 10.0
    dz[-1, :] = 10.0
    dz[9, 5:25] = 3.0  # 河心线更低
    cases.append(_case("interior_pin_line", poly, dmask, dz))

    # 用例 5：不规则随机域（连通），外环 Dirichlet
    h, w = 20, 28
    base = rng.random((h, w)) > 0.25
    from scipy.ndimage import binary_closing, label

    base = binary_closing(base, structure=np.ones((3, 3), bool), iterations=1)
    lbl, n = label(base, structure=np.ones((3, 3), bool))
    if n >= 1:
        sizes = [(lbl == i).sum() for i in range(1, n + 1)]
        biggest = int(np.argmax(sizes)) + 1
        poly = lbl == biggest
    else:
        poly = base
    eroded = binary_erosion(poly, iterations=1)
    dmask = poly & ~eroded
    dz = np.zeros((h, w), dtype=np.float64)
    dz[dmask] = 500.0 + 5.0 * rng.standard_normal(int(dmask.sum()))
    cases.append(_case("irregular_random", poly, dmask, dz))

    return cases


def main() -> None:
    cases = build_cases()
    OUT.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump({"cases": cases}, f)
    for c in cases:
        n_fin = sum(1 for v in c["expected"] if v is not None)
        print(f"  {c['name']}: {c['h']}x{c['w']}  finite={n_fin}")
    print(f"written {OUT}")


if __name__ == "__main__":
    main()
