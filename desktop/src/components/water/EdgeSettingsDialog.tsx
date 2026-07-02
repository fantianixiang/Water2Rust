import { Fragment } from "react";
import { RotateCcw } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { getEdgeGuidance } from "@/lib/tauri";
import { useWaterStore } from "@/store/water-store";

function rangeText(min: number, max: number | null): string {
  return `推荐 ${min} ~ ${max === null ? "∞" : max}`;
}

/** edge 参数设置子窗口：为每个 fclass 调 edgeexpand / depth。 */
export function EdgeSettingsDialog() {
  const showEdgeSettings = useWaterStore((s) => s.showEdgeSettings);
  const edgeParams = useWaterStore((s) => s.edgeParams);
  const set = useWaterStore((s) => s.set);
  const setEdgeParam = useWaterStore((s) => s.setEdgeParam);
  const loadGuidance = useWaterStore((s) => s.loadGuidance);

  const reset = async () => loadGuidance(await getEdgeGuidance());

  return (
    <Dialog
      open={showEdgeSettings}
      onOpenChange={(open) => set({ showEdgeSettings: open })}
    >
      <DialogContent className="max-w-xl">
        <DialogHeader>
          <DialogTitle>边缘深度 (edge) 参数设置</DialogTitle>
          <DialogDescription>
            为每个水体类别设置 edgeexpand（边缘外扩，米）与 depth（深度，米）。
            括号内为推荐范围，仅作提示、可超出。
          </DialogDescription>
        </DialogHeader>

        <div className="grid grid-cols-[5rem_1fr_1fr] items-center gap-x-4 gap-y-3">
          <div className="text-sm font-semibold">类别</div>
          <div className="text-sm font-semibold">edgeexpand</div>
          <div className="text-sm font-semibold">depth</div>

          {edgeParams.map((p) => (
            <Fragment key={p.fclass}>
              <div className="text-sm font-medium text-primary">{p.fclass}</div>
              <div>
                <Input
                  type="number"
                  step="0.1"
                  value={p.edge}
                  onChange={(e) =>
                    setEdgeParam(p.fclass, Number(e.target.value), p.depth)
                  }
                />
                <div className="mt-0.5 text-[10px] text-muted-foreground">
                  {rangeText(p.edgeMin, p.edgeMax)}
                </div>
              </div>
              <div>
                <Input
                  type="number"
                  step="0.1"
                  value={p.depth}
                  onChange={(e) =>
                    setEdgeParam(p.fclass, p.edge, Number(e.target.value))
                  }
                />
                <div className="mt-0.5 text-[10px] text-muted-foreground">
                  {rangeText(p.depthMin, p.depthMax)}
                </div>
              </div>
            </Fragment>
          ))}
        </div>

        <div className="flex items-center gap-3">
          <Button variant="outline" size="sm" onClick={reset}>
            <RotateCcw />
            恢复默认
          </Button>
          <span className="text-xs text-muted-foreground">
            修改即时生效，运行 edge 时采用当前值
          </span>
        </div>
      </DialogContent>
    </Dialog>
  );
}
