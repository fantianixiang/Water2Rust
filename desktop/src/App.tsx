import { useEffect } from "react";
import { Moon, Sun, Waves } from "lucide-react";

import { Button } from "@/components/ui/button";
import { WaterPage } from "@/components/water/WaterPage";
import { useAppStore } from "@/store/app-store";

/** Water2Rust 桌面成品页（Tauri 2 + React 19 + shadcn）。 */
function App() {
  const { dark, toggleDark } = useAppStore();

  useEffect(() => {
    document.documentElement.classList.toggle("dark", dark);
  }, [dark]);

  return (
    <div className="flex h-screen flex-col bg-background text-foreground">
      <header className="flex shrink-0 items-center justify-between border-b px-6 py-3">
        <div className="flex items-center gap-2">
          <Waves className="text-primary" />
          <span className="text-lg font-bold">Water2Rust</span>
          <span className="text-sm text-muted-foreground">水体处理工具链</span>
        </div>
        <Button variant="ghost" size="icon" onClick={toggleDark} title="切换暗色">
          {dark ? <Sun /> : <Moon />}
        </Button>
      </header>

      <main className="min-h-0 flex-1">
        <WaterPage />
      </main>
    </div>
  );
}

export default App;
