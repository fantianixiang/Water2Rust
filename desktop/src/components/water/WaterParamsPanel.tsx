import { Play, Settings } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Separator } from "@/components/ui/separator";
import {
  pickFile,
  pickSave,
  runPipeline,
  type PipelineParams,
} from "@/lib/tauri";
import { useWaterStore } from "@/store/water-store";
import { PathRow } from "./PathRow";

/** 左侧参数区：输入路径、任务勾选（edge 带齿轮）、运行。 */
export function WaterParamsPanel() {
  const waterPath = useWaterStore((s) => s.waterPath);
  const outputPath = useWaterStore((s) => s.outputPath);
  const demPath = useWaterStore((s) => s.demPath);
  const referencePath = useWaterStore((s) => s.referencePath);
  const doFclass = useWaterStore((s) => s.doFclass);
  const doEdge = useWaterStore((s) => s.doEdge);
  const doHydro = useWaterStore((s) => s.doHydro);
  const hydroWithDem = useWaterStore((s) => s.hydroWithDem);
  const running = useWaterStore((s) => s.running);
  const set = useWaterStore((s) => s.set);

  const run = async () => {
    const s = useWaterStore.getState();
    if (!s.waterPath.trim()) return s.appendLog("错误：请填写水体输入路径。\n");
    if (!s.outputPath.trim()) return s.appendLog("错误：请填写输出结果路径。\n");
    if (!(s.doFclass || s.doEdge || s.doHydro))
      return s.appendLog("错误：请至少勾选一个任务。\n");
    if (s.doHydro && !s.demPath.trim())
      return s.appendLog("错误：hydro 任务需要 DEM 影像路径。\n");

    const params: PipelineParams = {
      waterPath: s.waterPath.trim(),
      outputPath: s.outputPath.trim(),
      demPath: s.demPath.trim() || null,
      referencePath: s.referencePath.trim(),
      doFclass: s.doFclass,
      doEdge: s.doEdge,
      doHydro: s.doHydro,
      hydroWithDem: s.hydroWithDem,
      edgeOverrides: s.edgeParams.map((p) => ({
        fclass: p.fclass,
        edge: p.edge,
        depth: p.depth,
      })),
    };
    s.beginRun();
    try {
      await runPipeline(params);
    } catch (e) {
      s.fail(String(e));
    }
  };

  return (
    <div className="flex h-full flex-col gap-4 overflow-y-auto p-5">
      <h2 className="text-base font-semibold">参数设置</h2>

      <PathRow
        label="水体输入数据"
        hint="要素路径（.shp / .gpkg / .geojson）"
        value={waterPath}
        onChange={(v) => set({ waterPath: v })}
        onBrowse={() => pickFile("矢量", ["shp", "gpkg", "geojson", "json"])}
      />
      <PathRow
        label="输出结果（必填）"
        hint="作为基名，派生 _fclass.shp / _edge.shp / _hydro.tif"
        value={outputPath}
        onChange={(v) => set({ outputPath: v })}
        onBrowse={() => pickSave("water_out.shp", "Shapefile", ["shp"])}
      />
      <PathRow
        label="DEM 影像（hydro 需要）"
        hint="DEM 栅格（.tif）"
        value={demPath}
        onChange={(v) => set({ demPath: v })}
        onBrowse={() => pickFile("栅格", ["tif", "tiff"])}
      />
      <PathRow
        label="分类参考库"
        hint="全国水域分类参考 GeoPackage（可替换）"
        value={referencePath}
        onChange={(v) => set({ referencePath: v })}
        onBrowse={() => pickFile("GeoPackage", ["gpkg"])}
      />

      <Separator />

      <div>
        <h2 className="text-base font-semibold">任务选项 / 功能</h2>
        <p className="text-xs text-muted-foreground">
          fclass 是 edge / hydro 的前置条件，会自动注入
        </p>
      </div>

      <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
        <label className="flex items-center gap-2 text-sm">
          <Checkbox
            checked={doFclass}
            onCheckedChange={(v) => set({ doFclass: !!v })}
          />
          水域分类 (fclass)
        </label>
        <label className="flex items-center gap-2 text-sm">
          <Checkbox
            checked={doEdge}
            onCheckedChange={(v) => set({ doEdge: !!v })}
          />
          边缘深度 (edge)
        </label>
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7"
          title="设置 edge 参数（每类别 edgeexpand/depth）"
          onClick={() => set({ showEdgeSettings: true })}
        >
          <Settings />
        </Button>
        <label className="flex items-center gap-2 text-sm">
          <Checkbox
            checked={doHydro}
            onCheckedChange={(v) => set({ doHydro: !!v })}
          />
          水文DEM (hydro)
        </label>
      </div>

      <label
        className={`flex items-center gap-2 text-sm ${doHydro ? "" : "opacity-50"}`}
      >
        <Checkbox
          checked={hydroWithDem}
          disabled={!doHydro}
          onCheckedChange={(v) => set({ hydroWithDem: !!v })}
        />
        hydro 输出含 DEM 底图回填
      </label>

      <div className="pt-1">
        <Button onClick={run} disabled={running} className="min-w-24">
          <Play />
          {running ? "运行中…" : "运行"}
        </Button>
      </div>
    </div>
  );
}
