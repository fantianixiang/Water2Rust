"""系统检测「浮空水面穹顶」：水面显著高于同像素 DEM 且为局部水面极大值（穹顶）。

对 RND(随机)/IDN(确定)/waters.tif(全域真实) 统计，量化 Python 算法此弱点的普遍程度。
"""
import numpy as np
import rasterio
from rasterio.windows import Window
from rasterio.features import rasterize
import geopandas as gpd
from scipy.ndimage import maximum_filter

REF = rasterio.open('data/tmp/single8_rust.tif')


def lw(p):
    d = rasterio.open(p)
    c0 = round((REF.transform.c - d.transform.c) / d.transform.a)
    r0 = round((REF.transform.f - d.transform.f) / d.transform.e)
    return d.read(1, window=Window(c0, r0, REF.width, REF.height)), d.nodata


dem, dnd = lw('data/linzhi/dem.tif')
demf = np.where((dem > 0) & np.isfinite(dem), dem.astype(np.float64), np.nan)
g = gpd.read_file('data/tmp/single8.shp')
mask = rasterize([(g.iloc[0].geometry, 1)], out_shape=(REF.height, REF.width),
                 transform=REF.transform, fill=0, all_touched=True).astype(bool)


def detect(path, name):
    a, nd = lw(path)
    s = np.where(mask & (a != nd) & np.isfinite(a), a.astype(np.float64), np.nan)
    above = s - demf  # 水面 - DEM
    # 浮空穹顶：水面高于DEM > T 且是 5x5 局部水面极大值
    smax = maximum_filter(np.where(np.isfinite(s), s, -1e9), size=5)
    is_local_max = np.isfinite(s) & (s >= smax - 1e-6)
    for T in (5.0, 10.0):
        dome = mask & np.isfinite(above) & (above > T) & is_local_max
        n = int(dome.sum())
        peak = np.nanmax(above) if np.isfinite(above).any() else float('nan')
        print(f"[{name}] 浮空穹顶(水面高于DEM>{T:.0f}m 且局部极大): {n} 处  "
              f"全体 max(水面-DEM)={peak:.1f}m  "
              f"水面高于DEM>{T:.0f}m 总像素={int((mask & (above>T)).sum())}")
    return above


print("单 poly (single8) ROI 内:")
detect('data/tmp/single8.tif', 'RND随机')
detect('data/tmp/single8_pyseedidentity.tif', 'IDN确定')

# ---- 全域 waters.tif：抽样几个大河多边形统计 ----
print("\n全域 waters.tif（全部水体，随机 tiebreak）:")
W = rasterio.open('data/reslut/waters.tif')
src = rasterio.open('data/linzhi/dem.tif')
gg = gpd.read_file('data/reslut/result.shp')
riv = gg[gg['fclass'] == 'river']
a = 0.0001716614
tot_dome = 0
tot_water = 0
maxabove = 0.0
for i, row in riv.iterrows():
    b = row.geometry.bounds
    c0 = max(0, int((b[0] - W.transform.c) / a) - 2)
    r0 = max(0, int((W.transform.f - b[3]) / abs(a)) - 2)
    w = min(W.width - c0, int((b[2] - b[0]) / a) + 5)
    h = min(W.height - r0, int((b[3] - b[1]) / abs(a)) + 5)
    if w <= 0 or h <= 0 or w * h > 6_000_000:
        continue
    win = Window(c0, r0, w, h)
    wa = W.read(1, window=win)
    da = src.read(1, window=win)
    wt = W.window_transform(win)
    m = rasterize([(row.geometry, 1)], out_shape=(h, w), transform=wt, fill=0, all_touched=True).astype(bool)
    s = np.where(m & (wa != W.nodata) & np.isfinite(wa), wa.astype(np.float64), np.nan)
    dd = np.where((da > 0) & np.isfinite(da), da.astype(np.float64), np.nan)
    above = s - dd
    smax = maximum_filter(np.where(np.isfinite(s), s, -1e9), size=5)
    dome = m & np.isfinite(above) & (above > 10.0) & (s >= smax - 1e-6)
    tot_dome += int(dome.sum())
    tot_water += int((m & np.isfinite(s)).sum())
    if np.isfinite(above).any():
        maxabove = max(maxabove, float(np.nanmax(above)))
print(f"全域 river 多边形: 浮空穹顶(>10m且局部极大)={tot_dome} 处 / 水像素 {tot_water}  "
      f"全域 max(水面-DEM)={maxabove:.1f}m")
