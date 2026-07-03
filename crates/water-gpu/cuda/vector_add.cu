// vector_add —— 逐元素向量加（GPU）。Water2GPU CUDA 规范参考实现：
// 主机端 launcher（extern "C"）+ 严格错误/显存检查 + 分段事件计时（H2D/kernel/D2H, ms）。
// 接口声明见同名头 vector_add.cuh；通用宏见 common.cuh。

#include "vector_add.cuh"

// 计算核：每线程处理一个元素，越界判定 `if (i < n)`。
__global__ void vector_add_kernel(const float *a, const float *b, float *out,
                                  long long n) {
  long long i = (long long)blockIdx.x * blockDim.x + threadIdx.x;
  if (i < n) {
    out[i] = a[i] + b[i];
  }
}

extern "C" int water_vector_add(const float *h_a, const float *h_b,
                                float *h_out, long long n,
                                WaterKernelTiming *timing) {
  if (timing) {
    timing->h2d_ms = 0.0;
    timing->kernel_ms = 0.0;
    timing->d2h_ms = 0.0;
  }
  if (n <= 0) {
    return cudaSuccess;
  }
  const size_t bytes = (size_t)n * sizeof(float);

  // ① 显存充足性检查（a + b + out 三块）。
  CUDA_CHECK(cuda_require_free_mem(bytes * 3));

  // ② 设备内存分配。
  float *d_a = nullptr, *d_b = nullptr, *d_out = nullptr;
  CUDA_CHECK(cudaMalloc((void **)&d_a, bytes));
  CUDA_CHECK(cudaMalloc((void **)&d_b, bytes));
  CUDA_CHECK(cudaMalloc((void **)&d_out, bytes));

  CudaTimer timer;

  // ③ H2D 拷贝（分段计时）。
  timer.start();
  CUDA_CHECK(cudaMemcpy(d_a, h_a, bytes, cudaMemcpyHostToDevice));
  CUDA_CHECK(cudaMemcpy(d_b, h_b, bytes, cudaMemcpyHostToDevice));
  const double h2d_ms = timer.stop_ms();

  // ④ 核执行（分段计时）。
  const int block = 256;
  const long long grid = (n + block - 1) / block;
  timer.start();
  vector_add_kernel<<<(unsigned int)grid, block>>>(d_a, d_b, d_out, n);
  const double kernel_ms = timer.stop_ms();
  CUDA_CHECK_KERNEL("vector_add");

  // ⑤ D2H 拷贝（分段计时）。
  timer.start();
  CUDA_CHECK(cudaMemcpy(h_out, d_out, bytes, cudaMemcpyDeviceToHost));
  const double d2h_ms = timer.stop_ms();

  // ⑥ 释放。
  cudaFree(d_a);
  cudaFree(d_b);
  cudaFree(d_out);

  if (timing) {
    timing->h2d_ms = h2d_ms;
    timing->kernel_ms = kernel_ms;
    timing->d2h_ms = d2h_ms;
  }
  return cudaSuccess;
}
