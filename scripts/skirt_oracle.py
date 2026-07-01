import numpy as np
from scipy import ndimage as ndi

H, W = 40, 40
N = 5

# output_mask: 中心矩形水域
output_mask = np.zeros((H, W), dtype=bool)
output_mask[15:25, 12:28] = True

# surface: 水域内水位（带轻微梯度），域外 NaN
surface = np.full((H, W), np.nan, dtype=np.float32)
rr, cc = np.nonzero(output_mask)
surface[rr, cc] = (100.0 + 0.01 * cc + 0.02 * rr).astype(np.float32)

# dem: 向外升高的地形
yy, xx = np.mgrid[0:H, 0:W]
dem = (95.0 + 0.15 * np.abs(xx - 20) + 0.12 * np.abs(yy - 20)).astype(np.float32)

surf = surface.copy()
mask = output_mask.copy()

seed_mask = mask & np.isfinite(surf)
flat_added = ramp_added = 0
if np.any(seed_mask):
    flat_mask = ndi.binary_dilation(mask, iterations=N)
    ramp_outer_mask = ndi.binary_dilation(mask, iterations=2 * N)
    flat_only = flat_mask & ~mask
    ramp_only = ramp_outer_mask & ~flat_mask
    if np.any(flat_only) or np.any(ramp_only):
        dist_px, nn_idx = ndi.distance_transform_edt(
            ~seed_mask, return_distances=True, return_indices=True
        )
        if np.any(flat_only):
            fr, fc = np.nonzero(flat_only)
            surf[fr, fc] = surf[nn_idx[0][fr, fc], nn_idx[1][fr, fc]]
            flat_added = int(fr.size)
        if np.any(ramp_only):
            r_r, r_c = np.nonzero(ramp_only)
            wz = surf[nn_idx[0][r_r, r_c], nn_idx[1][r_r, r_c]].astype(np.float32)
            dem_at = dem[r_r, r_c].astype(np.float32)
            d_ramp = np.clip(
                dist_px[r_r, r_c].astype(np.float32) - np.float32(N), 0.0, np.float32(N)
            )
            t = d_ramp / np.float32(N + 1)
            blended = wz * (np.float32(1.0) - t) + dem_at * t
            blended = np.where(np.isfinite(dem_at), np.maximum(blended, wz), wz)
            surf[r_r, r_c] = blended.astype(np.float32)
            ramp_added = int(r_r.size)
        mask = ramp_outer_mask

with open("crates/water-hydro/tests/fixtures/skirt_ref.txt", "w") as fh:
    fh.write(f"{H} {W} {N} {flat_added} {ramp_added}\n")
    # dem
    for r in range(H):
        fh.write(" ".join(repr(float(dem[r, c])) for c in range(W)) + "\n")
    # input surface (NaN -> 'nan')
    for r in range(H):
        fh.write(" ".join(("nan" if not np.isfinite(surface[r, c]) else repr(float(surface[r, c]))) for c in range(W)) + "\n")
    # input mask
    for r in range(H):
        fh.write(" ".join(("1" if output_mask[r, c] else "0") for c in range(W)) + "\n")
    # output surface
    for r in range(H):
        fh.write(" ".join(("nan" if not np.isfinite(surf[r, c]) else repr(float(surf[r, c]))) for c in range(W)) + "\n")
    # output mask
    for r in range(H):
        fh.write(" ".join(("1" if mask[r, c] else "0") for c in range(W)) + "\n")

print("flat_added", flat_added, "ramp_added", ramp_added)
