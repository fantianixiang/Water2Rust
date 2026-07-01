"""对拍：两固定种子 Python 输出之间差异（不可约随机性）+ Rust vs 种子参照。"""
import numpy as np
import rasterio
from rasterio.windows import Window
import geopandas as gpd
from rasterio.features import rasterize

rust = rasterio.open('data/tmp/single8_rust.tif')
ra = rust.read(1); rnd = rust.nodata
g = gpd.read_file('data/tmp/single8.shp')
mask = rasterize([(g.iloc[0].geometry, 1)], out_shape=(rust.height, rust.width),
                 transform=rust.transform, fill=0, all_touched=True).astype(bool)
from scipy.ndimage import binary_erosion
inner = binary_erosion(mask, iterations=3)


def load_window(path):
    d = rasterio.open(path)
    c0 = round((rust.transform.c - d.transform.c) / d.transform.a)
    r0 = round((rust.transform.f - d.transform.f) / d.transform.e)
    return d.read(1, window=Window(c0, r0, rust.width, rust.height)), d.nodata


s1, n1 = load_window('data/tmp/single8_pyseed12345.tif')
s2, n2 = load_window('data/tmp/single8_pyseed999.tif')


def stats(a, na, b, nb, region, name):
    both = (a != na) & (b != nb) & np.isfinite(a) & np.isfinite(b) & region
    d = np.abs(a[both].astype(np.float64) - b[both].astype(np.float64))
    if d.size == 0:
        print(name, 'no overlap'); return
    print(f'{name}: n{int(both.sum())} max{d.max():.3f} mean{d.mean():.4f} p99{np.percentile(d,99):.3f} p50{np.percentile(d,50):.4f}')


print('=== 两固定种子之间（不可约随机性）===')
stats(s1, n1, s2, n2, mask, 'seed12345_vs_seed999 WATER')
stats(s1, n1, s2, n2, inner, 'seed12345_vs_seed999 INNER(erode3)')
print('=== Rust vs 种子参照 ===')
stats(ra, rnd, s1, n1, mask, 'rust_vs_seed12345 WATER')
stats(ra, rnd, s1, n1, inner, 'rust_vs_seed12345 INNER(erode3)')
stats(ra, rnd, s2, n2, mask, 'rust_vs_seed999   WATER')
stats(ra, rnd, s2, n2, inner, 'rust_vs_seed999   INNER(erode3)')
