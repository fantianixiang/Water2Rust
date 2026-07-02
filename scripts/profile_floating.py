import numpy as np, rasterio
from rasterio.windows import Window
from rasterio.features import rasterize
import geopandas as gpd

REF = rasterio.open('data/tmp/single8_rust.tif')


def lw(p):
    d = rasterio.open(p)
    c0 = round((REF.transform.c - d.transform.c) / d.transform.a)
    r0 = round((REF.transform.f - d.transform.f) / d.transform.e)
    return d.read(1, window=Window(c0, r0, REF.width, REF.height))


rnd = lw('data/tmp/single8.tif')
idn = lw('data/tmp/single8_pyseedidentity.tif')
dem = lw('data/linzhi/dem.tif')
g = gpd.read_file('data/tmp/single8.shp')
mask = rasterize([(g.iloc[0].geometry, 1)], out_shape=(REF.height, REF.width),
                 transform=REF.transform, fill=0, all_touched=True).astype(bool)

print('沿 col=517 纵剖面 (row: DEM RND IDN water?  RND-DEM IDN-DEM):')
for r in range(576, 600):
    m = 'W' if mask[r, 517] else '.'
    print(f'  {r}: DEM={dem[r,517]:6.1f} RND={rnd[r,517]:6.1f} IDN={idn[r,517]:6.1f} {m}  '
          f'RND-DEM={rnd[r,517]-dem[r,517]:6.1f} IDN-DEM={idn[r,517]-dem[r,517]:6.1f}')

print('\n沿 row=587 横剖面 (col: DEM RND IDN water?):')
for c in range(505, 530):
    m = 'W' if mask[587, c] else '.'
    print(f'  {c}: DEM={dem[587,c]:6.1f} RND={rnd[587,c]:6.1f} IDN={idn[587,c]:6.1f} {m}')

# 水体在该行的连通性
for row in [585, 587, 590]:
    cols = np.where(mask[row])[0]
    if cols.size:
        span = cols.max() - cols.min() + 1
        print(f'row{row}: 水体列 [{cols.min()},{cols.max()}] 像素数={cols.size} 跨度={span} '
              f'{"连续" if cols.size==span else "断裂!"}')
