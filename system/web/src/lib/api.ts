import camelcaseKeys from "camelcase-keys";
import { normalizeLang } from "@/lib/i18n";
import type { NetworkItem, SetupRequest } from "@/types";

const API_BASE =
  import.meta.env.VITE_API_BASE ??
  import.meta.env.VITE_NETWORK_API ??
  import.meta.env.VITE_API_URL ??
  "";

/** 0 = error, 1 = success (matches backend JSONReponseStatus) */
export type JSONResponseStatus = 0 | 1;

export interface JSONResponse<T = unknown> {
  status: JSONResponseStatus;
  message: string | null;
  data: T;
}

// Legacy Bearer fallback; browsers normally authenticate via the `os_session` cookie.
const TOKEN_STORAGE_KEY = "device_api_token";

// Listening window after a wake; keep aligned with system/server/config (WakeFollowup*S).
export const WAKE_FOLLOWUP_DEFAULT_S = 10;
export const WAKE_FOLLOWUP_CHOICES_S = [5, 10, 15, 20, 30];

export type WakeLightEffect = "solid" | "breathing" | "spin" | "blink";
export interface WakeLightConfig {
  enabled?: boolean;
  color?: [number, number, number];
  effect?: WakeLightEffect;
  speed?: number;
}
let apiToken: string =
  typeof window !== "undefined" ? sessionStorage.getItem(TOKEN_STORAGE_KEY) ?? "" : "";

export function setApiToken(token: string): void {
  apiToken = token ?? "";
  if (typeof window === "undefined") return;
  if (apiToken) sessionStorage.setItem(TOKEN_STORAGE_KEY, apiToken);
  else sessionStorage.removeItem(TOKEN_STORAGE_KEY);
}

export function getApiToken(): string {
  return apiToken;
}

/** Append ?token=<key> to a URL only when a legacy Bearer token is in play. */
export function withApiToken(url: string): string {
  if (!apiToken) return url;
  const sep = url.includes("?") ? "&" : "?";
  return `${url}${sep}token=${encodeURIComponent(apiToken)}`;
}

/** Build a `/api/hardware/<path>` URL. */
export function hwUrl(path: string): string {
  return withApiToken(`/api/hardware${path}`);
}

/** Build a `GET /api/agent/file` URL for a DEVICE-LOCAL path the agent named in a reply (a camera snapshot, a generated report). */
export function agentFileUrl(devicePath: string): string {
  return withApiToken(`${API_BASE}/api/agent/file?path=${encodeURIComponent(devicePath)}`);
}

/** Base64-encode a File without the data: prefix; FileReader avoids stack overflow on large files. */
export function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve((reader.result as string).split(",")[1]);
    reader.onerror = () => reject(new Error("Failed to read file"));
    reader.readAsDataURL(file);
  });
}

// Setup query params that may carry secrets; stripped before a URL propagates.
const SECRET_QUERY_KEYS = [
  "tele_token",
  "slack_bot_token",
  "slack_app_token",
  "discord_bot_token",
  "llm_api_key",
  "deepgram_api_key",
  "stt_api_key",
  "tts_api_key",
  "mqtt_password",
  "password",
  "admin_password",
];

/** Return window.location.search (or the given query string) with every known secret key removed. */
export function safeSearch(search?: string): string {
  const raw = search ?? (typeof window !== "undefined" ? window.location.search : "");
  if (!raw) return "";
  const p = new URLSearchParams(raw);
  let changed = false;
  for (const k of SECRET_QUERY_KEYS) {
    if (p.has(k)) {
      p.delete(k);
      changed = true;
    }
  }
  if (!changed) return raw;
  const out = p.toString();
  return out ? `?${out}` : "";
}

// Must match the key in hooks/setup/useSetupUrlParams.ts.
const SETUP_URL_SEARCH_STORE_KEY = "autonomous.setup_url_search.v1";

/** Scrub secret query params from window.location without a navigation. */
export function scrubLocationSecrets(): void {
  if (typeof window === "undefined") return;
  // /setup keeps the full URL so a reload re-reads its params.
  if (window.location.pathname === "/setup") return;
  const raw = window.location.search;
  const cleaned = safeSearch(raw);
  if (cleaned === raw) return;
  try {
    // Do not retain /login's ?password in sessionStorage.
    if (raw && window.location.pathname !== "/login") {
      sessionStorage.setItem(SETUP_URL_SEARCH_STORE_KEY, raw);
    }
  } catch {
    /* private-mode / storage disabled — URL scrub still proceeds */
  }
  const next = `${window.location.pathname}${cleaned}${window.location.hash}`;
  window.history.replaceState(null, "", next);
}

// Patched fetch: same-origin /api/* rides the session cookie plus any legacy Bearer.
if (typeof window !== "undefined" && !(window as unknown as { __osFetchPatched?: boolean }).__osFetchPatched) {
  const origFetch = window.fetch.bind(window);
  window.fetch = function patchedFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
    let url = "";
    if (typeof input === "string") url = input;
    else if (input instanceof URL) url = input.toString();
    else url = (input as Request).url;

    const isApiCall = url.startsWith("/api/") || url.includes("/api/");
    if (!isApiCall) return origFetch(input, init);

    // no-cors must pass through untouched: auth headers or credentials trigger preflight failures.
    if (init?.mode === "no-cors") return origFetch(input, init);

    const headers = new Headers(init?.headers);
    if (apiToken && !headers.has("Authorization")) {
      headers.set("Authorization", `Bearer ${apiToken}`);
    }
    return origFetch(input, { ...init, headers, credentials: "include" });
  };
  (window as unknown as { __osFetchPatched?: boolean }).__osFetchPatched = true;
}

async function apiRequest<T>(url: string, options?: RequestInit): Promise<T> {
  const headers = new Headers(options?.headers);
  if (apiToken && !headers.has("Authorization")) {
    headers.set("Authorization", `Bearer ${apiToken}`);
  }
  const res = await fetch(url, { credentials: "include", ...options, headers });
  let json: JSONResponse<T>;
  try {
    json = await res.json() as JSONResponse<T>;
  } catch (cause) {
    if (options?.signal?.aborted) throw cause;
    const err = new Error(res.ok ? "The server returned an invalid response. Try again." : `Request failed (HTTP ${res.status}).`) as Error & { status?: number };
    err.status = res.status;
    throw err;
  }
  if (json.status !== 1) {
    const msg =
      typeof json.message === "string" ? json.message : res.ok ? "Request failed" : res.statusText;
    const err = new Error(msg) as Error & { status?: number };
    err.status = res.status;
    throw err;
  }
  return json.data;
}

/** Converts object keys from snake_case to camelCase (uses camelcase-keys). */
export function parseSnakeToCamel<T = Record<string, unknown>>(
  raw: Record<string, unknown>,
  options?: { deep?: boolean }
): T {
  return camelcaseKeys(raw as Record<string, unknown>, { deep: options?.deep ?? false }) as T;
}

export async function getNetworks(): Promise<NetworkItem[]> {
  return apiRequest<NetworkItem[]>(`${API_BASE}/api/network`);
}

/** The Wi-Fi network wlan0 is associated with (null when not associated). */
export interface CurrentNetwork {
  ssid: string;
  signal: number;
  linkRate: number;
}

/** GET /api/network/current — the joined SSID, or null when wlan0 isn't associated. */
export async function getCurrentNetwork(signal?: AbortSignal): Promise<CurrentNetwork | null> {
  return apiRequest<CurrentNetwork | null>(`${API_BASE}/api/network/current`, { signal, cache: "no-store" });
}

export async function setupNetwork(ssid: string, password: string): Promise<string> {
  return apiRequest<string>(`${API_BASE}/api/network/setup`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ ssid, password }),
  });
}

export async function setupDevice(body: SetupRequest): Promise<boolean> {
  return apiRequest<boolean>(`${API_BASE}/api/device/setup`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
}

/** POST /api/device/wifi-provision — AP-portal setup path. */
export interface WifiProvisionBody {
  ssid: string;
  password?: string;
  llm_api_key?: string;
  llm_base_url?: string;
  llm_model?: string;
  stt_api_key?: string;
  stt_base_url?: string;
  stt_language?: string;
  tts_api_key?: string;
  // Explicit delete: an empty tts_api_key means "not sent".
  clear_tts_api_key?: boolean;
  tts_base_url?: string;
  tts_provider?: string;
  tts_voice?: string;
  admin_password?: string;
  // Only the sub-tokens matching `channel` are honored.
  channel?: string;
  telegram_bot_token?: string;
  telegram_user_id?: string;
  slack_bot_token?: string;
  slack_app_token?: string;
  slack_user_id?: string;
  discord_bot_token?: string;
  discord_guild_id?: string;
  discord_user_id?: string;
  bluebubbles_server_url?: string;
  bluebubbles_password?: string;
  bluebubbles_user_address?: string;
  bluebubbles_caller_context?: string;
}
export async function wifiProvision(body: WifiProvisionBody): Promise<boolean> {
  return apiRequest<boolean>(`${API_BASE}/api/device/wifi-provision`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
}

export interface SetupStatus {
  // Present during onboarding runtime preparation; absent on older servers.
  runtime_phase?: "preparing" | "ready" | "failed" | "";
  phase: "idle" | "connecting" | "connected" | "failed";
  lan_ip: string;
  error: string;
  mac: string;
  // Bumped when a setup run starts; absent on older builds.
  run?: number;
  // Decides initial vs continue wizard; absent on older builds.
  set_up_completed?: boolean;
}

/** Polled by Setup.tsx during the AP→STA transition. */
export async function getSetupStatus(): Promise<SetupStatus> {
  return apiRequest<SetupStatus>(`${API_BASE}/api/device/setup/status`);
}

export async function checkInternet(): Promise<boolean> {
  return apiRequest<boolean>(`${API_BASE}/api/network/check-internet`);
}


export async function getSetup(): Promise<boolean> {
  return apiRequest<boolean>(`${API_BASE}/api/setup`);
}

export type LLMConfigMode = "" | "os" | "runtime";

export type VoiceInputMode = "automatic" | "tap_to_talk";

/** Sanitized device config — Has* booleans replace raw secrets so they never reach the DOM / sessionStorage / HAR captures. */
/** Opt-ins for perceptions that upload camera frames, face crops or voice audio off the device. */
export interface CloudPerception {
  motion: boolean;
  emotion: boolean;
  pose: boolean;
  fire_hazard: boolean;
  speaker_id: boolean;
  speech_emotion: boolean;
}

export interface DeviceConfig {
  llm_config_mode?: LLMConfigMode;
  channel: string;
  telegram_user_id: string;
  slack_user_id: string;
  discord_guild_id: string;
  discord_user_id: string;
  bluebubbles_server_url: string;
  bluebubbles_user_address: string;
  bluebubbles_caller_context: string;
  llm_model: string;
  llm_base_url: string;
  llm_disable_thinking: boolean;
  stt_base_url: string;
  tts_base_url: string;
  stt_language: string;
  stt_model: string;
  tts_provider: string;
  tts_voice: string;
  tts_speed?: number;
  wakeword: boolean;
  voice_input_mode?: VoiceInputMode;
  cloud_perception?: CloudPerception;
  agent_name: string;
  wake_phrases: string[];
  // True when wake_phrases is the owner's edited list; wake_phrases_default is the built-in one.
  wake_phrases_custom?: boolean;
  wake_phrases_default?: string[];
  // Intern: wake_phrases are names heard anywhere in a sentence, not edge phrases.
  wake_name_anywhere?: boolean;
  // Chime on a spoken wake phrase (default true).
  wake_chime?: boolean;
  // Seconds the device keeps listening after a wake (WAKE_FOLLOWUP_MIN_S..MAX_S).
  wake_followup_s?: number;
  // Listening light on a wake; absent fields keep the board look.
  wake_light?: WakeLightConfig | null;
  wake_light_enabled?: boolean;
  // Tap-to-talk cues, separate from the wake cues (both default true).
  tap_chime?: boolean;
  tap_light?: WakeLightConfig | null;
  tap_light_enabled?: boolean;
  realtime?: {
    enabled?: boolean;
    provider?: string;
    model?: string;
    voice?: string;
    reasoning?: string;
    base_url?: string;
    has_api_key?: boolean;
    web_search?: boolean;
    // Effective fast voice setting (Gemini only, more Gemini usage); default on for Intern v2 only.
    fast_voice?: boolean;
    // Fast voice: longest wait (s) after a pause the turn model calls unfinished.
    fast_voice_pause_s?: number;
    // Fast voice: seconds to wait on a silent model before the main agent takes over.
    fast_voice_silent_timeout_s?: number;
  };
  device_id: string;
  mac: string;
  network_ssid: string;
  mqtt_endpoint: string;
  mqtt_username: string;
  mqtt_port: number;
  fa_channel: string;
  fd_channel: string;

  has_telegram_bot_token: boolean;
  has_slack_bot_token: boolean;
  has_slack_app_token: boolean;
  has_discord_bot_token: boolean;
  has_bluebubbles_password: boolean;
  has_llm_api_key: boolean;
  has_deepgram_api_key: boolean;
  has_stt_api_key: boolean;
  has_tts_api_key: boolean;
  has_network_password: boolean;
  has_mqtt_password: boolean;
  has_admin_password: boolean;
  /** True once the operator has replaced a shipped credential, so a restore is possible. */
  has_autonomous_defaults: boolean;
  /** Non-secret half of the stored Autonomous set, so the UI can tell whether the device is still on it. */
  autonomous_default_base_url?: string;
  autonomous_default_model?: string;
}

export async function getTTSVoices(provider?: string, lang?: string): Promise<string[]> {
  const qs = new URLSearchParams();
  if (provider) qs.set("provider", provider);
  if (lang) qs.set("lang", lang);
  const params = qs.toString() ? `?${qs.toString()}` : "";
  return apiRequest<string[]>(`${API_BASE}/api/device/voices${params}`);
}

export async function getTTSProviders(): Promise<string[]> {
  return apiRequest<string[]>(`${API_BASE}/api/device/tts-providers`);
}

export interface RealtimeOptions {
  providers: string[];
  voices: Record<string, string[]>;
  reasoning: Record<string, string[]>;
}

export async function getRealtimeOptions(): Promise<RealtimeOptions> {
  return apiRequest<RealtimeOptions>(`${API_BASE}/api/device/realtime-options`);
}

export interface AgentRuntimeStatus {
  current: string;
  options: string[];
  /** Whether the backend is actually answering, not merely selected. */
  ready: boolean;
  /** Stored remote-gateway config. */
  remote_url?: string;
  remote_token?: string;
}

export interface RuntimeLoginSession {
  id: string;
  runtime: string;
  provider: string;
  status: "starting" | "waiting" | "applying" | "success" | "error" | "cancelled";
  login_url?: string;
  user_code?: string;
  input_required?: boolean;
  error?: string;
}

export interface RuntimeLoginState {
  runtime: string;
  providers: { id: string; label: string }[];
  session?: RuntimeLoginSession;
}

export async function getRuntimeLogin(signal?: AbortSignal): Promise<RuntimeLoginState> {
  try {
    return await apiRequest(`${API_BASE}/api/device/runtime-login`, { signal, cache: "no-store" });
  } catch (err) {
    if ((err as Error & { status?: number }).status === 404) {
      throw new Error("Web sign-in is unavailable on this OS Server. Update OS Server and try again. You can also sign in using Terminal.");
    }
    throw err;
  }
}

export function startRuntimeLogin(runtime: string, provider: string, signal?: AbortSignal): Promise<RuntimeLoginSession> {
  return apiRequest(`${API_BASE}/api/device/runtime-login`, {
    method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ runtime, provider }), signal,
  });
}

export function submitRuntimeLoginCode(id: string, code: string, signal?: AbortSignal): Promise<RuntimeLoginSession> {
  return apiRequest(`${API_BASE}/api/device/runtime-login/code`, {
    method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ id, code }), signal,
  });
}

export function cancelRuntimeLogin(id: string, signal?: AbortSignal): Promise<RuntimeLoginSession> {
  return apiRequest(`${API_BASE}/api/device/runtime-login/${encodeURIComponent(id)}`, { method: "DELETE", signal });
}

export async function getAgentRuntime(): Promise<AgentRuntimeStatus> {
  return apiRequest<AgentRuntimeStatus>(`${API_BASE}/api/device/agent-runtime`);
}

/** Options for `setAgentRuntime`. */
export interface SetAgentRuntimeOptions {
  url?: string;
  token?: string;
}

/** POST /api/device/agent-runtime — swap the agentic backend (openclaw ⇄ hermes ⇄ remote). */
export async function setAgentRuntime(
  runtime: string,
  opts?: SetAgentRuntimeOptions,
): Promise<boolean> {
  const body: Record<string, string> = { runtime };
  if (opts?.url) body.url = opts.url;
  if (opts?.token) body.token = opts.token;
  return apiRequest<boolean>(`${API_BASE}/api/device/agent-runtime`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
}

export interface TimezoneStatus {
  current: string;
  zones: string[];
}

/** GET /api/device/timezone — current IANA zone + selectable list (from system tzdata). */
export async function getTimezone(): Promise<TimezoneStatus> {
  return apiRequest<TimezoneStatus>(`${API_BASE}/api/device/timezone`);
}

/** POST /api/device/timezone — apply an IANA zone. */
export async function setTimezone(timezone: string): Promise<boolean> {
  return apiRequest<boolean>(`${API_BASE}/api/device/timezone`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ timezone }),
  });
}

export interface TestTTSOptions {
  /** Per-preview speed override; does not change the saved setting. */
  speed?: number;
  text?: string;
  /** BCP-47 stt_language code; picks a friendly demo phrase in that language. */
  lang?: string;
  provider?: string;
  /** Optional URL/key override — lets the admin's Test Voice validate a pending edit BEFORE clicking Save Changes. */
  baseUrl?: string;
  apiKey?: string;
}

const TTS_DEMO_PHRASES: Record<string, string> = {
  en: "[laugh] Hey! How are you doing today?",
  ja: "[laugh] こんにちは！今日はどんな一日ですか？",
  vi: "[laugh] Chào bạn, hôm nay bạn thế nào?",
  "zh-CN": "[laugh] 嗨，你今天怎么样？",
  "zh-TW": "[laugh] 嗨，你今天怎麼樣？",
};

function demoPhraseFor(lang?: string): string {
  if (!lang) return TTS_DEMO_PHRASES.en;
  return TTS_DEMO_PHRASES[normalizeLang(lang)] || TTS_DEMO_PHRASES.en;
}

/** POST /api/voice/preview — optional baseUrl/apiKey override the saved config. */
export async function testTTSVoice(voice: string, opts: TestTTSOptions = {}): Promise<void> {
  await apiRequest<boolean>(`${API_BASE}/api/voice/preview`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      text: opts.text || demoPhraseFor(opts.lang),
      voice,
      provider: opts.provider || undefined,
      base_url: opts.baseUrl || undefined,
      api_key: opts.apiKey || undefined,
      speed: opts.speed,
    }),
  });
}

export async function getDeviceConfig(): Promise<DeviceConfig> {
  return apiRequest<DeviceConfig>(`${API_BASE}/api/device/config`);
}

export async function updateDeviceConfig(body: Partial<Record<string, unknown>>): Promise<boolean> {
  return apiRequest<boolean>(`${API_BASE}/api/device/config`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
}

export async function getGuardMode(): Promise<boolean> {
  const r = await apiRequest<{ guard_mode: boolean }>(`${API_BASE}/api/guard`);
  return r.guard_mode;
}

export async function disableGuardMode(): Promise<void> {
  await apiRequest(`${API_BASE}/api/guard/disable`, { method: "POST" });
}

/** Read-side of a static-credential connector (Facebook Fan Page, Gmail app password, …). */
export interface ConnectorInfo {
  connector: string;
  connected: boolean;
  auth_type?: string;
  user_email?: string;
  credentials?: Record<string, string>;
  /** Unix seconds when this device received the credentials, or 0/undefined when nothing has been set. */
  obtained_at?: number;
}

/** GET /api/device/connectors/:code — reads the on-disk entry the connectorWriter maintains. */
export async function getConnector(code: string): Promise<ConnectorInfo> {
  return apiRequest<ConnectorInfo>(`${API_BASE}/api/device/connectors/${encodeURIComponent(code)}`);
}

/** POST /api/device/connectors/pat — the local Settings UI's write path for a static-credential connector. */
export async function setConnectorPAT(body: {
  connector: string;
  api_key: string;
  user_email?: string;
  credentials?: Record<string, string>;
}): Promise<{ connector: string; auth_type: string; user_email?: string }> {
  return apiRequest(`${API_BASE}/api/device/connectors/pat`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
}

/** Read-side of the Shopify connection: the store and client ID, and whether a secret is saved. Never the secret or a token. */
export interface ShopifyInfo {
  connector: string;
  connected: boolean;
  shop?: string;
  client_id?: string;
  has_secret: boolean;
  scopes?: string[];
  /** Unix seconds when the credentials were saved on this device. */
  obtained_at?: number;
}

/** GET /api/device/connectors/shopify */
export async function getShopify(): Promise<ShopifyInfo> {
  return apiRequest<ShopifyInfo>(`${API_BASE}/api/device/connectors/shopify`);
}

/** PUT /api/device/connectors/shopify — the device checks the credentials with Shopify once, then saves them. */
export async function setShopify(body: {
  shop: string;
  client_id: string;
  client_secret: string;
}): Promise<ShopifyInfo> {
  return apiRequest<ShopifyInfo>(`${API_BASE}/api/device/connectors/shopify`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
}

/** Read side of the direct Amazon connection (the seller's own private Selling Partner API app). Never the secret or a token. */
export interface AmazonSPInfo {
  connector: string;
  connected: boolean;
  region?: string;
  client_id?: string;
  seller_id?: string;
  has_secret: boolean;
  has_refresh_token: boolean;
  /** Masked (dots plus the last 4 characters of a long value); the full values never reach the browser. */
  client_secret_hint?: string;
  refresh_token_hint?: string;
  obtained_at?: number;
}

/** GET /api/device/connectors/amazon_sp */
export async function getAmazonSP(): Promise<AmazonSPInfo> {
  return apiRequest<AmazonSPInfo>(`${API_BASE}/api/device/connectors/amazon_sp`);
}

/** PUT /api/device/connectors/amazon_sp — the device checks the three values with Login with Amazon once, then saves them. */
export async function setAmazonSP(body: {
  region?: string;
  client_id?: string;
  client_secret?: string;
  refresh_token?: string;
  /** Optional; sent alone it updates a saved connection without asking for the secrets again. */
  seller_id?: string;
}): Promise<AmazonSPInfo> {
  return apiRequest<AmazonSPInfo>(`${API_BASE}/api/device/connectors/amazon_sp`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
}

/** PUT /api/device/connectors/ebay — the device exchanges the eBay sign-in code (or checks a pasted refresh token), then saves the link. */
export async function setEbay(body: {
  environment: "production" | "sandbox";
  marketplace_id: string;
  client_id: string;
  client_secret: string;
  ru_name: string;
  code?: string;
  refresh_token?: string;
}): Promise<{ connector: string; connected: boolean; refresh_token_expires_at?: number }> {
  return apiRequest(`${API_BASE}/api/device/connectors/ebay`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
}

/** Amazon's optional skills (the Selling Partner toolkit) the robot can install after the Amazon link works. */
export interface AmazonToolkitInfo {
  /** False when the connectors skill (which ships the installer) is not on this device. */
  available: boolean;
  installed: { skill: string; commit: string }[];
  pinned_commit?: string;
}

/** GET /api/device/connectors/amazon/toolkit — which of Amazon's skills are installed. */
export async function getAmazonToolkit(): Promise<AmazonToolkitInfo> {
  return apiRequest<AmazonToolkitInfo>(`${API_BASE}/api/device/connectors/amazon/toolkit`);
}

/** POST /api/device/connectors/amazon/toolkit — install (downloads from GitHub at a pinned commit) or remove Amazon's skills. */
export async function setAmazonToolkit(action: "install" | "remove"): Promise<AmazonToolkitInfo> {
  return apiRequest<AmazonToolkitInfo>(`${API_BASE}/api/device/connectors/amazon/toolkit`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ action }),
  });
}

/** DELETE /api/device/connectors/:code — drops the on-disk entry (and any mcp.servers.<code> side-effect through the writer). */
export async function removeConnector(code: string): Promise<{ connector: string; removed: boolean }> {
  return apiRequest(`${API_BASE}/api/device/connectors/${encodeURIComponent(code)}`, {
    method: "DELETE",
  });
}

/** POST /api/login — server validates bcrypt(password) against config.AdminPasswordHash and sets the os_session cookie on success. */
export async function login(password: string): Promise<boolean> {
  return apiRequest<boolean>(`${API_BASE}/api/login`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ password }),
  });
}

export interface MCPTool { name: string; url: string; headers?: Record<string, string> }

/** GET /api/device/mcp-tools */
export async function listMCPTools(): Promise<MCPTool[]> {
  return apiRequest<MCPTool[]>(`${API_BASE}/api/device/mcp-tools`);
}

/** POST /api/device/mcp-tools */
export async function addMCPTool(tool: MCPTool): Promise<boolean> {
  return apiRequest<boolean>(`${API_BASE}/api/device/mcp-tools`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(tool),
  });
}

/** DELETE /api/device/mcp-tools/:name */
export async function removeMCPTool(name: string): Promise<boolean> {
  return apiRequest<boolean>(`${API_BASE}/api/device/mcp-tools/${encodeURIComponent(name)}`, {
    method: "DELETE",
  });
}

/** Amazon's sign-in runs on the device through the Hermes runtime; `supported` is false on other runtimes. */
export interface AmazonStatus { supported: boolean; reason?: string; connected: boolean; pending: boolean }

/** GET /api/device/amazon/status */
export async function getAmazonStatus(): Promise<AmazonStatus> {
  return apiRequest<AmazonStatus>(`${API_BASE}/api/device/amazon/status`);
}

/** POST /api/device/amazon/connect — returns Amazon's sign-in link to open in the owner's browser. */
export async function startAmazonConnect(): Promise<{ auth_url: string }> {
  return apiRequest<{ auth_url: string }>(`${API_BASE}/api/device/amazon/connect`, { method: "POST" });
}

/** POST /api/device/amazon/connect/finish — the address the browser landed on after Amazon's sign-in. */
export async function finishAmazonConnect(callbackUrl: string): Promise<boolean> {
  return apiRequest<boolean>(`${API_BASE}/api/device/amazon/connect/finish`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ callback_url: callbackUrl }),
  });
}

/** DELETE /api/device/amazon/connect — stops a sign-in the owner gave up on. */
export async function cancelAmazonConnect(): Promise<boolean> {
  return apiRequest<boolean>(`${API_BASE}/api/device/amazon/connect`, { method: "DELETE" });
}

/** DELETE /api/device/amazon — removes the Amazon connection and its saved sign-in. */
export async function disconnectAmazon(): Promise<boolean> {
  return apiRequest<boolean>(`${API_BASE}/api/device/amazon`, { method: "DELETE" });
}

export interface FoStatus { connected: boolean; needs_reconnect: boolean }

/** GET /api/device/fo/status — whether Fo is connected and whether its Wajo sign-in has died. */
export async function getFoStatus(): Promise<FoStatus> {
  return apiRequest<FoStatus>(`${API_BASE}/api/device/fo/status`);
}

/** POST /api/device/fo/connect — returns the Wajo sign-in URL for the Fo MCP tool. */
export async function startFoConnect(): Promise<{ auth_url: string }> {
  return apiRequest<{ auth_url: string }>(`${API_BASE}/api/device/fo/connect`, { method: "POST" });
}

/** POST /api/device/fo/connect/finish — takes the localhost URL the browser landed on after sign-in. */
export async function finishFoConnect(callbackURL: string): Promise<boolean> {
  return apiRequest<boolean>(`${API_BASE}/api/device/fo/connect/finish`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ callback_url: callbackURL }),
  });
}

export interface Plugin { name: string; version: string; description: string; status: string; url: string }

/** GET /api/plugin */
export async function listPlugins(): Promise<Plugin[]> {
  return apiRequest<Plugin[]>(`${API_BASE}/api/plugin`);
}

/** POST /api/plugin/install */
export async function installPlugin(url: string): Promise<boolean> {
  return apiRequest<boolean>(`${API_BASE}/api/plugin/install`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ url }),
  });
}

/** POST /api/plugin/:name/start */
export async function startPlugin(name: string): Promise<boolean> {
  return apiRequest<boolean>(`${API_BASE}/api/plugin/${encodeURIComponent(name)}/start`, {
    method: "POST",
  });
}

/** POST /api/plugin/:name/stop */
export async function stopPlugin(name: string): Promise<boolean> {
  return apiRequest<boolean>(`${API_BASE}/api/plugin/${encodeURIComponent(name)}/stop`, {
    method: "POST",
  });
}

/** DELETE /api/plugin/:name */
export async function uninstallPlugin(name: string): Promise<boolean> {
  return apiRequest<boolean>(`${API_BASE}/api/plugin/${encodeURIComponent(name)}`, {
    method: "DELETE",
  });
}

export interface ScheduleCadence {
  repeat: "daily" | "weekly" | "monthly" | "interval" | "once" | "manual";
  // 0=Sunday..6=Saturday (Go time.Weekday); 7 is also Sunday.
  days?: number[];
  day_of_month?: number;
  time?: string;
  /** Every fire time in a day (daily/weekly/monthly). */
  times?: string[];
  every_ms?: number; // "interval" gap — MILLISECONDS, not seconds
  at?: string;
}

/** "agent" sends `instructions` as a prompt; "speak" says it verbatim. Absent means "agent". */
export type ScheduleKind = "agent" | "speak";

/** HAL's hard TTS bound (MaxSpeakChars); HAL rejects rather than truncates. */
export const MAX_SPEAK_CHARS = 2000;

/** Normalise a possibly-absent kind. Mirrors ResolveKind in system/schedule/store.go. */
export function resolveScheduleKind(kind?: string | null): ScheduleKind {
  return kind?.trim().toLowerCase() === "speak" ? "speak" : "agent";
}

/** Most times one schedule may carry. Mirrors MaxTimesPerSchedule on the device. */
export const MAX_TIMES_PER_SCHEDULE = 12;

/** Effective fire times: `times` when set, else the single `time`. */
export function resolveCadenceTimes(c: ScheduleCadence | undefined): string[] {
  if (!c) return [];
  if (c.times?.length) return c.times;
  return c.time ? [c.time] : [];
}

/** A run's outcome, exactly as the device's runner reports it (system/schedule/runner.go RunReport.Status). */
export type ScheduleRunStatus = "success" | "failure" | "skipped";

export interface ScheduleItem {
  id: string;
  name: string;
  instructions: string;
  enabled: boolean;
  /** Absent/empty means "agent" — use resolveScheduleKind, never a bare read. */
  kind?: ScheduleKind;
  schedule: ScheduleCadence;
  end_at?: string;
  next_run_at?: string;
  last_run_at?: string;
  last_run_status?: ScheduleRunStatus;
  /** The last run's summary: the task name on success, the error on failure, "missing connector: <codes>" when skipped. */
  last_run_summary?: string;

  /** Backend revision; sent back as base_rev for compare-and-swap. */
  rev?: number;

  /** Set when a local change is queued but the backend has not confirmed it. */
  pending?: "create" | "update" | "delete";

  /** Correlates a pending row with its queue entry. */
  intent_id?: string;
}

/** Body of the device-side create/update endpoints — only the user-editable subset. */
export interface ScheduleWriteBody {
  name: string;
  instructions: string;
  /** Omit for an agent task; the device's handler defaults an absent kind to agent. */
  kind?: ScheduleKind;
  enabled?: boolean;
  template_code?: string;
  schedule: ScheduleCadence;
  end_at?: string;
}

/** What the write endpoints return: an ACCEPTED proposal, not a finished row. */
export interface SchedulePendingResult {
  intent_id: string;
  pending: "create" | "update" | "delete";
  id?: string;
  base_rev?: number;
}

export interface ScheduleList {
  timezone: string;
  schedules: ScheduleItem[];
}

/** GET /api/schedule/list */
export async function listSchedules(): Promise<ScheduleList> {
  return apiRequest<ScheduleList>(`${API_BASE}/api/schedule/list`);
}

export interface ScheduleRunResult {
  id: string;
  run_id: string;
  started_at: string;
  status: ScheduleRunStatus;
  summary: string;
}

/** POST /api/schedule/:id/run — the web UI's local "Run now". */
export async function runScheduleNow(id: string): Promise<ScheduleRunResult> {
  return apiRequest<ScheduleRunResult>(`${API_BASE}/api/schedule/${encodeURIComponent(id)}/run`, {
    method: "POST",
  });
}

// HuggingFace plugin discovery is parked (#213).
export interface StoreSkill {
  id: string;
  name: string;
  slug?: string;
  description?: string;
  version?: string;
  category_id?: string;
  plan_required?: string;
  author?: string;
  license?: string;
  size?: string;
  icon_url?: string;
  compatibility?: string[];
  download_count?: number;
  creator_type?: string;
  source?: string;
}

export interface StoreSkillList {
  data: StoreSkill[];
  total: number;
}

/** One file unpacked from a downloaded `.skill` archive. */
export interface SkillBundleFile {
  path: string;
  size: number;
  text?: string;
  binary?: boolean;
  truncated?: boolean;
}

export interface SkillBundle {
  id: string;
  files: SkillBundleFile[];
  skipped?: number;
}

/** GET /api/agent/skills/browse — catalog listing with optional filters. */
export async function browseStoreSkills(
  opts: { keyword?: string; page?: number; limit?: number } = {},
): Promise<StoreSkillList> {
  const q = new URLSearchParams();
  if (opts.keyword) q.set("keyword", opts.keyword);
  if (opts.page) q.set("page", String(opts.page));
  if (opts.limit) q.set("limit", String(opts.limit));
  const qs = q.toString();
  return apiRequest<StoreSkillList>(`${API_BASE}/api/agent/skills/browse${qs ? `?${qs}` : ""}`);
}

/** GET /api/agent/skills/bundle — downloads + unzips the skill server-side and returns its files. */
export async function fetchSkillBundle(id: string): Promise<SkillBundle> {
  return apiRequest<SkillBundle>(
    `${API_BASE}/api/agent/skills/bundle?id=${encodeURIComponent(id)}`);
}

/** A skill authored in the web UI's "Write skill" form. */
export interface SkillDraft {
  name: string;
  description: string;
  instructions: string;
}

/** POST /api/agent/skills — writes <name>/SKILL.md into the ACTIVE agent runtime's skills dir. */
export async function saveSkill(draft: SkillDraft): Promise<{ name: string; path: string }> {
  return apiRequest<{ name: string; path: string }>(`${API_BASE}/api/agent/skills`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(draft),
  });
}

/** One node in an installed skill's file tree. `children` is set only on dirs. */
export interface SkillNode {
  name: string;
  path: string;
  dir?: boolean;
  size?: number;
  children?: SkillNode[];
}

/** A skill present in the active runtime's skills dir. */
export interface InstalledSkill {
  name: string;
  description?: string;
  files: SkillNode[];
  /** Whether this directory name is currently present in the skill catalog. */
  store_availability?: "in_store" | "device_only" | "unknown";
  /** Newest mtime anywhere in the skill's tree, Unix SECONDS. */
  updated_at?: number;
}

/** GET /api/agent/skills — what the ACTIVE runtime currently has installed. */
export async function listInstalledSkills(): Promise<InstalledSkill[]> {
  return apiRequest<InstalledSkill[]>(`${API_BASE}/api/agent/skills`);
}
export async function publishSkill(name: string): Promise<void> {
  await apiRequest(`${API_BASE}/api/agent/skills/publish?name=${encodeURIComponent(name)}`, { method: "POST" });
}

/** GET /api/agent/skills/files — one installed skill's files with text inlined. */
export async function readSkillFiles(name: string): Promise<SkillBundle> {
  return apiRequest<SkillBundle>(
    `${API_BASE}/api/agent/skills/files?name=${encodeURIComponent(name)}`);
}

/** POST /api/agent/skills/upload — installs a `.skill`/`.zip` the operator picked from their machine. */
export async function uploadSkill(file: File): Promise<{ name: string; path: string }> {
  const body = new FormData();
  body.append("file", file);
  // No Content-Type: the browser sets the multipart boundary.
  return apiRequest<{ name: string; path: string }>(
    `${API_BASE}/api/agent/skills/upload`, { method: "POST", body });
}

/** DELETE /api/agent/skills — removes the skill from the ACTIVE runtime's skills dir. */
export async function deleteSkill(name: string): Promise<{ name: string; path: string }> {
  return apiRequest<{ name: string; path: string }>(
    `${API_BASE}/api/agent/skills?name=${encodeURIComponent(name)}`, { method: "DELETE" });
}

/** POST /api/agent/skills/install — device downloads the catalog's `.skill` archive and extracts it into the ACTIVE runtime's skills dir. */
export async function installStoreSkill(
  id: string, name?: string,
): Promise<{ name: string; path: string }> {
  return apiRequest<{ name: string; path: string }>(`${API_BASE}/api/agent/skills/install`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ id, name }),
  });
}

export async function logout(): Promise<boolean> {
  setApiToken("");
  return apiRequest<boolean>(`${API_BASE}/api/logout`, { method: "POST" });
}

/** POST /api/schedule — queue a create; a 202 proposal that runs only once the backend confirms. */
export async function createSchedule(body: ScheduleWriteBody): Promise<SchedulePendingResult> {
  return apiRequest<SchedulePendingResult>(`${API_BASE}/api/schedule`, {
    method: "POST",
    body: JSON.stringify(body),
  });
}

/** PATCH /api/schedule/:id — queue a device-originated edit. */
export async function updateSchedule(
  id: string,
  body: Partial<ScheduleWriteBody>,
): Promise<SchedulePendingResult> {
  return apiRequest<SchedulePendingResult>(`${API_BASE}/api/schedule/${encodeURIComponent(id)}`, {
    method: "PATCH",
    body: JSON.stringify(body),
  });
}

/** DELETE /api/schedule/:id — queue a delete; the task keeps running until the backend confirms. */
export async function deleteSchedule(id: string): Promise<SchedulePendingResult> {
  return apiRequest<SchedulePendingResult>(`${API_BASE}/api/schedule/${encodeURIComponent(id)}`, {
    method: "DELETE",
  });
}

export interface PiperCatalogEntry {
  name: string;
  language: string;
  lang_code: string;
  license: string;
  requires_attribution: boolean;
  size_mb: number;
  installed: boolean;
}
export interface PiperJob {
  active: boolean;
  kind: string;
  target: string;
  percent: number;
  bytes_done: number;
  bytes_total: number;
  error: string;
  done: boolean;
}
export interface PiperStatus {
  engine_installed: boolean;
  voices_installed: string[];
  default_voice: string;
  catalog: PiperCatalogEntry[];
  job: PiperJob;
}

/** Reply to an install/download request. `job` is present when one started. */
export interface PiperJobStart {
  status: string;
  already?: boolean;
  job?: PiperJob;
  message?: string;
}

/** Put one settings section back on the credentials the device shipped with. */
export async function restoreAutonomousDefaults(
  section: "llm" | "voice" | "realtime",
): Promise<boolean> {
  return apiRequest<boolean>(`${API_BASE}/api/device/restore-defaults`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ section }),
  });
}

export async function getPiperStatus(): Promise<PiperStatus> {
  return apiRequest<PiperStatus>(`${API_BASE}/api/voice/piper/status`);
}

/** Install the Piper engine. Already-installed returns ok, not an error. */
export async function installPiperEngine(): Promise<PiperJobStart> {
  return apiRequest<PiperJobStart>(`${API_BASE}/api/voice/piper/install`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: "{}",
  });
}

/** Delete a downloaded voice. Refused for the voice currently in use. */
export async function removePiperVoice(name: string): Promise<PiperJobStart> {
  return apiRequest<PiperJobStart>(`${API_BASE}/api/voice/piper/voice/remove`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name }),
  });
}

/** Download one catalogue voice (~63 MB). */
export async function installPiperVoice(name: string): Promise<PiperJobStart> {
  return apiRequest<PiperJobStart>(`${API_BASE}/api/voice/piper/voice`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name }),
  });
}
