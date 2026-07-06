// warp —— GPU 栅格重投影重采样（WGS84 地理 ↔ 局地 UTM 横轴墨卡托）。
//
// hydro 管线的工作 CRS 在源为地理坐标（EPSG:4326）时取局地 UTM 带；warp 即
// 地理↔UTM 的坐标变换 + 双线性/最近邻采样。此核把**每输出像元精确变换**（免 GDAL
// 近似变换器）+ 采样整体搬到 GPU：CPU 侧的逐行 PROJ 三角函数细分是原 warp 的主瓶颈。
//
// 椭球横轴墨卡托采用 **Snyder（USGS PP1395）四阶级数**（φ0=0、M0=0），在 UTM 带
// （中央经线 ±3°）内精度 ~mm；与 proj4rs 的 etmerc 有 ~sub-cm 级差异（可接受）。
// 采样约定（含 nodata 门控、containing 像元判定）对齐 CPU `warp_reproject.rs`。

// Windows/MSVC 的 <math.h> 默认不定义 M_PI，需先 _USE_MATH_DEFINES（Linux glibc 默认提供）；
// 必须在首个 math 头包含之前定义，故置于所有 include 之前。
#define _USE_MATH_DEFINES
#include "common.cuh"
#include <math.h>

// ── 椭球横轴墨卡托（Snyder）设备函数。角度均为弧度 ──

// 正向：地理 (lat,lon 弧度) → UTM (E,N 米)。φ0=0 → M0=0。
__device__ __forceinline__ void tm_forward(double lat, double lon, double lon0,
                                           double k0, double a, double es,
                                           double fe, double fn, double *E,
                                           double *N) {
  const double ep2 = es / (1.0 - es); // e'^2
  const double sinp = sin(lat), cosp = cos(lat), tanp = tan(lat);
  const double nrad = a / sqrt(1.0 - es * sinp * sinp);
  const double t = tanp * tanp;
  const double c = ep2 * cosp * cosp;
  const double A = (lon - lon0) * cosp;
  const double A2 = A * A, A3 = A2 * A, A4 = A3 * A, A5 = A4 * A, A6 = A5 * A;
  const double es2 = es * es, es3 = es2 * es;
  const double M =
      a * ((1.0 - es / 4.0 - 3.0 * es2 / 64.0 - 5.0 * es3 / 256.0) * lat -
           (3.0 * es / 8.0 + 3.0 * es2 / 32.0 + 45.0 * es3 / 1024.0) *
               sin(2.0 * lat) +
           (15.0 * es2 / 256.0 + 45.0 * es3 / 1024.0) * sin(4.0 * lat) -
           (35.0 * es3 / 3072.0) * sin(6.0 * lat));
  *E = fe + k0 * nrad *
                (A + (1.0 - t + c) * A3 / 6.0 +
                 (5.0 - 18.0 * t + t * t + 72.0 * c - 58.0 * ep2) * A5 / 120.0);
  *N = fn + k0 * (M + nrad * tanp *
                          (A2 / 2.0 +
                           (5.0 - t + 9.0 * c + 4.0 * c * c) * A4 / 24.0 +
                           (61.0 - 58.0 * t + t * t + 600.0 * c - 330.0 * ep2) *
                               A6 / 720.0));
}

// 逆向：UTM (x,y 米) → 地理 (lat,lon 弧度)。φ0=0 → M0=0。
__device__ __forceinline__ void tm_inverse(double x, double y, double lon0,
                                           double k0, double a, double es,
                                           double fe, double fn, double *lat,
                                           double *lon) {
  const double ep2 = es / (1.0 - es);
  const double es2 = es * es, es3 = es2 * es;
  const double e1 = (1.0 - sqrt(1.0 - es)) / (1.0 + sqrt(1.0 - es));
  const double e1_2 = e1 * e1, e1_3 = e1_2 * e1, e1_4 = e1_3 * e1;
  const double M = (y - fn) / k0; // M0=0
  const double mu =
      M / (a * (1.0 - es / 4.0 - 3.0 * es2 / 64.0 - 5.0 * es3 / 256.0));
  const double phi1 =
      mu + (3.0 * e1 / 2.0 - 27.0 * e1_3 / 32.0) * sin(2.0 * mu) +
      (21.0 * e1_2 / 16.0 - 55.0 * e1_4 / 32.0) * sin(4.0 * mu) +
      (151.0 * e1_3 / 96.0) * sin(6.0 * mu) +
      (1097.0 * e1_4 / 512.0) * sin(8.0 * mu);
  const double sinp1 = sin(phi1), cosp1 = cos(phi1), tanp1 = tan(phi1);
  const double C1 = ep2 * cosp1 * cosp1;
  const double T1 = tanp1 * tanp1;
  const double denom = 1.0 - es * sinp1 * sinp1;
  const double N1 = a / sqrt(denom);
  const double R1 = a * (1.0 - es) / (denom * sqrt(denom));
  const double D = (x - fe) / (N1 * k0);
  const double D2 = D * D, D3 = D2 * D, D4 = D3 * D, D5 = D4 * D, D6 = D5 * D;
  *lat = phi1 -
         (N1 * tanp1 / R1) *
             (D2 / 2.0 -
              (5.0 + 3.0 * T1 + 10.0 * C1 - 4.0 * C1 * C1 - 9.0 * ep2) * D4 /
                  24.0 +
              (61.0 + 90.0 * T1 + 298.0 * C1 + 45.0 * T1 * T1 - 252.0 * ep2 -
               3.0 * C1 * C1) *
                  D6 / 720.0);
  *lon = lon0 + (D - (1.0 + 2.0 * T1 + C1) * D3 / 6.0 +
                 (5.0 - 2.0 * C1 + 28.0 * T1 - 3.0 * C1 * C1 + 8.0 * ep2 +
                  24.0 * T1 * T1) *
                     D5 / 120.0) /
                    cosp1;
}

// 源像元采样：越界/非有限/等于 nodata → 无效（返回 0，valid=false）。
__device__ __forceinline__ float src_sample(const float *src, int sh, int sw,
                                             int r, int c, int has_nodata,
                                             float nodata, bool *valid) {
  if (r < 0 || r >= sh || c < 0 || c >= sw) {
    *valid = false;
    return 0.0f;
  }
  float v = src[(long long)r * sw + c];
  if (!isfinite(v) || (has_nodata && v == nodata)) {
    *valid = false;
    return 0.0f;
  }
  *valid = true;
  return v;
}

// 主核：每输出像元 → dst 坐标 → 变换到 src 坐标 → src 像元 → 采样。
// dst_is_utm=1：dst=UTM、src=地理（逆 TM，输出经纬度）；=0：dst=地理、src=UTM（正 TM）。
// resampling：0=最近邻，1=双线性。dst/src 仿射为 rasterio Affine 序 [a,b,c,d,e,f]。
__global__ void warp_kernel(const float *src, int sh, int sw, int has_nodata,
                            float nodata, const double *dt, const double *si,
                            int dw, int dh, int dst_is_utm, double lon0,
                            double k0, double a, double es, double fe,
                            double fn, int resampling, float *dst) {
  long long idx = (long long)blockIdx.x * blockDim.x + threadIdx.x;
  if (idx >= (long long)dw * dh)
    return;
  const int j = (int)(idx % dw); // col
  const int i = (int)(idx / dw); // row
  const double px = j + 0.5, py = i + 0.5;
  // dst 像元 → dst CRS 坐标。
  const double dx = dt[0] * px + dt[1] * py + dt[2];
  const double dy = dt[3] * px + dt[4] * py + dt[5];

  double sxc, syc; // src CRS 坐标
  const double DEG = 180.0 / M_PI, RAD = M_PI / 180.0;
  if (dst_is_utm) {
    // dst=UTM(米) → src=地理：逆 TM → (lat,lon 弧度) → 度。
    double la, lo;
    tm_inverse(dx, dy, lon0, k0, a, es, fe, fn, &la, &lo);
    sxc = lo * DEG; // src x = 经度(度)
    syc = la * DEG; // src y = 纬度(度)
  } else {
    // dst=地理(度: dx=经度, dy=纬度) → src=UTM：正 TM。
    double E, N;
    tm_forward(dy * RAD, dx * RAD, lon0, k0, a, es, fe, fn, &E, &N);
    sxc = E;
    syc = N;
  }
  // src CRS 坐标 → src 像元（逆仿射）。
  const double scol = si[0] * sxc + si[1] * syc + si[2];
  const double srow = si[3] * sxc + si[4] * syc + si[5];

  float out = nanf("");
  // GDAL 门控：源点须落在源栅格 [0,W)×[0,H) 内。
  if (scol >= 0.0 && scol < (double)sw && srow >= 0.0 && srow < (double)sh) {
    if (resampling == 0) {
      // 最近邻。
      int ic = (int)floor(scol), ir = (int)floor(srow);
      bool ok;
      float v = src_sample(src, sh, sw, ir, ic, has_nodata, nodata, &ok);
      if (ok)
        out = v;
    } else {
      // 双线性：containing 像元无效 → NaN；否则对有效邻居加权归一。
      bool okc;
      src_sample(src, sh, sw, (int)floor(srow), (int)floor(scol), has_nodata,
                 nodata, &okc);
      if (okc) {
        const double cx = scol - 0.5, cy = srow - 0.5;
        const double fi0 = floor(cx), fj0 = floor(cy);
        const int i0 = (int)fi0, j0 = (int)fj0; // i0=col, j0=row
        const double rx = cx - fi0, ry = cy - fj0;
        const int nr[4] = {j0, j0, j0 + 1, j0 + 1};
        const int nc[4] = {i0, i0 + 1, i0, i0 + 1};
        const double nw[4] = {(1.0 - rx) * (1.0 - ry), rx * (1.0 - ry),
                              (1.0 - rx) * ry, rx * ry};
        double acc = 0.0, wsum = 0.0;
        for (int k = 0; k < 4; ++k) {
          if (nw[k] == 0.0)
            continue;
          bool ok;
          float v =
              src_sample(src, sh, sw, nr[k], nc[k], has_nodata, nodata, &ok);
          if (ok) {
            acc += nw[k] * (double)v;
            wsum += nw[k];
          }
        }
        if (wsum > 0.0)
          out = (float)(acc / wsum);
      }
    }
  }
  dst[(long long)i * dw + j] = out;
}

// 主机端 launcher：H2D 源 → 上传仿射/参数 → 核 → D2H 目标。分段计时。
// dt/si 为 6 元 double 仿射（rasterio 序）；src_inv 由 Rust 侧预先求逆。
extern "C" int water_warp_reproject(const float *h_src, int sh, int sw,
                                    int has_nodata, float nodata,
                                    const double *h_dt, const double *h_si,
                                    int dw, int dh, int dst_is_utm, double lon0,
                                    double k0, double a, double es, double fe,
                                    double fn, int resampling, float *h_dst,
                                    WaterKernelTiming *timing) {
  if (timing) {
    timing->h2d_ms = 0.0;
    timing->kernel_ms = 0.0;
    timing->d2h_ms = 0.0;
  }
  if (sh <= 0 || sw <= 0 || dw <= 0 || dh <= 0) {
    return cudaSuccess;
  }
  const size_t src_bytes = (size_t)sh * sw * sizeof(float);
  const size_t dst_bytes = (size_t)dw * dh * sizeof(float);

  CUDA_CHECK(cuda_require_free_mem(src_bytes + dst_bytes + 4096));

  float *d_src = nullptr, *d_dst = nullptr;
  double *d_dt = nullptr, *d_si = nullptr;
  CUDA_CHECK(cudaMalloc((void **)&d_src, src_bytes));
  CUDA_CHECK(cudaMalloc((void **)&d_dst, dst_bytes));
  CUDA_CHECK(cudaMalloc((void **)&d_dt, 6 * sizeof(double)));
  CUDA_CHECK(cudaMalloc((void **)&d_si, 6 * sizeof(double)));

  CudaTimer timer;
  timer.start();
  CUDA_CHECK(cudaMemcpy(d_src, h_src, src_bytes, cudaMemcpyHostToDevice));
  CUDA_CHECK(cudaMemcpy(d_dt, h_dt, 6 * sizeof(double), cudaMemcpyHostToDevice));
  CUDA_CHECK(cudaMemcpy(d_si, h_si, 6 * sizeof(double), cudaMemcpyHostToDevice));
  const double h2d_ms = timer.stop_ms();

  const int block = 256;
  const long long grid = ((long long)dw * dh + block - 1) / block;
  timer.start();
  warp_kernel<<<(unsigned int)grid, block>>>(
      d_src, sh, sw, has_nodata, nodata, d_dt, d_si, dw, dh, dst_is_utm, lon0,
      k0, a, es, fe, fn, resampling, d_dst);
  const double kernel_ms = timer.stop_ms();
  CUDA_CHECK_KERNEL("warp_reproject");

  timer.start();
  CUDA_CHECK(cudaMemcpy(h_dst, d_dst, dst_bytes, cudaMemcpyDeviceToHost));
  const double d2h_ms = timer.stop_ms();

  cudaFree(d_src);
  cudaFree(d_dst);
  cudaFree(d_dt);
  cudaFree(d_si);

  if (timing) {
    timing->h2d_ms = h2d_ms;
    timing->kernel_ms = kernel_ms;
    timing->d2h_ms = d2h_ms;
  }
  return cudaSuccess;
}
