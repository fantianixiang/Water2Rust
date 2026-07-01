import rasterio, numpy as np, geopandas as gpd
from rasterio.windows import Window
from rasterio.features import rasterize

r = rasterio.open('data/tmp/single8_rust.tif'); rust = r.read(1); rnd = r.nodata
w = rasterio.open('data/tmp/single8.tif')
col0 = round((r.transform.c - w.transform.c) / w.transform.a)
row0 = round((r.transform.f - w.transform.f) / w.transform.e)
ref = w.read(1, window=Window(col0, row0, r.width, r.height)); wnd = w.nodata
g = gpd.read_file('data/tmp/single8.shp')
mask = rasterize([(g.iloc[0].geometry, 1)], out_shape=(r.height, r.width),
                 transform=r.transform, fill=0, all_touched=True).astype(bool)
both = (rust != rnd) & (ref != wnd) & np.isfinite(rust) & np.isfinite(ref)
d = np.abs(rust.astype(np.float64) - ref.astype(np.float64))
m = mask & both
b = (~mask) & both
print('WATER  n', int(m.sum()), 'max', round(float(d[m].max()), 4),
      'mean', round(float(d[m].mean()), 5), 'p99', round(float(np.percentile(d[m], 99)), 4))
print('BKGND  n', int(b.sum()), 'max', round(float(d[b].max()), 4),
      'mean', round(float(d[b].mean()), 5), 'p99', round(float(np.percentile(d[b], 99)), 4))
