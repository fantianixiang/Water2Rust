import { FolderOpen } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

interface PathRowProps {
  label: string;
  hint: string;
  value: string;
  onChange: (v: string) => void;
  onBrowse: () => Promise<string | null>;
}

/** 「标签 + 输入框 + 浏览按钮 + 说明」的路径行。 */
export function PathRow({ label, hint, value, onChange, onBrowse }: PathRowProps) {
  const browse = async () => {
    const picked = await onBrowse();
    if (picked) onChange(picked);
  };

  return (
    <div className="space-y-1">
      <label className="text-sm font-medium">{label}</label>
      <div className="flex gap-2">
        <Input value={value} onChange={(e) => onChange(e.target.value)} />
        <Button variant="outline" size="sm" onClick={browse} className="shrink-0">
          <FolderOpen />
          浏览
        </Button>
      </div>
      <p className="text-xs text-muted-foreground">{hint}</p>
    </div>
  );
}
