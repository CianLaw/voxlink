import { useState, useRef, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

// 从 localStorage 读取 API Key，首次使用需在设置中配置
function getApiKey(): string | null {
  try {
    const s = JSON.parse(localStorage.getItem("voxlink_settings") || "{}");
    return s.apiKey || null;
  } catch { return null; }
}

export default function IslandApp() {
  const [isRecording, setIsRecording] = useState(false);
  const [isPolishing, setIsPolishing] = useState(false);

  const waveRef = useRef<HTMLCanvasElement>(null);
  const animRef = useRef<number>(0);
  const recognitionRef = useRef<SpeechRecognition | null>(null);
  const finalTextRef = useRef("");
  const interimTextRef = useRef("");
  const waveIntensityRef = useRef(0.05);
  const isRecRef = useRef(false);
  const silenceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // ========== 声波动画 ==========
  const startWave = useCallback(() => {
    const canvas = waveRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    let phase = 0, intensity = 0.05;
    waveIntensityRef.current = 0.05;

    const draw = () => {
      const w = 90, h = 30, dpr = window.devicePixelRatio || 1;
      canvas.width = w * dpr; canvas.height = h * dpr;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, w, h);

      const target = waveIntensityRef.current;
      intensity += (target - intensity) * 0.08;
      phase += 0.06 + intensity * 0.04;

      const grad = ctx.createLinearGradient(0, h / 2 - 8, 0, h / 2 + 8);
      grad.addColorStop(0, `rgba(100,200,255,${0.2 + intensity * 0.6})`);
      grad.addColorStop(0.5, `rgba(180,160,255,${0.3 + intensity * 0.5})`);
      grad.addColorStop(1, `rgba(255,120,200,${0.2 + intensity * 0.4})`);

      ctx.strokeStyle = grad;
      ctx.lineWidth = 1.8;
      ctx.lineCap = "round";
      ctx.beginPath();
      const cy = h / 2;
      for (let i = 0; i <= 60; i++) {
        const x = (i / 60) * w;
        const a1 = Math.sin(x * 0.08 + phase) * intensity * 10;
        const a2 = Math.sin(x * 0.15 - phase * 1.2) * intensity * 6;
        const a3 = Math.sin(x * 0.25 + phase * 0.8) * intensity * 3;
        const env = Math.exp(-Math.pow((x - w / 2) / (w * 0.35), 2));
        const y = cy + (a1 + a2 + a3) * env;
        i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y);
      }
      ctx.stroke();

      if (intensity > 0.15) {
        ctx.beginPath();
        ctx.strokeStyle = `rgba(150,180,255,${intensity * 0.25})`;
        ctx.lineWidth = 1;
        for (let i = 0; i <= 60; i++) {
          const x = (i / 60) * w;
          const a = Math.sin(x * 0.12 - phase * 0.9) * intensity * 5;
          const env = Math.exp(-Math.pow((x - w / 2) / (w * 0.4), 2));
          const y = cy + a * env;
          i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y);
        }
        ctx.stroke();
      }
      animRef.current = requestAnimationFrame(draw);
    };
    draw();
  }, []);

  const stopWave = useCallback(() => {
    if (animRef.current) cancelAnimationFrame(animRef.current);
    const c = waveRef.current;
    if (c) c.getContext("2d")?.clearRect(0, 0, c.width, c.height);
  }, []);

  // ========== 语音识别 ==========
  const startRecording = useCallback(async () => {
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      (window as any).__micStream = stream;
    } catch {
      console.error("麦克风权限被拒绝");
      return;
    }

    const SR = window.SpeechRecognition || window.webkitSpeechRecognition;
    if (!SR) return;

    const recog = new SR();
    recog.continuous = true;
    recog.interimResults = true;
    recog.maxAlternatives = 3;

    // 读取设置中的语言
    let lang = "zh-CN";
    try {
      const s = JSON.parse(localStorage.getItem("voxlink_settings") || "{}");
      if (s.lang) lang = s.lang;
    } catch { }
    recog.lang = lang;

    let acc = "";

    recog.onstart = () => {
      setIsRecording(true);
      isRecRef.current = true;
      finalTextRef.current = "";
      interimTextRef.current = "";
      waveIntensityRef.current = 0.05;
      startWave();
    };

    recog.onresult = (e: SpeechRecognitionEvent) => {
      let interim = "", best = "";
      for (let i = e.resultIndex; i < e.results.length; i++) {
        const r = e.results[i];
        let bt = r[0].transcript, bc = r[0].confidence || 0;
        for (let j = 1; j < r.length; j++) {
          if ((r[j].confidence || 0) > bc) { bc = r[j].confidence || 0; bt = r[j].transcript; }
        }
        if (r.isFinal && bc >= 0.3) best += bt;
        else interim += bt;
      }
      if (best) acc += best;
      finalTextRef.current = acc;
      interimTextRef.current = interim;
      const has = acc.length > 0 || interim.length > 0;
      waveIntensityRef.current = has ? 0.8 : 0.15;
      if (silenceTimerRef.current) clearTimeout(silenceTimerRef.current);
      silenceTimerRef.current = setTimeout(() => { waveIntensityRef.current = 0.08; }, 3000);
    };

    recog.onerror = (e: SpeechRecognitionErrorEvent) => {
      if (e.error === "no-speech" || e.error === "aborted") return;
      doStop();
    };

    recog.onend = () => { if (isRecRef.current) try { recog.start(); } catch { } };

    recognitionRef.current = recog;
    try { recog.start(); } catch { }
  }, []);

  const doStop = useCallback(() => {
    isRecRef.current = false;
    setIsRecording(false);
    waveIntensityRef.current = 0.05;
    if (silenceTimerRef.current) clearTimeout(silenceTimerRef.current);
    stopWave();
    if (recognitionRef.current) try { recognitionRef.current.stop(); } catch { }
    const s = (window as any).__micStream;
    if (s) { s.getTracks().forEach((t: MediaStreamTrack) => t.stop()); (window as any).__micStream = null; }
  }, [stopWave]);

  // ========== 确认输入 ==========
  const confirmInput = useCallback(async () => {
    const text = (finalTextRef.current + interimTextRef.current).trim();
    if (!text) { doStop(); return; }

    // 读取设置
    let shouldPolish = false;
    const apiKey = getApiKey();
    let targetModel = "gemini-2.0-flash";
    try {
      const s = JSON.parse(localStorage.getItem("voxlink_settings") || "{}");
      shouldPolish = s.aiPolish || false;
      if (s.aiModel) targetModel = s.aiModel;
    } catch { }

    doStop();

    if (shouldPolish) {
      setIsPolishing(true);
      try {
        const res = await fetch(`https://generativelanguage.googleapis.com/v1beta/models/${targetModel}:generateContent?key=${apiKey}`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            contents: [{ parts: [{ text: `你是一位专业的文字编辑。请对以下语音识别的文字进行润色和排版：\n\n要求：\n1. 去除重复词句、口头禅和冗余表达\n2. 修正语法错误，优化表达\n3. 合理分段，添加适当标点\n4. 去除语气词，将口语化转为书面语\n5. 保持原文核心意思不变\n\n原文：\n${text}\n\n请直接返回润色后的文字，不要添加任何解释。` }] }],
            generationConfig: { temperature: 0.3, maxOutputTokens: 2048 },
          }),
        });
        if (res.ok) {
          const data = await res.json();
          const polished = data.candidates?.[0]?.content?.parts?.[0]?.text?.trim();
          if (polished && polished !== text) {
            await navigator.clipboard.writeText(polished);
          } else {
            await navigator.clipboard.writeText(text);
          }
        } else {
          await navigator.clipboard.writeText(text);
        }
      } catch {
        await navigator.clipboard.writeText(text);
      }
      setIsPolishing(false);
    } else {
      await navigator.clipboard.writeText(text);
    }

    // 保存到历史
    try {
      const items = JSON.parse(localStorage.getItem("voxlink_history") || "[]");
      items.unshift({ id: Date.now(), originalText: text, createdAt: new Date().toLocaleString("zh-CN"), isPolished: shouldPolish, isTranslated: false, polishedText: null, translatedText: null });
      localStorage.setItem("voxlink_history", JSON.stringify(items.slice(0, 100)));
    } catch { }

    await invoke("cmd_hide_island");
  }, [doStop]);

  // ========== 取消 ==========
  const cancel = useCallback(() => {
    doStop();
    invoke("cmd_hide_island");
  }, [doStop]);

  // ========== 监听 Tauri 事件 ==========
  useEffect(() => {
    const { listen } = require("@tauri-apps/api/event");
    const unsubs: (() => void)[] = [];

    listen("island:show", () => { startRecording(); })
      .then((u: () => void) => unsubs.push(u));
    listen("island:hide", () => { doStop(); })
      .then((u: () => void) => unsubs.push(u));

    return () => unsubs.forEach(u => u());
  }, [startRecording, doStop]);

  return (
    <div className="w-[260px] h-[56px] flex items-center justify-center" style={{ background: "transparent" }}>
      <div className="glass-dark w-full h-full rounded-full flex items-center justify-between px-4 relative"
        style={{
          background: "linear-gradient(135deg, rgba(25,25,35,0.92) 0%, rgba(35,30,50,0.92) 50%, rgba(25,25,35,0.92) 100%)",
          backdropFilter: "blur(40px) saturate(180%)",
          border: "1px solid rgba(255,255,255,0.06)",
          boxShadow: "0 12px 40px rgba(0,0,0,0.55), inset 0 1px 0 rgba(255,255,255,0.08)",
        }}>
        {/* 取消 */}
        <button onClick={cancel}
          className="w-8 h-8 rounded-full flex items-center justify-center text-white/40 hover:text-white/80 transition-all hover:scale-90 active:scale-75 flex-shrink-0 relative z-[2]"
          style={{ background: "rgba(255,255,255,0.06)" }}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        </button>

        {/* 声波 or 润色 */}
        {isPolishing ? (
          <div className="flex items-center gap-2 relative z-[2]">
            <div className="w-4 h-4 rounded-full border-2 border-white/10 border-t-green-400" style={{ animation: "spin 0.8s linear infinite" }} />
            <span className="text-white/60 text-xs">润色中</span>
          </div>
        ) : (
          <canvas ref={waveRef} width={90} height={30} style={{ width: 90, height: 30 }} className="relative z-[2]" />
        )}

        {/* 确认 */}
        {!isPolishing && (
          <button onClick={confirmInput}
            className="w-8 h-8 rounded-full flex items-center justify-center transition-all hover:scale-90 active:scale-75 flex-shrink-0 relative z-[2]"
            style={{ background: "rgba(52,199,89,0.15)", color: "#34C759" }}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"><polyline points="20 6 9 17 4 12"/></svg>
          </button>
        )}
      </div>
    </div>
  );
}
