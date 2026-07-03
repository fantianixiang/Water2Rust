# Water2Rust Desktop（Tauri 2 + React 19）

Water2Rust 的桌面前端，技术栈迁移目标（对应 `shadcn` 分支）：

| 技术 | 版本 | 用途 |
|---|---|---|
| React | 19 | UI 框架 |
| Vite | 6 | 构建工具 |
| Tauri | 2 | Rust 后端 + WebView 桌面壳 |
| Zustand | 5 | 全局 UI 状态 |
| TanStack Query | v5 | 异步/原生端数据状态 |
| Tailwind CSS | 3.4 | 原子化 CSS |
| shadcn/ui | 最新 | 基于 Radix 的可复制组件 |
| lucide-react | 最新 | 图标库 |
| react-hook-form + zod | - | 表单与校验 |
| react-resizable-panels | - | 可调面板布局 |
| @monaco-editor/react | - | 代码编辑器 |

> Rust 业务能力（water-core/io/fclass/hydro/edge-depth + eci-gdal）保持纯 Rust，
> 作为 Tauri 后端经 `#[tauri::command]` 暴露给 React 前端调用。

## 目录结构

```text
desktop/
  package.json / vite.config.ts / tsconfig*.json    # 前端工程
  tailwind.config.js / postcss.config.js            # Tailwind 3.4
  components.json                                    # shadcn 配置
  index.html
  src/
    main.tsx / App.tsx / index.css
    lib/utils.ts                # shadcn cn 辅助
    components/ui/button.tsx     # shadcn 组件
    store/app-store.ts           # Zustand
  src-tauri/                     # Tauri 2 Rust 后端壳（workspace 成员）
    Cargo.toml / tauri.conf.json / build.rs
    src/{main.rs, lib.rs}        # #[tauri::command]
    capabilities/default.json
    icons/                       # 应用图标
```

## 前置

- Node.js ≥ 20（含 npm）
- Rust ≥ 1.85（cargo）
- Windows：WebView2 Runtime（Win10/11 通常已内置）

## 开发与构建

```bash
cd desktop
npm install

# 仅前端（浏览器预览，后端命令不可用）
npm run dev
npm run build          # tsc + vite build → dist/

# Tauri 桌面应用（前端 + Rust 后端一起跑）
npm run tauri dev
npm run tauri build    # 打包安装程序
```

后端 Rust 壳也可单独编译验证：

```bash
cargo check -p water-desktop
```

## 迁移进度

- [x] 脚手架：React 19 + Vite 6 + Tauri 2 + Tailwind 3.4 + shadcn 基座
- [x] 状态/异步：Zustand 5 + TanStack Query v5；图标 lucide-react
- [x] 前端 ↔ 后端链路自检（`greet` 命令）
- [x] water 业务页（fclass / edge / hydro）迁移到 React/shadcn，UI 按 shadcn 风格美化
- [x] Tauri 命令封装 water-fclass/hydro/edge-depth（`run_pipeline` + 事件回传进度/日志）
- [x] ts-rs 类型安全绑定（Rust 结构体 → `src/lib/bindings/*.ts`，前端共享类型单一来源）
- [x] react-hook-form + zod（edge 参数子窗口校验）、react-resizable-panels 布局、Monaco 日志编辑器
- [ ] Storybook（可选）；tauri-specta（已用 ts-rs 满足类型绑定，可后续替换）

> 重新生成 ts-rs 绑定：`cargo test -p water-desktop`（导出到 `desktop/src/lib/bindings/`）。

> 注：Tauri 依赖树中 cookie 0.18.1 与 time 0.3.52+ 不兼容，`src-tauri/Cargo.toml`
> 已将 time 约束为 `<0.3.52`（本仓库 gitignore 了 Cargo.lock）。
