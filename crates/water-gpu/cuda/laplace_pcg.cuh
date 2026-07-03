// laplace_pcg —— 无矩阵（matrix-free）Jacobi-PCG 求解 5 点 Dirichlet Laplacian（FP64）。
//
// 不显式装配 CSR：用 5 点 stencil 直接施加算子 M：
//   (M z)[r,c] = deg[r,c] * z[r,c] - (z[上] + z[下] + z[左] + z[右])   （网格外/非内部记 0）
// deg[r,c] = 该像素在域内的邻居数（内部+Dirichlet），Dirichlet 的定值已并入 RHS b。
// 非内部像素 deg=0、b=0（矢量在此恒 0），故 stencil 对所有网格内邻居求和即等价只减内部邻居。
//
// 接口声明见本头；通用宏见 common.cuh。规范：NVIDIA 风格 .cu + .cuh + FFI。
#pragma once

#include "common.cuh"

extern "C" {

// 无矩阵 Jacobi-PCG 求解 `M z = b`（FP64）。
//
// `h_deg`/`h_b`/`h_z` 为 `height*width` 行主序全网格数组（f64）：
//   - `h_deg`：对角/度场（非内部像素置 0，兼作内部掩膜）；
//   - `h_b`：RHS（非内部置 0）；
//   - `h_z`：输出解（会被覆盖；内部初值 0）。
// 迭代到 `||r||_2 / ||b||_2 < rtol` 或达 `max_iter`。
// 返回 `cudaError_t`（0=成功）；`out_iters`/`out_res` 写实际迭代数/最终相对残差；
// `timing` 写 H2D / 求解循环 / D2H 分段耗时（ms）。
int water_laplace_pcg(const double *h_deg, const double *h_b, double *h_z,
                      int height, int width, double rtol, int max_iter,
                      int *out_iters, double *out_res, WaterKernelTiming *timing);

} // extern "C"
