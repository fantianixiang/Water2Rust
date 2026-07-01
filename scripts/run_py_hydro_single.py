from pathlib import Path
from modules.waters import generate_hydro_water_dem

r = generate_hydro_water_dem(
    dem_path=Path('E:/Projects/Water2Rust/data/linzhi/dem.tif'),
    water_path=Path('E:/Projects/Water2Rust/data/tmp/single8.shp'),
    output_path=Path('E:/Projects/Water2Rust/data/tmp/single8_py.tif'),
    output_mode='water_surface_with_dem',
    all_touched=True,
    debug=False,
)
print('OUT', r)
