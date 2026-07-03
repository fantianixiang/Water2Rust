import { useEffect } from "react";
import { zodResolver } from "@hookform/resolvers/zod";
import { useFieldArray, useForm } from "react-hook-form";
import { RotateCcw, Save } from "lucide-react";
import { z } from "zod";

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

const rowSchema = z.object({
  fclass: z.string(),
  edge: z.coerce.number({ message: "需为数值" }).min(0, "≥ 0"),
  depth: z.coerce.number({ message: "需为数值" }).min(0, "≥ 0"),
  edgeMin: z.number(),
  edgeMax: z.number().nullable(),
  depthMin: z.number(),
  depthMax: z.number().nullable(),
});
const schema = z.object({ rows: z.array(rowSchema) });
type FormValues = z.infer<typeof schema>;

function rangeText(min: number, max: number | null): string {
  return `推荐 ${min} ~ ${max === null ? "∞" : max}`;
}

/** edge 参数设置子窗口：react-hook-form + zod 校验，应用后写回全局状态。 */
export function EdgeSettingsDialog() {
  const showEdgeSettings = useWaterStore((s) => s.showEdgeSettings);
  const edgeParams = useWaterStore((s) => s.edgeParams);
  const set = useWaterStore((s) => s.set);
  const setEdgeParam = useWaterStore((s) => s.setEdgeParam);

  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: { rows: [] },
  });
  const { fields } = useFieldArray({ control: form.control, name: "rows" });
  const errors = form.formState.errors;

  // 打开时以当前配置初始化表单。
  useEffect(() => {
    if (showEdgeSettings) {
      form.reset({ rows: edgeParams.map((p) => ({ ...p })) });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [showEdgeSettings]);

  const resetDefault = async () => {
    const items = await getEdgeGuidance();
    form.reset({ rows: items.map((p) => ({ ...p })) });
  };

  const onSubmit = form.handleSubmit((data) => {
    data.rows.forEach((r) => setEdgeParam(r.fclass, Number(r.edge), Number(r.depth)));
    set({ showEdgeSettings: false });
  });

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
            括号内为推荐范围，仅作提示、可超出；值须 ≥ 0。
          </DialogDescription>
        </DialogHeader>

        <form onSubmit={onSubmit} className="space-y-4">
          <div className="grid grid-cols-[5rem_1fr_1fr] items-start gap-x-4 gap-y-3">
            <div className="pt-2 text-sm font-semibold">类别</div>
            <div className="pt-2 text-sm font-semibold">edgeexpand</div>
            <div className="pt-2 text-sm font-semibold">depth</div>

            {fields.map((field, i) => (
              <div key={field.id} className="contents">
                <div className="pt-2 text-sm font-medium text-primary">
                  {field.fclass}
                </div>
                <div>
                  <Input
                    type="number"
                    step="0.1"
                    {...form.register(`rows.${i}.edge`)}
                  />
                  <div className="mt-0.5 text-[10px] text-muted-foreground">
                    {errors.rows?.[i]?.edge ? (
                      <span className="text-destructive">
                        {errors.rows[i]?.edge?.message}
                      </span>
                    ) : (
                      rangeText(field.edgeMin, field.edgeMax)
                    )}
                  </div>
                </div>
                <div>
                  <Input
                    type="number"
                    step="0.1"
                    {...form.register(`rows.${i}.depth`)}
                  />
                  <div className="mt-0.5 text-[10px] text-muted-foreground">
                    {errors.rows?.[i]?.depth ? (
                      <span className="text-destructive">
                        {errors.rows[i]?.depth?.message}
                      </span>
                    ) : (
                      rangeText(field.depthMin, field.depthMax)
                    )}
                  </div>
                </div>
              </div>
            ))}
          </div>

          <div className="flex items-center gap-3">
            <Button type="submit" size="sm">
              <Save />
              应用
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={resetDefault}
            >
              <RotateCcw />
              恢复默认
            </Button>
            <span className="text-xs text-muted-foreground">
              应用后运行 edge 时采用当前值
            </span>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
}
