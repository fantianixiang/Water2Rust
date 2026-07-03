// laplace_pcg —— 无矩阵 Jacobi-PCG 求解 5 点 Dirichlet Laplacian（FP64）。
// Water2GPU CUDA 规范实现：设备核 + 主机端 launcher + 严格检查 + 分段计时。
// 接口声明见 laplace_pcg.cuh；通用宏见 common.cuh。

#include "laplace_pcg.cuh"

#include <cmath>
#include <cstdlib>

// 线程配置：块 256，网格上限 1024（grid-stride 覆盖任意 n）。
static const int PCG_BLOCK = 256;
static const int PCG_GRID_CAP = 1024;

static inline int pcg_grid(int n) {
  int g = (n + PCG_BLOCK - 1) / PCG_BLOCK;
  return g < PCG_GRID_CAP ? g : PCG_GRID_CAP;
}

// ── 5 点 stencil 无矩阵 SpMV：out = M z ──
__global__ void k_spmv(const double *deg, const double *z, double *out, int H,
                       int W) {
  int n = H * W;
  int stride = blockDim.x * gridDim.x;
  for (int idx = blockIdx.x * blockDim.x + threadIdx.x; idx < n; idx += stride) {
    double d = deg[idx];
    if (d == 0.0) {
      out[idx] = 0.0;
      continue;
    }
    int r = idx / W;
    int c = idx % W;
    double s = d * z[idx];
    if (r > 0)
      s -= z[idx - W];
    if (r < H - 1)
      s -= z[idx + W];
    if (c > 0)
      s -= z[idx - 1];
    if (c < W - 1)
      s -= z[idx + 1];
    out[idx] = s;
  }
}

// ── Jacobi 预条件：out = (deg>0) ? r/deg : 0 ──
__global__ void k_elemdiv(const double *r, const double *deg, double *out,
                          int n) {
  int stride = blockDim.x * gridDim.x;
  for (int i = blockIdx.x * blockDim.x + threadIdx.x; i < n; i += stride) {
    double d = deg[i];
    out[i] = (d > 0.0) ? r[i] / d : 0.0;
  }
}

// ── y += a * x ──
__global__ void k_axpy(double *y, double a, const double *x, int n) {
  int stride = blockDim.x * gridDim.x;
  for (int i = blockIdx.x * blockDim.x + threadIdx.x; i < n; i += stride) {
    y[i] += a * x[i];
  }
}

// ── p = zpc + beta * p ──
__global__ void k_update_p(double *p, const double *zpc, double beta, int n) {
  int stride = blockDim.x * gridDim.x;
  for (int i = blockIdx.x * blockDim.x + threadIdx.x; i < n; i += stride) {
    p[i] = zpc[i] + beta * p[i];
  }
}

// ── 点积部分和：每块规约到 partial[blockIdx.x] ──
__global__ void k_dot_partial(const double *a, const double *b, double *partial,
                              int n) {
  __shared__ double sdata[PCG_BLOCK];
  int tid = threadIdx.x;
  int stride = blockDim.x * gridDim.x;
  double sum = 0.0;
  for (int i = blockIdx.x * blockDim.x + tid; i < n; i += stride) {
    sum += a[i] * b[i];
  }
  sdata[tid] = sum;
  __syncthreads();
  for (int s = blockDim.x / 2; s > 0; s >>= 1) {
    if (tid < s) {
      sdata[tid] += sdata[tid + s];
    }
    __syncthreads();
  }
  if (tid == 0) {
    partial[blockIdx.x] = sdata[0];
  }
}

// 主机侧点积：k_dot_partial + 拷回 partial 主机求和（确定性）。
static double device_dot(const double *da, const double *db, double *d_partial,
                         double *h_partial, int n, int grid) {
  k_dot_partial<<<grid, PCG_BLOCK>>>(da, db, d_partial, n);
  cudaMemcpy(h_partial, d_partial, (size_t)grid * sizeof(double),
             cudaMemcpyDeviceToHost);
  double s = 0.0;
  for (int i = 0; i < grid; i++) {
    s += h_partial[i];
  }
  return s;
}

extern "C" int water_laplace_pcg(const double *h_deg, const double *h_b,
                                 double *h_z, int height, int width,
                                 double rtol, int max_iter, int *out_iters,
                                 double *out_res, WaterKernelTiming *timing) {
  if (timing) {
    timing->h2d_ms = 0.0;
    timing->kernel_ms = 0.0;
    timing->d2h_ms = 0.0;
  }
  if (out_iters)
    *out_iters = 0;
  if (out_res)
    *out_res = 0.0;
  const int n = height * width;
  if (n <= 0)
    return cudaSuccess;
  const size_t bytes = (size_t)n * sizeof(double);
  const int grid = pcg_grid(n);

  // 显存：deg,b,z,r,p,Ap,zpc + partial。
  CUDA_CHECK(cuda_require_free_mem(bytes * 7 + (size_t)grid * sizeof(double)));

  double *d_deg, *d_b, *d_z, *d_r, *d_p, *d_Ap, *d_zpc, *d_partial;
  CUDA_CHECK(cudaMalloc((void **)&d_deg, bytes));
  CUDA_CHECK(cudaMalloc((void **)&d_b, bytes));
  CUDA_CHECK(cudaMalloc((void **)&d_z, bytes));
  CUDA_CHECK(cudaMalloc((void **)&d_r, bytes));
  CUDA_CHECK(cudaMalloc((void **)&d_p, bytes));
  CUDA_CHECK(cudaMalloc((void **)&d_Ap, bytes));
  CUDA_CHECK(cudaMalloc((void **)&d_zpc, bytes));
  CUDA_CHECK(cudaMalloc((void **)&d_partial, (size_t)grid * sizeof(double)));
  double *h_partial = (double *)std::malloc((size_t)grid * sizeof(double));
  if (!h_partial)
    return cudaErrorMemoryAllocation;

  CudaTimer timer;

  // ── H2D ──
  timer.start();
  CUDA_CHECK(cudaMemcpy(d_deg, h_deg, bytes, cudaMemcpyHostToDevice));
  CUDA_CHECK(cudaMemcpy(d_b, h_b, bytes, cudaMemcpyHostToDevice));
  CUDA_CHECK(cudaMemset(d_z, 0, bytes)); // 初值 z = 0
  const double h2d_ms = timer.stop_ms();

  // ── 求解循环（Jacobi-PCG） ──
  timer.start();
  // r = b - M z = b（z=0）
  CUDA_CHECK(cudaMemcpy(d_r, d_b, bytes, cudaMemcpyDeviceToDevice));
  double bnorm = std::sqrt(device_dot(d_b, d_b, d_partial, h_partial, n, grid));
  if (bnorm == 0.0)
    bnorm = 1.0; // 全零 RHS：解为 0
  // zpc = Minv r; p = zpc; rz = r·zpc
  k_elemdiv<<<grid, PCG_BLOCK>>>(d_r, d_deg, d_zpc, n);
  CUDA_CHECK(cudaMemcpy(d_p, d_zpc, bytes, cudaMemcpyDeviceToDevice));
  double rz = device_dot(d_r, d_zpc, d_partial, h_partial, n, grid);

  int iter = 0;
  double rel = 1.0;
  for (; iter < max_iter; iter++) {
    // Ap = M p
    k_spmv<<<grid, PCG_BLOCK>>>(d_deg, d_p, d_Ap, height, width);
    double pAp = device_dot(d_p, d_Ap, d_partial, h_partial, n, grid);
    if (pAp == 0.0)
      break;
    double alpha = rz / pAp;
    // z += alpha p ; r -= alpha Ap
    k_axpy<<<grid, PCG_BLOCK>>>(d_z, alpha, d_p, n);
    k_axpy<<<grid, PCG_BLOCK>>>(d_r, -alpha, d_Ap, n);
    double rnorm = std::sqrt(device_dot(d_r, d_r, d_partial, h_partial, n, grid));
    rel = rnorm / bnorm;
    if (rel < rtol) {
      iter++;
      break;
    }
    // zpc = Minv r ; rznew = r·zpc ; beta = rznew/rz ; p = zpc + beta p
    k_elemdiv<<<grid, PCG_BLOCK>>>(d_r, d_deg, d_zpc, n);
    double rznew = device_dot(d_r, d_zpc, d_partial, h_partial, n, grid);
    double beta = rznew / rz;
    k_update_p<<<grid, PCG_BLOCK>>>(d_p, d_zpc, beta, n);
    rz = rznew;
  }
  CUDA_CHECK_KERNEL("laplace_pcg");
  const double kernel_ms = timer.stop_ms();

  // ── D2H ──
  timer.start();
  CUDA_CHECK(cudaMemcpy(h_z, d_z, bytes, cudaMemcpyDeviceToHost));
  const double d2h_ms = timer.stop_ms();

  std::free(h_partial);
  cudaFree(d_deg);
  cudaFree(d_b);
  cudaFree(d_z);
  cudaFree(d_r);
  cudaFree(d_p);
  cudaFree(d_Ap);
  cudaFree(d_zpc);
  cudaFree(d_partial);

  if (out_iters)
    *out_iters = iter;
  if (out_res)
    *out_res = rel;
  if (timing) {
    timing->h2d_ms = h2d_ms;
    timing->kernel_ms = kernel_ms;
    timing->d2h_ms = d2h_ms;
  }
  return cudaSuccess;
}
