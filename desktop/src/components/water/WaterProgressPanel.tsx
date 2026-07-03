import { useEffect, useState } from "react";
import { CheckCircle2, Clock, ListChecks, Loader2 } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Progress } from "@/components/ui/progress";
import { useWaterStore, type TaskStatus } from "@/store/water-store";

type BadgeVariant = "default" | "secondary" | "destructive" | "outline" | "muted";

function statusMeta(status: TaskStatus): {
  value: number;
  text: string;
  indicator: string;
  badge: BadgeVariant;
} {
  switch (status) {
    case "failed":
      return { value: 100, text: "失败", indicator: "bg-destructive", badge: "destructive" };
    case "done":
      return { value: 100, text: "完成", indicator: "bg-primary", badge: "default" };
    case "running":
      return { value: 100, text: "运行中", indicator: "bg-primary animate-pulse", badge: "default" };
    default:
      return { value: 0, text: "等待中", indicator: "", badge: "muted" };
  }
}

/** 运行耗时显示：运行中实时计时，结束后定格。 */
function ElapsedBadge() {
  const running = useWaterStore((s) => s.running);
  const startedAt = useWaterStore((s) => s.startedAt);
  const elapsedMs = useWaterStore((s) => s.elapsedMs);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!running) return;
    const id = setInterval(() => setNow(Date.now()), 100);
    return () => clearInterval(id);
  }, [running]);

  const ms = running && startedAt ? now - startedAt : elapsedMs;
  if (ms == null) return null;
  return (
    <Badge variant={running ? "default" : "secondary"} className="ml-auto gap-1 tabular-nums">
      <Clock className="h-3 w-3" />
      {(ms / 1000).toFixed(1)}s
    </Badge>
  );
}

/** 右上：按选中任务分别显示进度条 + 结果保存位置。 */
export function WaterProgressPanel() {
  const running = useWaterStore((s) => s.running);
  const taskBars = useWaterStore((s) => s.taskBars);
  const outputs = useWaterStore((s) => s.outputs);

  return (
    <div className="flex h-full flex-col gap-4 overflow-y-auto p-5">
      <div className="flex items-center gap-2">
        <ListChecks className="h-4 w-4 text-muted-foreground" />
        <h2 className="text-sm font-semibold">任务进度</h2>
        {running && <Loader2 className="h-4 w-4 animate-spin text-primary" />}
        <ElapsedBadge />
      </div>

      {taskBars.length === 0 ? (
        <p className="rounded-md border border-dashed p-3 text-xs text-muted-foreground">
          勾选任务并点击「运行」后，将按任务分别显示进度
        </p>
      ) : (
        <div className="space-y-3">
          {taskBars.map((b) => {
            const m = statusMeta(b.status);
            return (
              <div key={b.key} className="space-y-1.5">
                <div className="flex items-center justify-between">
                  <span className="text-sm font-medium">{b.name}</span>
                  <Badge variant={m.badge}>{m.text}</Badge>
                </div>
                <Progress value={m.value} indicatorClassName={m.indicator} />
              </div>
            );
          })}
        </div>
      )}

      <div>
        {outputs.length === 0 ? (
          <p className="text-xs text-muted-foreground">结果保存位置将显示在此处</p>
        ) : (
          <div className="rounded-md border bg-muted/30 p-3">
            <div className="mb-2 flex items-center gap-2">
              <CheckCircle2 className="h-4 w-4 text-primary" />
              <span className="text-sm font-medium">结果已保存</span>
            </div>
            <div className="space-y-1.5">
              {outputs.map((o) => (
                <div key={o.task} className="flex items-baseline gap-2">
                  <Badge variant="secondary">{o.task}</Badge>
                  <span className="break-all font-mono text-xs">{o.path}</span>
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
