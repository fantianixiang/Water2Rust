// laplace_mg —— 聚合多重网格 V-cycle 预条件的 matrix-free PCG（实现）。
// 接口/层次结构约定见 laplace_mg.cuh；通用宏见 common.cuh。
//
// 混合精度（V9）：**外层共轭梯度保 FP64**（残差递推需高精度以收敛到 1e-13），
// **V-cycle 预条件用 FP32**（预条件只需近似，精度不影响最终解，只影响迭代数；5070 FP32=64×FP64
// 且访存流量减半）。故层次算子/工作缓冲为 float，外层 A_0/CG 向量为 double，两者间以转换核桥接。

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

// ── 加权紧凑 5 点 SpMV：out = A x（模板于精度 T）──
template <typename T>
__global__ void kw_spmv(const T *diag, const int *nbr, const T *wgt, const T *x,
                        T *out, int n) {
  int stride = blockDim.x * gridDim.x;
  for (int i = blockIdx.x * blockDim.x + threadIdx.x; i < n; i += stride) {
    T s = diag[i] * x[i];
    const int *nb = nbr + (long long)i * 4;
    const T *w = wgt + (long long)i * 4;
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

// ── 单位权紧凑 5 点 SpMV（外层 FP64 A_0，level 0 权恒 1，免 wgt 数组）──
template <typename T>
__global__ void k_spmv_unit(const T *diag, const int *nbr, const T *x, T *out,
                            int n) {
  int stride = blockDim.x * gridDim.x;
  for (int i = blockIdx.x * blockDim.x + threadIdx.x; i < n; i += stride) {
    T s = diag[i] * x[i];
    const int *nb = nbr + (long long)i * 4;
#pragma unroll
    for (int k = 0; k < 4; k++) {
      int j = nb[k];
      if (j >= 0) {
        s -= x[j];
      }
    }
    out[i] = s;
  }
}

// ── 阻尼 Jacobi 一遍：xnew = x + omega*(b - A x)/diag（模板于精度 T）──
template <typename T>
__global__ void kw_jacobi(const T *diag, const int *nbr, const T *wgt,
                          const T *b, const T *x, T *xnew, T omega, int n) {
  int stride = blockDim.x * gridDim.x;
  for (int i = blockIdx.x * blockDim.x + threadIdx.x; i < n; i += stride) {
    T d = diag[i];
    T ax = d * x[i];
    const int *nb = nbr + (long long)i * 4;
    const T *w = wgt + (long long)i * 4;
#pragma unroll
    for (int k = 0; k < 4; k++) {
      int j = nb[k];
      if (j >= 0) {
        ax -= w[k] * x[j];
      }
    }
    xnew[i] = (d > T(0)) ? x[i] + omega * (b[i] - ax) / d : T(0);
  }
}

// ── 残差：r = b - A x（模板于精度 T）──
template <typename T>
__global__ void kw_residual(const T *diag, const int *nbr, const T *wgt,
                            const T *b, const T *x, T *r, int n) {
  int stride = blockDim.x * gridDim.x;
  for (int i = blockIdx.x * blockDim.x + threadIdx.x; i < n; i += stride) {
    T ax = diag[i] * x[i];
    const int *nb = nbr + (long long)i * 4;
    const T *w = wgt + (long long)i * 4;
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

// ── 限制 R = P^T（gather）：bc[J] = Σ_k (child[J*4+k] >= 0 ? rf[child]:0) ──
template <typename T>
__global__ void kw_restrict(const int *child, const T *rf, T *bc, int nc) {
  int stride = blockDim.x * gridDim.x;
  for (int J = blockIdx.x * blockDim.x + threadIdx.x; J < nc; J += stride) {
    const int *ch = child + (long long)J * 4;
    T s = T(0);
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
template <typename T>
__global__ void kw_prolong_add(const int *agg, const T *xc, T *xf, int nf) {
  int stride = blockDim.x * gridDim.x;
  for (int i = blockIdx.x * blockDim.x + threadIdx.x; i < nf; i += stride) {
    int J = agg[i];
    if (J >= 0) {
      xf[i] += xc[J];
    }
  }
}

// ── 精度转换核（外层 FP64 残差 ↔ V-cycle FP32）──
__global__ void k_d2f(const double *src, float *dst, int n) {
  int stride = blockDim.x * gridDim.x;
  for (int i = blockIdx.x * blockDim.x + threadIdx.x; i < n; i += stride) {
    dst[i] = (float)src[i];
  }
}
__global__ void k_f2d(const float *src, double *dst, int n) {
  int stride = blockDim.x * gridDim.x;
  for (int i = blockIdx.x * blockDim.x + threadIdx.x; i < n; i += stride) {
    dst[i] = (double)src[i];
  }
}

// ── 外层 CG（FP64）：y += a*x / p = z + beta*p / 点积 ──
__global__ void kw_axpy(double *y, double a, const double *x, int n) {
  int stride = blockDim.x * gridDim.x;
  for (int i = blockIdx.x * blockDim.x + threadIdx.x; i < n; i += stride) {
    y[i] += a * x[i];
  }
}
__global__ void kw_update_p(double *p, const double *z, double beta, int n) {
  int stride = blockDim.x * gridDim.x;
  for (int i = blockIdx.x * blockDim.x + threadIdx.x; i < n; i += stride) {
    p[i] = z[i] + beta * p[i];
  }
}
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

// ── 设备端单层上下文（V-cycle 精度 = float）──
struct MgLevel {
  int n = 0;
  int grid = 0;
  float *diag = nullptr; // 算子对角（FP32）
  int *nbr = nullptr;    // 4n 邻居下标
  float *wgt = nullptr;  // 4n 边权（FP32）
  int *agg = nullptr;    // n → l+1（最粗层为空）
  int *child = nullptr;  // 4n ← l-1（最细层为空）
  float *x = nullptr;    // 解
  float *b = nullptr;    // rhs
  float *r = nullptr;    // 残差
  float *tmp = nullptr;  // Jacobi ping-pong
};

// 阻尼 Jacobi 光滑 count 遍（ping-pong，结果留在 x）。
static void mg_smooth(const MgLevel &L, const float *b, float *x, float omega,
                      int count) {
  if (count <= 0) {
    return;
  }
  float *cur = x;
  float *other = L.tmp;
  for (int s = 0; s < count; s++) {
    kw_jacobi<float><<<L.grid, MG_BLOCK>>>(L.diag, L.nbr, L.wgt, b, cur, other,
                                           omega, L.n);
    float *t = cur;
    cur = other;
    other = t;
  }
  if (cur != x) {
    cudaMemcpy(x, cur, (size_t)L.n * sizeof(float), cudaMemcpyDeviceToDevice);
  }
}

// 一次 V-cycle（FP32）：从零初值求 x0 ≈ A_0^{-1} b0。b0（lv[0].b）需已填。
static void mg_vcycle(std::vector<MgLevel> &lv, int pre, int post, int coarse,
                      float omega) {
  const int L = (int)lv.size();
  // 下行：光滑 → 残差 → 限制
  for (int l = 0; l < L - 1; l++) {
    cudaMemset(lv[l].x, 0, (size_t)lv[l].n * sizeof(float));
    mg_smooth(lv[l], lv[l].b, lv[l].x, omega, pre);
    kw_residual<float><<<lv[l].grid, MG_BLOCK>>>(lv[l].diag, lv[l].nbr,
                                                 lv[l].wgt, lv[l].b, lv[l].x,
                                                 lv[l].r, lv[l].n);
    kw_restrict<float><<<lv[l + 1].grid, MG_BLOCK>>>(lv[l + 1].child, lv[l].r,
                                                     lv[l + 1].b, lv[l + 1].n);
  }
  // 最粗层：多遍 Jacobi 近似求解
  {
    const int l = L - 1;
    cudaMemset(lv[l].x, 0, (size_t)lv[l].n * sizeof(float));
    mg_smooth(lv[l], lv[l].b, lv[l].x, omega, coarse);
  }
  // 上行：延拓校正 → 后光滑
  for (int l = L - 2; l >= 0; l--) {
    kw_prolong_add<float><<<lv[l].grid, MG_BLOCK>>>(lv[l].agg, lv[l + 1].x,
                                                    lv[l].x, lv[l].n);
    mg_smooth(lv[l], lv[l].b, lv[l].x, omega, post);
  }
}

extern "C" int water_laplace_pcg_mg(
    int n_levels, const int *level_n, const float *diag_all,
    const int *nbr_all, const float *wgt_all, const int *agg_all,
    const int *child_all, const double *diag0_f64, const double *h_b,
    double *h_z, double rtol, int max_iter, int mg_pre, int mg_post,
    int mg_coarse, double mg_omega, int *out_iters, double *out_res,
    WaterKernelTiming *timing) {
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
  const float omega = (float)mg_omega;

  // 段偏移（前缀和）：off1 用于 n 长数组，off4 用于 4n 长数组。
  std::vector<size_t> off1(L), off4(L);
  size_t s1 = 0, s4 = 0;
  for (int l = 0; l < L; l++) {
    off1[l] = s1;
    off4[l] = s4;
    s1 += (size_t)level_n[l];
    s4 += (size_t)level_n[l] * 4;
  }

  // 粗略显存估算：FP32 层次每层 ~ (4+16+16+4+4+4+4)n + agg/child(4+16)n；外层 FP64 7*n0 + diag0。
  size_t need = 0;
  for (int l = 0; l < L; l++) {
    need += (size_t)level_n[l] * (4 + 16 + 16 + 4 + 4 + 4 + 4 + 4 + 16);
  }
  need += bytes0 * 8 + (size_t)grid0 * sizeof(double);
  CUDA_CHECK(cuda_require_free_mem(need));

  std::vector<MgLevel> lv(L);

  CudaTimer timer;

  // ── H2D：逐层分配并上传 FP32 算子 + 工作缓冲 ──
  timer.start();
  for (int l = 0; l < L; l++) {
    const int n = level_n[l];
    lv[l].n = n;
    lv[l].grid = mg_grid(n);
    const size_t fn = (size_t)n * sizeof(float);
    const size_t f4 = (size_t)n * 4 * sizeof(float);
    const size_t i4 = (size_t)n * 4 * sizeof(int);
    CUDA_CHECK(cudaMalloc((void **)&lv[l].diag, fn));
    CUDA_CHECK(cudaMalloc((void **)&lv[l].nbr, i4));
    CUDA_CHECK(cudaMalloc((void **)&lv[l].wgt, f4));
    CUDA_CHECK(cudaMalloc((void **)&lv[l].x, fn));
    CUDA_CHECK(cudaMalloc((void **)&lv[l].b, fn));
    CUDA_CHECK(cudaMalloc((void **)&lv[l].r, fn));
    CUDA_CHECK(cudaMalloc((void **)&lv[l].tmp, fn));
    CUDA_CHECK(cudaMemcpy(lv[l].diag, diag_all + off1[l], fn,
                          cudaMemcpyHostToDevice));
    CUDA_CHECK(cudaMemcpy(lv[l].nbr, nbr_all + off4[l], i4,
                          cudaMemcpyHostToDevice));
    CUDA_CHECK(cudaMemcpy(lv[l].wgt, wgt_all + off4[l], f4,
                          cudaMemcpyHostToDevice));
    if (l < L - 1) { // agg → l+1
      CUDA_CHECK(cudaMalloc((void **)&lv[l].agg, (size_t)n * sizeof(int)));
      CUDA_CHECK(cudaMemcpy(lv[l].agg, agg_all + off1[l],
                            (size_t)n * sizeof(int), cudaMemcpyHostToDevice));
    }
    if (l >= 1) { // child ← l-1
      CUDA_CHECK(cudaMalloc((void **)&lv[l].child, i4));
      CUDA_CHECK(cudaMemcpy(lv[l].child, child_all + off4[l], i4,
                            cudaMemcpyHostToDevice));
    }
  }

  // 外层 CG（FP64）：A_0 对角 diag0 + PCG 向量 z,r,p,Ap,zpc,b。nbr0 复用 lv[0].nbr。
  double *d_diag0, *d_z, *d_r, *d_p, *d_Ap, *d_zpc, *d_b, *d_partial;
  CUDA_CHECK(cudaMalloc((void **)&d_diag0, bytes0));
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
  CUDA_CHECK(cudaMemcpy(d_diag0, diag0_f64, bytes0, cudaMemcpyHostToDevice));
  CUDA_CHECK(cudaMemcpy(d_b, h_b, bytes0, cudaMemcpyHostToDevice));
  CUDA_CHECK(cudaMemset(d_z, 0, bytes0));
  const double h2d_ms = timer.stop_ms();

  // ── 求解循环（混合精度：FP64 外层 CG + FP32 MG V-cycle 预条件）──
  timer.start();
  CUDA_CHECK(cudaMemcpy(d_r, d_b, bytes0, cudaMemcpyDeviceToDevice)); // r=b（z=0）
  double bnorm = std::sqrt(mg_dot(d_b, d_b, d_partial, h_partial, n0, grid0));
  if (bnorm == 0.0)
    bnorm = 1.0;
  // zpc = M^{-1} r：r(FP64)→b0(FP32) → V-cycle → x0(FP32)→zpc(FP64)
  k_d2f<<<grid0, MG_BLOCK>>>(d_r, lv[0].b, n0);
  mg_vcycle(lv, mg_pre, mg_post, mg_coarse, omega);
  k_f2d<<<grid0, MG_BLOCK>>>(lv[0].x, d_zpc, n0);
  CUDA_CHECK(cudaMemcpy(d_p, d_zpc, bytes0, cudaMemcpyDeviceToDevice));
  double rz = mg_dot(d_r, d_zpc, d_partial, h_partial, n0, grid0);

  int iter = 0;
  double rel = 1.0;
  for (; iter < max_iter; iter++) {
    // Ap = A_0 p（FP64 单位权 SpMV）
    k_spmv_unit<double><<<grid0, MG_BLOCK>>>(d_diag0, lv[0].nbr, d_p, d_Ap, n0);
    double pAp = mg_dot(d_p, d_Ap, d_partial, h_partial, n0, grid0);
    if (pAp == 0.0)
      break;
    double alpha = rz / pAp;
    kw_axpy<<<grid0, MG_BLOCK>>>(d_z, alpha, d_p, n0);
    kw_axpy<<<grid0, MG_BLOCK>>>(d_r, -alpha, d_Ap, n0);
    double rnorm = std::sqrt(mg_dot(d_r, d_r, d_partial, h_partial, n0, grid0));
    rel = rnorm / bnorm;
    if (rel < rtol) {
      iter++;
      break;
    }
    // zpc = M^{-1} r（FP32 V-cycle）
    k_d2f<<<grid0, MG_BLOCK>>>(d_r, lv[0].b, n0);
    mg_vcycle(lv, mg_pre, mg_post, mg_coarse, omega);
    k_f2d<<<grid0, MG_BLOCK>>>(lv[0].x, d_zpc, n0);
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
    cudaFree(lv[l].x);
    cudaFree(lv[l].b);
    cudaFree(lv[l].r);
    cudaFree(lv[l].tmp);
    cudaFree(lv[l].agg);
    cudaFree(lv[l].child);
  }
  cudaFree(d_diag0);
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
