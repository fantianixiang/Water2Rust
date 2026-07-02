"""输出全域 waters.tif 中最严重浮空水面穹顶的坐标（3857 + 4326），供渲染软件查看。"""
import numpy as np
import rasterio
from rasterio.windows import Window
from rasterio.features import rasterize
import geopandas as gpd
from scipy.ndimage import maximum_filter
from pyproj import Transformer

W = rasterio.open('data/reslut/waters.tif')
src = rasterio.open('data/linzhi/dem.tif')
gg = gpd.read_file('data/reslut/result.shp')
riv = gg[gg['fclass'] == 'river']
a = W.transform.a  # 像素宽(度)
to3857 = Transformer.from_crs('EPSG:4326', 'EPSG:3857', always_xy=True)

cands = []  # (above, lon, lat, x3857, y3857, wsurf, dem)
for _, row in riv.iterrows():
    b = row.geometry.bounds
    c0 = max(0, int((b[0] - W.transform.c) / a) - 2)
    r0 = max(0, int((W.transform.f - b[3]) / abs(a)) - 2)
    w = min(W.width - c0, int((b[2] - b[0]) / a) + 5)
    h = min(W.height - r0, int((b[3] - b[1]) / abs(a)) + 5)
    if w <= 0 or h <= 0 or w * h > 6_000_000:
        continue
    win = Window(c0, r0, w, h)
    wa = W.read(1, window=win).astype(np.float64)
    da = src.read(1, window=win).astype(np.float64)
    wt = W.window_transform(win)
    m = rasterize([(row.geometry, 1)], out_shape=(h, w), transform=wt, fill=0, all_touched=True).astype(bool)
    s = np.where(m & (wa != W.nodata) & np.isfinite(wa), wa, np.nan)
    dd = np.where((da > 0) & np.isfinite(da), da, np.nan)
    above = s - dd
    smax = maximum_filter(np.where(np.isfinite(s), s, -1e9), size=5)
    dome = m & np.isfinite(above) & (above > 15.0) & (s >= smax - 1e-6)
    rr, cc = np.where(dome)
    for r, c in zip(rr, cc):
        lon = wt.c + (c + 0.5) * wt.a
        lat = wt.f + (r + 0.5) * wt.e
        x, y = to3857.transform(lon, lat)
        cands.append((float(above[r, c]), lon, lat, x, y, float(s[r, c]), float(dd[r, c])))

cands.sort(reverse=True)
print(f"检测到 {len(cands)} 处浮空穹顶(水面高于DEM>15m且局部极大)。最严重 20 处：\n")
print(f"{'高出(m)':>7} {'水面(m)':>8} {'DEM(m)':>8}   {'3857_X':>14} {'3857_Y':>14}   {'经度lon':>11} {'纬度lat':>10}")
seen = []
for above, lon, lat, x, y, ws, dem in cands[:60]:
    # 去重：跳过与已列出点相距 < 50m 的
    if any(abs(x - px) < 50 and abs(y - py) < 50 for px, py in seen):
        continue
    seen.append((x, y))
    print(f"{above:7.1f} {ws:8.1f} {dem:8.1f}   {x:14.2f} {y:14.2f}   {lon:11.6f} {lat:10.6f}")
    if len(seen) >= 20:
        break
