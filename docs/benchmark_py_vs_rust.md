# 水体流水线基线对比：Python vs Rust（fclass / edge / hydro）

> 记录日期：2026-07-03  
> 背景：`hydro` 水面 DEM 求解器由无重排序的 `nalgebra-sparse CscCholesky` 换成 **faer AMD 稀疏 Cholesky**
> （修复大水域填充失控 / 内存爆炸，见 [HYDRO.md](HYDRO.md) 阶段 1，commit `c7991fd`）。
> 本文记录修复后 Rust 实现与原 Python 基线在同数据上的耗时对比。

## 1. 环境

| 项 | Python 基线 | Rust |
| --- | --- | --- |
| 代码 | `MyProject`（原始实现），入口 `modules.waters.gui_schedule.run_gui_pipeline` | `Water2Rust` `rust/shadcn` @ `24dd3ce` |
| 运行方式 | conda `myproject_py310`（Python 3.10.20，GDAL 3.13.1，scipy spsolve） | `target/release/water2rust.exe`（release 优化） |
| Laplace 求解 | `scipy.sparse.linalg.spsolve`（SuperLU + COLAMD 重排序） | faer 0.24 稀疏 Cholesky（AMD 重排序，`Par::Seq`） |
| 打包产物 | `dist/data_tool_v0.0.1.9/data_tool.exe`（PyInstaller onedir，含计时日志） | — |

计时方式：Python 用 `time.perf_counter()` 记录各阶段内部耗时（[gui_schedule.py](file:///E:/Projects/MyProject/modules/waters/gui_schedule.py) 新增）；
Rust 用 PowerShell `Measure-Command` 计每个 CLI 子命令的 wall time。同机顺序执行，无并发争用。

## 2. 数据

| 数据集 | DEM 尺寸 | 像素数 | 水体要素 | 说明 |
| --- | --- | --- | --- | --- |
| `data/linzhi_clip` | 3036 × 3076 | ~9.3 M | 6（大河#36 1711×977 / 中#5 / 小#7 + 3） | 裁剪的「大中小」测试集 |
| `data/linzhi`（全量） | 26492 × 18138 | ~480 M | 41（lake 2 / river 32 / water 7） | 林芝全量，含巨型水体 |

DEM 源 CRS EPSG:4326，工作 CRS 本地 UTM（EPSG:32646）。

## 3. 结果

### 3.1 裁剪数据（大中小，全分辨率 · 单窗口 · 单线程 · 两侧同条件）

| 阶段 | Python | Rust |
| --- | --- | --- |
| fclass | 0.1 s | 2.0 s |
| edge | 0.0 s | 0.0 s |
| hydro | 3.5 s | **2.9 s** |
| 合计（内部计时） | 3.6 s | — |
| 合计（wall） | 4.8 s | 4.9 s |

条件一致（都是全分辨率单窗口单线程）时，**hydro 求解 Rust 略快**（2.9 vs 3.5 s）。

### 3.2 全量数据

| 阶段 | Python | Rust（单线程） | Rust（4 线程·当前默认） |
| --- | --- | --- | --- |
| fclass | 0.4 s | 0.24 s | 0.24 s |
| edge | 0.0 s | 0.0 s | 0.0 s |
| hydro | 53.8 s | 104.7 s | **48.5 s** |
| 合计 | 54.4 s | 104.9 s | **48.7 s** |

> 📌 **fclass 更新（2026-07-03）**：全量 fclass 由 7.1 s 优化到 **0.24 s（约 30×）**（commit `68c81cb`，
> 改为 CPU 侧惰性 / 空间过滤读取参考库）。上表已反映；fclass 非多线程，故单/4 线程列相同。
> 裁剪数据（§3.1）的旧值 2.0 s 为优化前测得，post-opt 未单独复测，暂保留原值。

- hydro 瓦片改 **4 线程 rayon 并行**（保持全分辨率）后：**104.7 s → 48.5 s（2.16×）**，输出 MD5 与单线程**逐位一致**（精度不变）。
- 未达满 4× 加速：全量仅 6 个含水瓦片（跨 4 线程负载不均），且 with_dem 整幅 DEM 读/写与解码 I/O 为串行瓶颈。

> ⚠️ Python 与 Rust 全量 hydro 的**分辨率**仍不同（见下节），但 **Rust 4 线程全分辨率 48.5 s 已快于 Python 半分辨率 4 worker 的 53.8 s**。

## 4. 关键差异与解读

全量 hydro 的默认策略两侧不同，导致 wall time 不可直接对比：

| 维度 | Python 全量 hydro | Rust 全量 hydro |
| --- | --- | --- |
| 超预算处理 | ROI > 2.5亿像素 → **降采样 factor=2**（工作网格 9069×13246，约 1/4 像素） | **全分辨率**瓦片（不降采样） |
| 并行 | **4 个瓦片 worker**（multiprocessing） | 瓦片循环 **4 线程 rayon**（默认，faer 求解内部 Par::Seq） |
| 输出分辨率 | 半分辨率 | 全分辨率 |

- Python 的 53.8 s 是在**约 1/4 像素量 + 4 核并行**下取得；输出为半分辨率。
- Rust 现为 **4 线程·全分辨率 48.5 s**（单线程 104.7 s → 2.16×）：即使保持全分辨率，仍快于 Python 的半分辨率结果。
- 因此「wall time」已不再误导：同为 4 核时，Rust 全分辨率反而更快。裁剪数据（同条件）也显示 Rust hydro 求解更快。

其它观察：
- **fclass（已优化，2026-07-03 更新）**：早期 Rust fclass 每次**急加载整个参考库** `waters_china.gpkg`（rusqlite 读全图层 + rstar 建索引），全量 7.1 s，慢于 Python 0.4 s。commit `68c81cb` 改为 **CPU 侧惰性 / 空间过滤读取**后，全量降至 **0.24 s（约 30×）**，已快于 Python。（裁剪数据旧值 2.0 s 为优化前测得。）
- **hydro 内存**：修复前 Rust（无重排序 Cholesky）全量 30 min 未完成、内存 3→4.7 GB 猛涨；修复后全量 105 s 完成、内存平稳 ~3.4 GB。这是本次修复的核心收益（正确性经 `laplace_parity` 等对拍逐位一致）。

## 5. 结论与后续优化

- **核心目标达成**：faer 修复使 Rust hydro 从「大水域算爆 / 不可完成」变为可稳定完成；同条件下求解更快、内存可控。
- **公平对比结论**（4 线程后）：**Rust 4 线程全分辨率 hydro 48.5 s < Python 半分辨率 4 worker 53.8 s**，即保持更高输出质量的同时仍更快。
- **Rust 后续可选优化**：
  1. ~~hydro 瓦片 rayon 并行~~ ✅ 已完成（4 线程，104.7 s → 48.5 s，输出逐位一致）；
  2. 超预算时提供**可选降采样**开关（对齐 Python 默认，或保留全分辨率为高质量模式）；
  3. ~~**fclass 惰性 / 空间过滤读取**参考库，消除急加载开销~~ ✅ 已完成（commit `68c81cb`，全量 7.1 s → 0.24 s）。

## 附：复现命令

```powershell
# Rust（release）
$exe="target\release\water2rust.exe"; $d="data\linzhi"; $ref="...\waters_china.gpkg"
Measure-Command { & $exe fclass     --water "$d\waters.shp"        --output "$d\waters_fclass.shp" --reference-path $ref }
Measure-Command { & $exe edge-depth --water "$d\waters_fclass.shp" --output "$d\waters_edge.shp" }
Measure-Command { & $exe hydro      --dem "$d\dem.tif" --water "$d\waters_fclass.shp" --output "$d\waters_hydro.tif" }

# Python 基线（conda myproject_py310）：调用 run_gui_pipeline(selected_tasks=["fclass","edge","hydro"], ...)
# 各阶段耗时见日志 "waters task done: <stage> (耗时 X.Xs)" 与 "waters pipeline finished (总耗时 X.Xs)"
# 注意：Windows 多进程需 if __name__ == "__main__": 守卫。
```
