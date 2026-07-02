"""可视化问题区域（浮空水面）：多边形/骨架/水面/DEM 结构。"""
import numpy as np
import rasterio
from rasterio.windows import Window
from rasterio.features import rasterize
import geopandas as gpd
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt

REF = rasterio.open('data/tmp/single8_rust.tif')
RND = 'data/tmp/single8.tif'
IDN = 'data/tmp/single8_pyseedidentity.tif'
DEM = 'data/linzhi/dem.tif'
SHP = 'data/tmp/single8.shp'


def load_win(path):
    d = rasterio.open(path)
    c0 = round((REF.transform.c - d.transform.c) / d.transform.a)
    r0 = round((REF.transform.f - d.transform.f) / d.transform.e)
    return d.read(1, window=Window(c0, r0, REF.width, REF.height)), d.nodata


rnd_a, _ = load_win(RND)
idn_a, _ = load_win(IDN)
dem_a, _ = load_win(DEM)
g = gpd.read_file(SHP)
mask = rasterize([(g.iloc[0].geometry, 1)], out_shape=(REF.height, REF.width),
                 transform=REF.transform, fill=0, all_touched=True).astype(bool)

# 问题区域 + 更大上下文
R0, R1, C0, C1 = 540, 660, 470, 560
sl = (slice(R0, R1), slice(C0, C1))
dem_c = np.where(dem_a > 0, dem_a, np.nan)[sl]
rnd_c = np.where(mask & (rnd_a > -9000), rnd_a, np.nan)[sl]
idn_c = np.where(mask & (idn_a > -9000), idn_a, np.nan)[sl]
mask_c = mask[sl]
diff_c = np.where(mask_c, np.abs(rnd_a[sl] - idn_a[sl]), np.nan)

vmin = np.nanmin([np.nanmin(rnd_c), np.nanmin(idn_c)])
vmax = np.nanmax([np.nanmax(rnd_c), np.nanmax(idn_c)])

fig, ax = plt.subplots(2, 3, figsize=(16, 10))
im0 = ax[0, 0].imshow(dem_c, cmap='terrain'); ax[0, 0].set_title('DEM (terrain)'); plt.colorbar(im0, ax=ax[0, 0])
ax[0, 1].imshow(mask_c, cmap='Blues'); ax[0, 1].set_title('polygon mask')
im2 = ax[0, 2].imshow(diff_c, cmap='hot'); ax[0, 2].set_title('|RND-IDN| water diff'); plt.colorbar(im2, ax=ax[0, 2])
im3 = ax[1, 0].imshow(rnd_c, cmap='viridis', vmin=vmin, vmax=vmax); ax[1, 0].set_title('RND water surface'); plt.colorbar(im3, ax=ax[1, 0])
im4 = ax[1, 1].imshow(idn_c, cmap='viridis', vmin=vmin, vmax=vmax); ax[1, 1].set_title('IDN water surface'); plt.colorbar(im4, ax=ax[1, 1])
# 水面-DEM（正=水在地形之上=浮空）
above = np.where(mask_c, idn_a[sl] - np.where(dem_a[sl] > 0, dem_a[sl], np.nan), np.nan)
im5 = ax[1, 2].imshow(above, cmap='RdBu_r', vmin=-15, vmax=15); ax[1, 2].set_title('IDN water - DEM (>0=浮空)'); plt.colorbar(im5, ax=ax[1, 2])
for a in ax.flat:
    a.plot(517 - C0, 587 - R0, 'rx', ms=12, mew=2)
plt.tight_layout()
plt.savefig('data/tmp/floating_region.png', dpi=90)
print('saved data/tmp/floating_region.png')

# 沿问题列 517 的纵剖面
col = 517 - C0
print('\n沿 col=517 纵剖面 (row, DEM, RND, IDN, mask):')
for r in range(575, 605):
    rr = r - R0
    m = mask[r, 517]
    print(f"  {r}: DEM={dem_a[r,517]:6.1f}  RND={rnd_a[r,517]:6.1f}  IDN={idn_a[r,517]:6.1f}  water={'Y' if m else '.'}")
