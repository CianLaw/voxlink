import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

const APP_VERSION = "1.1.0";

const LANGS = [
  { code: "zh-CN", label: "中文（简体）" },
  { code: "zh-TW", label: "中文（繁体）" },
  { code: "en-US", label: "English (US)" },
  { code: "en-GB", label: "English (UK)" },
  { code: "ja-JP", label: "日本語" },
  { code: "ko-KR", label: "한국어" },
];

const AI_MODELS = [
  { value: "gemini-2.0-flash", label: "Gemini 2.0 Flash" },
  { value: "gemini-2.0-flash-lite", label: "Gemini 2.0 Flash Lite" },
  { value: "gemini-1.5-flash", label: "Gemini 1.5 Flash" },
  { value: "gemini-1.5-pro", label: "Gemini 1.5 Pro" },
];

const TRANS_LANGS = [
  { code: "en", label: "English" },
  { code: "zh", label: "中文" },
  { code: "ja", label: "日本語" },
  { code: "ko", label: "한국어" },
  { code: "fr", label: "Français" },
  { code: "de", label: "Deutsch" },
  { code: "es", label: "Español" },
  { code: "ru", label: "Русский" },
  { code: "it", label: "Italiano" },
  { code: "pt", label: "Português" },
];

interface Settings {
  lang: string;
  autoPunc: boolean;
  autoSpace: boolean;
  aiPolish: boolean;
  aiModel: string;
  apiKey: string;
  translation: boolean;
  targetLang: string;
  shortcuts: { toggle: string; confirm: string; cancel: string };
}

const DEFAULT_SETTINGS: Settings = {
  lang: "zh-CN", autoPunc: true, autoSpace: true,
  aiPolish: false, aiModel: "gemini-2.0-flash", apiKey: "",
  translation: false, targetLang: "en",
  shortcuts: { toggle: "ctrl+shift+v", confirm: "ctrl+enter", cancel: "escape" },
};

function loadSettings(): Settings {
  try {
    const saved = localStorage.getItem("voxlink_settings");
    return saved ? { ...DEFAULT_SETTINGS, ...JSON.parse(saved) } : DEFAULT_SETTINGS;
  } catch { return DEFAULT_SETTINGS; }
}

export default function App() {
  const [page, setPage] = useState<"settings" | "changelog">("settings");
  const [s, setS] = useState<Settings>(loadSettings);
  const [toast, setToast] = useState("");
  const [micOk, setMicOk] = useState(false);

  // 弹窗状态
  const [showLangSheet, setShowLangSheet] = useState(false);
  const [showApiKeyModal, setShowApiKeyModal] = useState(false);
  const [showShortcutModal, setShowShortcutModal] = useState(false);
  const [showTransLangSheet, setShowTransLangSheet] = useState(false);
  const [editingKey, setEditingKey] = useState<string>("");
  const [shortcutInput, setShortcutInput] = useState("");
  const [apiKeyInput, setApiKeyInput] = useState(s.apiKey);
  const [selectedModel, setSelectedModel] = useState(s.aiModel);

  const update = useCallback((patch: Partial<Settings>) => {
    setS(prev => {
      const next = { ...prev, ...patch };
      localStorage.setItem("voxlink_settings", JSON.stringify(next));
      return next;
    });
  }, []);

  const showT = useCallback((msg: string) => { setToast(msg); setTimeout(() => setToast(""), 2500); }, []);

  // 检查麦克风
  useEffect(() => {
    navigator.mediaDevices?.getUserMedia({ audio: true })
      .then(s => { s.getTracks().forEach(t => t.stop()); setMicOk(true); })
      .catch(() => setMicOk(false));
  }, []);

  // 快捷键监听
  useEffect(() => {
    if (!showShortcutModal) return;
    const handleDown = (e: KeyboardEvent) => {
      e.preventDefault();
      const parts: string[] = [];
      if (e.ctrlKey) parts.push("ctrl");
      if (e.shiftKey) parts.push("shift");
      if (e.altKey) parts.push("alt");
      if (e.metaKey) parts.push("meta");
      if (!["Control", "Shift", "Alt", "Meta"].includes(e.key)) parts.push(e.key.toLowerCase());
      setShortcutInput(parts.join("+"));
    };
    const handleUp = (e: KeyboardEvent) => {
      if (e.key === "Escape") { setShowShortcutModal(false); return; }
      if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;
      if (shortcutInput) {
        update({ shortcuts: { ...s.shortcuts, [editingKey]: shortcutInput } });
      }
      setShowShortcutModal(false);
      setShortcutInput("");
    };
    window.addEventListener("keydown", handleDown);
    window.addEventListener("keyup", handleUp);
    return () => { window.removeEventListener("keydown", handleDown); window.removeEventListener("keyup", handleUp); };
  }, [showShortcutModal, shortcutInput, editingKey, s.shortcuts, update]);

  // 解析快捷键显示
  const parseKbd = (k: string) => k.split(/[+]/).map(p => {
    const t = p.trim().toLowerCase();
    if (t === "ctrl" || t === "control") return "Ctrl";
    if (t === "shift") return "Shift";
    if (t === "alt") return "Alt";
    if (t === "meta" || t === "cmd" || t === "command") return "Cmd";
    if (t === "enter") return "Enter";
    if (t === "escape") return "Esc";
    return t.charAt(0).toUpperCase() + t.slice(1);
  }).join(" + ");

  const requestMic = async () => {
    try { const st = await navigator.mediaDevices.getUserMedia({ audio: true }); st.getTracks().forEach(t => t.stop()); setMicOk(true); showT("麦克风已授权"); }
    catch { showT("麦克风权限被拒绝"); setMicOk(false); }
  };

  return (
    <div className="min-h-screen relative" style={{ background: "linear-gradient(180deg, #f0f4f8 0%, #e8eef5 50%, #f5f7fa 100%)", width: 430, height: 780, overflow: "hidden" }}>
      <div className="bg-dot" style={{ width: 280, height: 280, background: "linear-gradient(135deg, #93c5fd, #c4b5fd)", top: -60, right: -60 }} />
      <div className="bg-dot" style={{ width: 200, height: 200, background: "linear-gradient(135deg, #fbcfe8, #ddd6fe)", bottom: 120, left: -40, opacity: 0.3 }} />

      {toast && (
        <div className="fixed top-10 left-1/2 z-[99999] px-6 py-2.5 rounded-2xl text-white text-sm whitespace-nowrap"
          style={{ transform: "translateX(-50%)", background: "rgba(20,20,30,0.82)", backdropFilter: "blur(40px)", border: "1px solid rgba(255,255,255,0.06)", animation: "scaleIn 0.3s cubic-bezier(0.34,1.56,0.64,1)" }}>
          {toast}
        </div>
      )}

      <div className="flex flex-col h-full relative z-10">
        {/* 头部 */}
        <div className="flex items-center px-5 pt-4 pb-2">
          {page === "changelog" ? (
            <button onClick={() => setPage("settings")} className="w-9 h-9 rounded-full flex items-center justify-center text-slate-500 transition-all active:scale-90" style={{ background: "rgba(255,255,255,0.5)", backdropFilter: "blur(20px)" }}>
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"><path d="M15 18l-6-6 6-6"/></svg>
            </button>
          ) : (
            <div className="w-9 h-9 rounded-xl flex items-center justify-center" style={{ background: "linear-gradient(135deg, #3B82F6, #8B5CF6)", boxShadow: "0 4px 12px rgba(59,130,246,0.25)" }}>
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="white" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round"><path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z"/><path d="M19 10v2a7 7 0 0 1-14 0v-2"/><line x1="12" x2="12" y1="19" y2="22"/></svg>
            </div>
          )}
          <span className="ml-3 text-lg font-bold text-slate-800">
            {page === "changelog" ? "更新日志" : "设置"}
          </span>
          <span className="ml-auto text-[10px] text-slate-400">v{APP_VERSION}</span>
        </div>

        {/* 内容 */}
        <div className="flex-1 overflow-y-auto px-5 pb-8" style={{ scrollbarWidth: "none" }}>
          {page === "settings" ? (
            <>
              {/* 关于 */}
              <div className="text-center py-5">
                <div className="w-14 h-14 rounded-2xl mx-auto mb-2 flex items-center justify-center" style={{ background: "linear-gradient(135deg, #3B82F6, #8B5CF6)", boxShadow: "0 4px 16px rgba(59,130,246,0.25)" }}>
                  <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="white" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round"><path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z"/><path d="M19 10v2a7 7 0 0 1-14 0v-2"/><line x1="12" x2="12" y1="19" y2="22"/></svg>
                </div>
                <div className="text-base font-bold text-slate-800">VoxLink</div>
                <div className="text-xs text-slate-400">v{APP_VERSION} &middot; 高精度语音输入助手</div>
                <div className="text-[11px] text-slate-400 mt-1">快捷键 Ctrl+Shift+V 快速唤起</div>
              </div>

              {/* AI 润色 */}
              <div className="text-[11px] font-semibold text-slate-400 uppercase tracking-wider mb-2">AI 润色</div>
              <div className="glass rounded-2xl p-4 mb-2">
                <div className="glass-inner flex items-center justify-between">
                  <div><div className="text-[15px] font-medium text-slate-800">启用 AI 润色</div><div className="text-xs text-slate-400">语音输入后自动润色排版</div></div>
                  <div className={`toggle-track ${s.aiPolish ? "on" : ""}`} onClick={() => update({ aiPolish: !s.aiPolish })}><div className="toggle-knob" /></div>
                </div>
              </div>
              <div className="glass rounded-2xl p-4 mb-3 cursor-pointer" onClick={() => { setApiKeyInput(s.apiKey); setSelectedModel(s.aiModel); setShowApiKeyModal(true); }}>
                <div className="glass-inner flex items-center justify-between">
                  <div><div className="text-[15px] font-medium text-slate-800">Gemini API Key</div><div className="text-xs text-slate-400">{s.apiKey ? "已配置" : "输入你的 API 密钥"}</div></div>
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="#94a3b8" strokeWidth="2"><path d="M9 18l6-6-6-6"/></svg>
                </div>
              </div>

              {/* 翻译 */}
              <div className="text-[11px] font-semibold text-slate-400 uppercase tracking-wider mb-2 mt-5">翻译</div>
              <div className="glass rounded-2xl p-4 mb-2">
                <div className="glass-inner flex items-center justify-between">
                  <div><div className="text-[15px] font-medium text-slate-800">启用翻译</div><div className="text-xs text-slate-400">在历史记录中显示翻译按钮</div></div>
                  <div className={`toggle-track ${s.translation ? "on" : ""}`} onClick={() => update({ translation: !s.translation })}><div className="toggle-knob" /></div>
                </div>
              </div>
              <div className="glass rounded-2xl p-4 mb-3 cursor-pointer" onClick={() => setShowTransLangSheet(true)}>
                <div className="glass-inner flex items-center justify-between">
                  <div><div className="text-[15px] font-medium text-slate-800">翻译目标语言</div><div className="text-xs text-slate-400">翻译输出的语言</div></div>
                  <div className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm font-medium text-blue-500" style={{ background: "rgba(255,255,255,0.6)", backdropFilter: "blur(20px)" }}>
                    {TRANS_LANGS.find(l => l.code === s.targetLang)?.label || "English"}
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 9l6 6 6-6"/></svg>
                  </div>
                </div>
              </div>

              {/* 输入设置 */}
              <div className="text-[11px] font-semibold text-slate-400 uppercase tracking-wider mb-2 mt-5">输入设置</div>
              <div className="glass rounded-2xl p-4 mb-2 cursor-pointer" onClick={() => setShowLangSheet(true)}>
                <div className="glass-inner flex items-center justify-between">
                  <div><div className="text-[15px] font-medium text-slate-800">识别语言</div><div className="text-xs text-slate-400">语音识别的目标语言</div></div>
                  <div className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm font-medium text-blue-500" style={{ background: "rgba(255,255,255,0.6)", backdropFilter: "blur(20px)" }}>
                    {LANGS.find(l => l.code === s.lang)?.label || "中文（简体）"}
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 9l6 6 6-6"/></svg>
                  </div>
                </div>
              </div>
              <div className="glass rounded-2xl p-4 mb-2">
                <div className="glass-inner flex items-center justify-between">
                  <div><div className="text-[15px] font-medium text-slate-800">自动添加标点</div><div className="text-xs text-slate-400">在识别结果中自动插入标点</div></div>
                  <div className={`toggle-track ${s.autoPunc ? "on" : ""}`} onClick={() => update({ autoPunc: !s.autoPunc })}><div className="toggle-knob" /></div>
                </div>
              </div>
              <div className="glass rounded-2xl p-4 mb-3">
                <div className="glass-inner flex items-center justify-between">
                  <div><div className="text-[15px] font-medium text-slate-800">自动空格</div><div className="text-xs text-slate-400">中英文之间自动添加空格</div></div>
                  <div className={`toggle-track ${s.autoSpace ? "on" : ""}`} onClick={() => update({ autoSpace: !s.autoSpace })}><div className="toggle-knob" /></div>
                </div>
              </div>

              {/* 快捷键 */}
              <div className="text-[11px] font-semibold text-slate-400 uppercase tracking-wider mb-2 mt-5">快捷键</div>
              {[
                { key: "toggle", label: "语音输入开关", desc: "开始/停止语音输入" },
                { key: "confirm", label: "确认输入", desc: "确认并插入识别结果" },
                { key: "cancel", label: "取消输入", desc: "取消当前识别" },
              ].map(sk => (
                <div key={sk.key} className="glass rounded-2xl p-4 mb-2 cursor-pointer"
                  onClick={() => { setEditingKey(sk.key); setShortcutInput(""); setShowShortcutModal(true); }}>
                  <div className="glass-inner flex items-center justify-between">
                    <div><div className="text-[15px] font-medium text-slate-800">{sk.label}</div><div className="text-xs text-slate-400">{sk.desc}</div></div>
                    <span className="kbd-badge">{parseKbd(s.shortcuts[sk.key as keyof typeof s.shortcuts] || "")}</span>
                  </div>
                </div>
              ))}

              {/* 权限 */}
              <div className="text-[11px] font-semibold text-slate-400 uppercase tracking-wider mb-2 mt-5">权限</div>
              <div className="glass rounded-2xl p-4 mb-3 cursor-pointer" onClick={() => { if (!micOk) requestMic(); }}>
                <div className="glass-inner flex items-center justify-between">
                  <div><div className="text-[15px] font-medium text-slate-800">麦克风权限</div><div className="text-xs text-slate-400">用于捕获语音输入{!micOk ? "（点击授权）" : ""}</div></div>
                  <div className={`flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-medium ${micOk ? "bg-green-50 text-green-600" : "bg-red-50 text-red-500"}`}>
                    <div className={`w-1.5 h-1.5 rounded-full ${micOk ? "bg-green-500" : "bg-red-500"}`} />
                    {micOk ? "已授权" : "未授权"}
                  </div>
                </div>
              </div>

              {/* 关于 */}
              <div className="text-[11px] font-semibold text-slate-400 uppercase tracking-wider mb-2 mt-5">关于</div>
              <div className="glass rounded-2xl p-4 mb-2 cursor-pointer" onClick={() => setPage("changelog")}>
                <div className="glass-inner flex items-center justify-between">
                  <div><div className="text-[15px] font-medium text-slate-800">更新日志</div><div className="text-xs text-slate-400">查看版本更新记录</div></div>
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="#94a3b8" strokeWidth="2"><path d="M9 18l6-6-6-6"/></svg>
                </div>
              </div>
              <div className="glass rounded-2xl p-4 mb-2">
                <div className="glass-inner">
                  <div className="text-[15px] font-medium text-slate-800 mb-1">作者信息</div>
                  <div className="text-xs text-slate-500">作者: @没</div>
                  <div className="text-xs text-slate-500">邮箱: lao88888@agent.qq.com</div>
                  <div className="text-xs text-slate-500">备用: lao132973264188@gmail.com</div>
                </div>
              </div>
              <div className="h-10" />
            </>
          ) : (
            /* 更新日志页面 */
            <div className="relative">
              <div className="absolute left-[15px] top-3 bottom-0 w-[2px]" style={{ background: "linear-gradient(180deg, #3B82F6 0%, #8B5CF6 50%, rgba(139,92,246,0.1) 100%)" }} />
              {[
                { v: "v1.1.0", d: "2026-07-15", h: ["语音识别引擎深度优化，模糊发音也能精准识别", "新增更新日志与版本推送", "灵动岛精简为叉+声波+勾", "代码全面优化提升流畅性", "新增系统托盘与后台运行", "新增 macOS .pkg / Windows .msi 安装包"] },
                { v: "v1.0.2", d: "2026-07-14", h: ["新增 AI 润色与翻译功能", "新增自定义快捷键", "新增 iOS 风格弹窗", "液态玻璃 UI 全面覆盖"] },
                { v: "v1.0.0", d: "2026-07-12", h: ["VoxLink 首发", "语音识别输入", "6种语言支持", "AI润色排版", "快捷键操作", "液态玻璃设计"] },
              ].map((item, idx) => (
                <div key={item.v} className="relative pl-9 mb-5">
                  <div className={`absolute left-0 top-1 w-8 h-8 rounded-full flex items-center justify-center text-xs ${idx === 0 ? "text-white" : "text-slate-400"}`}
                    style={idx === 0 ? { background: "linear-gradient(135deg, #3B82F6, #8B5CF6)", boxShadow: "0 2px 8px rgba(59,130,246,0.3)" } : { background: "rgba(226,232,240,0.8)" }}>
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"><polyline points="20 6 9 17 4 12"/></svg>
                  </div>
                  <div className="glass rounded-2xl p-4">
                    <div className="glass-inner">
                      <div className="flex items-center gap-2 mb-1">
                        <span className={`text-sm font-bold ${idx === 0 ? "text-blue-600" : "text-slate-700"}`}>{item.v}</span>
                        {idx === 0 && <span className="text-[10px] px-2 py-0.5 rounded-full text-white font-semibold" style={{ background: "linear-gradient(135deg, #3B82F6, #8B5CF6)" }}>最新</span>}
                      </div>
                      <div className="text-xs text-slate-400 mb-2">{item.d}</div>
                      <ul className="space-y-1.5">
                        {item.h.map((h, hi) => (
                          <li key={hi} className="flex items-start gap-2 text-sm text-slate-600">
                            <span className="w-1 h-1 rounded-full bg-blue-400 mt-2 flex-shrink-0" />{h}
                          </li>
                        ))}
                      </ul>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>

      {/* ====== 语言选择弹窗 ====== */}
      {showLangSheet && (
        <div className="fixed inset-0 z-[99998]" style={{ animation: "fadeIn 0.35s ease" }}>
          <div className="absolute inset-0 bg-black/35" onClick={() => setShowLangSheet(false)} />
          <div className="absolute bottom-0 left-0 right-0 z-[99999] px-3 pb-8" style={{ animation: "slideUp 0.4s cubic-bezier(0.32,0.72,0,1)" }}>
            <div className="rounded-2xl overflow-hidden" style={{ background: "rgba(245,247,250,0.92)", backdropFilter: "blur(60px)", boxShadow: "0 -4px 40px rgba(0,0,0,0.15), inset 0 1px 0 rgba(255,255,255,0.5)" }}>
              <div className="text-center py-3 text-xs text-slate-400 font-medium border-b border-slate-200/50">选择识别语言</div>
              {LANGS.map(l => (
                <div key={l.code} className="flex items-center justify-between px-5 py-3.5 text-base cursor-pointer active:bg-blue-50/50 text-blue-500" style={{ borderBottom: "0.5px solid rgba(0,0,0,0.06)" }} onClick={() => { update({ lang: l.code }); setShowLangSheet(false); }}>
                  {l.label}
                  {s.lang === l.code && <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="#007AFF" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"><polyline points="20 6 9 17 4 12"/></svg>}
                </div>
              ))}
            </div>
            <div className="mt-2 rounded-2xl text-center py-3.5 text-blue-500 font-semibold text-base cursor-pointer active:bg-slate-100" style={{ background: "rgba(255,255,255,0.95)" }} onClick={() => setShowLangSheet(false)}>取消</div>
          </div>
        </div>
      )}

      {/* ====== 翻译语言弹窗 ====== */}
      {showTransLangSheet && (
        <div className="fixed inset-0 z-[99998]" style={{ animation: "fadeIn 0.35s ease" }}>
          <div className="absolute inset-0 bg-black/35" onClick={() => setShowTransLangSheet(false)} />
          <div className="absolute bottom-0 left-0 right-0 z-[99999] px-3 pb-8" style={{ animation: "slideUp 0.4s cubic-bezier(0.32,0.72,0,1)" }}>
            <div className="rounded-2xl overflow-hidden" style={{ background: "rgba(245,247,250,0.92)", backdropFilter: "blur(60px)", boxShadow: "0 -4px 40px rgba(0,0,0,0.15)" }}>
              <div className="text-center py-3 text-xs text-slate-400 font-medium border-b border-slate-200/50">选择翻译目标语言</div>
              {TRANS_LANGS.map(l => (
                <div key={l.code} className="flex items-center justify-between px-5 py-3.5 text-base cursor-pointer active:bg-blue-50/50 text-blue-500" style={{ borderBottom: "0.5px solid rgba(0,0,0,0.06)" }} onClick={() => { update({ targetLang: l.code }); setShowTransLangSheet(false); }}>
                  {l.label}
                  {s.targetLang === l.code && <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="#007AFF" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"><polyline points="20 6 9 17 4 12"/></svg>}
                </div>
              ))}
            </div>
            <div className="mt-2 rounded-2xl text-center py-3.5 text-blue-500 font-semibold text-base cursor-pointer active:bg-slate-100" style={{ background: "rgba(255,255,255,0.95)" }} onClick={() => setShowTransLangSheet(false)}>取消</div>
          </div>
        </div>
      )}

      {/* ====== API Key 弹窗 ====== */}
      {showApiKeyModal && (
        <div className="fixed inset-0 z-[99990] flex items-center justify-center" style={{ background: "rgba(0,0,0,0.4)", animation: "fadeIn 0.2s ease" }} onClick={() => setShowApiKeyModal(false)}>
          <div className="glass mx-5 w-full max-w-sm rounded-3xl p-6" style={{ animation: "scaleIn 0.35s cubic-bezier(0.34,1.56,0.64,1)" }} onClick={e => e.stopPropagation()}>
            <div className="glass-inner">
              <h3 className="text-lg font-bold text-slate-800 text-center mb-1">API 密钥设置</h3>
              <p className="text-xs text-slate-400 text-center mb-5">配置你的 Google Gemini API</p>
              <div className="mb-4">
                <label className="text-[11px] font-semibold text-slate-400 uppercase tracking-wider mb-2 block">AI 模型</label>
                <div className="grid grid-cols-2 gap-2">
                  {AI_MODELS.map(m => (
                    <button key={m.value} onClick={() => setSelectedModel(m.value)}
                      className={`py-2.5 px-3 rounded-xl text-sm font-medium transition-all ${selectedModel === m.value ? "text-white" : "text-slate-600 bg-slate-100 hover:bg-slate-200"}`}
                      style={selectedModel === m.value ? { background: "linear-gradient(135deg, #3B82F6, #8B5CF6)", boxShadow: "0 2px 8px rgba(59,130,246,0.25)" } : {}}>
                      {m.label}
                    </button>
                  ))}
                </div>
              </div>
              <div className="mb-5">
                <label className="text-[11px] font-semibold text-slate-400 uppercase tracking-wider mb-2 block">API 密钥</label>
                <input type="text" value={apiKeyInput} onChange={e => setApiKeyInput(e.target.value)} className="apikey-input" placeholder="留空使用默认 Key..." />
                <p className="text-[11px] text-slate-400 mt-1.5">留空则使用管理员提供的默认 Key</p>
              </div>
              <div className="flex gap-3">
                <button onClick={() => { setApiKeyInput(""); update({ apiKey: "", aiModel: selectedModel }); setShowApiKeyModal(false); }}
                  className="flex-1 py-3 rounded-xl text-sm font-semibold text-slate-500 transition-all active:scale-95" style={{ background: "#f1f5f9" }}>重置</button>
                <button onClick={() => { update({ apiKey: apiKeyInput.trim(), aiModel: selectedModel }); setShowApiKeyModal(false); showT("已保存"); }}
                  className="flex-1 py-3 rounded-xl text-sm font-semibold text-white transition-all active:scale-95" style={{ background: "linear-gradient(135deg, #3B82F6, #8B5CF6)", boxShadow: "0 4px 12px rgba(59,130,246,0.25)" }}>保存</button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* ====== 快捷键弹窗 ====== */}
      {showShortcutModal && (
        <div className="fixed inset-0 z-[99990] flex items-center justify-center" style={{ background: "rgba(0,0,0,0.4)", animation: "fadeIn 0.2s ease" }} onClick={() => setShowShortcutModal(false)}>
          <div className="glass mx-5 w-full max-w-sm rounded-3xl p-6" style={{ animation: "scaleIn 0.35s cubic-bezier(0.34,1.56,0.64,1)" }} onClick={e => e.stopPropagation()}>
            <div className="glass-inner text-center">
              <h3 className="text-lg font-bold text-slate-800 mb-1">自定义快捷键</h3>
              <p className="text-xs text-slate-400 mb-6">
                {editingKey === "toggle" && "语音输入开关"}
                {editingKey === "confirm" && "确认输入"}
                {editingKey === "cancel" && "取消输入"}
              </p>
              <div className="bg-slate-50 rounded-2xl p-6 mb-4 min-h-[80px] flex items-center justify-center">
                <span className="text-2xl font-bold text-slate-700 font-mono">{shortcutInput ? parseKbd(shortcutInput) : "按下快捷键..."}</span>
              </div>
              <p className="text-xs text-slate-400 mb-5">按下你想要的快捷键组合，按 Esc 取消</p>
              <button onClick={() => setShowShortcutModal(false)} className="w-full py-3 rounded-xl text-sm font-semibold text-slate-500 transition-all active:scale-95" style={{ background: "#f1f5f9" }}>取消</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
