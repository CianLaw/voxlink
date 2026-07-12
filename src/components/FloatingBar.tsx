import { useMemo } from "react";
import type { AppStateData } from "../App";

interface FloatingBarProps {
  appState: AppStateData;
  onDismiss: () => void;
}

/**
 * 灵动岛风格悬浮条组件
 * 毛玻璃效果 + 波形动画 + 状态指示
 */
function FloatingBar({ appState, onDismiss }: FloatingBarProps) {
  const { state, transcript, errorMessage } = appState;

  const isExpanded = state === "processing" || state === "injecting" || state === "error";

  const statusText = useMemo(() => {
    switch (state) {
      case "idle":
        return "";
      case "listening":
        return "正在聆听...";
      case "processing":
        return "正在识别...";
      case "injecting":
        return "正在注入...";
      case "error":
        return errorMessage || "出错了";
    }
  }, [state, errorMessage]);

  const statusDotClass = useMemo(() => {
    switch (state) {
      case "idle":
        return "status-dot idle";
      case "listening":
        return "status-dot listening";
      case "processing":
        return "status-dot processing";
      case "injecting":
        return "status-dot injecting";
      case "error":
        return "status-dot error";
    }
  }, [state]);

  const islandClass = useMemo(() => {
    const classes = ["floating-island", "transition-glow"];
    if (isExpanded) classes.push("expanded");
    if (state === "listening") classes.push("listening");
    return classes.join(" ");
  }, [isExpanded, state]);

  return (
    <div
      className={islandClass}
      style={{
        padding: isExpanded ? "16px 24px" : "12px 20px",
        minWidth: isExpanded ? "320px" : "200px",
        maxWidth: isExpanded ? "480px" : "320px",
        transition: "all 0.4s cubic-bezier(0.34, 1.56, 0.64, 1)",
      }}
    >
      {/* 收起状态：仅显示波形 + 状态 */}
      {!isExpanded && (
        <div className="flex items-center gap-3">
          {/* 波形动画 */}
          <div className="flex items-center gap-[2px] h-6">
            {Array.from({ length: 10 }).map((_, i) => (
              <span
                key={i}
                className="wave-bar"
                style={{
                  height: `${12 + Math.sin((i / 10) * Math.PI) * 8}px`,
                }}
              />
            ))}
          </div>
          {/* 状态指示点 */}
          <span className={statusDotClass} />
          {/* 状态文字 */}
          <span className="text-white text-xs font-medium opacity-80">
            {statusText}
          </span>
        </div>
      )}

      {/* 展开状态：显示识别文本 */}
      {isExpanded && (
        <div className="flex flex-col gap-3">
          {/* 顶部：状态指示 */}
          <div className="flex items-center gap-2">
            <span className={statusDotClass} />
            <span className="text-white text-xs font-medium opacity-80">
              {statusText}
            </span>
            <div className="flex-1" />
            {/* 关闭按钮 */}
            <button
              onClick={onDismiss}
              className="w-5 h-5 rounded-full flex items-center justify-center
                         bg-white/10 hover:bg-white/20 transition-colors cursor-pointer"
              aria-label="关闭"
            >
              <svg
                width="10"
                height="10"
                viewBox="0 0 10 10"
                fill="none"
                xmlns="http://www.w3.org/2000/svg"
              >
                <path
                  d="M1 1L9 9M9 1L1 9"
                  stroke="white"
                  strokeWidth="1.5"
                  strokeLinecap="round"
                />
              </svg>
            </button>
          </div>

          {/* 中部：识别文本 */}
          {state === "processing" && transcript && (
            <div className="text-reveal">
              <p className="text-white text-sm leading-relaxed opacity-90">
                {transcript}
              </p>
              {/* 加载动画 */}
              <div className="flex items-center gap-1 mt-2">
                <span className="w-1.5 h-1.5 rounded-full bg-white/40 animate-bounce" style={{ animationDelay: "0s" }} />
                <span className="w-1.5 h-1.5 rounded-full bg-white/40 animate-bounce" style={{ animationDelay: "0.15s" }} />
                <span className="w-1.5 h-1.5 rounded-full bg-white/40 animate-bounce" style={{ animationDelay: "0.3s" }} />
              </div>
            </div>
          )}

          {state === "injecting" && (
            <div className="flex items-center gap-2">
              <svg
                className="animate-spin"
                width="16"
                height="16"
                viewBox="0 0 16 16"
                fill="none"
                xmlns="http://www.w3.org/2000/svg"
              >
                <circle
                  cx="8"
                  cy="8"
                  r="6"
                  stroke="rgba(52, 211, 153, 0.3)"
                  strokeWidth="2"
                />
                <path
                  d="M8 2a6 6 0 0 1 5.2 9"
                  stroke="#34D399"
                  strokeWidth="2"
                  strokeLinecap="round"
                />
              </svg>
              <span className="text-white text-sm opacity-80">
                已注入文本
              </span>
            </div>
          )}

          {state === "error" && (
            <div className="flex items-center gap-2">
              <svg
                width="16"
                height="16"
                viewBox="0 0 16 16"
                fill="none"
                xmlns="http://www.w3.org/2000/svg"
              >
                <circle cx="8" cy="8" r="6" stroke="#FB7185" strokeWidth="1.5" />
                <path
                  d="M8 5v3M8 11h.01"
                  stroke="#FB7185"
                  strokeWidth="1.5"
                  strokeLinecap="round"
                />
              </svg>
              <span className="text-white text-sm opacity-80">
                {errorMessage}
              </span>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export default FloatingBar;