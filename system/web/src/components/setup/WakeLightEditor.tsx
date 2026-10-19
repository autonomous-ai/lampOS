import { useEffect, useRef, useState } from "react";
import { hwUrl, type WakeLightEffect } from "@/lib/api";
import { fromHsv, swatch, toHsv, type RGB, type WakeLook } from "@/lib/ledColor";
import { C, HINT_STYLE, LABEL_STYLE } from "./shared";

const COLORS: { label: string; color: RGB }[] = [
  { label: "Listening blue", color: [4, 8, 16] },
  { label: "Warm amber", color: [16, 9, 3] },
  { label: "Mint", color: [3, 14, 8] },
  { label: "Violet", color: [10, 4, 16] },
  { label: "Soft white", color: [10, 10, 10] },
];

const EFFECTS: { value: WakeLightEffect; label: string }[] = [
  { value: "solid", label: "Steady" },
  { value: "breathing", label: "Breathing" },
  { value: "spin", label: "Spin" },
  { value: "blink", label: "Blink" },
];

// A cue may be brighter than the resting glow, but stays well under a reading lamp.
const MAX_LEVEL = 128;
const SPEED_MIN = 0.25;
const SPEED_MAX = 3;
const SLIDER_STYLE: React.CSSProperties = { width: "100%", accentColor: "var(--lm-amber)" };

// Rough on-screen period of each effect at `speed`, mirroring hal/drivers/rgb/effects.py.
function animation(effect: WakeLightEffect, speed: number): string {
  if (effect === "breathing") return `wl-breathe ${(3 / speed).toFixed(2)}s ease-in-out infinite`;
  if (effect === "blink") return `wl-blink ${(1 / (3 * speed)).toFixed(2)}s steps(1) infinite`;
  if (effect === "spin") return `wl-spin ${(0.96 / speed).toFixed(2)}s linear infinite`;
  return "none";
}

function sameColor(a: RGB, b: RGB): boolean {
  return a[0] === b[0] && a[1] === b[1] && a[2] === b[2];
}

export function WakeLightEditor({ look, onChange }: { look: WakeLook; onChange: (v: WakeLook) => void }) {
  const initial = toHsv(look.color);
  const [hue, setHue] = useState(Math.round(initial.h));
  const [sat, setSat] = useState(Math.round(initial.s * 100) / 100);
  const [level, setLevel] = useState(Math.max(1, Math.min(MAX_LEVEL, initial.v)));
  const [deviceNote, setDeviceNote] = useState("");
  const timer = useRef<number | null>(null);

  // Play each change on the device once the owner pauses, like the resting light does.
  useEffect(() => {
    if (timer.current) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      fetch(hwUrl("/led/wake-light/preview"), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(look),
      })
        .then((r) => (r.ok ? r.json() : Promise.reject(new Error(String(r.status)))))
        .then((r: { painted: boolean }) => setDeviceNote(r.painted ? "" : "The light is busy right now (speaking, music or sleep), so the device preview was skipped."))
        .catch(() => setDeviceNote("Could not preview on the device."));
    }, 300);
    return () => { if (timer.current) window.clearTimeout(timer.current); };
  }, [look]);

  const pickColor = (color: RGB) => {
    const hsv = toHsv(color);
    setHue(Math.round(hsv.h));
    setSat(Math.round(hsv.s * 100) / 100);
    setLevel(Math.max(1, Math.min(MAX_LEVEL, hsv.v)));
    onChange({ ...look, color });
  };

  const onSlider = (h: number, s: number, l: number) => {
    setHue(h); setSat(s); setLevel(l);
    const color = fromHsv(h, s, l);
    // Very low levels can round every channel to zero; keep at least a glimmer.
    onChange({ ...look, color: Math.max(...color) > 0 ? color : [1, 1, 1] });
  };

  const chip = (key: string, selected: boolean, onClick: () => void, children: React.ReactNode) => (
    <button
      key={key}
      type="button"
      onClick={onClick}
      aria-pressed={selected}
      style={{
        display: "flex", alignItems: "center", gap: 8,
        padding: "6px 11px", borderRadius: 999, cursor: "pointer",
        background: selected ? C.amberDim : C.surface,
        border: `1px solid ${selected ? C.amber : C.border}`,
        color: selected ? C.amber : C.text, fontSize: 12.5,
      }}
    >
      {children}
    </button>
  );

  const dot = (color: RGB) => (
    <span style={{ width: 14, height: 14, borderRadius: "50%", flexShrink: 0, background: swatch(color), border: `1px solid ${C.border}` }} />
  );

  const fullColor = swatch(look.color);

  return (
    <div style={{ marginTop: 10, padding: 14, borderRadius: 10, border: `1px solid ${C.border}`, background: C.surface }}>
      <style>{`
        @keyframes wl-breathe { 0%, 100% { opacity: 1 } 50% { opacity: .15 } }
        @keyframes wl-blink { 0% { opacity: 1 } 50% { opacity: 0 } }
        @keyframes wl-spin { to { transform: rotate(360deg) } }
        @media (prefers-reduced-motion: reduce) { .wl-preview { animation: none !important } }
      `}</style>

      <div style={{ display: "flex", alignItems: "center", gap: 14, marginBottom: 14 }}>
        <span
          className="wl-preview"
          aria-hidden
          style={{
            width: 44, height: 44, borderRadius: "50%", flexShrink: 0,
            background: look.effect === "spin"
              ? `conic-gradient(${fullColor} 0deg 60deg, transparent 120deg 360deg)`
              : fullColor,
            boxShadow: look.effect === "spin" ? `inset 0 0 0 3px ${C.border}` : `0 0 14px ${fullColor}`,
            animation: animation(look.effect, look.speed),
          }}
        />
        <div style={HINT_STYLE}>
          Changes play on the device for a few seconds so you can see them.
          They are kept when you save.
          {deviceNote && <div style={{ color: C.amber, marginTop: 4 }}>{deviceNote}</div>}
        </div>
      </div>

      <div style={LABEL_STYLE}>Colour</div>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 8, marginBottom: 14 }}>
        {COLORS.map((c) => chip(c.label, sameColor(look.color, c.color), () => pickColor(c.color), <>{dot(c.color)}{c.label}</>))}
      </div>

      <div style={{ display: "grid", gap: 10, marginBottom: 16 }}>
        <div>
          <label htmlFor="wake-light-hue" style={{ fontSize: 12, color: C.textMuted }}>Hue</label>
          <input id="wake-light-hue" type="range" min={0} max={359} value={hue}
            onChange={(e) => onSlider(Number(e.target.value), sat, level)} style={SLIDER_STYLE} />
          <div aria-hidden style={{ height: 6, borderRadius: 3, marginTop: 2, background: "linear-gradient(to right, #f00, #ff0, #0f0, #0ff, #00f, #f0f, #f00)" }} />
        </div>
        <div>
          <label htmlFor="wake-light-sat" style={{ fontSize: 12, color: C.textMuted }}>White ↔ colour</label>
          <input id="wake-light-sat" type="range" min={0} max={100} value={Math.round(sat * 100)}
            onChange={(e) => onSlider(hue, Number(e.target.value) / 100, level)} style={SLIDER_STYLE} />
        </div>
        <div>
          <label htmlFor="wake-light-level" style={{ fontSize: 12, color: C.textMuted }}>
            Brightness ({Math.round((level / MAX_LEVEL) * 100)}%)
          </label>
          <input id="wake-light-level" type="range" min={1} max={MAX_LEVEL} value={level}
            onChange={(e) => onSlider(hue, sat, Number(e.target.value))} style={SLIDER_STYLE} />
        </div>
      </div>

      <div style={LABEL_STYLE}>Effect</div>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 8, marginBottom: 14 }}>
        {EFFECTS.map((e) => chip(e.value, look.effect === e.value, () => onChange({ ...look, effect: e.value }), e.label))}
      </div>

      {look.effect !== "solid" && (
        <div>
          <label htmlFor="wake-light-speed" style={{ fontSize: 12, color: C.textMuted }}>
            Speed ({look.speed.toFixed(2).replace(/\.?0+$/, "")}×)
          </label>
          <input id="wake-light-speed" type="range" min={SPEED_MIN} max={SPEED_MAX} step={0.25} value={look.speed}
            onChange={(e) => onChange({ ...look, speed: Number(e.target.value) })} style={SLIDER_STYLE} />
        </div>
      )}

      <div style={{ marginTop: 10, fontSize: 11.5, color: C.textMuted, fontFamily: "monospace" }}>
        RGB({look.color.join(", ")})
      </div>
    </div>
  );
}
