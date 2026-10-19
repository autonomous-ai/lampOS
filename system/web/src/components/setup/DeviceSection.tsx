import { useState } from "react";
import { toast } from "sonner";
import { Eye, EyeOff, Copy, Check, Cpu, Fingerprint, Network, X } from "lucide-react";
import { SecretUpdateField } from "@/components/SecretUpdateField";
import { WAKE_FOLLOWUP_CHOICES_S, WAKE_FOLLOWUP_DEFAULT_S, type VoiceInputMode } from "@/lib/api";
import { WAKE_LOOK_DEFAULT, swatch, type WakeLook } from "@/lib/ledColor";
import { WakeLightEditor } from "./WakeLightEditor";
import { copyText } from "@/lib/clipboard";
import { C, Field, PasswordField, SectionCard, LABEL_STYLE, HINT_STYLE, INPUT_STYLE, INPUT_READONLY_STYLE, INPUT_PAD_ONE_ICON, FIELD_GAP, ADMIN_PASSWORD_MIN } from "./shared";

// Mirrors config.NormalizeWakePhrases on the server.
const MAX_WAKE_PHRASES = 30;
const MAX_WAKE_PHRASE_CHARS = 40;

function normalizePhrase(raw: string): string {
  return raw.trim().split(/\s+/).join(" ").toLowerCase();
}

// Editable wake phrases: chips with remove, an add field, and restore-defaults.
// The chime and the light are the two ways listening is noticed (after a wake
// phrase, or when a tap-to-talk recording starts); at least one stays on.
function ListenCues({ id, chimeLabel, lightLabel, chime, setChime, lightOn, setLightOn, look, setLook }: {
  id: "wake" | "tap";
  chimeLabel: string; lightLabel: string;
  chime: boolean; setChime: (v: boolean) => void;
  lightOn: boolean; setLightOn: (v: boolean) => void;
  look: WakeLook; setLook: (v: WakeLook) => void;
}) {
  const [open, setOpen] = useState(false);
  const row: React.CSSProperties = { display: "flex", alignItems: "center", gap: 8, fontSize: 13, color: C.text };
  return (
    <div style={{ marginTop: 12, display: "grid", gap: 8 }}>
      <label htmlFor={`${id}_chime`} style={{ ...row, cursor: lightOn ? "pointer" : "not-allowed" }}>
        <input id={`${id}_chime`} type="checkbox" checked={chime} disabled={chime && !lightOn}
          onChange={(e) => setChime(e.target.checked)} style={{ flexShrink: 0 }} />
        {chimeLabel}
      </label>
      <div style={{ ...row, flexWrap: "wrap" }}>
        <label htmlFor={`${id}_light`} style={{ ...row, cursor: chime ? "pointer" : "not-allowed" }}>
          <input id={`${id}_light`} type="checkbox" checked={lightOn} disabled={lightOn && !chime}
            onChange={(e) => { setLightOn(e.target.checked); if (!e.target.checked) setOpen(false); }} style={{ flexShrink: 0 }} />
          {lightLabel}
        </label>
        {lightOn && (
          <button
            type="button"
            onClick={() => setOpen((o) => !o)}
            aria-expanded={open}
            aria-controls={`${id}-light-editor`}
            style={{
              display: "flex", alignItems: "center", gap: 6, padding: "3px 10px", borderRadius: 999,
              background: open ? C.amberDim : C.surface, border: `1px solid ${open ? C.amber : C.border}`,
              color: open ? C.amber : C.text, fontSize: 12, cursor: "pointer",
            }}
          >
            <span style={{ width: 12, height: 12, borderRadius: "50%", background: swatch(look.color), border: `1px solid ${C.border}` }} />
            {open ? "Done" : "Customize light"}
          </button>
        )}
      </div>
      {(!chime || !lightOn) && (
        <div style={HINT_STYLE}>
          Keep the chime or the light on, so you can tell the device heard you.
        </div>
      )}
      {lightOn && open && (
        <div id={`${id}-light-editor`}>
          <WakeLightEditor look={look} onChange={setLook} />
        </div>
      )}
    </div>
  );
}

function WakeFollowupSelect({ value, onChange }: { value: number; onChange: (v: number) => void }) {
  // Keep a value saved outside the presets (e.g. via the API) selectable.
  const choices = WAKE_FOLLOWUP_CHOICES_S.includes(value)
    ? WAKE_FOLLOWUP_CHOICES_S
    : [...WAKE_FOLLOWUP_CHOICES_S, value].sort((a, b) => a - b);
  return (
    <div style={{ marginTop: 12 }}>
      <label htmlFor="wake_followup_s" style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 13, color: C.text }}>
        Keep listening for
        <select
          id="wake_followup_s"
          value={value}
          onChange={(e) => onChange(Number(e.target.value))}
          style={{ ...INPUT_STYLE, width: "auto", padding: "4px 8px", fontSize: 13 }}
        >
          {choices.map((s) => (
            <option key={s} value={s}>{s} seconds</option>
          ))}
        </select>
      </label>
      <div style={{ marginTop: 4 }}>
        After a wake or a reply, you can keep talking without the wake phrase for this long.
      </div>
    </div>
  );
}

function WakePhraseEditor({ phrases, setPhrases, defaults, nameAnywhere = false }: {
  phrases: string[];
  setPhrases: (v: string[]) => void;
  defaults: string[];
  // Entries are names heard anywhere in a sentence, so a single word is expected.
  nameAnywhere?: boolean;
}) {
  const noun = nameAnywhere ? "name" : "phrase";
  const [draft, setDraft] = useState("");
  const [error, setError] = useState("");
  const isDefault = phrases.length === defaults.length && phrases.every((p, i) => p === defaults[i]);
  const singleWord = nameAnywhere ? [] : phrases.filter((p) => !p.includes(" "));

  const add = () => {
    const phrase = normalizePhrase(draft);
    if (!phrase) return;
    if ([...phrase].length > MAX_WAKE_PHRASE_CHARS) return setError(`Keep it under ${MAX_WAKE_PHRASE_CHARS} characters.`);
    if (!/\p{L}/u.test(phrase)) return setError(`A wake ${noun} needs at least one letter.`);
    if (phrases.includes(phrase)) return setError(`That ${noun} is already in the list.`);
    if (phrases.length >= MAX_WAKE_PHRASES) return setError(`At most ${MAX_WAKE_PHRASES} ${noun}s.`);
    setPhrases([...phrases, phrase]);
    setDraft("");
    setError("");
  };

  return (
    <div>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
        {phrases.map((phrase) => (
          <span key={phrase} style={{ display: "inline-flex", alignItems: "center", gap: 4, padding: "3px 4px 3px 7px", borderRadius: 5, background: C.surface, border: `1px solid ${C.border}`, color: C.text, fontFamily: "monospace" }}>
            {phrase}
            <button type="button" aria-label={`Remove ${phrase}`} onClick={() => setPhrases(phrases.filter((p) => p !== phrase))}
              style={{ display: "inline-flex", padding: 2, border: 0, background: "transparent", color: C.textMuted, cursor: "pointer" }}>
              <X size={12} />
            </button>
          </span>
        ))}
        {phrases.length === 0 && (
          <span style={{ color: C.textMuted }}>No {noun}s. The built-in list is used after you save.</span>
        )}
      </div>
      <div style={{ display: "flex", gap: 8, marginTop: 10, flexWrap: "wrap" }}>
        <input id="wake_phrase_new" value={draft} placeholder="e.g. sam"
          onChange={(e) => { setDraft(e.target.value); setError(""); }}
          onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); add(); } }}
          style={{ ...INPUT_STYLE, flex: "1 1 200px", padding: "7px 10px", fontSize: 13 }} />
        <button type="button" onClick={add} disabled={!normalizePhrase(draft)}
          style={{ padding: "7px 14px", borderRadius: 8, border: `1px solid ${C.amber}`, background: C.amberDim, color: C.amber, fontSize: 13, cursor: "pointer", opacity: normalizePhrase(draft) ? 1 : 0.5 }}>
          Add
        </button>
        <button type="button" onClick={() => { setPhrases(defaults); setError(""); }} disabled={isDefault || defaults.length === 0}
          style={{ padding: "7px 12px", borderRadius: 8, border: `1px solid ${C.border}`, background: "transparent", color: C.textDim, fontSize: 13, cursor: isDefault ? "default" : "pointer", opacity: isDefault ? 0.5 : 1 }}>
          Restore defaults
        </button>
      </div>
      {error && <div role="alert" style={{ marginTop: 6, color: C.red }}>{error}</div>}
      {singleWord.length > 0 && (
        <div style={{ marginTop: 6, color: C.yellow }}>
          Single words ({singleWord.join(", ")}) can wake the device by accident in normal conversation.
        </div>
      )}
    </div>
  );
}

// Read-only MAC field masked behind ••••, with an eye toggle to reveal.
function MaskedReadField({ label, id, value }: {
  label: string; id: string; value: string;
}) {
  const [show, setShow] = useState(false);
  const displayed = show ? value : "•".repeat(Math.min(12, value.length || 8));
  return (
    <div style={{ marginBottom: FIELD_GAP }}>
      <label htmlFor={id} style={LABEL_STYLE}>{label}</label>
      <div style={{ position: "relative" }}>
        <input
          id={id} type="text" value={displayed} readOnly
          style={{
            ...INPUT_STYLE,
            ...INPUT_READONLY_STYLE,
            padding: INPUT_PAD_ONE_ICON,
            fontFamily: "ui-monospace, monospace",
          }}
        />
        <button
          type="button" onClick={() => setShow((v) => !v)} tabIndex={-1}
          className="lm-eye-btn"
          aria-label={show ? "Hide MAC" : "Show MAC"}
          style={{
            position: "absolute", right: 5, top: "50%", transform: "translateY(-50%)",
            height: 32, width: 32, padding: 0, background: "none", border: "none",
            cursor: "pointer", display: "flex", alignItems: "center", justifyContent: "center",
          }}
        >
          {show ? <EyeOff size={14} /> : <Eye size={14} />}
        </button>
      </div>
    </div>
  );
}

// Advisory password strength meter under the admin password input.
function PasswordStrength({ value }: { value: string }) {
  if (!value) return null;
  const tooShort = value.length < ADMIN_PASSWORD_MIN;
  if (tooShort) {
    return (
      <StrengthRow level={-1} color={C.red}
        message={`At least ${ADMIN_PASSWORD_MIN} characters needed (${value.length}/${ADMIN_PASSWORD_MIN}).`} />
    );
  }
  const classes = [/[a-z]/, /[A-Z]/, /[0-9]/, /[^a-zA-Z0-9]/].filter((re) => re.test(value)).length;
  let score = 0;
  if (value.length >= 12) score += 1;
  if (classes >= 2) score += 1;
  if (classes >= 3) score += 1;
  const level = score === 0 ? 0 : score <= 2 ? 1 : 2;
  const messages = [
    "A bit simple — add a capital letter, number, or symbol to make it stronger.",
    "Good — add a symbol or make it longer for a stronger password.",
    "Strong password.",
  ];
  const colors = [C.yellow, C.yellow, C.green];
  return <StrengthRow level={level} color={colors[level]} message={messages[level]} />;
}

// 3-segment strength bar + hint; level -1 = invalid.
function StrengthRow({ level, color, message }: { level: number; color: string; message: string }) {
  const filled = level + 1;
  return (
    <div style={{ marginTop: -4, marginBottom: 12 }}>
      <div style={{ display: "flex", gap: 4 }}>
        {[0, 1, 2].map((i) => (
          <div key={i} style={{
            flex: 1, height: 3, borderRadius: 2,
            background: i < filled ? color : C.border,
            transition: "background 0.2s",
          }} />
        ))}
      </div>
      <div style={{ fontSize: 12, color, marginTop: 5 }}>
        {message}
      </div>
    </div>
  );
}

function MetaRow({ icon, label, children }: { icon: React.ReactNode; label: string; children: React.ReactNode }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "11px 14px", minHeight: 44 }}>
      <span style={{ flexShrink: 0, color: C.textMuted, display: "flex" }}>{icon}</span>
      <span style={{ flexShrink: 0, fontSize: 12.5, color: C.textDim, width: 84 }}>{label}</span>
      <div style={{ flex: 1, minWidth: 0, display: "flex", alignItems: "center", justifyContent: "flex-end", gap: 8 }}>
        {children}
      </div>
    </div>
  );
}

function DeviceMetaCard({ deviceId, mac }: { deviceId: string; mac?: string }) {
  const [copied, setCopied] = useState(false);
  const [showMac, setShowMac] = useState(false);
  const copyId = async () => {
    const ok = await copyText(deviceId);
    if (ok) {
      setCopied(true);
      toast.success("Device ID copied");
      setTimeout(() => setCopied(false), 1400);
    } else {
      toast.error("Copy failed — please copy the Device ID manually");
    }
  };
  const maskedMac = showMac ? mac : "•".repeat(Math.min(14, (mac?.length ?? 0) || 8));
  const valueText: React.CSSProperties = {
    fontFamily: "ui-monospace, monospace", fontSize: 13, color: C.text,
    overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap",
  };
  const iconBtn: React.CSSProperties = {
    flexShrink: 0, height: 28, width: 28, padding: 0, borderRadius: 7,
    background: "transparent", border: "none", cursor: "pointer",
    display: "flex", alignItems: "center", justifyContent: "center",
    color: C.textMuted, transition: "background 0.15s, color 0.15s",
  };
  return (
    <div style={{ marginTop: 4 }}>
      <div style={{ ...LABEL_STYLE, marginBottom: 8 }}>Device info</div>
      <div style={{
        border: `1px solid ${C.border}`, borderRadius: 12,
        background: C.surface, overflow: "hidden",
      }}>
        <MetaRow icon={<Fingerprint size={15} />} label="Device ID">
          <span style={valueText} title={deviceId}>{deviceId || "—"}</span>
          {deviceId && (
            <button
              type="button" onClick={copyId} tabIndex={-1}
              className="lm-eye-btn" style={iconBtn}
              aria-label="Copy Device ID" title={copied ? "Copied!" : "Copy"}
            >
              {copied ? <Check size={14} style={{ color: C.green }} /> : <Copy size={14} />}
            </button>
          )}
        </MetaRow>
        {mac && (
          <>
            <div style={{ height: 1, background: C.border }} />
            <MetaRow icon={<Network size={15} />} label="MAC">
              <span style={valueText} title={showMac ? mac : undefined}>{maskedMac}</span>
              <button
                type="button" onClick={() => setShowMac((v) => !v)} tabIndex={-1}
                className="lm-eye-btn" style={iconBtn}
                aria-label={showMac ? "Hide MAC" : "Show MAC"} title={showMac ? "Hide" : "Reveal"}
              >
                {showMac ? <EyeOff size={14} /> : <Eye size={14} />}
              </button>
            </MetaRow>
          </>
        )}
      </div>
    </div>
  );
}

export function DeviceSection({
  active, deviceId, setDeviceId, mac,
  adminPassword, setAdminPassword,
  adminPasswordConfirm, setAdminPasswordConfirm,
  rotateAdminPassword, setRotateAdminPassword,
  voiceInputMode = "automatic", setVoiceInputMode,
  wakeWord, setWakeWord,
  agentName, wakePhrases, setWakePhrases, wakePhrasesDefault, wakeNameAnywhere = false, wakeChime, setWakeChime,
  wakeFollowupS, setWakeFollowupS,
  wakeLightOn, setWakeLightOn, wakeLook, setWakeLook,
  tapChime, setTapChime, tapLightOn, setTapLightOn, tapLook, setTapLook,
}: {
  active: boolean;
  deviceId: string;
  setDeviceId: (v: string) => void;
  mac?: string;
  adminPassword?: string;
  setAdminPassword?: (v: string) => void;
  adminPasswordConfirm?: string;
  setAdminPasswordConfirm?: (v: string) => void;
  // Empty means keep the existing password.
  rotateAdminPassword?: string;
  setRotateAdminPassword?: (v: string) => void;
  voiceInputMode?: VoiceInputMode;
  setVoiceInputMode?: (v: VoiceInputMode) => void;
  wakeWord?: boolean;
  setWakeWord?: (v: boolean) => void;
  agentName?: string;
  wakePhrases?: string[];
  // Present only where the phrases can be edited (Settings).
  setWakePhrases?: (v: string[]) => void;
  wakePhrasesDefault?: string[];
  wakeNameAnywhere?: boolean;
  wakeChime?: boolean;
  setWakeChime?: (v: boolean) => void;
  wakeFollowupS?: number;
  setWakeFollowupS?: (v: number) => void;
  wakeLightOn?: boolean;
  setWakeLightOn?: (v: boolean) => void;
  wakeLook?: WakeLook;
  setWakeLook?: (v: WakeLook) => void;
  // Tap-to-talk cues, separate from the wake cues; present only in Settings.
  tapChime?: boolean;
  setTapChime?: (v: boolean) => void;
  tapLightOn?: boolean;
  setTapLightOn?: (v: boolean) => void;
  tapLook?: WakeLook;
  setTapLook?: (v: WakeLook) => void;
}) {
  const showAdminPasswordFields = setAdminPassword !== undefined;
  const showRotateField = setRotateAdminPassword !== undefined;
  const mismatch =
    showAdminPasswordFields &&
    !!adminPasswordConfirm &&
    !!adminPassword &&
    adminPassword !== adminPasswordConfirm;
  const description = showAdminPasswordFields
    ? "Set an admin password — you'll use it to sign in from any browser after setup."
    : "Your device's identity and admin login.";
  return (
    <SectionCard id="device" title="Device" active={active} description={description} icon={<Cpu size={17} />}>
      {showAdminPasswordFields && (
        <>
          <PasswordField
            label="Admin Password"
            id="admin_password"
            value={adminPassword ?? ""}
            onChange={setAdminPassword!}
            placeholder={`At least ${ADMIN_PASSWORD_MIN} characters`}
          />
          <PasswordStrength value={adminPassword ?? ""} />
          <PasswordField
            label="Confirm Password"
            id="admin_password_confirm"
            value={adminPasswordConfirm ?? ""}
            onChange={setAdminPasswordConfirm!}
            placeholder="Re-enter password"
            error={mismatch ? "Passwords don't match." : undefined}
          />
        </>
      )}
      {showRotateField && (
        <>
          <SecretUpdateField
            label="Admin Password"
            id="admin_password"
            configured={true}
            value={rotateAdminPassword ?? ""}
            onChange={setRotateAdminPassword!}
            placeholder={`New password (min ${ADMIN_PASSWORD_MIN} chars)`}
          />
          <PasswordStrength value={rotateAdminPassword ?? ""} />
        </>
      )}

      {setVoiceInputMode && (
        <fieldset aria-describedby="voice-input-help" style={{ margin: "18px 0 0", padding: "14px 0 0", border: 0, borderTop: `1px solid ${C.border}` }}>
          <legend style={{ ...LABEL_STYLE, padding: "0 8px 0 0" }}>How to talk</legend>
          <div style={{ display: "flex", flexWrap: "wrap", gap: 10 }}>
            {([
              ["automatic", "Automatic"],
              ["tap_to_talk", "Tap to talk"],
            ] as const).map(([mode, label]) => (
              <label key={mode} style={{ display: "flex", flex: "1 1 140px", alignItems: "center", gap: 8, padding: "10px 12px", borderRadius: 8, border: `1px solid ${voiceInputMode === mode ? C.amber : C.border}`, background: voiceInputMode === mode ? C.amberDim : C.surface, color: C.text, fontSize: 13, cursor: "pointer" }}>
                <input type="radio" name="voice_input_mode" value={mode} checked={voiceInputMode === mode} onChange={() => setVoiceInputMode(mode)} />
                {label}
              </label>
            ))}
          </div>
          <p id="voice-input-help" style={{ ...HINT_STYLE, margin: "8px 0 0" }}>
            {voiceInputMode === "automatic"
              ? "Your device detects speech and sends it when you finish speaking."
              : "Tap once, then speak after the chime or light. Tap again when you finish to send. Pausing won't send your message. If you don't tap again within about 30 seconds, recording stops and nothing is sent. While your device is speaking, a tap stops it; tap again to start recording."}
          </p>
          {voiceInputMode === "tap_to_talk" && (
            <p style={{ ...HINT_STYLE, margin: "5px 0 0" }}>
              No wake phrase or gaze needed. Your automatic wake preference is kept. The physical mic switch still applies.
            </p>
          )}
          {voiceInputMode === "tap_to_talk" && setTapChime && setTapLightOn && setTapLook && (
            <ListenCues
              id="tap"
              chimeLabel="Play a chime when recording starts"
              lightLabel="Light up while recording"
              chime={tapChime ?? true} setChime={setTapChime}
              lightOn={tapLightOn ?? true} setLightOn={setTapLightOn}
              look={tapLook ?? WAKE_LOOK_DEFAULT} setLook={setTapLook}
            />
          )}
        </fieldset>
      )}

      {setWakeWord && voiceInputMode === "automatic" && (
        <div style={{ marginTop: 18, paddingTop: 14, borderTop: `1px solid ${C.border}` }}>
          <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer", fontSize: 13, color: C.text }}>
            <input type="checkbox" checked={wakeWord ?? false} onChange={(e) => setWakeWord(e.target.checked)} style={{ flexShrink: 0 }} />
            <span style={{ whiteSpace: "nowrap" }}>Require attention trigger</span>
          </label>
          <div style={{ ...HINT_STYLE, marginTop: 5, marginLeft: 23 }}>
            Require an attention trigger before handling speech. When off, your device keeps its existing always-listening behavior.
          </div>
          {setWakePhrases ? (
            <div style={{ ...HINT_STYLE, marginTop: 10, marginLeft: 23 }}>
              <label htmlFor="wake_phrase_new" style={{ display: "block", marginBottom: 6 }}>
                {wakeNameAnywhere ? (
                  <>
                    Names your device answers to (other triggers: tap, gaze, or a recognized person){agentName ? ` — current agent: ${agentName}` : ""}.
                    {" "}Use a name anywhere in what you say: &quot;hey intern&quot;, &quot;hello intern, what&apos;s the weather?&quot; or &quot;what time is it, intern?&quot;.
                    {" "}Say it on its own and wait for the chime, or together with your request.
                  </>
                ) : (
                  <>
                    Spoken wake phrases (other triggers: tap, gaze, or a recognized person){agentName ? ` — current agent: ${agentName}` : ""}.
                    {" "}Say a phrase on its own and wait for the chime, then speak, or say it together with your request.
                  </>
                )}
              </label>
              <WakePhraseEditor phrases={wakePhrases ?? []} setPhrases={setWakePhrases} defaults={wakePhrasesDefault ?? []} nameAnywhere={wakeNameAnywhere} />
              {setWakeChime && setWakeLightOn && setWakeLook ? (
                <ListenCues
                  id="wake"
                  chimeLabel="Play a chime when a wake phrase is heard"
                  lightLabel="Light up when a wake phrase is heard"
                  chime={wakeChime ?? true} setChime={setWakeChime}
                  lightOn={wakeLightOn ?? true} setLightOn={setWakeLightOn}
                  look={wakeLook ?? WAKE_LOOK_DEFAULT} setLook={setWakeLook}
                />
              ) : setWakeChime && (
                <label htmlFor="wake_chime" style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 12, cursor: "pointer", fontSize: 13, color: C.text }}>
                  <input id="wake_chime" type="checkbox" checked={wakeChime ?? true} onChange={(e) => setWakeChime(e.target.checked)} style={{ flexShrink: 0 }} />
                  Play a chime when a wake phrase is heard
                </label>
              )}
              {setWakeFollowupS && (
                <WakeFollowupSelect value={wakeFollowupS ?? WAKE_FOLLOWUP_DEFAULT_S} onChange={setWakeFollowupS} />
              )}
            </div>
          ) : !!wakePhrases?.length && (
            <div style={{ ...HINT_STYLE, marginTop: 10, marginLeft: 23 }}>
              <div style={{ marginBottom: 6 }}>
                Spoken wake phrases (other triggers: tap, gaze, or a recognized person){agentName ? ` — current agent: ${agentName}` : ""}:
              </div>
              <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
                {wakePhrases.map((phrase) => (
                  <code key={phrase} style={{ padding: "3px 6px", borderRadius: 5, background: C.surface, border: `1px solid ${C.border}`, color: C.text }}>
                    {phrase}
                  </code>
                ))}
              </div>
            </div>
          )}
        </div>
      )}

      {showRotateField ? (
        <DeviceMetaCard deviceId={deviceId} mac={mac} />
      ) : (
        <>
          <Field label="Device ID" id="device_id" value={deviceId} onChange={setDeviceId} placeholder="device-001" readOnly />
          {mac && <MaskedReadField label="MAC" id="mac" value={mac} />}
        </>
      )}
    </SectionCard>
  );
}
