// Water2GPU CUDA 通用头 —— 严格错误/显存检查 + 分段事件计时（毫秒）。
//
// 规范对标 NVIDIA cuda-samples 的 Common/helper_cuda.h（checkCudaErrors 模式）
// 与 cudaEvent 计时。所有计算模块 .cu 均 `#include "common.cuh"`。
#pragma once

#include <cstddef>
#include <cstdio>
#include <cuda_runtime.h>

// ── 每个计算模块的分段耗时（毫秒），与 Rust `#[repr(C)] KernelTiming` 内存对齐 ──
struct WaterKernelTiming {
  double h2d_ms;    // 主机 → 设备拷贝耗时
  double kernel_ms; // 核执行耗时
  double d2h_ms;    // 设备 → 主机拷贝耗时
};

// ── 严格错误检查（对标 NVIDIA checkCudaErrors）：包裹每个 CUDA 运行时/库调用 ──
// 失败即打印 文件:行 '调用' -> 错误名(码): 描述，并从当前函数返回该 cudaError_t。
#define CUDA_CHECK(call)                                                        \
  do {                                                                         \
    cudaError_t _err = (call);                                                 \
    if (_err != cudaSuccess) {                                                 \
      std::fprintf(stderr, "[CUDA_CHECK] %s:%d '%s' -> %s(%d): %s\n", __FILE__, \
                   __LINE__, #call, cudaGetErrorName(_err), (int)_err,         \
                   cudaGetErrorString(_err));                                  \
      return _err;                                                             \
    }                                                                          \
  } while (0)

// ── 核启动后检查：捕获启动配置错误 + 同步捕获执行期错误 ──
#define CUDA_CHECK_KERNEL(name)                                                 \
  do {                                                                         \
    cudaError_t _e = cudaGetLastError();                                       \
    if (_e != cudaSuccess) {                                                   \
      std::fprintf(stderr, "[KERNEL %s] launch %s:%d -> %s\n", name, __FILE__, \
                   __LINE__, cudaGetErrorString(_e));                          \
      return _e;                                                               \
    }                                                                          \
    _e = cudaDeviceSynchronize();                                             \
    if (_e != cudaSuccess) {                                                   \
      std::fprintf(stderr, "[KERNEL %s] sync %s:%d -> %s\n", name, __FILE__,   \
                   __LINE__, cudaGetErrorString(_e));                          \
      return _e;                                                               \
    }                                                                          \
  } while (0)

// ── 显存充足性检查：分配前确认可用显存 ≥ 需求，不足即明确报错（而非 OOM 崩溃）──
static inline cudaError_t cuda_require_free_mem(size_t need_bytes) {
  size_t free_b = 0, total_b = 0;
  cudaError_t e = cudaMemGetInfo(&free_b, &total_b);
  if (e != cudaSuccess) {
    return e;
  }
  if (need_bytes > free_b) {
    std::fprintf(stderr,
                 "[MEM] 需 %zu 字节 > 可用 %zu 字节（总 %zu）：显存不足\n",
                 need_bytes, free_b, total_b);
    return cudaErrorMemoryAllocation;
  }
  return cudaSuccess;
}

// ── 分段事件计时器（cudaEvent，毫秒）—— 对标 NVIDIA cuda-samples 的 event 计时 ──
// 用法：CudaTimer t; t.start(); <段>; double ms = t.stop_ms();
struct CudaTimer {
  cudaEvent_t s_, e_;
  CudaTimer() {
    cudaEventCreate(&s_);
    cudaEventCreate(&e_);
  }
  ~CudaTimer() {
    cudaEventDestroy(s_);
    cudaEventDestroy(e_);
  }
  void start() { cudaEventRecord(s_); }
  double stop_ms() {
    cudaEventRecord(e_);
    cudaEventSynchronize(e_);
    float ms = 0.0f;
    cudaEventElapsedTime(&ms, s_, e_);
    return (double)ms;
  }
};
