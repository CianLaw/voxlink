import { useState, useEffect, useCallback } from "react";
import FloatingBar from "./components/FloatingBar";

type AppState = "idle" | "listening" | "processing" | "injecting" | "error";

export interface AppStateData {
  state: AppState;
  transcript: string;
  errorMessage: string;
}

function App() {
  const [appState, setAppState] = useState<AppStateData>({
    state: "idle",
    transcript: "",
    errorMessage: "",
  });

  const [visible, setVisible] = useState(false);

  // 监听 Tauri 后端事件
  useEffect(() => {
    let unlisten: (() => void) | undefined;

    async function setupListener() {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        unlisten = await listen<AppStateData>("voxlink-state", (event) => {
          setAppState(event.payload);
          if (event.payload.state !== "idle") {
            setVisible(true);
          }
        });
      } catch {
        // 开发模式下 Tauri API 不可用，使用模拟数据
        console.log("[VoxLink] Running in dev mode, Tauri APIs unavailable");
      }
    }

    setupListener();

    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  // 模拟快捷键激活（开发模式）
  const handleToggle = useCallback(() => {
    setVisible((prev) => !prev);
    if (!visible) {
      setAppState({
        state: "listening",
        transcript: "",
        errorMessage: "",
      });
    }
  }, [visible]);

  // 监听全局快捷键（实际由 Tauri 后端处理）
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // macOS: Option+Space, Windows: Alt+Space
      if (e.altKey && e.code === "Space") {
        e.preventDefault();
        handleToggle();
      }
      if (e.code === "Escape") {
        setVisible(false);
        setAppState({ state: "idle", transcript: "", errorMessage: "" });
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleToggle]);

  // 模拟语音识别流程（开发预览用）
  useEffect(() => {
    if (appState.state === "listening" && visible) {
      const timer = setTimeout(() => {
        setAppState((prev) => ({
          ...prev,
          state: "processing",
          transcript: "你好，这是一段语音输入的测试文本。",
        }));
      }, 3000);

      const timer2 = setTimeout(() => {
        setAppState((prev) => ({
          ...prev,
          state: "injecting",
        }));
      }, 4200);

      const timer3 = setTimeout(() => {
        setAppState({ state: "idle", transcript: "", errorMessage: "" });
        setVisible(false);
      }, 5200);

      return () => {
        clearTimeout(timer);
        clearTimeout(timer2);
        clearTimeout(timer3);
      };
    }
  }, [appState.state, visible]);

  if (!visible) return null;

  return (
    <div className="fixed inset-0 flex items-start justify-center pt-8 pointer-events-none">
      <div className="pointer-events-auto animate-scale-in">
        <FloatingBar appState={appState} onDismiss={() => setVisible(false)} />
      </div>
    </div>
  );
}

export default App;