import { useEffect, useState } from "react";
import { toast } from "sonner";
import { C, ConfirmDialog, HINT_STYLE, SectionCard } from "@/components/setup/shared";
import { disableGuardMode, getDeviceConfig, getGuardMode, updateDeviceConfig } from "@/lib/api";
import type { CloudPerception } from "@/lib/api";

const ALL_OFF: CloudPerception = {
  motion: false, emotion: false, pose: false, fire_hazard: false, speaker_id: false, speech_emotion: false,
};

type Option = { key: keyof CloudPerception; source: "camera" | "voice"; label: string; purpose: string; upload: string; short: string };

// Each entry says what the camera or microphone is used for and exactly what is uploaded
// for cloud analysis, so turning it on is an informed choice.
const OPTIONS: Option[] = [
  {
    key: "pose",
    source: "camera",
    label: "Posture reminders",
    purpose: "Uses the camera to notice when you sit badly for a long time and reminds you to adjust. "
      + "A reminder sent to your Telegram includes the photos with the worst posture.",
    upload: "Uploads one full camera image for cloud analysis about every 30 seconds while someone is in front of the robot.",
    short: "Posture reminders (one full image about every 30 s while someone is in front of the robot; "
      + "reminders sent to Telegram include photos)",
  },
  {
    key: "fire_hazard",
    source: "camera",
    label: "Fire and smoke hints",
    purpose: "Uses the camera to look for visible flames or smoke and mention them. Best effort only: "
      + "this is not a smoke or fire alarm and does not replace one.",
    upload: "Uploads full camera images for cloud analysis about every 5 seconds, even when nobody is in the room.",
    short: "Fire and smoke hints (full images, about every 5 s, even with nobody in the room)",
  },
  {
    key: "motion",
    source: "camera",
    label: "Activity recognition",
    purpose: "Uses the camera to recognise what you are doing (for example working or stretching) so the robot can react.",
    upload: "Uploads full camera images for cloud analysis about every 2 seconds while someone has been seen recently.",
    short: "Activity recognition (full images, about every 2 s while someone is around)",
  },
  {
    key: "emotion",
    source: "camera",
    label: "Facial emotion",
    purpose: "Uses the camera to read facial expressions so the robot can respond to how people seem to feel.",
    upload: "Uploads a cropped photo of each detected face for cloud analysis about every 2 seconds.",
    short: "Facial emotion (a cropped photo of each face, about every 2 s)",
  },
  {
    key: "speaker_id",
    source: "voice",
    label: "Voice recognition",
    purpose: "Recognises who is speaking so the robot can call people by name and remember things about them. "
      + "Enrolling a voice also needs this. When off, the robot does not know who is talking.",
    upload: "Uploads a clip of each thing someone says (up to 20 seconds) for cloud analysis, "
      + "including voices of people who are not enrolled.",
    short: "Voice recognition (a clip of each thing anyone says, up to 20 s)",
  },
  {
    key: "speech_emotion",
    source: "voice",
    label: "Voice emotion",
    purpose: "Reads mood from the tone of voice so the robot can respond to how people sound.",
    upload: "Uploads the audio of each conversation turn for cloud analysis.",
    short: "Voice emotion (the audio of each conversation turn)",
  },
];
const CAMERA_OPTIONS = OPTIONS.filter((o) => o.source === "camera");
const VOICE_OPTIONS = OPTIONS.filter((o) => o.source === "voice");

// Perception requests go to the configured LLM base URL (HAL DL_BACKEND_URL), so show its host.
function recipientHost(baseUrl: string | undefined): string {
  if (!baseUrl) return "";
  try {
    return new URL(baseUrl).host;
  } catch {
    return baseUrl;
  }
}

function same(a: CloudPerception, b: CloudPerception): boolean {
  return OPTIONS.every(({ key }) => a[key] === b[key]);
}

export function PrivacySection({ active }: { active: boolean }) {
  const [saved, setSaved] = useState<CloudPerception | null>(null);
  const [draft, setDraft] = useState<CloudPerception>(ALL_OFF);
  const [applying, setApplying] = useState(false);
  const [recipient, setRecipient] = useState("");
  const [confirming, setConfirming] = useState(false);
  const [guard, setGuard] = useState<boolean | null>(null);
  const [guardBusy, setGuardBusy] = useState(false);

  useEffect(() => {
    getDeviceConfig()
      .then((cfg) => {
        const current = { ...ALL_OFF, ...cfg.cloud_perception };
        setSaved(current);
        setDraft(current);
        setRecipient(recipientHost(cfg.llm_base_url));
      })
      .catch(() => toast.error("Could not load privacy settings"));
    getGuardMode().then(setGuard).catch(() => setGuard(null));
  }, []);

  async function turnOffGuard() {
    setGuardBusy(true);
    try {
      await disableGuardMode();
      setGuard(false);
      toast.success("Guard mode is off");
    } catch (e) {
      toast.error((e as Error).message || "Could not turn off guard mode");
    } finally {
      setGuardBusy(false);
    }
  }

  async function apply() {
    setApplying(true);
    try {
      await updateDeviceConfig({ cloud_perception: draft });
      setSaved(draft);
      toast.success("Applied on the robot");
    } catch (e) {
      toast.error((e as Error).message || "Could not apply privacy settings");
    } finally {
      setApplying(false);
    }
  }

  const dirty = saved !== null && !same(saved, draft);
  // Only turning something on needs consent; turning off applies without a prompt.
  const newlyEnabled = saved === null ? [] : OPTIONS.filter(({ key }) => draft[key] && !saved[key]);
  const destination = recipient || "the configured server";
  const newSources = new Set(newlyEnabled.map((o) => o.source));
  const uploadWhat = newSources.size > 1 ? "camera images and voice audio" : newSources.has("voice") ? "voice audio" : "camera images";
  const whoIsIncluded = newSources.size > 1
    ? "Anyone in view of the camera or speaking near the robot is included."
    : newSources.has("voice") ? "Anyone speaking near the robot is included." : "Anyone in view of the camera is included.";

  function renderOption({ key, label, purpose, upload }: Option) {
    return (
      <label
        key={key}
        htmlFor={`privacy-${key}`}
        style={{ display: "flex", alignItems: "flex-start", gap: 10, marginBottom: 14, cursor: "pointer" }}
      >
        <input
          id={`privacy-${key}`}
          type="checkbox"
          style={{ flexShrink: 0, marginTop: 3 }}
          checked={draft[key]}
          onChange={(e) => setDraft({ ...draft, [key]: e.target.checked })}
        />
        <span>
          <span style={{ display: "block", fontSize: 13.5, color: C.text }}>{label}</span>
          <span style={{ ...HINT_STYLE, display: "block" }}>{purpose}</span>
          <span style={{ ...HINT_STYLE, display: "block" }}>
            {upload} Sent to {destination}.
          </span>
        </span>
      </label>
    );
  }

  return (
    <SectionCard id="privacy" title="Privacy" active={active}>
      <div style={{ fontSize: 12.5, color: C.textDim, marginBottom: 14, lineHeight: 1.6 }}>
        Face detection, Face ID, presence and gaze run on this device and send no images to
        a cloud server. Faces it does not know are remembered on the device as face data so it
        can count visits; they are deleted after 30 days without a visit, and you can delete
        them sooner under Unknown faces. When a face without a name has been seen twice, the robot keeps one
        photo on the device for 24 hours so it can ask whether to remember that person.
        It also keeps a photo each time someone arrives (for 72 hours) and a few dozen recent
        frames used to aim where it looks; these stay on the device and are not uploaded.
        The features below need a cloud server: when on, the robot uploads camera images for
        analysis. They are off until you turn them on. Anyone in view of the camera is
        included, not only you. Turning a feature off stops its new uploads once the robot
        confirms the change; an upload already in progress may still finish. The server
        deletes each image or voice clip as soon as it has been analysed and does not keep it.
      </div>
      <div style={{ fontSize: 12.5, color: C.text, marginBottom: 14 }}>
        Images and voice clips are sent to: <strong>{recipient || "no server configured (nothing is sent)"}</strong>
      </div>

      {saved === null ? (
        <div style={{ fontSize: 12, color: C.textMuted }}>Loading…</div>
      ) : (
        <>
          {CAMERA_OPTIONS.map(renderOption)}

          <div style={{ borderTop: `1px solid ${C.border}`, marginTop: 4, paddingTop: 14, marginBottom: 14 }}>
            <div style={{ fontSize: 13.5, color: C.text, marginBottom: 6 }}>Microphone</div>
            <div style={{ fontSize: 12.5, color: C.textDim, lineHeight: 1.6 }}>
              The robot listens on the device for speech. When it hears someone talking, it streams
              that audio to a speech-to-text service ({destination}, or Deepgram if you entered your
              own Deepgram key) to catch its name and understand requests. This includes speech that
              was not meant for the robot. During a live voice conversation, audio also goes to the
              voice AI provider. Muting the microphone stops all of it. The features below upload
              voice audio for extra analysis and are off until you turn them on; anyone speaking
              near the robot is included, not only you.
            </div>
          </div>
          {VOICE_OPTIONS.map(renderOption)}

          <div style={{ display: "flex", alignItems: "center", gap: 12, marginTop: 6 }}>
            <button
              type="button"
              onClick={() => { if (newlyEnabled.length > 0) setConfirming(true); else void apply(); }}
              disabled={!dirty || applying}
              style={{
                padding: "6px 18px", borderRadius: 8, fontSize: 12, fontWeight: 600, border: "none",
                cursor: !dirty || applying ? "not-allowed" : "pointer",
                background: !dirty || applying ? C.surface : C.amber,
                color: !dirty || applying ? C.textMuted : "var(--lm-on-amber)",
                opacity: !dirty || applying ? 0.6 : 1,
              }}
            >
              {applying ? "Applying…" : "Apply"}
            </button>
            <span style={{ fontSize: 12, color: C.textMuted }}>
              Apply waits until the robot confirms the change.
            </span>
          </div>
        </>
      )}

      <div style={{ borderTop: `1px solid ${C.border}`, marginTop: 18, paddingTop: 14 }}>
        <div style={{ fontSize: 13.5, color: C.text }}>
          Guard mode: <strong>{guard === null ? "unknown" : guard ? "on" : "off"}</strong>
        </div>
        <div style={{ ...HINT_STYLE, marginTop: 4 }}>
          While guard mode is on, each person, movement or fire hint the robot notices is sent to
          your Telegram with a camera photo. It is off until you ask the robot to guard the room.
        </div>
        {guard && (
          <button
            type="button"
            onClick={() => void turnOffGuard()}
            disabled={guardBusy}
            style={{
              marginTop: 10, padding: "6px 18px", borderRadius: 8, fontSize: 12, fontWeight: 600,
              border: "none", cursor: guardBusy ? "not-allowed" : "pointer",
              background: C.amber, color: "var(--lm-on-amber)", opacity: guardBusy ? 0.6 : 1,
            }}
          >
            {guardBusy ? "Turning off…" : "Turn off guard mode"}
          </button>
        )}
      </div>

      {confirming && (
        <ConfirmDialog
          title={`Upload ${uploadWhat} to the cloud?`}
          message={`The robot will upload ${uploadWhat} to ${destination} for cloud analysis: `
            + `${newlyEnabled.map((o) => o.short).join("; ")}. `
            + `${whoIsIncluded} You can turn this off at any time; `
            + "the robot then stops new uploads once it confirms. "
            + "The server deletes each image or clip as soon as it has been analysed and does not keep it."}
          confirmLabel="Turn on and upload"
          onConfirm={() => { setConfirming(false); void apply(); }}
          onCancel={() => setConfirming(false)}
        />
      )}
    </SectionCard>
  );
}
