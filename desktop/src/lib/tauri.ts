// Tauri 后端命令、事件与原生对话框的类型化封装。
// 共享数据类型由 Rust 结构体经 ts-rs 生成（见 ./bindings/），此处 re-export。
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";

import type { EdgeGuidanceItem } from "./bindings/EdgeGuidanceItem";
import type { PipelineParams } from "./bindings/PipelineParams";
import type { TaskOutput } from "./bindings/TaskOutput";

export type { EdgeGuidanceItem } from "./bindings/EdgeGuidanceItem";
export type { EdgeOverride } from "./bindings/EdgeOverride";
export type { PipelineParams } from "./bindings/PipelineParams";
export type { TaskOutput } from "./bindings/TaskOutput";

/** 取 8 个 fclass 的 edge/depth 默认值与推荐范围。 */
export function getEdgeGuidance(): Promise<EdgeGuidanceItem[]> {
  return invoke<EdgeGuidanceItem[]>("get_edge_guidance");
}

/** 提交流水线（后台执行，进度经事件回传）。 */
export function runPipeline(params: PipelineParams): Promise<void> {
  return invoke("run_pipeline", { params });
}

/** 后端是否编译进了 GPU（CUDA）支持。 */
export function gpuAvailable(): Promise<boolean> {
  return invoke<boolean>("gpu_available");
}

// ── 事件订阅 ─────────────────────────────────────────────
export function onLog(cb: (line: string) => void): Promise<UnlistenFn> {
  return listen<string>("pipeline://log", (e) => cb(e.payload));
}
export function onTaskStart(cb: (task: string) => void): Promise<UnlistenFn> {
  return listen<string>("pipeline://task-start", (e) => cb(e.payload));
}
export function onTaskDone(cb: (task: string) => void): Promise<UnlistenFn> {
  return listen<string>("pipeline://task-done", (e) => cb(e.payload));
}
export function onDone(cb: (outputs: TaskOutput[]) => void): Promise<UnlistenFn> {
  return listen<TaskOutput[]>("pipeline://done", (e) => cb(e.payload));
}
export function onFailed(cb: (msg: string) => void): Promise<UnlistenFn> {
  return listen<string>("pipeline://failed", (e) => cb(e.payload));
}

// ── 原生文件对话框 ───────────────────────────────────────
export async function pickFile(
  name: string,
  extensions: string[]
): Promise<string | null> {
  const res = await open({ multiple: false, filters: [{ name, extensions }] });
  return typeof res === "string" ? res : null;
}

export async function pickSave(
  defaultName: string,
  name: string,
  extensions: string[]
): Promise<string | null> {
  const res = await save({
    defaultPath: defaultName,
    filters: [{ name, extensions }],
  });
  return res ?? null;
}
