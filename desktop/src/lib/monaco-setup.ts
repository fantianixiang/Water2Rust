// 将 Monaco 配置为本地打包（离线可用，不从 CDN 加载），并注册 worker。
// 仅日志查看用 plaintext，故只需 editor.worker。
import { loader } from "@monaco-editor/react";
// 仅引入编辑器内核（不含各语言贡献），日志用 plaintext 即可，显著减小包体。
import * as monaco from "monaco-editor/esm/vs/editor/editor.api";
import editorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";

// Vite 环境下用打包的 worker，替代默认的 CDN 加载。
(self as unknown as { MonacoEnvironment: monaco.Environment }).MonacoEnvironment =
  {
    getWorker: () => new editorWorker(),
  };

loader.config({ monaco });
