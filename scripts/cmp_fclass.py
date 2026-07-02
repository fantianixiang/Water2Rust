import os
import sys

os.environ.setdefault("PYTHONPATH", r"E:\Projects\MyProject")
sys.path.insert(0, r"E:\Projects\MyProject")

import geopandas as gpd
from modules.waters.fclass.pipeline import assign_water_fclass

water = r"E:\Projects\Water2Rust\data\reslut\result.shp"
out = r"E:\Projects\Water2Rust\data\tmp\fclass_py.shp"
ref = r"E:\Projects\MyProject\global_datas\waters_china.gpkg"

assign_water_fclass(water, out, reference_path=ref, osm_mode="local")

py = gpd.read_file(out)
print("PY cols", list(py.columns))
print("PY counts", py["fclass"].value_counts().to_dict())

rust = gpd.read_file(r"E:\Projects\Water2Rust\data\tmp\fclass_rust.shp")
print("RUST cols", list(rust.columns))
print("RUST counts", rust["fclass"].value_counts().to_dict())

# per-feature compare by featureid if present else by order
if "featureid" in py.columns and "featureid" in rust.columns:
    pj = py.set_index("featureid")["fclass"].to_dict()
    rj = rust.set_index("featureid")["fclass"].to_dict()
    diffs = [(k, pj[k], rj.get(k)) for k in pj if rj.get(k) != pj[k]]
    print("diff_by_featureid", len(diffs))
    for d in diffs[:20]:
        print("  ", d)
else:
    diffs = [
        (i, a, b)
        for i, (a, b) in enumerate(zip(py["fclass"], rust["fclass"]))
        if a != b
    ]
    print("diff_by_order", len(diffs))
    for d in diffs[:20]:
        print("  ", d)
