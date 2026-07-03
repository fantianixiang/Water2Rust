"""真实地形 Laplace 系统三方对比之 Python 侧：加载转储的真实系统 `.bin`，
端到端计时 scipy `spsolve`（原 Python 基线求解器），与 Rust/GPU 同系统对比。

复刻原 Python `solve_laplace_dirichlet` 的装配：内部变量 = poly & ~dmask；
M[idx,idx] = 域内邻居数；内部邻居 -1；Dirichlet 邻居定值并入 b。

用法：python scripts/bench_laplace_real.py <sys.bin>
二进制格式（小端）：i64 h,w + u8 poly + u8 dmask + f64 dz。
"""

import struct
import sys
import time

import numpy as np
import scipy.sparse as sp
from scipy.sparse.linalg import spsolve


def load(path):
    with open(path, "rb") as f:
        raw = f.read()
    h, w = struct.unpack_from("<qq", raw, 0)
    off = 16
    n = h * w
    poly = np.frombuffer(raw, np.uint8, n, off).reshape(h, w).astype(bool)
    off += n
    dmask = np.frombuffer(raw, np.uint8, n, off).reshape(h, w).astype(bool)
    off += n
    dz = np.frombuffer(raw, "<f8", n, off).reshape(h, w).copy()
    return poly, dmask, dz


def assemble(poly, dmask, dz):
    h, w = poly.shape
    interior = poly & ~dmask
    idx = -np.ones((h, w), dtype=np.int64)
    ys, xs = np.where(interior)
    idx[ys, xs] = np.arange(len(ys))
    n = len(ys)
    rows, cols, vals = [], [], []
    b = np.zeros(n)
    offs = [(-1, 0), (1, 0), (0, -1), (0, 1)]
    for k in range(n):
        r, c = ys[k], xs[k]
        nnb = 0
        for dr, dc in offs:
            nr, nc = r + dr, c + dc
            if nr < 0 or nr >= h or nc < 0 or nc >= w:
                continue
            if poly[nr, nc] and not dmask[nr, nc]:
                rows.append(k)
                cols.append(idx[nr, nc])
                vals.append(-1.0)
                nnb += 1
            elif dmask[nr, nc]:
                b[k] += dz[nr, nc]
                nnb += 1
        rows.append(k)
        cols.append(k)
        vals.append(float(nnb))
    a = sp.csc_matrix((vals, (rows, cols)), shape=(n, n))
    return a, b


def main():
    path = sys.argv[1]
    poly, dmask, dz = load(path)
    n_int = int((poly & ~dmask).sum())
    print(f"系统 {path}: {poly.shape[0]}x{poly.shape[1]}, n_int={n_int}")

    # 端到端（装配 + spsolve），best-of-3。
    best = float("inf")
    for _ in range(3):
        t0 = time.perf_counter()
        a, b = assemble(poly, dmask, dz)
        spsolve(a, b)
        best = min(best, (time.perf_counter() - t0) * 1e3)
    # 仅 spsolve（排除装配）。
    a, b = assemble(poly, dmask, dz)
    best_solve = float("inf")
    for _ in range(3):
        t0 = time.perf_counter()
        spsolve(a, b)
        best_solve = min(best_solve, (time.perf_counter() - t0) * 1e3)
    print(f"python_e2e_ms={best:.2f} python_spsolve_ms={best_solve:.2f}")


if __name__ == "__main__":
    main()
