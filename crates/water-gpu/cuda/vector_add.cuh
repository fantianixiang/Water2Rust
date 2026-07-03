// vector_add —— 逐元素向量加（GPU），全链路自检参考模板。
//
// 声明本 .cu 对外暴露的主机端 launcher（extern "C"，供 Rust FFI 调用）。
// 每个 .cu 都配套同名 .cuh，声明其 kernel/接口（Water2GPU CUDA 规范 ①）。
#pragma once

#include "common.cuh"

extern "C" {

// 在 GPU 上计算 out[i] = a[i] + b[i]（i < n）。
//
// 主机数组 `h_a`/`h_b`/`h_out` 长度均为 `n`。内部完成 显存检查 → cudaMalloc →
// H2D → kernel → D2H → 释放，全程 CUDA_CHECK 严格校验。
// 返回 `cudaError_t`（0 = 成功）；`timing` 非空时写入 H2D/kernel/D2H 分段耗时（ms）。
int water_vector_add(const float *h_a, const float *h_b, float *h_out,
                     long long n, WaterKernelTiming *timing);

} // extern "C"
