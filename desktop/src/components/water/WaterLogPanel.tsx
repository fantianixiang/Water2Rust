import { useEffect, useRef } from "react";
import Editor, { type OnMount } from "@monaco-editor/react";
import { Eraser, ScrollText } from "lucide-react";

import { Button } from "@/components/ui/button";
import { useAppStore } from "@/store/app-store";
import { useWaterStore } from "@/store/water-store";
import "@/lib/monaco-setup";

type MonacoEditor = Parameters<OnMount>[0];

/** 右下：详细日志（Monaco 只读编辑器，行号 + 自动滚动到底部）。 */
export function WaterLogPanel() {
  const log = useWaterStore((s) => s.log);
  const clearLog = useWaterStore((s) => s.clearLog);
  const dark = useAppStore((s) => s.dark);
  const editorRef = useRef<MonacoEditor | null>(null);

  const onMount: OnMount = (editor) => {
    editorRef.current = editor;
  };

  // 日志更新后滚动到底部。
  useEffect(() => {
    const ed = editorRef.current;
    const model = ed?.getModel();
    if (ed && model) ed.revealLine(model.getLineCount());
  }, [log]);

  return (
    <div className="flex h-full flex-col p-4">
      <div className="mb-2 flex items-center justify-between">
        <div className="flex items-center gap-2">
          <ScrollText className="h-4 w-4 text-muted-foreground" />
          <h2 className="text-sm font-semibold">详细日志</h2>
        </div>
        <Button variant="ghost" size="sm" onClick={clearLog}>
          <Eraser />
          清空
        </Button>
      </div>
      <div className="flex-1 overflow-hidden rounded-md border">
        <Editor
          height="100%"
          language="plaintext"
          theme={dark ? "vs-dark" : "vs"}
          value={log || "（暂无日志）"}
          onMount={onMount}
          options={{
            readOnly: true,
            domReadOnly: true,
            minimap: { enabled: false },
            fontSize: 12,
            lineNumbers: "on",
            scrollBeyondLastLine: false,
            wordWrap: "on",
            renderLineHighlight: "none",
            automaticLayout: true,
            folding: false,
            overviewRulerLanes: 0,
          }}
        />
      </div>
    </div>
  );
}
