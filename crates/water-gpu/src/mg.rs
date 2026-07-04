//! 聚合多重网格（unsmoothed aggregation）层次结构构建 + MG 预条件 PCG 桥接。
//!
//! 主机侧用 2×2 网格聚合逐层粗化，构建每层加权紧凑 5 点 **Galerkin** 算子
//! `A_{l+1} = P^T A_l P`（`P` = 分片常数注入延拓，`R = P^T` 限制）。因 2×2 方形聚合下
//! 粗图仍是 ≤4 度网格，故各层算子保持紧凑 5 点（带权）形式，可复用 GPU SpMV/Jacobi。
//! 聚合天然贴合拓扑（对细长河比节点几何粗化更稳健），Galerkin+对称光滑保证预条件 SPD、
//! CG 有效、与 faer 保真。层次结构展平后交 [`crate::laplace_pcg_mg`] 上传运行。
//! CUDA 侧见 [cuda/laplace_mg.cu](../cuda/laplace_mg.cu)。

use crate::{GpuError, KernelTiming, PcgResult, Result};

extern "C" {
    /// CUDA 侧 `water_laplace_pcg_mg`（cuda/laplace_mg.cu）：MG 预条件 matrix-free FP64 PCG。
    #[allow(clippy::too_many_arguments)]
    fn water_laplace_pcg_mg(
        n_levels: i32,
        level_n: *const i32,
        diag_all: *const f64,
        nbr_all: *const i32,
        wgt_all: *const f64,
        agg_all: *const i32,
        child_all: *const i32,
        h_b: *const f64,
        h_z: *mut f64,
        rtol: f64,
        max_iter: i32,
        mg_pre: i32,
        mg_post: i32,
        mg_coarse: i32,
        mg_omega: f64,
        out_iters: *mut i32,
        out_res: *mut f64,
        timing: *mut KernelTiming,
    ) -> i32;
}

/// 单层（主机侧）：加权紧凑 5 点算子 + 几何坐标 + 到上/下层的聚合映射。
struct Level {
    n: usize,
    diag: Vec<f64>,  // n
    nbr: Vec<i32>,   // 4n（邻居变量下标，-1 无）
    wgt: Vec<f64>,   // 4n（对应边权，0 无）
    rows: Vec<i32>,  // n（本层变量的粗网格行，用于下一层聚合）
    cols: Vec<i32>,  // n（列）
    agg: Vec<i32>,   // n（→ 下一层聚合下标；最粗层为空）
    child: Vec<i32>, // 4n（← 上一层子变量下标，-1 补齐；最细层为空）
}

/// 停止粗化阈值：变量数 ≤ 此值即作最粗层（多遍 Jacobi 近似求解）。
const COARSE_N: usize = 64;
/// 最大层数（防御性上限）。
const MAX_LEVELS: usize = 24;

/// 由最细层 `(diag, nbr, rows, cols)`（单位权 5 点 Dirichlet Laplacian）构建聚合多重网格层次。
fn build_hierarchy(diag0: &[f64], nbr0: &[i32], rows0: &[i32], cols0: &[i32]) -> Vec<Level> {
    let n0 = diag0.len();
    // 最细层：边权 = 1.0（有内部邻居处），否则 0。
    let wgt0: Vec<f64> = nbr0.iter().map(|&j| if j >= 0 { 1.0 } else { 0.0 }).collect();
    let mut levels: Vec<Level> = vec![Level {
        n: n0,
        diag: diag0.to_vec(),
        nbr: nbr0.to_vec(),
        wgt: wgt0,
        rows: rows0.to_vec(),
        cols: cols0.to_vec(),
        agg: Vec::new(),
        child: Vec::new(),
    }];

    while levels.last().unwrap().n > COARSE_N && levels.len() < MAX_LEVELS {
        let coarse = coarsen(levels.last().unwrap());
        // 无进展（无法再粗化）则停止，避免死循环。
        if coarse.child_of_next.len() / 4 >= levels.last().unwrap().n {
            break;
        }
        // 把 agg / child 回填到相邻两层。
        let fine = levels.last_mut().unwrap();
        fine.agg = coarse.agg;
        let mut next = coarse.next;
        next.child = coarse.child_of_next;
        levels.push(next);
    }
    levels
}

/// 一次粗化的产物：细层的 `agg`、粗层本身、以及粗层的 `child`（← 细层）。
struct Coarsen {
    agg: Vec<i32>,
    next: Level,
    child_of_next: Vec<i32>,
}

/// 对 `fine` 做一次 2×2 聚合粗化，返回 Galerkin 粗层算子（加权紧凑 5 点）。
/// 用稠密块索引表 + 直接粗邻居槽累加（粗图 ≤4 度），避免 HashMap（SipHash）开销。
fn coarsen(fine: &Level) -> Coarsen {
    let nf = fine.n;
    // ① 2×2 聚合：块坐标 (row/2, col/2) → 粗变量下标（首见即分配）。
    //    用稠密块表（粗 bbox = 细 bbox/4）代替 HashMap，O(1) 查表。
    let max_br = fine.rows.iter().map(|&r| r / 2).max().unwrap_or(0);
    let max_bc = fine.cols.iter().map(|&c| c / 2).max().unwrap_or(0);
    let bw = (max_bc as usize) + 1;
    let bh = (max_br as usize) + 1;
    let mut block_map = vec![-1i32; bh * bw];
    let mut agg = vec![-1i32; nf];
    let mut crows: Vec<i32> = Vec::new();
    let mut ccols: Vec<i32> = Vec::new();
    for i in 0..nf {
        let br = fine.rows[i] / 2;
        let bc = fine.cols[i] / 2;
        let key = br as usize * bw + bc as usize;
        let mut idx = block_map[key];
        if idx < 0 {
            idx = crows.len() as i32;
            block_map[key] = idx;
            crows.push(br);
            ccols.push(bc);
        }
        agg[i] = idx;
    }
    let nc = crows.len();

    // ② 子表 child[J*4+k]（← 细层）+ Galerkin 对角 diag_c[J] = Σ_{i∈J} diag[i]。
    let mut child = vec![-1i32; nc * 4];
    let mut child_cnt = vec![0u8; nc];
    let mut diag_c = vec![0.0f64; nc];
    for i in 0..nf {
        let j = agg[i] as usize;
        diag_c[j] += fine.diag[i];
        let s = child_cnt[j] as usize;
        debug_assert!(s < 4, "2×2 聚合每粗变量至多 4 子");
        child[j * 4 + s] = i as i32;
        child_cnt[j] = (s + 1) as u8;
    }

    // ③ 直接累加到粗邻居槽（≤4 度网格），免全局边表；无向边只处理一次（i<j）。
    //    diag_c[J] -= 2·内部边权；粗边权 = Σ 跨界细边权。
    let mut nbr_c = vec![-1i32; nc * 4];
    let mut wgt_c = vec![0.0f64; nc * 4];
    let add_edge = |a: usize, b: i32, w: f64, nbr_c: &mut Vec<i32>, wgt_c: &mut Vec<f64>| {
        let base = a * 4;
        for s in 0..4 {
            if nbr_c[base + s] == b {
                wgt_c[base + s] += w;
                return;
            }
            if nbr_c[base + s] < 0 {
                nbr_c[base + s] = b;
                wgt_c[base + s] = w;
                return;
            }
        }
        debug_assert!(false, "2×2 聚合粗图应为 ≤4 度网格");
    };
    for i in 0..nf {
        let capj = agg[i];
        for k in 0..4 {
            let j = fine.nbr[i * 4 + k];
            if j < 0 || (j as usize) <= i {
                continue; // 仅 i<j 一次
            }
            let w = fine.wgt[i * 4 + k];
            let capk = agg[j as usize];
            if capj == capk {
                diag_c[capj as usize] -= 2.0 * w; // 内部边
            } else {
                add_edge(capj as usize, capk, w, &mut nbr_c, &mut wgt_c);
                add_edge(capk as usize, capj, w, &mut nbr_c, &mut wgt_c);
            }
        }
    }

    let next = Level {
        n: nc,
        diag: diag_c,
        nbr: nbr_c,
        wgt: wgt_c,
        rows: crows,
        cols: ccols,
        agg: Vec::new(),
        child: Vec::new(),
    };
    Coarsen {
        agg,
        next,
        child_of_next: child,
    }
}

/// MG 预条件 matrix-free FP64 PCG 求解 5 点 Dirichlet Laplacian（紧凑变量 + 聚合多重网格）。
///
/// 输入最细层为 [`crate::laplace_pcg_compact`] 同款紧凑系统，另需每变量的像素坐标
/// `rows`/`cols`（供 2×2 聚合建层次）：
/// - `diag`/`nbr`/`b`：长度 n / 4n / n 的加权紧凑 5 点系统（单位权，`nbr[i*4+k]` = -1 表非内部）；
/// - `rows`/`cols`：长度 n，各变量的网格行/列坐标。
/// - `pre`/`post`/`coarse`/`omega`：V-cycle 前/后光滑次数、最粗层光滑次数、阻尼 Jacobi 因子。
///
/// 返回解 `z`（变量序）+ 迭代/残差/耗时。层次结构在主机构建、GPU 上跑 V-cycle 预条件 CG。
#[allow(clippy::too_many_arguments)]
pub fn laplace_pcg_mg(
    diag: &[f64],
    nbr: &[i32],
    b: &[f64],
    rows: &[i32],
    cols: &[i32],
    rtol: f64,
    max_iter: i32,
    pre: i32,
    post: i32,
    coarse: i32,
    omega: f64,
) -> Result<PcgResult> {
    let n = diag.len();
    if b.len() != n || nbr.len() != n * 4 || rows.len() != n || cols.len() != n {
        return Err(GpuError::InvalidInput(format!(
            "diag/b/rows/cols 长度应为 n={n}，nbr 为 4n={}；实为 b={}, nbr={}, rows={}, cols={}",
            n * 4,
            b.len(),
            nbr.len(),
            rows.len(),
            cols.len()
        )));
    }
    let mut z = vec![0.0f64; n];
    let mut iters: i32 = 0;
    let mut residual: f64 = 0.0;
    let mut timing = KernelTiming::default();
    if n == 0 {
        return Ok(PcgResult {
            z,
            iters,
            residual,
            timing,
        });
    }

    // 主机构建层次结构并展平（前缀和段偏移由 CUDA 侧据 level_n 计算）。
    let levels = build_hierarchy(diag, nbr, rows, cols);
    let l = levels.len();
    let level_n: Vec<i32> = levels.iter().map(|lv| lv.n as i32).collect();
    let mut diag_all: Vec<f64> = Vec::new();
    let mut nbr_all: Vec<i32> = Vec::new();
    let mut wgt_all: Vec<f64> = Vec::new();
    let mut agg_all: Vec<i32> = Vec::new();
    let mut child_all: Vec<i32> = Vec::new();
    for (li, lv) in levels.iter().enumerate() {
        diag_all.extend_from_slice(&lv.diag);
        nbr_all.extend_from_slice(&lv.nbr);
        wgt_all.extend_from_slice(&lv.wgt);
        // agg 段（最粗层无 → 补零占位）。
        if li < l - 1 {
            agg_all.extend_from_slice(&lv.agg);
        } else {
            agg_all.extend(std::iter::repeat(0i32).take(lv.n));
        }
        // child 段（最细层无 → 补 -1 占位）。
        if li >= 1 {
            child_all.extend_from_slice(&lv.child);
        } else {
            child_all.extend(std::iter::repeat(-1i32).take(lv.n * 4));
        }
    }

    let code = unsafe {
        water_laplace_pcg_mg(
            l as i32,
            level_n.as_ptr(),
            diag_all.as_ptr(),
            nbr_all.as_ptr(),
            wgt_all.as_ptr(),
            agg_all.as_ptr(),
            child_all.as_ptr(),
            b.as_ptr(),
            z.as_mut_ptr(),
            rtol,
            max_iter,
            pre,
            post,
            coarse,
            omega,
            &mut iters,
            &mut residual,
            &mut timing,
        )
    };
    if code != 0 {
        return Err(GpuError::Cuda(code));
    }
    Ok(PcgResult {
        z,
        iters,
        residual,
        timing,
    })
}
