// NVTX 区间封装（header-only nvtx3，无需链接 libnvToolsExt）。
// 供 Nsight Systems `--nvtx` 时间线按「系统 / 阶段」命名着色。Rust 经 FFI 调用。
#include <nvtx3/nvToolsExt.h>

extern "C" void water_nvtx_push(const char *name) { nvtxRangePushA(name); }
extern "C" void water_nvtx_pop(void) { nvtxRangePop(); }
