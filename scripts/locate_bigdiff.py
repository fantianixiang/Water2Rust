"""定位全幅最大 diff：判断是否集中在瓦片接缝 / 湖泊 / 浮空区。"""
import numpy as np
import rasterio
from rasterio.features import rasterize
import geopandas as gpd

R = rasterio.open('data/tmp/full_rust.tif')
P = rasterio.open('data/tmp/full_pyident.tif')
rnd, pnd = R.nodata, P.nodata
g = gpd.read_file('data/reslut/result.shp')
lake_g = g[g['fclass'].isin(['lake', 'water', 'reservoir'])]
riv_g = g[g['fclass'] == 'river']

TILE = 8192
BAND = 1000
top = []  # (diff, r, c)
lake_big = 0
riv_big = 0
seam_big = 0
tot_big = 0
for r0 in range(0, R.height, BAND):
    h = min(BAND, R.height - r0)
    ra = R.read(1, window=((r0, r0 + h), (0, R.width)))
    pa = P.read(1, window=((r0, r0 + h), (0, R.width)))
    both = (ra != rnd) & (pa != pnd) & np.isfinite(ra) & np.isfinite(pa)
    d = np.where(both, np.abs(ra.astype(np.float64) - pa.astype(np.float64)), 0.0)
    big = d > 5.0
    if not big.any():
        continue
    rr, cc = np.where(big)
    tr = rr + r0
    tot_big += rr.size
    # 瓦片接缝：距最近 8192 边界 <=2 像素
    near_seam = (np.minimum(tr % TILE, TILE - tr % TILE) <= 2) | \
                (np.minimum(cc % TILE, TILE - cc % TILE) <= 2)
    seam_big += int(near_seam.sum())
    for rr_, cc_, dd_ in zip(tr, cc, d[big]):
        if len(top) < 15:
            top.append((dd_, int(rr_), int(cc_)))
        else:
            mn = min(top)
            if dd_ > mn[0]:
                top[top.index(mn)] = (dd_, int(rr_), int(cc_))

# 判定 top 点落在 lake 还是 river
transform = R.transform
def classify(r, c):
    x = transform.c + (c + 0.5) * transform.a
    y = transform.f + (r + 0.5) * transform.e
    from shapely.geometry import Point
    pt = Point(x, y)
    for _, row in lake_g.iterrows():
        if row.geometry.contains(pt):
            return 'LAKE'
    for _, row in riv_g.iterrows():
        if row.geometry.contains(pt):
            return 'RIVER'
    return 'skirt/out'

print(f"diff>5m 总数: {tot_big}  近瓦片接缝(<=2px): {seam_big} ({100*seam_big/max(1,tot_big):.1f}%)")
print("\n最大 diff 15 处 (diff, row, col, 类别, seam近?):")
for d, r, c in sorted(top, reverse=True):
    seam = min(r % TILE, TILE - r % TILE) <= 2 or min(c % TILE, TILE - c % TILE) <= 2
    print(f"  {d:7.2f}m  ({r},{c})  {classify(r,c):9s}  seam={seam}")
