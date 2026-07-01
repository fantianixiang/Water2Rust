"""裁剪一小块真实林芝 DEM + 造一个带 fclass 的小河流多边形，用于端到端冒烟/对拍。"""
import rasterio
from rasterio.windows import Window
import geopandas as gpd
from shapely.geometry import box
import os

os.makedirs("data/linzhi/crop", exist_ok=True)

c0, r0, w, h = 12000, 8000, 400, 400
with rasterio.open("data/linzhi/dem.tif") as d:
    win = Window(c0, r0, w, h)
    arr = d.read(1, window=win)
    t = d.window_transform(win)
    prof = d.profile.copy()
    prof.update(width=w, height=h, transform=t, compress="lzw")
    with rasterio.open("data/linzhi/crop/dem_crop.tif", "w", **prof) as o:
        o.write(arr, 1)
    left, top = t * (0, 0)
    right, bottom = t * (w, h)
    print("crop extent lon", left, right, "lat", bottom, top)

# 河流矩形（落在裁剪范围内部）
minx = left + (right - left) * 0.3
maxx = left + (right - left) * 0.7
miny = bottom + (top - bottom) * 0.35
maxy = bottom + (top - bottom) * 0.6
poly = box(minx, miny, maxx, maxy)
gdf = gpd.GeoDataFrame({"fclass": ["river"]}, geometry=[poly], crs="EPSG:4326")
gdf.to_file("data/linzhi/crop/water.shp")
print("water bbox", minx, miny, maxx, maxy)
