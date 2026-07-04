// laplace_mg —— 聚合几何多重网格（unsmoothed aggregation V-cycle）预条件的
// matrix-free FP64 PCG，求解 5 点 Dirichlet Laplacian。
//
// 动机：真实细长河 Jacobi-PCG 迭代数巨大（~1200），低频误差需**粗网格校正**才能高效消除。
// 本模块用 2×2 聚合构建层次结构（每层加权紧凑 5 点 Galerkin 算子 A_l = P^T A P，
// P 为分片常数注入延拓，R = P^T 限制），以对称 V-cycle 作 PCG 预条件（SPD → CG 有效、保真）。
//
// 层次结构在主机（Rust）构建后**展平上传**：n_levels 层，第 l 层 n_l 个变量。
// 各数组按层展平（用 level_n 的前缀和定位段）：
//   - diag_all / agg_all：段长 n_l；nbr_all / wgt_all / child_all：段长 4*n_l。
//   - A_l：diag_l（对角）、nbr_l（第 k 邻居的变量下标，-1 无）、wgt_l（对应边权，>0）。
//   - agg_l（l < L-1）：本层各变量在 l+1 层的聚合下标。
//   - child_l（l >= 1）：本层各粗变量在 l-1 层的至多 4 个子变量下标（-1 补齐）。
// RHS h_b / 输出 h_z 仅第 0 层（长度 n_0）。
//
// 接口声明见本头；通用宏见 common.cuh。规范：NVIDIA 风格 .cu + .cuh + FFI。
#pragma once

#include "common.cuh"

extern "C" {

// 聚合多重网格 V-cycle 预条件的 matrix-free PCG 求解 `A_0 z = b`（混合精度）。
//
// **混合精度**：层次算子 diag_all/wgt_all 为 FP32（V-cycle 预条件用，精度不影响最终解、只影响迭代数）；
// `diag0_f64` 为第 0 层 FP64 对角（外层 CG 的 A_0 单位权 SpMV 用），长度 n_0；h_b/h_z 为 FP64、仅第 0 层。
//
// V-cycle 参数：`mg_pre`/`mg_post` 每层前/后阻尼 Jacobi 光滑次数（相等以保对称）、
// `mg_coarse` 最粗层光滑次数、`mg_omega` 阻尼 Jacobi 因子（如 0.8）。
// 迭代到 `||r||_2/||b||_2 < rtol` 或达 `max_iter`。返回 cudaError_t（0=成功）；
// `out_iters`/`out_res` 写实际迭代/最终相对残差；`timing` 写 H2D/求解/D2H 分段耗时（ms）。
int water_laplace_pcg_mg(int n_levels, const int *level_n,
                         const float *diag_all, const int *nbr_all,
                         const float *wgt_all, const int *agg_all,
                         const int *child_all, const double *diag0_f64,
                         const double *h_b, double *h_z, double rtol,
                         int max_iter, int mg_pre, int mg_post, int mg_coarse,
                         double mg_omega, int *out_iters, double *out_res,
                         WaterKernelTiming *timing);

} // extern "C"
