import { useEffect } from "react";
import { useMutation } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { Droplets, Moon, Sun, Waves } from "lucide-react";

import { Button } from "@/components/ui/button";
import { useAppStore } from "@/store/app-store";

/**
 * Water2Rust 桌面前端骨架（Tauri 2 + React 19 + Tailwind + shadcn）。
 *
 * 当前仅验证技术栈打通：调用后端 `greet` 命令、shadcn 按钮、lucide 图标、
 * Zustand 暗色切换、TanStack Query 异步态。water 业务页在后续迁移。
 */
function App() {
  const { dark, toggleDark } = useAppStore();

  useEffect(() => {
    document.documentElement.classList.toggle("dark", dark);
  }, [dark]);

  const ping = useMutation({
    mutationFn: async () => {
      // 仅在 Tauri 运行时可用；纯浏览器预览会抛错，由 onError 兜底。
      return invoke<string>("greet", { name: "Water2Rust" });
    },
  });

  return (
    <div className="min-h-screen bg-background text-foreground">
      <header className="flex items-center justify-between border-b px-6 py-3">
        <div className="flex items-center gap-2">
          <Waves className="text-primary" />
          <span className="text-lg font-bold">Water2Rust</span>
          <span className="text-sm text-muted-foreground">水体处理工具链</span>
        </div>
        <Button variant="ghost" size="icon" onClick={toggleDark} title="切换暗色">
          {dark ? <Sun /> : <Moon />}
        </Button>
      </header>

      <main className="mx-auto max-w-2xl px-6 py-10">
        <div className="rounded-lg border bg-card p-6 shadow-sm">
          <div className="mb-4 flex items-center gap-2">
            <Droplets className="text-primary" />
            <h2 className="text-base font-semibold">技术栈自检</h2>
          </div>
          <p className="mb-4 text-sm text-muted-foreground">
            点击下方按钮调用 Tauri 后端命令，验证前端 ↔ Rust 后端链路。
          </p>
          <Button onClick={() => ping.mutate()} disabled={ping.isPending}>
            {ping.isPending ? "调用中…" : "调用后端 greet"}
          </Button>
          <div className="mt-4 text-sm">
            {ping.isSuccess && (
              <span className="text-primary">后端返回：{ping.data}</span>
            )}
            {ping.isError && (
              <span className="text-destructive">
                调用失败（需在 Tauri 运行时下）：{String(ping.error)}
              </span>
            )}
          </div>
        </div>

        <p className="mt-6 text-xs text-muted-foreground">
          栈：React 19 · Vite 6 · Tauri 2 · Tailwind 3.4 · shadcn/ui · Zustand 5 ·
          TanStack Query v5 · lucide-react。water 业务页迁移中。
        </p>
      </main>
    </div>
  );
}

export default App;
