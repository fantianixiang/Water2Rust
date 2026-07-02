// Tauri 后端命令、事件与原生对话框的类型化封装。
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";

/** 某 fclass 的 edge/depth 调参规格（`*Max === null` 表示 ∞）。 */
export interface EdgeGuidanceItem {
  fclass: string;
  edge: number;
  depth: number;
  edgeMin: number;
  edgeMax: number | null;
  depthMin: number;
  depthMax: number | null;
}

export interface EdgeOverride {
  fclass: string;
  edge: number;
  depth: number;
}

export interface PipelineParams {
  waterPath: string;
  outputPath: string;
  demPath: string | null;
  referencePath: string;
  doFclass: boolean;
  doEdge: boolean;
  doHydro: boolean;
  hydroWithDem: boolean;
  edgeOverrides: EdgeOverride[];
}

export interface TaskOutput {
  task: string;
  path: string;
}

/** 取 8 个 fclass 的 edge/depth 默认值与推荐范围。 */
export function getEdgeGuidance(): Promise<EdgeGuidanceItem[]> {
  return invoke<EdgeGuidanceItem[]>("get_edge_guidance");
}

/** 提交流水线（后台执行，进度经事件回传）。 */
export function runPipeline(params: PipelineParams): Promise<void> {
  return invoke("run_pipeline", { params });
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
