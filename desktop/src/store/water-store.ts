import { create } from "zustand";

import type { EdgeGuidanceItem, TaskOutput } from "@/lib/tauri";

/** 默认分类参考库（用户指定，可在界面替换）。 */
export const DEFAULT_REFERENCE =
  "E:\\Projects\\MyProject\\global_datas\\waters_china.gpkg";

export type TaskStatus = "pending" | "running" | "done" | "failed";

export interface TaskBar {
  key: string;
  name: string;
  status: TaskStatus;
}

/** 单个 fclass 的可编辑 edge/depth 参数 + 推荐范围。 */
export interface EdgeParam extends EdgeGuidanceItem {}

interface WaterState {
  waterPath: string;
  outputPath: string;
  demPath: string;
  referencePath: string;

  doFclass: boolean;
  doEdge: boolean;
  doHydro: boolean;
  hydroWithDem: boolean;

  running: boolean;
  startedAt: number | null;
  elapsedMs: number | null;
  taskBars: TaskBar[];
  log: string;
  outputs: TaskOutput[];

  edgeParams: EdgeParam[];
  showEdgeSettings: boolean;

  set: (patch: Partial<WaterState>) => void;
  setEdgeParam: (fclass: string, edge: number, depth: number) => void;
  loadGuidance: (items: EdgeGuidanceItem[]) => void;
  appendLog: (line: string) => void;
  clearLog: () => void;
  beginRun: () => void;
  markTask: (key: string, status: TaskStatus) => void;
  finish: (outputs: TaskOutput[]) => void;
  fail: (msg: string) => void;
}

const TASK_NAMES: Record<string, string> = {
  fclass: "水域分类 (fclass)",
  edge: "边缘深度 (edge)",
  hydro: "水文DEM (hydro)",
};

export const useWaterStore = create<WaterState>((set, get) => ({
  waterPath: "",
  outputPath: "",
  demPath: "",
  referencePath: DEFAULT_REFERENCE,

  doFclass: true,
  doEdge: false,
  doHydro: false,
  hydroWithDem: false,

  running: false,
  startedAt: null,
  elapsedMs: null,
  taskBars: [],
  log: "",
  outputs: [],

  edgeParams: [],
  showEdgeSettings: false,

  set: (patch) => set(patch),

  setEdgeParam: (fclass, edge, depth) =>
    set((s) => ({
      edgeParams: s.edgeParams.map((p) =>
        p.fclass === fclass ? { ...p, edge, depth } : p
      ),
    })),

  loadGuidance: (items) => set({ edgeParams: items.map((i) => ({ ...i })) }),

  appendLog: (line) => set((s) => ({ log: s.log + line })),
  clearLog: () => set({ log: "" }),

  beginRun: () => {
    const s = get();
    const needFclass = s.doFclass || s.doEdge || s.doHydro;
    const bars: TaskBar[] = [];
    if (needFclass)
      bars.push({ key: "fclass", name: TASK_NAMES.fclass, status: "pending" });
    if (s.doEdge)
      bars.push({ key: "edge", name: TASK_NAMES.edge, status: "pending" });
    if (s.doHydro)
      bars.push({ key: "hydro", name: TASK_NAMES.hydro, status: "pending" });
    set({
      running: true,
      startedAt: Date.now(),
      elapsedMs: null,
      taskBars: bars,
      outputs: [],
      log: s.log + "\n───────── 开始运行 ─────────\n",
    });
  },

  markTask: (key, status) =>
    set((s) => ({
      taskBars: s.taskBars.map((b) =>
        b.key === key ? { ...b, status } : b
      ),
    })),

  finish: (outputs) =>
    set((s) => {
      const elapsedMs = s.startedAt ? Date.now() - s.startedAt : null;
      const secs = elapsedMs != null ? (elapsedMs / 1000).toFixed(1) : "?";
      return {
        running: false,
        elapsedMs,
        outputs,
        log: s.log + `\n[完成] 运行成功，耗时 ${secs}s。\n`,
      };
    }),

  fail: (msg) =>
    set((s) => {
      const elapsedMs = s.startedAt ? Date.now() - s.startedAt : null;
      const secs = elapsedMs != null ? (elapsedMs / 1000).toFixed(1) : "?";
      return {
        running: false,
        elapsedMs,
        taskBars: s.taskBars.map((b) =>
          b.status === "running" ? { ...b, status: "failed" as const } : b
        ),
        log: s.log + `\n[失败] ${msg}（耗时 ${secs}s）\n`,
      };
    }),
}));
