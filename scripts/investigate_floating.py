"""排查「浮空水面」bug：定位误差最大处，检查周围平滑度 + 内在检测浮空水面。

浮空水面定义：水面高程显著高于其周围河岸 DEM（水不可能高过岸），或水面在平滑区域
突然出现 spike。分别对 Rust、Python(确定性 identity)、Python(随机) 三个输出检查。
"""
import numpy as np
import rasterio
from rasterio.windows import Window
from rasterio.features import rasterize
import geopandas as gpd
from scipy.ndimage import binary_dilation, binary_erosion, generic_filter

DEM = 'data/linzhi/dem.tif'
RUST = 'data/tmp/single8_rust.tif'
PY_ID = 'data/tmp/single8_pyseedidentity.tif'
SHP = 'data/tmp/single8.shp'


def load_win(path, ref):
    d = rasterio.open(path)
    c0 = round((ref.transform.c - d.transform.c) / d.transform.a)
    r0 = round((ref.transform.f - d.transform.f) / d.transform.e)
    return d.read(1, window=Window(c0, r0, ref.width, ref.height)), d.nodata


rust = rasterio.open(RUST)
ra = rust.read(1)
rnd = rust.nodata
py, pnd = load_win(PY_ID, rust)
dem, dnd = load_win(DEM, rust)
g = gpd.read_file(SHP)
mask = rasterize([(g.iloc[0].geometry, 1)], out_shape=(rust.height, rust.width),
                 transform=rust.transform, fill=0, all_touched=True).astype(bool)

# 有效水像素
rv = (ra != rnd) & np.isfinite(ra)
pv = (py != pnd) & np.isfinite(py)
dv = (dem != dnd) & np.isfinite(dem) & (dem != 0)

print(f"水像素数 mask={int(mask.sum())}  DEM有效={int((mask&dv).sum())}")

# ---- 1) 内在浮空检测：水面 - 局部DEM（水面应 <= 岸边DEM）----
def report_floating(surf, sv, name):
    w = mask & sv & dv
    diff = np.where(w, surf.astype(np.float64) - dem.astype(np.float64), np.nan)  # 水面 - DEM
    d = diff[w]
    print(f"\n[{name}] 水面-DEM: n={d.size} min={d.min():.2f} max={d.max():.2f} "
          f"mean={d.mean():.2f} p50={np.percentile(d,50):.2f} p99={np.percentile(d,99):.2f}")
    # 水面高于DEM > 2m 的像素（可疑浮空/漫滩）
    above = w & (diff > 2.0)
    print(f"    水面高于DEM>2m: {int(above.sum())} 像素 "
          f"({100*above.sum()/max(1,w.sum()):.1f}%)  最高出 {np.nanmax(diff):.1f}m")
    return diff


rd = report_floating(ra, rv, 'RUST')
pd_ = report_floating(py, pv, 'PY_identity')

# ---- 2) Rust vs Python 误差最大处的局部平滑度 ----
both = mask & rv & pv
err = np.where(both, np.abs(ra.astype(np.float64) - py.astype(np.float64)), 0.0)
print(f"\n[Rust vs PY_identity] 水体误差 max={err.max():.3f} "
      f"p99={np.percentile(err[both],99):.3f} p50={np.percentile(err[both],50):.4f}")

# 局部粗糙度：3x3 标准差
def roughness(arr, valid):
    a = np.where(valid, arr.astype(np.float64), np.nan)
    def f(v):
        vv = v[np.isfinite(v)]
        return vv.std() if vv.size >= 3 else np.nan
    return generic_filter(a, f, size=3, mode='constant', cval=np.nan)


dem_rough = roughness(dem, dv)

# top-10 误差点
idx = np.argsort(err[both])[::-1]
rr, cc = np.where(both)
print("\n误差最大 10 处（含局部 DEM 粗糙度 / 水面-DEM）：")
print(" row  col   err   rust    py    dem  demRough(3x3)  rust-dem  py-dem")
for k in idx[:10]:
    r, c = rr[k], cc[k]
    print(f"{r:4d} {c:4d} {err[r,c]:6.2f} {ra[r,c]:7.1f} {py[r,c]:6.1f} "
          f"{dem[r,c]:6.1f} {dem_rough[r,c]:8.2f} {ra[r,c]-dem[r,c]:8.2f} {py[r,c]-dem[r,c]:7.2f}")

# ---- 3) 误差与局部粗糙度的相关性 ----
er = err[both]
dr = dem_rough[both]
ok = np.isfinite(dr)
if ok.sum() > 10:
    corr = np.corrcoef(er[ok], dr[ok])[0, 1]
    print(f"\ncorr(水体误差, 局部DEM粗糙度) = {corr:.3f}")
    big = both & (err > 2.0)
    if big.any():
        print(f"误差>2m 像素: {int(big.sum())}, 其处平均DEM粗糙度={np.nanmean(dem_rough[big]):.2f} "
              f"vs 全体水体平均={np.nanmean(dem_rough[both]):.2f}")
    else:
        print("确定性参照下无 >2m 水体误差（符合 parity）")
