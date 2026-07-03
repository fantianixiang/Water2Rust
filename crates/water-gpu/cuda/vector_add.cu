// 最小 GPU 计算核：逐元素向量加，用于端到端验证 nvcc→PTX→cudarc→GPU 全链路。
// 复杂业务核（Laplace 稀疏求解、warp 重投影等）后续加入 cuda/ 目录并在 build.rs 注册。

extern "C" __global__ void vector_add(const float *a, const float *b, float *out,
                                      size_t n) {
  size_t i = blockIdx.x * blockDim.x + threadIdx.x;
  if (i < n) {
    out[i] = a[i] + b[i];
  }
}
