import { create } from "zustand";

/** 全局 UI 状态（Zustand）。后续 water 页的表单/任务状态在此扩展。 */
interface AppState {
  /** 暗色模式开关。 */
  dark: boolean;
  toggleDark: () => void;
}

export const useAppStore = create<AppState>((set) => ({
  dark: false,
  toggleDark: () => set((s) => ({ dark: !s.dark })),
}));
