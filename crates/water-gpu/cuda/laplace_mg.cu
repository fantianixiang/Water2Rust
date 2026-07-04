// laplace_mg —— 聚合多重网格 V-cycle 预条件的 matrix-free FP64 PCG（实现）。
// 接口/层次结构约定见 laplace_mg.cuh；通用宏见 common.cuh。

#include "laplace_mg.cuh"

#include <cmath>
#include <cstdlib>
#include <vector>

static const int MG_BLOCK = 256;
static const int MG_GRID_CAP = 1024;

static inline int mg_grid(int n) {
  int g = (n + MG_BLOCK - 1) / MG_BLOCK;
  return g < MG_GRID_CAP ? g : MG_GRID_CAP;
}

// ── 加权紧凑 5 点 SpMV：out = A x（out[i] = diag[i]*x[i] - Σ_k wgt[k]*x[nbr[k]]）──
__global__ void kw_spmv(const double *diag, const int *nbr, const double *wgt,
                        const double *x, double *out, int n) {
  int stride = blockDim.x * gridDim.x;
  for (int i = blockIdx.x * blockDim.x + threadIdx.x; i < n; i += stride) {
    double s = diag[i] * x[i];
    const int *nb = nbr + (long long)i * 4;
    const double *w = wgt + (long long)i * 4;
#pragma unroll
    for (int k = 0; k < 4; k++) {
      int j = nb[k];
      if (j >= 0) {
        s -= w[k] * x[j];
      }
    }
    out[i] = s;
  }
}

// ── 残差：r = b - A x ──
__global__ void kw_residual(const double *diag, const int *nbr,
                            const double *wgt, const double *b, const double *x,
                            double *r, int n) {
  int stride = blockDim.x * gridDim.x;
  for (int i = blockIdx.x * blockDim.x + threadIdx.x; i < n; i += stride) {
    double ax = diag[i] * x[i];
    const int *nb = nbr + (long long)i * 4;
    const double *w = wgt + (long long)i * 4;
#pragma unroll
    for (int k = 0; k < 4; k++) {
      int j = nb[k];
      if (j >= 0) {
        ax -= w[k] * x[j];
      }
    }
    r[i] = b[i] - ax;
  }
}

// ── 阻尼 Jacobi 一遍：xnew = x + omega*(b - A x)/diag ──
__global__ void kw_jacobi(const double *diag, const int *nbr, const double *wgt,
                          const double *b, const double *x, double *xnew,
                          double omega, int n) {
  int stride = blockDim.x * gridDim.x;
  for (int i = blockIdx.x * blockDim.x + threadIdx.x; i < n; i += stride) {
    double d = diag[i];
    double ax = d * x[i];
    const int *nb = nbr + (long long)i * 4;
    const double *w = wgt + (long long)i * 4;
#pragma unroll
    for (int k = 0; k < 4; k++) {
      int j = nb[k];
      if (j >= 0) {
        ax -= w[k] * x[j];
      }
    }
    xnew[i] = (d > 0.0) ? x[i] + omega * (b[i] - ax) / d : 0.0;
  }
}

// ── 限制 R = P^T（gather）：bc[J] = Σ_k (child[J*4+k] >= 0 ? rf[child]:0) ──
__global__ void kw_restrict(const int *child, const double *rf, double *bc,
                            int nc) {
  int stride = blockDim.x * gridDim.x;
  for (int J = blockIdx.x * blockDim.x + threadIdx.x; J < nc; J += stride) {
    const int *ch = child + (long long)J * 4;
    double s = 0.0;
#pragma unroll
    for (int k = 0; k < 4; k++) {
      int i = ch[k];
      if (i >= 0) {
        s += rf[i];
      }
    }
    bc[J] = s;
  }
}

// ── 延拓校正 P（inject）：xf[i] += xc[agg[i]] ──
__global__ void kw_prolong_add(const int *agg, const double *xc, double *xf,
                               int nf) {
  int stride = blockDim.x * gridDim.x;
  for (int i = blockIdx.x * blockDim.x + threadIdx.x; i < nf; i += stride) {
    int J = agg[i];
    if (J >= 0) {
      xf[i] += xc[J];
    }
  }
}

// ── y += a * x ──
__global__ void kw_axpy(double *y, double a, const double *x, int n) {
  int stride = blockDim.x * gridDim.x;
  for (int i = blockIdx.x * blockDim.x + threadIdx.x; i < n; i += stride) {
    y[i] += a * x[i];
  }
}

// ── p = z + beta * p ──
__global__ void kw_update_p(double *p, const double *z, double beta, int n) {
  int stride = blockDim.x * gridDim.x;
  for (int i = blockIdx.x * blockDim.x + threadIdx.x; i < n; i += stride) {
    p[i] = z[i] + beta * p[i];
  }
}

// ── 点积部分和（每块规约）──
__global__ void kw_dot_partial(const double *a, const double *b, double *part,
                               int n) {
  __shared__ double sd[MG_BLOCK];
  int tid = threadIdx.x;
  int stride = blockDim.x * gridDim.x;
  double sum = 0.0;
  for (int i = blockIdx.x * blockDim.x + tid; i < n; i += stride) {
    sum += a[i] * b[i];
  }
  sd[tid] = sum;
  __syncthreads();
  for (int s = blockDim.x / 2; s > 0; s >>= 1) {
    if (tid < s) {
      sd[tid] += sd[tid + s];
    }
    __syncthreads();
  }
  if (tid == 0) {
    part[blockIdx.x] = sd[0];
  }
}

// 主机侧点积（部分和拷回主机求和，确定性）。
static double mg_dot(const double *da, const double *db, double *d_part,
                     double *h_part, int n, int grid) {
  kw_dot_partial<<<grid, MG_BLOCK>>>(da, db, d_part, n);
  cudaMemcpy(h_part, d_part, (size_t)grid * sizeof(double),
             cudaMemcpyDeviceToHost);
  double s = 0.0;
  for (int i = 0; i < grid; i++) {
    s += h_part[i];
  }
  return s;
}

// ── 设备端单层上下文 ──
struct MgLevel {
  int n = 0;
  int grid = 0;
  double *diag = nullptr; // 算子对角
  int *nbr = nullptr;     // 4n 邻居下标
  double *wgt = nullptr;  // 4n 边权
  int *agg = nullptr;     // n → l+1（最粗层为空）
  int *child = nullptr;   // 4n ← l-1（最细层为空）
  double *x = nullptr;    // 解（l>=1 分配；l=0 用外部 zpc）
  double *b = nullptr;    // rhs（l>=1 分配；l=0 用外部 r）
  double *r = nullptr;    // 残差
  double *tmp = nullptr;  // Jacobi ping-pong 缓冲
};

// 阻尼 Jacobi 光滑 count 遍（ping-pong，结果留在 x）。
static void mg_smooth(const MgLevel &L, const double *b, double *x, double omega,
                      int count) {
  if (count <= 0) {
    return;
  }
  double *cur = x;
  double *other = L.tmp;
  for (int s = 0; s < count; s++) {
    kw_jacobi<<<L.grid, MG_BLOCK>>>(L.diag, L.nbr, L.wgt, b, cur, other, omega,
                                    L.n);
    double *t = cur;
    cur = other;
    other = t;
  }
  if (cur != x) {
    cudaMemcpy(x, cur, (size_t)L.n * sizeof(double), cudaMemcpyDeviceToDevice);
  }
}

// 一次 V-cycle：从零初值求 x0 ≈ A_0^{-1} b0（预条件应用 M^{-1} b0）。b0 只读。
static void mg_vcycle(std::vector<MgLevel> &lv, const double *b0, double *x0,
                      int pre, int post, int coarse, double omega) {
  const int L = (int)lv.size();
  // 下行：光滑 → 残差 → 限制
  for (int l = 0; l < L - 1; l++) {
    const double *bl = (l == 0) ? b0 : lv[l].b;
    double *xl = (l == 0) ? x0 : lv[l].x;
    cudaMemset(xl, 0, (size_t)lv[l].n * sizeof(double));
    mg_smooth(lv[l], bl, xl, omega, pre);
    kw_residual<<<lv[l].grid, MG_BLOCK>>>(lv[l].diag, lv[l].nbr, lv[l].wgt, bl,
                                          xl, lv[l].r, lv[l].n);
    kw_restrict<<<lv[l + 1].grid, MG_BLOCK>>>(lv[l + 1].child, lv[l].r,
                                              lv[l + 1].b, lv[l + 1].n);
  }
  // 最粗层：多遍 Jacobi 近似求解
  {
    const int l = L - 1;
    const double *bl = (l == 0) ? b0 : lv[l].b;
    double *xl = (l == 0) ? x0 : lv[l].x;
    cudaMemset(xl, 0, (size_t)lv[l].n * sizeof(double));
    mg_smooth(lv[l], bl, xl, omega, coarse);
  }
  // 上行：延拓校正 → 后光滑
  for (int l = L - 2; l >= 0; l--) {
    const double *bl = (l == 0) ? b0 : lv[l].b;
    double *xl = (l == 0) ? x0 : lv[l].x;
    kw_prolong_add<<<lv[l].grid, MG_BLOCK>>>(lv[l].agg, lv[l + 1].x, xl,
                                             lv[l].n);
    mg_smooth(lv[l], bl, xl, omega, post);
  }
}

extern "C" int water_laplace_pcg_mg(
    int n_levels, const int *level_n, const double *diag_all,
    const int *nbr_all, const double *wgt_all, const int *agg_all,
    const int *child_all, const double *h_b, double *h_z, double rtol,
    int max_iter, int mg_pre, int mg_post, int mg_coarse, double mg_omega,
    int *out_iters, double *out_res, WaterKernelTiming *timing) {
  if (timing) {
    timing->h2d_ms = 0.0;
    timing->kernel_ms = 0.0;
    timing->d2h_ms = 0.0;
  }
  if (out_iters)
    *out_iters = 0;
  if (out_res)
    *out_res = 0.0;
  if (n_levels <= 0 || level_n[0] <= 0)
    return cudaSuccess;

  const int L = n_levels;
  const int n0 = level_n[0];
  const size_t bytes0 = (size_t)n0 * sizeof(double);
  const int grid0 = mg_grid(n0);

  // 段偏移（前缀和）：off1 用于 n 长数组，off4 用于 4n 长数组。
  std::vector<size_t> off1(L), off4(L);
  size_t s1 = 0, s4 = 0;
  for (int l = 0; l < L; l++) {
    off1[l] = s1;
    off4[l] = s4;
    s1 += (size_t)level_n[l];
    s4 += (size_t)level_n[l] * 4;
  }

  // 粗略显存估算：每层算子+工作缓冲 ~ (8+16+32+8+8)n + (l>=1:16)n + PCG 6*n0。
  size_t need = 0;
  for (int l = 0; l < L; l++) {
    need += (size_t)level_n[l] * (8 + 16 + 32 + 8 + 8 + 4 + 16);
  }
  need += bytes0 * 7 + (size_t)grid0 * sizeof(double);
  CUDA_CHECK(cuda_require_free_mem(need));

  std::vector<MgLevel> lv(L);

  CudaTimer timer;

  // ── H2D：逐层分配并上传算子 + 分配工作缓冲 ──
  timer.start();
  for (int l = 0; l < L; l++) {
    const int n = level_n[l];
    lv[l].n = n;
    lv[l].grid = mg_grid(n);
    const size_t bn = (size_t)n * sizeof(double);
    const size_t b4 = (size_t)n * 4 * sizeof(double);
    const size_t i4 = (size_t)n * 4 * sizeof(int);
    CUDA_CHECK(cudaMalloc((void **)&lv[l].diag, bn));
    CUDA_CHECK(cudaMalloc((void **)&lv[l].nbr, i4));
    CUDA_CHECK(cudaMalloc((void **)&lv[l].wgt, b4));
    CUDA_CHECK(cudaMalloc((void **)&lv[l].r, bn));
    CUDA_CHECK(cudaMalloc((void **)&lv[l].tmp, bn));
    CUDA_CHECK(cudaMemcpy(lv[l].diag, diag_all + off1[l], bn,
                          cudaMemcpyHostToDevice));
    CUDA_CHECK(cudaMemcpy(lv[l].nbr, nbr_all + off4[l], i4,
                          cudaMemcpyHostToDevice));
    CUDA_CHECK(cudaMemcpy(lv[l].wgt, wgt_all + off4[l], b4,
                          cudaMemcpyHostToDevice));
    if (l < L - 1) { // agg → l+1
      CUDA_CHECK(cudaMalloc((void **)&lv[l].agg, (size_t)n * sizeof(int)));
      CUDA_CHECK(cudaMemcpy(lv[l].agg, agg_all + off1[l],
                            (size_t)n * sizeof(int), cudaMemcpyHostToDevice));
    }
    if (l >= 1) { // child ← l-1，且工作解/rhs 缓冲
      CUDA_CHECK(cudaMalloc((void **)&lv[l].child, i4));
      CUDA_CHECK(cudaMemcpy(lv[l].child, child_all + off4[l], i4,
                            cudaMemcpyHostToDevice));
      CUDA_CHECK(cudaMalloc((void **)&lv[l].x, bn));
      CUDA_CHECK(cudaMalloc((void **)&lv[l].b, bn));
    }
  }

  // PCG 向量（第 0 层）：z, r, p, Ap, zpc, b。
  double *d_z, *d_r, *d_p, *d_Ap, *d_zpc, *d_b, *d_partial;
  CUDA_CHECK(cudaMalloc((void **)&d_z, bytes0));
  CUDA_CHECK(cudaMalloc((void **)&d_r, bytes0));
  CUDA_CHECK(cudaMalloc((void **)&d_p, bytes0));
  CUDA_CHECK(cudaMalloc((void **)&d_Ap, bytes0));
  CUDA_CHECK(cudaMalloc((void **)&d_zpc, bytes0));
  CUDA_CHECK(cudaMalloc((void **)&d_b, bytes0));
  CUDA_CHECK(cudaMalloc((void **)&d_partial, (size_t)grid0 * sizeof(double)));
  double *h_partial = (double *)std::malloc((size_t)grid0 * sizeof(double));
  if (!h_partial)
    return cudaErrorMemoryAllocation;
  CUDA_CHECK(cudaMemcpy(d_b, h_b, bytes0, cudaMemcpyHostToDevice));
  CUDA_CHECK(cudaMemset(d_z, 0, bytes0));
  const double h2d_ms = timer.stop_ms();

  // ── 求解循环（MG 预条件 PCG）──
  timer.start();
  CUDA_CHECK(cudaMemcpy(d_r, d_b, bytes0, cudaMemcpyDeviceToDevice)); // r=b（z=0）
  double bnorm =
      std::sqrt(mg_dot(d_b, d_b, d_partial, h_partial, n0, grid0));
  if (bnorm == 0.0)
    bnorm = 1.0;
  // zpc = M^{-1} r（一次 V-cycle）；p = zpc；rz = r·zpc
  mg_vcycle(lv, d_r, d_zpc, mg_pre, mg_post, mg_coarse, mg_omega);
  CUDA_CHECK(cudaMemcpy(d_p, d_zpc, bytes0, cudaMemcpyDeviceToDevice));
  double rz = mg_dot(d_r, d_zpc, d_partial, h_partial, n0, grid0);

  int iter = 0;
  double rel = 1.0;
  for (; iter < max_iter; iter++) {
    kw_spmv<<<grid0, MG_BLOCK>>>(lv[0].diag, lv[0].nbr, lv[0].wgt, d_p, d_Ap,
                                 n0);
    double pAp = mg_dot(d_p, d_Ap, d_partial, h_partial, n0, grid0);
    if (pAp == 0.0)
      break;
    double alpha = rz / pAp;
    kw_axpy<<<grid0, MG_BLOCK>>>(d_z, alpha, d_p, n0);
    kw_axpy<<<grid0, MG_BLOCK>>>(d_r, -alpha, d_Ap, n0);
    double rnorm =
        std::sqrt(mg_dot(d_r, d_r, d_partial, h_partial, n0, grid0));
    rel = rnorm / bnorm;
    if (rel < rtol) {
      iter++;
      break;
    }
    mg_vcycle(lv, d_r, d_zpc, mg_pre, mg_post, mg_coarse, mg_omega);
    double rznew = mg_dot(d_r, d_zpc, d_partial, h_partial, n0, grid0);
    double beta = rznew / rz;
    kw_update_p<<<grid0, MG_BLOCK>>>(d_p, d_zpc, beta, n0);
    rz = rznew;
  }
  CUDA_CHECK_KERNEL("laplace_pcg_mg");
  const double kernel_ms = timer.stop_ms();

  // ── D2H ──
  timer.start();
  CUDA_CHECK(cudaMemcpy(h_z, d_z, bytes0, cudaMemcpyDeviceToHost));
  const double d2h_ms = timer.stop_ms();

  std::free(h_partial);
  for (int l = 0; l < L; l++) {
    cudaFree(lv[l].diag);
    cudaFree(lv[l].nbr);
    cudaFree(lv[l].wgt);
    cudaFree(lv[l].r);
    cudaFree(lv[l].tmp);
    cudaFree(lv[l].agg);
    cudaFree(lv[l].child);
    cudaFree(lv[l].x);
    cudaFree(lv[l].b);
  }
  cudaFree(d_z);
  cudaFree(d_r);
  cudaFree(d_p);
  cudaFree(d_Ap);
  cudaFree(d_zpc);
  cudaFree(d_b);
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
