import { useEffect, useRef } from "react";
import { Eraser } from "lucide-react";

import { Button } from "@/components/ui/button";
import { useWaterStore } from "@/store/water-store";

/** 右下：详细日志（tracing 实时事件），自动滚动到底部。 */
export function WaterLogPanel() {
  const log = useWaterStore((s) => s.log);
  const clearLog = useWaterStore((s) => s.clearLog);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (ref.current) ref.current.scrollTop = ref.current.scrollHeight;
  }, [log]);

  return (
    <div className="flex h-full flex-col p-5">
      <div className="mb-2 flex items-center justify-between">
        <h2 className="text-base font-semibold">详细日志</h2>
        <Button variant="ghost" size="sm" onClick={clearLog}>
          <Eraser />
          清空
        </Button>
      </div>
      <div
        ref={ref}
        className="flex-1 overflow-auto whitespace-pre-wrap rounded-md border bg-muted/30 p-3 font-mono text-xs"
      >
        {log || "（暂无日志）"}
      </div>
    </div>
  );
}
