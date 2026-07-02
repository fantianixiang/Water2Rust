import { Loader2 } from "lucide-react";

import { Progress } from "@/components/ui/progress";
import { useWaterStore, type TaskStatus } from "@/store/water-store";

function statusMeta(status: TaskStatus) {
  switch (status) {
    case "failed":
      return { value: 100, text: "失败", indicator: "bg-destructive", cls: "text-destructive" };
    case "done":
      return { value: 100, text: "完成", indicator: "bg-primary", cls: "text-primary" };
    case "running":
      return { value: 100, text: "运行中…", indicator: "bg-primary animate-pulse", cls: "text-primary" };
    default:
      return { value: 0, text: "等待中", indicator: "", cls: "text-muted-foreground" };
  }
}

/** 右上：按选中任务分别显示进度条 + 结果保存位置。 */
export function WaterProgressPanel() {
  const running = useWaterStore((s) => s.running);
  const taskBars = useWaterStore((s) => s.taskBars);
  const outputs = useWaterStore((s) => s.outputs);

  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto p-5">
      <div className="flex items-center gap-2">
        <h2 className="text-base font-semibold">任务进度</h2>
        {running && <Loader2 className="h-4 w-4 animate-spin text-primary" />}
      </div>

      {taskBars.length === 0 ? (
        <p className="text-xs text-muted-foreground">
          勾选任务并点击「运行」后，将按任务分别显示进度
        </p>
      ) : (
        <div className="space-y-3">
          {taskBars.map((b) => {
            const m = statusMeta(b.status);
            return (
              <div key={b.key} className="space-y-1">
                <div className="flex items-center justify-between text-sm">
                  <span className="font-medium">{b.name}</span>
                  <span className={`text-xs ${m.cls}`}>{m.text}</span>
                </div>
                <Progress value={m.value} indicatorClassName={m.indicator} />
              </div>
            );
          })}
        </div>
      )}

      <div className="mt-1">
        {outputs.length === 0 ? (
          <p className="text-xs text-muted-foreground">结果保存位置将显示在此处</p>
        ) : (
          <div className="space-y-1">
            <p className="text-sm font-medium">结果已保存：</p>
            {outputs.map((o) => (
              <div key={o.task} className="flex items-baseline gap-2">
                <span className="text-sm font-semibold text-primary">[{o.task}]</span>
                <span className="break-all font-mono text-xs">{o.path}</span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
