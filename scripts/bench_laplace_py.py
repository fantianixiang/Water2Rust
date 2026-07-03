"""Laplace 求解基线基准：scipy.sparse.linalg.spsolve（Python 原基线求解器）。

在与 Rust/GPU 完全相同的 2D Poisson-Dirichlet SPD 矩阵上计时，用于三方对比
（Python spsolve vs Rust faer vs GPU matrix-free PCG）。

矩阵：m×m 网格 5 点 Laplacian，对角 4、非对角 -1（Dirichlet 边界），
RHS b[i] = ((i*7+3) % 13) - 6 —— 与 water-gpu profile 的构造逐一致。
"""

import time

import numpy as np
import scipy.sparse as sp
from scipy.sparse.linalg import spsolve


def build(m: int):
    # 2D Laplacian = kron(I,T) + kron(T,I)，T 为 1D Laplacian(diag 2, off -1)。
    ee = np.ones(m)
    t = sp.diags([-ee[1:], 2 * ee, -ee[1:]], [-1, 0, 1], shape=(m, m))
    ii = sp.identity(m)
    a = (sp.kron(ii, t) + sp.kron(t, ii)).tocsc()
    n = m * m
    b = np.array([((i * 7 + 3) % 13) - 6 for i in range(n)], dtype=np.float64)
    return a, b


def main():
    sizes = [30, 70, 120, 200, 320, 500, 720, 1000]
    # 预热（首次 spsolve 载入 SuperLU）。
    a, b = build(16)
    spsolve(a, b)

    print(f"{'n':>9} {'spsolve_ms':>12}")
    results = {}
    for m in sizes:
        a, b = build(m)
        # 取 3 次最小值（排除抖动）；大尺寸只跑 1 次。
        reps = 3 if m <= 200 else 1
        best = float("inf")
        for _ in range(reps):
            t0 = time.perf_counter()
            spsolve(a, b)
            best = min(best, (time.perf_counter() - t0) * 1e3)
        results[m * m] = best
        print(f"{m*m:>9} {best:>12.2f}")

    print("\n# 复制到对比表用（n: spsolve_ms）")
    print(results)


if __name__ == "__main__":
    main()
