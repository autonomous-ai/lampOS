import { SettingsSelect } from "@/components/SettingsSelect";
import { useEffect, useState } from "react";
import { C, LockedField, LockedPasswordField, SectionCard } from "@/components/setup/shared";
import { getRealtimeOptions } from "@/lib/api";
import type { LlmLoadedState } from "@/hooks/setup/types";

// Keep these lists in sync with system/server/config/realtime.go (ValidateRealtimeKnobs) and the HAL enums.
const PROVIDERS = ["gemini", "openai", "gptlive", "pipecat_v1", "none"];

const PROVIDER_LABEL: Record<string, string> = {
  gemini: "Gemini",
  openai: "OpenAI",
  gptlive: "GPT-Live",
  pipecat_v1: "Pipecat v1 (on-device)",
  none: "None",
};
const displayProvider = (v: string): string =>
  PROVIDER_LABEL[v] ?? (v ? v[0].toUpperCase() + v.slice(1) : v);
const VOICES: Record<string, string[]> = {
  gemini: [
    "Puck", "Charon", "Kore", "Fenrir", "Aoede", "Zephyr", "Leda", "Orus",
    "Callirrhoe", "Autonoe", "Enceladus", "Iapetus", "Umbriel", "Algieba",
    "Despina", "Erinome", "Algenib", "Rasalgethi", "Laomedeia", "Achernar",
    "Alnilam", "Schedar", "Gacrux", "Pulcherrima", "Achird", "Zubenelgenubi",
    "Vindemiatrix", "Sadachbia", "Sadaltager", "Sulafat",
  ],
  openai: ["alloy", "ash", "coral", "echo", "fable", "onyx", "nova", "sage", "shimmer"],
  // gpt-live-1 voices only; Realtime-only names (alloy/ash) are rejected at session.start.
  gptlive: [
    "marin", "quartz", "ripple", "vesper", "willow", "stone", "gleam",
    "meridian", "bossa", "tempo", "beacon", "delta", "cinder",
  ],
  pipecat_v1: [],
};
const REASONING: Record<string, string[]> = {
  gemini: ["MINIMAL", "LOW", "MEDIUM", "HIGH"],
  openai: ["minimal", "low", "medium", "high", "xhigh"],
  gptlive: [],
  pipecat_v1: [],
};

export interface RealtimeLoadedState {
  apiKey: boolean;
}

// Seconds; keep within FastVoicePauseMinS..MaxS in system/server/config/realtime.go.
const FAST_VOICE_PAUSE_CHOICES_S = [1, 1.5, 2, 3, 4, 6];
// Seconds; keep within FastVoiceSilentTimeoutMinS..MaxS.
const FAST_VOICE_SILENT_CHOICES_S = [3, 4, 5, 6, 8];

// Keep a saved value outside the presets selectable.
const withValue = (choices: number[], v: number) =>
  choices.includes(v) ? choices : [...choices, v].sort((a, b) => a - b);

const selectStyle = {
  width: "100%", boxSizing: "border-box" as const,
  background: C.surface, border: `1px solid ${C.border}`,
  borderRadius: 7, padding: "8px 11px",
  fontSize: 12.5, color: C.text, outline: "none", cursor: "pointer",
};

const labelStyle = { display: "block", fontSize: 12, color: C.textDim, marginBottom: 5 };

export function RealtimeSection({
  active,
  realtimeLoaded, llmLoaded,
  enabled, setEnabled,
  provider, setProvider,
  voice, setVoice,
  reasoning, setReasoning,
  apiKey, setApiKey,
  baseUrl, setBaseUrl,
  webSearch, setWebSearch,
  fastVoice, setFastVoice,
  fastVoicePauseS, setFastVoicePauseS,
  fastVoiceSilentS, setFastVoiceSilentS,
}: {
  active: boolean;
  realtimeLoaded: RealtimeLoadedState;
  llmLoaded: LlmLoadedState;
  enabled: boolean; setEnabled: (v: boolean) => void;
  provider: string; setProvider: (v: string) => void;
  voice: string; setVoice: (v: string) => void;
  reasoning: string; setReasoning: (v: string) => void;
  apiKey: string; setApiKey: (v: string) => void;
  baseUrl: string; setBaseUrl: (v: string) => void;
  webSearch: boolean; setWebSearch: (v: boolean) => void;
  fastVoice: boolean; setFastVoice: (v: boolean) => void;
  fastVoicePauseS: number; setFastVoicePauseS: (v: number) => void;
  fastVoiceSilentS: number; setFastVoiceSilentS: (v: number) => void;
}) {
  // Options come from the API; the const lists above are only a fallback.
  const [opts, setOpts] = useState<{ providers: string[]; voices: Record<string, string[]>; reasoning: Record<string, string[]> } | null>(null);
  useEffect(() => { getRealtimeOptions().then(setOpts).catch(() => {}); }, []);
  const providers = opts?.providers ?? PROVIDERS;
  const voices = (opts?.voices ?? VOICES)[provider] ?? [];
  const reasonings = (opts?.reasoning ?? REASONING)[provider] ?? [];

  // Switching provider resets voice/reasoning to that provider's defaults so we never submit, e.g., an OpenAI voice while provider=gemini (server rejects it).
  function onProviderChange(p: string) {
    setProvider(p);
    if (p === "none") return;
    if (!(VOICES[p] ?? []).includes(voice)) setVoice((VOICES[p] ?? [])[0] ?? "");
    if (!(REASONING[p] ?? []).includes(reasoning)) setReasoning((REASONING[p] ?? [])[0] ?? "");
  }

  return (
    <SectionCard id="realtime" title="Realtime" active={active}>
      <label style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 12, cursor: "pointer", fontSize: 12.5, color: C.text }}>
        <input type="checkbox" checked={enabled} onChange={(e) => setEnabled(e.target.checked)} style={{ flexShrink: 0 }} />
        <span style={{ whiteSpace: "nowrap" }}>Enable realtime voice</span>
      </label>
      <div style={{ marginBottom: 12 }}>
        <label htmlFor="realtime_provider" style={labelStyle}>Provider</label>
        <SettingsSelect id="realtime_provider" value={provider} onValueChange={(value) => onProviderChange(value)} style={selectStyle}>
          {providers.map((p) => <option key={p} value={p}>{displayProvider(p)}</option>)}
        </SettingsSelect>
      </div>

      {provider !== "none" && (
        <>

          {reasonings.length > 0 && (
            <div style={{ marginBottom: 12 }}>
              <label htmlFor="realtime_reasoning" style={labelStyle}>Reasoning (cost — cheapest first)</label>
              <SettingsSelect id="realtime_reasoning" value={reasoning} onValueChange={(value) => setReasoning(value)} style={selectStyle}>
                {reasonings.map((r) => <option key={r} value={r}>{r}</option>)}
              </SettingsSelect>
            </div>
          )}

          {provider === "gemini" && (
            <label htmlFor="realtime_fast_voice" style={{ display: "flex", alignItems: "flex-start", gap: 8, fontSize: 12.5, color: C.text, cursor: "pointer", marginBottom: 12 }}>
              <input id="realtime_fast_voice" type="checkbox" checked={fastVoice} onChange={(e) => setFastVoice(e.target.checked)} style={{ marginTop: 3, flexShrink: 0 }} />
              <span style={{ minWidth: 0 }}>
                <span style={{ whiteSpace: "nowrap" }}>Faster replies</span>
                <span style={{ display: "block", fontSize: 12, color: C.textDim, marginTop: 3 }}>
                  Answers usually start within 1.5–3 s instead of 5–10 s: your speech is sent while you are still talking, the end of your turn is detected sooner, and replies play in the Gemini voice chosen below. Main-agent replies keep the Voice page&apos;s voice.
                </span>
                <span style={{ display: "block", fontSize: 12, color: C.textDim, marginTop: 3 }}>
                  Uses more Gemini tokens. A long pause mid-sentence may end your turn early; set how long to wait below.
                </span>
              </span>
            </label>
          )}
          {provider === "gemini" && fastVoice && (
            <div style={{ marginBottom: 12 }}>
              <label htmlFor="realtime_fast_voice_pause" style={labelStyle}>Wait after you pause</label>
              <SettingsSelect id="realtime_fast_voice_pause" value={String(fastVoicePauseS)} onValueChange={(value) => setFastVoicePauseS(Number(value))} style={selectStyle}>
                {withValue(FAST_VOICE_PAUSE_CHOICES_S, fastVoicePauseS).map((s) => <option key={s} value={String(s)}>{s} seconds</option>)}
              </SettingsSelect>
              <p style={{ fontSize: 12, color: C.textDim, marginTop: 5, marginBottom: 0 }}>
                How long the device waits when you pause mid-sentence before it answers. Shorter answers sooner but may cut you off.
              </p>
            </div>
          )}
          {provider === "gemini" && fastVoice && (
            <div style={{ marginBottom: 12 }}>
              <label htmlFor="realtime_fast_voice_silent" style={labelStyle}>Hand off when Gemini stays silent</label>
              <SettingsSelect id="realtime_fast_voice_silent" value={String(fastVoiceSilentS)} onValueChange={(value) => setFastVoiceSilentS(Number(value))} style={selectStyle}>
                {withValue(FAST_VOICE_SILENT_CHOICES_S, fastVoiceSilentS).map((s) => <option key={s} value={String(s)}>after {s} seconds</option>)}
              </SettingsSelect>
              <p style={{ fontSize: 12, color: C.textDim, marginTop: 5, marginBottom: 0 }}>
                If Gemini sends nothing for this long, the main agent answers instead. A search that is still working is not cut off.
              </p>
            </div>
          )}
          {/* Gemini's own voice speaks only with Faster replies; otherwise the Voice page does. */}
          <div style={{ marginBottom: 12, display: provider === "gemini" && fastVoice ? "block" : "none" }}>
            <label htmlFor="realtime_voice" style={labelStyle}>Voice</label>
            <SettingsSelect id="realtime_voice" value={voice} onValueChange={(value) => setVoice(value)} style={selectStyle}>
              {voices.map((v) => <option key={v} value={v}>{v}</option>)}
            </SettingsSelect>
            <p style={{ fontSize: 12, color: C.textDim, marginTop: 5, marginBottom: 0 }}>
              The voice Faster replies speak in.
            </p>
          </div>

          <LockedPasswordField lockedInitially={realtimeLoaded.apiKey || llmLoaded.apiKey} label="API Key (optional)" id="realtime_api_key" value={apiKey} onChange={setApiKey} placeholder="sk-... / AIza..." />
          <p style={{ fontSize: 12, color: C.textDim, marginTop: -8, marginBottom: 12 }}>
            Leave blank to reuse the AI brain key.
          </p>
          {provider === "pipecat_v1" ? (
            <>
              <div style={{ fontSize: 12, color: C.textDim, marginBottom: 12 }}>
                Runs the voice pipeline on the robot (its own STT + a text LLM, spoken by the TTS voice above).
                LLM endpoint: <code>realtime.pipecat_v1.base_url</code> in config.json — default is the low-latency Qwen relay.
              </div>
              <label style={{ display: "flex", alignItems: "flex-start", gap: 8, fontSize: 12.5, color: C.text, cursor: "pointer", marginBottom: 12 }}>
                <input type="checkbox" checked={webSearch} onChange={(e) => setWebSearch(e.target.checked)} style={{ marginTop: 3, flexShrink: 0 }} />
                <span style={{ minWidth: 0 }}>
                  <span style={{ whiteSpace: "nowrap" }}>Web search</span>
                  <span style={{ display: "block", fontSize: 12, color: C.textDim, marginTop: 3 }}>
                    Answer public live facts (weather, news, scores, prices) in-session through the Google-Search relay.
                  </span>
                  <span style={{ display: "block", fontSize: 12, color: C.textDim, marginTop: 3 }}>
                    Off: those questions are delegated to the main agent instead. One relay call (~4 s) per lookup.
                  </span>
                </span>
              </label>
            </>
          ) : (
            <>
              <LockedField lockedInitially={llmLoaded.baseUrl} label="Base URL (optional)" id="realtime_base_url" value={baseUrl} onChange={setBaseUrl} placeholder="wss://… /ws/gemini" />
              <p style={{ fontSize: 12, color: C.textDim, marginTop: -8, marginBottom: 12 }}>
                Leave blank to derive from the AI brain base URL.
              </p>
            </>
          )}
        </>
      )}
    </SectionCard>
  );
}
