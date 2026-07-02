"""全幅对拍 Rust vs Python(identity)：分条带统计 diff，量化 parity。"""
import numpy as np
import rasterio

R = rasterio.open('data/tmp/full_rust.tif')
P = rasterio.open('data/tmp/full_pyident.tif')
assert (R.width, R.height) == (P.width, P.height)
rnd, pnd = R.nodata, P.nodata

BAND = 1000
tot_both = 0
tot_diff_gt = {0.01: 0, 0.1: 0, 1.0: 0, 5.0: 0}
maxd = 0.0
sumd = 0.0
# 采样收集分位数
sample = []
only_r = 0
only_p = 0
for r0 in range(0, R.height, BAND):
    h = min(BAND, R.height - r0)
    ra = R.read(1, window=((r0, r0 + h), (0, R.width)))
    pa = P.read(1, window=((r0, r0 + h), (0, R.width)))
    rv = (ra != rnd) & np.isfinite(ra)
    pv = (pa != pnd) & np.isfinite(pa)
    only_r += int((rv & ~pv).sum())
    only_p += int((pv & ~rv).sum())
    both = rv & pv
    if not both.any():
        continue
    d = np.abs(ra[both].astype(np.float64) - pa[both].astype(np.float64))
    tot_both += d.size
    sumd += float(d.sum())
    maxd = max(maxd, float(d.max()))
    for t in tot_diff_gt:
        tot_diff_gt[t] += int((d > t).sum())
    nz = d[d > 1e-6]
    if nz.size:
        take = nz if nz.size <= 20000 else np.random.choice(nz, 20000, replace=False)
        sample.append(take)

print(f"全幅有效重叠像素: {tot_both:,}")
print(f"仅 Rust 有效: {only_r:,}   仅 Python 有效: {only_p:,}")
print(f"全体 diff: mean={sumd/max(1,tot_both):.5f}m  max={maxd:.2f}m")
print(f"  diff>0.01m: {tot_diff_gt[0.01]:,} ({100*tot_diff_gt[0.01]/tot_both:.3f}%)")
print(f"  diff>0.1m : {tot_diff_gt[0.1]:,} ({100*tot_diff_gt[0.1]/tot_both:.3f}%)")
print(f"  diff>1m   : {tot_diff_gt[1.0]:,} ({100*tot_diff_gt[1.0]/tot_both:.4f}%)")
print(f"  diff>5m   : {tot_diff_gt[5.0]:,} ({100*tot_diff_gt[5.0]/tot_both:.4f}%)")
if sample:
    s = np.concatenate(sample)
    print(f"非零 diff 分位(采样 {s.size}): p50={np.percentile(s,50):.4f} "
          f"p90={np.percentile(s,90):.3f} p99={np.percentile(s,99):.3f} max={s.max():.2f}")
