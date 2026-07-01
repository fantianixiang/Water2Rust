"""复算 Python hydro 对 single8 的源窗口 ROI，与 Rust 的 (4764,8868,1159,706) 对比。"""
import math
import rasterio
import geopandas as gpd
from rasterio.warp import transform_bounds as warp_transform_bounds

dem = rasterio.open('data/linzhi/dem.tif')
src_crs = dem.crs
src_t = dem.transform
W, H = dem.width, dem.height

g = gpd.read_file('data/tmp/single8.shp')  # EPSG:4326
target_crs = 'EPSG:32646'
g_utm = g.to_crs(target_crs)
union = g_utm.unary_union
ub = union.bounds  # (minx,miny,maxx,maxy) in UTM
print('water_union UTM bounds', ub)

wbs = warp_transform_bounds(target_crs, src_crs, *ub, densify_pts=21)
print('water_bounds_source (4326, densify21)', wbs)

inv = ~src_t
c_a, r_a = inv * (wbs[0], wbs[3])
c_b, r_b = inv * (wbs[2], wbs[1])
col_off = int(math.floor(min(c_a, c_b)))
row_off = int(math.floor(min(r_a, r_b)))
col_end = int(math.ceil(max(c_a, c_b)))
row_end = int(math.ceil(max(r_a, r_b)))
span = max(col_end - col_off, row_end - row_off)
pad = max(64, int(0.05 * span))
roi_col = max(0, col_off - pad)
roi_row = max(0, row_off - pad)
roi_ce = min(W, col_end + pad)
roi_re = min(H, row_end + pad)
print(f'PY window: col_off={roi_col} row_off={roi_row} w={roi_ce-roi_col} h={roi_re-roi_row} pad={pad}')
print('RUST window: col_off=4764 row_off=8868 w=1159 h=706')
