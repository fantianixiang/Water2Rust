"""核对 Rust edge_depth_guidance 默认值 与 Python WATER_FCLASS_EDGE_DEPTH 全 8 类一致。"""
import sys
sys.path.insert(0, 'E:/Projects/MyProject')
from modules.waters.settings import WATER_FCLASS_EDGE_DEPTH as W

# Rust edge_depth_guidance() 默认值（来自 crates/water-core/src/edge_depth.rs）
rust = {
    'stream': (1.0, 1.5), 'dock': (3.0, 5.0), 'water': (5.0, 3.0), 'river': (10.0, 2.8),
    'lake': (15.0, 4.5), 'reservoir': (3.0, 6.0), 'glacier': (10.0, 6.0), 'sea': (15.0, 8.0),
}
print(f"{'fclass':10s} {'Python(edge,depth)':22s} {'Rust':16s} match")
ok = True
for k in sorted(W):
    pe, pd = float(W[k]['edgeexpand']), float(W[k]['depth'])
    re, rd = rust.get(k, (None, None))
    m = (pe == re and pd == rd)
    ok &= m
    print(f"{k:10s} ({pe:>7},{pd:>6})        ({re},{rd})   {'OK' if m else 'MISMATCH'}")
print('\n全部一致' if ok else '\n存在不一致！')
