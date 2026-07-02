"""测试用户假设：medial_axis 随机 tiebreak 是否偶尔造出「浮空水面」spike。

对比同为 Python、同输入、仅 tiebreak 不同的两版：
  single8.tif           = 随机 tiebreak（原参照）
  single8_pyseedidentity = 确定性 identity
找最大差处，打印各自局部 7x7 水面块 + DEM，判断是否「平滑区突现 spike」。
"""
import numpy as np
import rasterio
from rasterio.windows import Window
from rasterio.features import rasterize
import geopandas as gpd

REF = rasterio.open('data/tmp/single8_rust.tif')  # 仅用其网格/范围对齐
RND = 'data/tmp/single8.tif'
IDN = 'data/tmp/single8_pyseedidentity.tif'
DEM = 'data/linzhi/dem.tif'
SHP = 'data/tmp/single8.shp'


def load_win(path):
    d = rasterio.open(path)
    c0 = round((REF.transform.c - d.transform.c) / d.transform.a)
    r0 = round((REF.transform.f - d.transform.f) / d.transform.e)
    return d.read(1, window=Window(c0, r0, REF.width, REF.height)), d.nodata


rnd_a, rnd_nd = load_win(RND)
idn_a, idn_nd = load_win(IDN)
dem_a, dem_nd = load_win(DEM)
g = gpd.read_file(SHP)
mask = rasterize([(g.iloc[0].geometry, 1)], out_shape=(REF.height, REF.width),
                 transform=REF.transform, fill=0, all_touched=True).astype(bool)

both = mask & (rnd_a != rnd_nd) & (idn_a != idn_nd) & np.isfinite(rnd_a) & np.isfinite(idn_a)
err = np.where(both, np.abs(rnd_a.astype(np.float64) - idn_a.astype(np.float64)), 0.0)
print(f"随机 vs 确定性(同为Python) 水体差: max={err.max():.2f} "
      f"p99={np.percentile(err[both],99):.2f} p50={np.percentile(err[both],50):.4f} "
      f"n>2m={int((both&(err>2)).sum())} n>5m={int((both&(err>5)).sum())}")

rr, cc = np.where(both)
order = np.argsort(err[both])[::-1]


def patch(arr, r, c, k=3):
    out = []
    for dr in range(-k, k + 1):
        row = []
        for dc in range(-k, k + 1):
            rr2, cc2 = r + dr, c + dc
            if 0 <= rr2 < arr.shape[0] and 0 <= cc2 < arr.shape[1]:
                row.append(f"{arr[rr2,cc2]:6.0f}")
            else:
                row.append("   .  ")
        out.append(' '.join(row))
    return out


def local_std(arr, r, c, k=3):
    sub = arr[max(0, r-k):r+k+1, max(0, c-k):c+k+1].astype(np.float64)
    sub = sub[np.isfinite(sub)]
    return sub.std() if sub.size > 2 else np.nan


print("\n=== 最大差 5 处：各自 7x7 局部块（RND=随机, IDN=确定, DEM）===")
for k in order[:5]:
    r, c = rr[k], cc[k]
    print(f"\n--- ({r},{c}) 差={err[r,c]:.2f}m  RND={rnd_a[r,c]:.1f} IDN={idn_a[r,c]:.1f} DEM={dem_a[r,c]:.1f}"
          f"  局部std: RND={local_std(rnd_a,r,c):.2f} IDN={local_std(idn_a,r,c):.2f} DEM={local_std(dem_a,r,c):.2f} ---")
    pr, pi, pd = patch(rnd_a, r, c), patch(idn_a, r, c), patch(dem_a, r, c)
    print("  RND(随机)        IDN(确定)        DEM")
    for a, b, cd in zip(pr, pi, pd):
        print(f"  {a}   {b}   {cd}")

# 浮空判据：随机版某像素显著偏离其局部中位数（spike），而确定版不偏
from scipy.ndimage import median_filter
rnd_med = median_filter(np.where(both, rnd_a.astype(np.float64), np.nan), size=5)
idn_med = median_filter(np.where(both, idn_a.astype(np.float64), np.nan), size=5)
rnd_spike = np.where(both, np.abs(rnd_a - rnd_med), 0.0)
idn_spike = np.where(both, np.abs(idn_a - idn_med), 0.0)
print(f"\n偏离局部中位数(5x5) spike: 随机 max={np.nanmax(rnd_spike):.2f} p99={np.nanpercentile(rnd_spike[both],99):.2f}"
      f"  |  确定 max={np.nanmax(idn_spike[both]):.2f} p99={np.nanpercentile(idn_spike[both],99):.2f}")
print("（若随机 spike 明显大于确定，则随机 tiebreak 确实造成浮空/突变）")
