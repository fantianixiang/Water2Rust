import { useEffect } from "react";
import { Panel, PanelGroup, PanelResizeHandle } from "react-resizable-panels";

import {
  getEdgeGuidance,
  onDone,
  onFailed,
  onLog,
  onTaskDone,
  onTaskStart,
} from "@/lib/tauri";
import { useWaterStore } from "@/store/water-store";
import { EdgeSettingsDialog } from "./EdgeSettingsDialog";
import { WaterLogPanel } from "./WaterLogPanel";
import { WaterParamsPanel } from "./WaterParamsPanel";
import { WaterProgressPanel } from "./WaterProgressPanel";

/** water 成品页：左参数 / 右（上进度 + 下日志），可调面板；订阅后端事件。 */
export function WaterPage() {
  useEffect(() => {
    getEdgeGuidance()
      .then((items) => useWaterStore.getState().loadGuidance(items))
      .catch(() => {});

    const subs = [
      onLog((l) => useWaterStore.getState().appendLog(l)),
      onTaskStart((t) => useWaterStore.getState().markTask(t, "running")),
      onTaskDone((t) => useWaterStore.getState().markTask(t, "done")),
      onDone((o) => useWaterStore.getState().finish(o)),
      onFailed((m) => useWaterStore.getState().fail(m)),
    ];
    return () => {
      subs.forEach((p) => p.then((un) => un()).catch(() => {}));
    };
  }, []);

  return (
    <>
      <PanelGroup direction="horizontal" className="h-full">
        <Panel defaultSize={40} minSize={30}>
          <WaterParamsPanel />
        </Panel>
        <PanelResizeHandle className="w-px bg-border transition-colors hover:bg-primary/40" />
        <Panel minSize={30}>
          <PanelGroup direction="vertical">
            <Panel defaultSize={45} minSize={20}>
              <WaterProgressPanel />
            </Panel>
            <PanelResizeHandle className="h-px bg-border transition-colors hover:bg-primary/40" />
            <Panel minSize={20}>
              <WaterLogPanel />
            </Panel>
          </PanelGroup>
        </Panel>
      </PanelGroup>
      <EdgeSettingsDialog />
    </>
  );
}
