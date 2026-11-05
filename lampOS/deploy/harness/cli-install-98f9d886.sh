#!/bin/sh
# harness — one-line installer, served from the CDN at
# https://cdn.autonomous.ai/harness/cli/install.sh (source of truth: THIS file, cli/scripts/install.sh in
# the autonomous-harness repo — published with `make upload-cli-install-sh` from the repo root, and
# checked by cli/src/scripts/install.spec.ts). Not the same thing as cli/scripts/install-cli.sh, which
# installs the CLI from a working tree for local development and publishes nothing.
# The website (autonomous-code, apps/web) still answers the OLD URL, https://harness.autonomous.ai/cli/
# install.sh, but only as a redirect to the CDN one (its next.config.js) — kept for anyone with the old
# link already saved.
#   curl -fsSL https://cdn.autonomous.ai/harness/cli/install.sh | bash               # the CLI and hn
#   curl -fsSL https://cdn.autonomous.ai/harness/cli/install.sh | sh -s -- --desktop  # Desktop: runtime + CLI only
#   curl -fsSL https://cdn.autonomous.ai/harness/cli/install.sh | sh -s -- --host     # Desktop: host requirements only
#   hn                            # Harness in this terminal: starts locally, no login required
#   harness login
#   harness start
#   harness remote-password set   # so your other machines (and `harness remote`) can reach this one
#
# Downloads the self-contained CLI bundle from the public GCS manifest and installs a `harness` command.
# Login and start are deliberately separate: login saves a native SSO session, while start launches the
# adapter from that session. The running daemon auto-updates itself thereafter (polls the same manifest).
# There is no system Node prerequisite. The CLI is plain JS (no native binary, no code-signing) and it runs on a
# checksum-verified Node this installer puts in ~/.harness/runtime — the same runtime Desktop Harness
# manages, shared on purpose so one cli.js is never run by two different Nodes. Your own Node and nvm
# are neither required nor touched.
#
# What the CLI RUNS is a short list — tmux (its only terminal backend), `ps`, and on a Linux desktop
# the clipboard helper for the active display — and step 1 checks exactly that list. Everything else
# here (Homebrew, apt, curl/tar/sed/awk/sha256sum) is only a way of obtaining one of those or of
# downloading a runtime, and is looked at only when the thing it obtains is missing: a Mac with tmux
# never hears about Homebrew, and the download tools are checked only when something is downloaded.
# On a Mac without tmux, Homebrew is used if it is already there; otherwise tmux comes the way Node
# does — a checksum-verified MANAGED build from our manifest into ~/.harness/runtime, no compiler,
# no package manager and no password (see cli/scripts/build-managed-tmux.sh).
# Desktop mode (`--desktop`) trusts the app's own host pre-flight and skips step 1; host mode
# (`--host`) is the app handing step 1 to a real terminal for its password prompts and stops after it.
# The runtime's absolute path is baked into the launcher, so a Finder launch — where PATH is
# launchd's bare /usr/bin:/bin:/usr/sbin:/sbin — resolves the CLI exactly like a terminal does.
# POSIX sh (so `| sh` and `| bash` both work).
set -eu

# No argument is the complete standalone/server installer. Desktop owns host pre-flight and passes
# --desktop so this script installs only the managed runtime and CLI instead of asking twice for the
# same package-manager/admin work — or --host for the opposite half.
INSTALL_MODE=standalone
case "${1:-}" in
  "") ;;
  --desktop)
    INSTALL_MODE=desktop
    shift
    ;;
  --host)
    INSTALL_MODE=host
    shift
    ;;
  *)
    echo "✗ Unsupported installer argument: $1" >&2
    echo "  This installer no longer accepts a machine token." >&2
    echo "  Run the installer without arguments, then: harness login && harness start" >&2
    exit 2
    ;;
esac
if [ "$#" -gt 0 ]; then
  echo "✗ $INSTALL_MODE mode accepts no additional arguments." >&2
  echo "✗ This installer no longer accepts a machine token." >&2
  exit 2
fi

METADATA_URL="${HARNESS_METADATA_URL:-https://storage.googleapis.com/s3-autonomous-upgrade-3/harness/cli/metadata.json}"
# Published by `make upload-node-runtime` in the desktop repo; the desktop app reads the same manifest
# (its --dart-define is spelled the same) so both installers land on identical bytes.
RUNTIME_METADATA_URL="${HARNESS_RUNTIME_METADATA_URL:-https://storage.googleapis.com/s3-autonomous-upgrade-3/harness/runtime/metadata.json}"
# Published by release-tmux-runtime.yml (`make upload-tmux-runtime`): macOS tmux built against static
# libevent/ncurses, its own manifest because the Node one already has a "darwin-arm64" key.
TMUX_METADATA_URL="${HARNESS_TMUX_METADATA_URL:-https://storage.googleapis.com/s3-autonomous-upgrade-3/harness/runtime/tmux/metadata.json}"
# Published by release-grid-runtime.yml: the grid CLI at the version this harness release PINS, in its
# own manifest for the reason tmux has one. This installer lays the first one down (step 3b); the
# daemon follows the pin on every start after that (ensureManagedGrid, cli/src/lib/runtimeInstall.ts).
GRID_METADATA_URL="${HARNESS_GRID_METADATA_URL:-https://storage.googleapis.com/s3-autonomous-upgrade-3/harness/runtime/grid/metadata.json}"
# Published by release-tui.yml: hn, the native terminal client, one static binary per platform.
TUI_METADATA_URL="${HARNESS_TUI_MANIFEST_URL:-https://storage.googleapis.com/s3-autonomous-upgrade-3/harness/tui/metadata.json}"
HARNESS_KEY="${HARNESS_KEY:-cli}"
CLI_DIR="$HOME/.harness/cli"
RUNTIME_DIR="$HOME/.harness/runtime"
CURRENT_NODE_FILE="$RUNTIME_DIR/current-node"
CURRENT_TMUX_FILE="$RUNTIME_DIR/current-tmux"
CURRENT_GRID_FILE="$RUNTIME_DIR/current-grid"
BIN_DIR="$HOME/.local/bin"
LAUNCHER="$BIN_DIR/harness"
# Where `harness tui` looks for hn's binary (cli/src/tui/index.ts), and the `hn` command.
TUI_BIN="$HOME/.harness/bin/harness-tui"
HN_LAUNCHER="$BIN_DIR/hn"

# The PATH of the terminal this runs in, before the steps below put ~/.local/bin on the installer's
# own: the closing advice is about the person's PATH. Checked against the installer's, the advice to
# fix it never printed, and on a fresh Mac the `hn` it said to type was "command not found" (macOS VM,
# 2026-10-09).
ORIGINAL_PATH="$PATH"

# A computer with no coding agent and no Harness yet, before this install changes that: hn's first
# window opens on OpenCode there, so step 4b downloads it, as the desktop app does during its setup
# (desktop/lib/bootstrap/agent_prefetch.dart `alreadyHasAnAgent`, the same places).
no_agent_yet() {
  for path in "$HOME/.harness/cli" "$HOME/.claude" "$HOME/.codex" "$HOME/.opencode/bin/opencode" \
      "$HOME/.bun/bin/opencode" "$HOME/.config/opencode" "$HOME/.local/share/opencode"; do
    [ -e "$path" ] && return 1
  done
  for name in claude codex opencode; do
    command -v "$name" >/dev/null 2>&1 && return 1
  done
  return 0
}
NEW_COMPUTER=0
if no_agent_yet; then NEW_COMPUTER=1; fi

# Shared by every download in this file — tmux in step 1 and Node in step 2 — so both manifests are
# read by one implementation. Everything here must run in plain POSIX sh: there is no Node yet.

# Retry interrupted connections without requiring a recent curl's --retry-all-errors.
# Buffer manifest stdout per attempt: a partial response must never prefix the next JSON document.
download() {
  for _download_attempt in 1 2 3; do
    if _download_stdout="$(curl -fsSL "$@" --connect-timeout 20 --max-time 600)"; then
      printf '%s' "$_download_stdout"
      return 0
    else
      _download_status=$?
    fi
    case "$_download_status" in
      6|7|18|28|35|52|55|56|92) ;; # DNS, connection, partial transfer, timeout, TLS reset, HTTP/2
      *) return "$_download_status" ;; # HTTP refusal, certificate verification, local file error
    esac
    [ "$_download_attempt" -lt 3 ] || return "$_download_status"
    echo "  ▸ Connection interrupted; retrying download ($((_download_attempt + 1))/3)…" >&2
    sleep 2
  done
}

# sha256 is the one tool that genuinely differs between the two platforms; the pair smooths it over.
sha256_of() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    echo "✗ Need shasum or sha256sum to verify a download, and found neither." >&2
    exit 11
  fi
}

# Our manifests are published one key per line; pull just one platform's object out and read its
# fields. No jq/python dependency, which a bare machine may equally not have.
manifest_entry() { # <manifest json> <platform>
  printf '%s\n' "$1" | sed -n "/\"$2\"[[:space:]]*:[[:space:]]*{/,/}/p"
}
entry_field() { # <entry> <key>
  printf '%s\n' "$1" | sed -n "s/.*\"$2\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p" | head -1
}

# The platform key our manifests use for this computer, or nothing on one we publish nothing for.
manifest_platform() {
  case "$(uname -s)" in
    Darwin) _os=darwin ;;
    Linux)  _os=linux ;;
    *)      return 0 ;;
  esac
  case "$(uname -m)" in
    arm64|aarch64) _arch=arm64 ;;
    x86_64|amd64)  _arch=x64 ;;
    *)             return 0 ;;
  esac
  printf '%s-%s\n' "$_os" "$_arch"
}

# Ensure ~/.local/bin is on PATH (per shell), idempotently — step 4 below, and host mode too, which
# may have just linked ~/.local/bin/tmux and stops before step 4.
add_path_line='export PATH="$HOME/.local/bin:$PATH"'
marker='# added by harness installer'
ensure_rc() {
  rc="$1"
  [ -f "$rc" ] || : > "$rc"
  if ! grep -qF "$marker" "$rc" 2>/dev/null; then
    printf '\n%s\n%s\n' "$marker" "$add_path_line" >> "$rc"
    echo "  ✓ added ~/.local/bin to PATH in $rc"
  fi
}
ensure_path_rc() {
case "$(basename "${SHELL:-/bin/sh}")" in
  zsh)  ensure_rc "$HOME/.zshrc" ;;
  # `.bashrc` alone is not enough. A LOGIN shell reads .bash_profile / .bash_login / .profile and never
  # .bashrc — that is `bash -lc`, `ssh host cmd`, most CI, and every `docker exec … bash -l`. On a
  # desktop or an ssh session the shell is interactive so .bashrc is read and the gap is invisible;
  # inside a container it means `harness` is never on PATH no matter how many new shells you open.
  # macOS already covered this by also writing .bash_profile (Terminal starts login shells); do the
  # same on Linux, writing whichever login file bash will actually consult — it reads the FIRST that
  # exists, so appending to a lower-priority one would be silently ignored.
  bash) ensure_rc "$HOME/.bashrc"
        if [ -f "$HOME/.bash_profile" ]; then ensure_rc "$HOME/.bash_profile"
        elif [ -f "$HOME/.bash_login" ]; then ensure_rc "$HOME/.bash_login"
        elif [ "$(uname)" = "Darwin" ]; then ensure_rc "$HOME/.bash_profile"
        else ensure_rc "$HOME/.profile"
        fi ;;
  fish) mkdir -p "$HOME/.config/fish"
        rc="$HOME/.config/fish/config.fish"; [ -f "$rc" ] || : > "$rc"
        grep -qF "$marker" "$rc" 2>/dev/null || printf '\n%s\nfish_add_path %s\n' "$marker" "$HOME/.local/bin" >> "$rc" ;;
  *)    ensure_rc "$HOME/.profile" ;;
esac
}

# The managed grid for this computer: download, verify, unpack under ~/.harness/runtime and record it
# in current-grid (what the daemon reads, like current-node). Returns non-zero, having said why,
# instead of exiting: the caller decides that a missing grid is not a failed install — the harness
# works without one, and the daemon fetches it on its next start. Laid down READ-ONLY, the bin
# directory too: `grid update` replaces the binary with a rename INTO that directory, and a directory
# it cannot write to is what makes that fail loudly instead of overwriting the pin. Never linked into
# ~/.local/bin — see the call site.
# hn — Harness in a terminal: tmux's keys and ~/.tmux.conf, every harness on every machine. Its
# binary from release-tui.yml's manifest, checksum-verified and run once before it replaces anything,
# where `harness tui` finds it. Optional like the grid: `hn` fetches the binary itself on its first
# run (`harness tui --install`), so a failure here says so and the install goes on.
install_hn() {
  platform="$(manifest_platform)"
  [ -n "$platform" ] || {
    echo "  ✗ No hn build is published for $(uname -s)/$(uname -m)." >&2
    return 1
  }
  tui_manifest="$(download "$TUI_METADATA_URL")" || {
    echo "  ✗ Could not fetch the hn manifest: $TUI_METADATA_URL" >&2
    return 1
  }
  tui_version="$(printf '%s\n' "$tui_manifest" | sed -n 's/^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -1)"
  tui_entry="$(manifest_entry "$tui_manifest" "$platform")"
  tui_url="$(entry_field "$tui_entry" url)"
  tui_sha="$(entry_field "$tui_entry" sha256)"
  if [ -z "$tui_url" ] || [ -z "$tui_sha" ]; then
    echo "  ✗ The hn manifest has no '$platform' build." >&2
    return 1
  fi
  mkdir -p "$(dirname "$TUI_BIN")"
  tui_staging="$TUI_BIN.$$.tmp"
  echo "  ▸ downloading hn${tui_version:+ $tui_version} ($platform)…"
  if ! download "$tui_url" -o "$tui_staging"; then
    echo "  ✗ Could not download $tui_url" >&2
    rm -f "$tui_staging"
    return 1
  fi
  tui_got="$(sha256_of "$tui_staging")"
  if [ "$tui_got" != "$tui_sha" ]; then
    echo "  ✗ hn download failed checksum verification (expected $tui_sha, got $tui_got)" >&2
    rm -f "$tui_staging"
    return 1
  fi
  chmod 755 "$tui_staging"
  if ! "$tui_staging" --version >/dev/null 2>&1; then
    echo "  ✗ hn does not run on this computer." >&2
    rm -f "$tui_staging"
    return 1
  fi
  mv -f "$tui_staging" "$TUI_BIN"
  echo "  ✓ installed hn${tui_version:+ $tui_version} → $TUI_BIN"
  return 0
}

install_managed_grid() {
  platform="$(manifest_platform)"
  [ -n "$platform" ] || {
    echo "  ✗ No managed grid is published for $(uname -s)/$(uname -m)." >&2
    return 1
  }
  grid_manifest="$(download "$GRID_METADATA_URL")" || {
    echo "  ✗ Could not fetch the grid manifest: $GRID_METADATA_URL" >&2
    return 1
  }
  grid_entry="$(manifest_entry "$grid_manifest" "$platform")"
  grid_url="$(entry_field "$grid_entry" url)"
  grid_sha="$(entry_field "$grid_entry" sha256)"
  grid_root="$(entry_field "$grid_entry" archiveRoot)"
  grid_version="$(entry_field "$grid_entry" version)"
  if [ -z "$grid_url" ] || [ -z "$grid_sha" ] || [ -z "$grid_root" ] || [ -z "$grid_version" ]; then
    echo "  ✗ The grid manifest has no usable '$platform' entry." >&2
    return 1
  fi
  grid_target="$RUNTIME_DIR/$grid_root"
  if [ ! -x "$grid_target/bin/grid" ]; then
    mkdir -p "$RUNTIME_DIR"
    chmod 700 "$RUNTIME_DIR" 2>/dev/null || true
    grid_staging="$RUNTIME_DIR/.grid-staging-$$"
    rm -rf "$grid_staging"
    mkdir -p "$grid_staging"
    echo "  ▸ downloading grid $grid_version ($platform)…"
    if ! download "$grid_url" -o "$grid_staging/grid.tar.gz"; then
      echo "  ✗ Could not download $grid_url" >&2
      rm -rf "$grid_staging"
      return 1
    fi
    grid_got="$(sha256_of "$grid_staging/grid.tar.gz")"
    if [ "$grid_got" != "$grid_sha" ]; then
      echo "  ✗ grid download failed checksum verification (expected $grid_sha, got $grid_got)" >&2
      rm -rf "$grid_staging"
      return 1
    fi
    if ! tar -xzf "$grid_staging/grid.tar.gz" -C "$grid_staging" || [ ! -x "$grid_staging/$grid_root/bin/grid" ]; then
      echo "  ✗ The grid archive has no $grid_root/bin/grid" >&2
      rm -rf "$grid_staging"
      return 1
    fi
    # A half-laid-down target from an earlier attempt may already be read-only; give it back first.
    chmod -R u+w "$grid_target" 2>/dev/null || true
    rm -rf "$grid_target"
    mv "$grid_staging/$grid_root" "$grid_target"
    rm -rf "$grid_staging"
  fi
  chmod 555 "$grid_target/bin/grid" "$grid_target/bin" 2>/dev/null || true
  # Its update check off, as the daemon keeps it: this binary is the pin, not grid's to replace.
  if ! GRID_NO_UPDATE_CHECK=1 "$grid_target/bin/grid" --version >/dev/null 2>&1; then
    echo "  ✗ The managed grid does not run on this computer: $grid_target/bin/grid" >&2
    return 1
  fi
  printf '%s\n' "$grid_target/bin/grid" > "$CURRENT_GRID_FILE"
  chmod 600 "$CURRENT_GRID_FILE" 2>/dev/null || true
  echo "  ✓ installed grid $grid_version → $grid_target"
  return 0
}

# 1. Host requirements (standalone and --host). The CLI runs tmux, `ps`, and on a Linux desktop the
#    clipboard helper — those are checked, and only what is missing is obtained. This installer
#    already runs in a terminal, so package-manager password prompts stay with the OS — never Harness.
require_command() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "✗ Required system tool is missing: $1" >&2
    exit 11
  }
}

apt_as_root() {
  if [ "$(id -u)" -eq 0 ]; then
    apt-get "$@"
  elif command -v sudo >/dev/null 2>&1; then
    sudo apt-get "$@"
  else
    echo "✗ Installing system packages needs root access, but sudo is unavailable." >&2
    return 1
  fi
}

install_with_apt() {
  if apt_as_root install -y "$@"; then
    return 0
  fi
  echo "▸ Refreshing package indexes before retrying"
  apt_as_root update || true
  apt_as_root install -y "$@"
}

tmux_runs() {
  command -v tmux >/dev/null 2>&1 && tmux -V >/dev/null 2>&1
}

# The managed tmux for this Mac: download, verify, unpack under ~/.harness/runtime, record it in
# current-tmux (what the daemon reads, like current-node) and link it as ~/.local/bin/tmux so every
# shell that has ~/.local/bin — which step 4 arranges — resolves the same binary the daemon runs.
install_managed_tmux() {
  platform="$(manifest_platform)"
  case "$platform" in
    darwin-*) ;;
    *)
      echo "✗ No managed tmux is published for $(uname -s)/$(uname -m). Install tmux (brew install tmux), then retry." >&2
      exit 22
      ;;
  esac
  echo "▸ Installing the managed tmux into $RUNTIME_DIR"
  tmux_manifest="$(download "$TMUX_METADATA_URL")" || {
    echo "✗ Could not fetch the tmux manifest: $TMUX_METADATA_URL" >&2
    echo "  Install tmux yourself (brew install tmux), then retry." >&2
    exit 22
  }
  tmux_entry="$(manifest_entry "$tmux_manifest" "$platform")"
  tmux_url="$(entry_field "$tmux_entry" url)"
  tmux_sha="$(entry_field "$tmux_entry" sha256)"
  tmux_root="$(entry_field "$tmux_entry" archiveRoot)"
  tmux_version="$(entry_field "$tmux_entry" version)"
  if [ -z "$tmux_url" ] || [ -z "$tmux_sha" ] || [ -z "$tmux_root" ] || [ -z "$tmux_version" ]; then
    echo "✗ The tmux manifest has no usable '$platform' entry. Install tmux yourself (brew install tmux), then retry." >&2
    exit 22
  fi
  tmux_target="$RUNTIME_DIR/$tmux_root"
  if [ ! -x "$tmux_target/bin/tmux" ]; then
    mkdir -p "$RUNTIME_DIR"
    chmod 700 "$RUNTIME_DIR" 2>/dev/null || true
    tmux_staging="$RUNTIME_DIR/.tmux-staging-$$"
    rm -rf "$tmux_staging"
    mkdir -p "$tmux_staging"
    trap 'rm -rf "$tmux_staging"' EXIT INT TERM
    echo "  ▸ downloading tmux $tmux_version ($platform)…"
    download "$tmux_url" -o "$tmux_staging/tmux.tar.gz" || {
      echo "✗ Could not download $tmux_url" >&2
      exit 22
    }
    tmux_got="$(sha256_of "$tmux_staging/tmux.tar.gz")"
    if [ "$tmux_got" != "$tmux_sha" ]; then
      echo "✗ tmux download failed checksum verification (expected $tmux_sha, got $tmux_got)" >&2
      exit 22
    fi
    tar -xzf "$tmux_staging/tmux.tar.gz" -C "$tmux_staging" || {
      echo "✗ Could not unpack the tmux archive" >&2
      exit 22
    }
    [ -x "$tmux_staging/$tmux_root/bin/tmux" ] || {
      echo "✗ The tmux archive has no $tmux_root/bin/tmux" >&2
      exit 22
    }
    rm -rf "$tmux_target"
    mv "$tmux_staging/$tmux_root" "$tmux_target"
    rm -rf "$tmux_staging"
    trap - EXIT INT TERM
  fi
  "$tmux_target/bin/tmux" -V >/dev/null 2>&1 || {
    echo "✗ The managed tmux does not run on this computer: $tmux_target/bin/tmux" >&2
    exit 22
  }
  printf '%s\n' "$tmux_target/bin/tmux" > "$CURRENT_TMUX_FILE"
  chmod 600 "$CURRENT_TMUX_FILE" 2>/dev/null || true
  mkdir -p "$BIN_DIR"
  ln -sfn "$tmux_target/bin/tmux" "$BIN_DIR/tmux"
  export PATH="$BIN_DIR:$PATH"
  echo "  ✓ installed tmux $tmux_version → $tmux_target"
}

if [ "$INSTALL_MODE" != "desktop" ]; then
case "$(uname -s)" in
  Darwin)
    if tmux_runs; then
      : # tmux runs. How it got here is nobody's business, and nothing else is looked at.
    else
      # A Homebrew installed for a different shell is still installed — and if it is here at all,
      # its tmux is preferred: it is what the owner already maintains. The prefixes are overridable
      # so a test (or an unusual install) can say where, or that there is none.
      if ! command -v brew >/dev/null 2>&1; then
        for brew_prefix in ${HARNESS_HOMEBREW_PREFIXES:-/opt/homebrew /usr/local}; do
          if [ -x "$brew_prefix/bin/brew" ]; then
            eval "$("$brew_prefix/bin/brew" shellenv 2>/dev/null || true)"
            break
          fi
        done
      fi
      if command -v brew >/dev/null 2>&1; then
        echo "▸ Installing tmux via Homebrew"
        # --force-bottle: use Homebrew's prebuilt bottle if there is one, and fail FAST if there is
        # not, rather than dragging the person into a from-source build. Homebrew stopped shipping
        # Intel (x86_64) bottles in 2025, so on an Intel Mac `brew install tmux` would otherwise
        # compile tmux + its deps and demand the Command Line Tools — the exact slow, password-and-
        # compiler path the managed build exists to avoid. When no bottle is available this returns
        # non-zero and the managed download below takes over.
        brew install --force-bottle tmux || echo "▸ No Homebrew tmux bottle for this Mac; using the managed build instead."
      fi
      # No Homebrew, or a Homebrew that could not (no bottle): the managed build. Nothing to compile,
      # nothing to ask a password for — the same checksum-verified download Node gets in step 2.
      tmux_runs || install_managed_tmux
    fi
    ;;
  Linux)
    missing_host_packages=""
    tmux_runs || missing_host_packages="$missing_host_packages tmux"
    command -v ps >/dev/null 2>&1 || missing_host_packages="$missing_host_packages procps"
    # Native image paste needs the helper for the display protocol this process will use. A
    # genuinely headless server has no OS clipboard, so installing either package there would add a
    # sudo prompt without adding a capability; the CLI deliberately uses its file-path fallback.
    clipboard_command=""
    clipboard_package=""
    if [ -n "${WAYLAND_DISPLAY:-}" ]; then
      clipboard_command="wl-copy"
      clipboard_package="wl-clipboard"
    elif [ -n "${DISPLAY:-}" ]; then
      clipboard_command="xclip"
      clipboard_package="xclip"
    else
      echo "▸ No X11 or Wayland display detected; native image clipboard is not applicable."
    fi
    if [ -n "$clipboard_command" ] && ! command -v "$clipboard_command" >/dev/null 2>&1; then
      missing_host_packages="$missing_host_packages $clipboard_package"
    fi
    if [ -n "$missing_host_packages" ]; then
      command -v apt-get >/dev/null 2>&1 || {
        echo "✗ Required Linux packages are missing:$missing_host_packages" >&2
        echo "  Automatic install supports apt-based Linux only; install them with this distribution's package manager, then retry." >&2
        exit 22
      }
      echo "▸ Installing required Linux packages (you may be asked for your password):$missing_host_packages"
      # Intentional word splitting: this list contains only package names selected above.
      # shellcheck disable=SC2086
      install_with_apt $missing_host_packages || {
        echo "✗ Could not install${missing_host_packages} via apt-get." >&2
        echo "  Retry with: sudo apt-get install -y$missing_host_packages" >&2
        # 23 is the clipboard helper's own code, kept for a transaction that was only that.
        if [ "$missing_host_packages" = " $clipboard_package" ]; then exit 23; fi
        exit 22
      }
    fi
    command -v ps >/dev/null 2>&1 || {
      echo "✗ ps is required but did not pass verification." >&2
      exit 22
    }
    if [ -n "$clipboard_command" ]; then
      command -v "$clipboard_command" >/dev/null 2>&1 || {
        echo "✗ $clipboard_command is required but did not pass verification." >&2
        exit 23
      }
      echo "✓ Linux image clipboard ready ($clipboard_command)"
    fi
    ;;
  *)
    echo "✗ Automatic Harness installation supports macOS and Linux only." >&2
    exit 10
    ;;
esac

tmux_runs || {
  echo "✗ tmux is required but did not pass verification." >&2
  exit 22
}
echo "✓ tmux ready ($(tmux -V))"
if [ "$INSTALL_MODE" = "host" ]; then
  ensure_path_rc
  echo "✓ Host requirements ready."
  exit 0
fi
else
  echo "▸ Desktop mode: trusting the app's host pre-flight; skipping system packages and tmux setup."
fi

# 2. Resolve the Node that will run the CLI (and this installer's own JSON parse + sha256 verify in
#    step 3). The CLI runs on the MANAGED runtime under ~/.harness/runtime, not on whatever Node the
#    computer happens to have:
#      1. $HARNESS_NODE_BINARY — Desktop Harness handing over the runtime it just validated.
#      2. ~/.harness/runtime/current-node — the managed runtime, if it is already here.
#      3. otherwise, download it.
#    A Node on PATH is used only as a last resort, on a platform we publish no runtime for.
#
#    Why not prefer the user's own Node: it is the version-drift bug in a different shirt. A CLI
#    driven from the desktop app and from a terminal must be ONE cli.js on ONE Node, and a `node`
#    that PATH resolves is none of stable, shared, or ours — it moves with nvm, it differs between a
#    Finder launch and a Terminal launch, and it can be upgraded out from under a running daemon.
#    Nothing outside ~/.harness is read or changed either way; the user's own Node is left alone.
#
#    Everything in this step must run in plain POSIX sh: there is no Node yet to lean on.

# Prints the major version of $1, or nothing when it cannot be run.
node_major() {
  "$1" -e 'process.stdout.write(String(process.versions.node.split(".")[0]))' 2>/dev/null || true
}

node_is_usable() {
  [ -n "${1:-}" ] || return 1
  [ -x "$1" ] || return 1
  _major="$(node_major "$1")"
  [ -n "$_major" ] || return 1
  [ "$_major" -ge 20 ] 2>/dev/null || return 1
  return 0
}

NODE_BIN="${HARNESS_NODE_BINARY:-}"
if [ -n "$NODE_BIN" ]; then
  # An explicit override is an instruction, not a hint: if it is unusable, say so rather than
  # quietly downloading a second runtime behind the caller's back.
  if [ ! -x "$NODE_BIN" ]; then
    echo "✗ HARNESS_NODE_BINARY is not an executable Node runtime: $NODE_BIN" >&2
    exit 1
  fi
  if ! node_is_usable "$NODE_BIN"; then
    echo "✗ HARNESS_NODE_BINARY is older than Node 20: $NODE_BIN ($("$NODE_BIN" -v 2>/dev/null || echo unknown))" >&2
    exit 1
  fi
fi

if [ -z "$NODE_BIN" ] && [ -r "$CURRENT_NODE_FILE" ]; then
  managed_node="$(cat "$CURRENT_NODE_FILE" 2>/dev/null || true)"
  # Only ever trust a path inside the runtime directory we own.
  case "$managed_node" in
    "$RUNTIME_DIR"/*)
      if node_is_usable "$managed_node"; then
        NODE_BIN="$managed_node"
        echo "▸ Using the managed Node runtime already installed ($("$NODE_BIN" -v))"
      fi
      ;;
  esac
fi

if [ -z "$NODE_BIN" ]; then
  # Fetch the same checksum-pinned runtime Desktop Harness uses. A Node already on PATH is
  # deliberately NOT preferred here — see the note in step 1 — but it is still the last resort on a
  # platform we publish no runtime for, so an exotic box keeps working instead of being locked out.
  node_platform=""
  case "$(uname -s)" in
    Darwin) node_os=darwin ;;
    Linux)  node_os=linux ;;
    *)      node_os="" ;;
  esac
  case "$(uname -m)" in
    arm64|aarch64) node_arch=arm64 ;;
    x86_64|amd64)  node_arch=x64 ;;
    *)             node_arch="" ;;
  esac
  [ -n "$node_os" ] && [ -n "$node_arch" ] && node_platform="$node_os-$node_arch"

  if [ -z "$node_platform" ]; then
    path_node="$(command -v node 2>/dev/null || true)"
    if [ -n "$path_node" ] && node_is_usable "$path_node"; then
      echo "▸ No managed Node runtime is published for $(uname -s)/$(uname -m) — using $path_node"
      NODE_BIN="$path_node"
    else
      echo "✗ No managed Node runtime is published for $(uname -s)/$(uname -m), and no Node >= 20 on PATH." >&2
      echo "  Install Node 20+ from https://nodejs.org, then re-run this installer." >&2
      exit 1
    fi
  fi
fi

if [ -z "$NODE_BIN" ]; then

  # Fetching, unpacking and verifying the runtime is the only work in this file that needs these
  # host tools (step 3 is Node, and Node needs none of them) — so they are asked for here, once the
  # download is known to be happening, and nowhere earlier. macOS ships them all; a minimal Linux
  # can lack curl, and that is what apt is for.
  if [ "$(uname -s)" = "Linux" ]; then
    missing_download_tools=""
    command -v curl >/dev/null 2>&1 || missing_download_tools="$missing_download_tools curl"
    command -v tar >/dev/null 2>&1 || missing_download_tools="$missing_download_tools tar"
    command -v sed >/dev/null 2>&1 || missing_download_tools="$missing_download_tools sed"
    command -v awk >/dev/null 2>&1 || missing_download_tools="$missing_download_tools gawk"
    command -v sha256sum >/dev/null 2>&1 || missing_download_tools="$missing_download_tools coreutils"
    if [ -n "$missing_download_tools" ]; then
      command -v apt-get >/dev/null 2>&1 || {
        echo "✗ Downloading the Node runtime needs tools this computer lacks:$missing_download_tools" >&2
        echo "  Automatic install supports apt-based Linux only; install them with this distribution's package manager, then retry." >&2
        exit 11
      }
      echo "▸ Installing the tools needed to download the runtime (you may be asked for your password):$missing_download_tools"
      # shellcheck disable=SC2086
      install_with_apt $missing_download_tools || {
        echo "✗ Could not install the download tools via apt-get." >&2
        exit 11
      }
    fi
  fi
  require_command curl
  require_command tar
  require_command sed
  require_command awk

  echo "▸ Installing the Harness Node runtime into $RUNTIME_DIR"
  echo "  The CLI runs on its own Node so it behaves the same from a terminal and from the app."
  echo "  Your system Node, nvm and Homebrew are not read or changed."
  runtime_manifest="$(download "$RUNTIME_METADATA_URL")" || {
    echo "✗ Could not fetch the Node runtime manifest: $RUNTIME_METADATA_URL" >&2
    exit 1
  }
  runtime_entry="$(manifest_entry "$runtime_manifest" "$node_platform")"
  node_url="$(entry_field "$runtime_entry" url)"
  node_sha="$(entry_field "$runtime_entry" sha256)"
  node_root="$(entry_field "$runtime_entry" archiveRoot)"
  node_version="$(entry_field "$runtime_entry" version)"
  if [ -z "$node_url" ] || [ -z "$node_sha" ] || [ -z "$node_root" ] || [ -z "$node_version" ]; then
    echo "✗ The Node runtime manifest has no usable '$node_platform' entry." >&2
    exit 1
  fi

  mkdir -p "$RUNTIME_DIR"
  chmod 700 "$RUNTIME_DIR" 2>/dev/null || true
  node_target="$RUNTIME_DIR/node-$node_version-$node_platform"
  if [ ! -x "$node_target/bin/node" ]; then
    node_staging="$RUNTIME_DIR/.node-staging-$$"
    rm -rf "$node_staging"
    mkdir -p "$node_staging"
    # Staging never outlives this script, however it ends — a half-unpacked runtime that looked
    # complete would be worse than no runtime at all.
    trap 'rm -rf "$node_staging"' EXIT INT TERM
    echo "  ▸ downloading Node $node_version ($node_platform)…"
    download "$node_url" -o "$node_staging/node.tar.gz" || {
      echo "✗ Could not download $node_url" >&2
      exit 1
    }
    node_got="$(sha256_of "$node_staging/node.tar.gz")"
    if [ "$node_got" != "$node_sha" ]; then
      echo "✗ Node download failed checksum verification (expected $node_sha, got $node_got)" >&2
      exit 1
    fi
    tar -xzf "$node_staging/node.tar.gz" -C "$node_staging" || {
      echo "✗ Could not unpack the Node archive" >&2
      exit 1
    }
    [ -x "$node_staging/$node_root/bin/node" ] || {
      echo "✗ The Node archive has no $node_root/bin/node" >&2
      exit 1
    }
    mv "$node_staging/$node_root" "$node_target"
    rm -rf "$node_staging"
    trap - EXIT INT TERM
  fi

  NODE_BIN="$node_target/bin/node"
  if ! node_is_usable "$NODE_BIN"; then
    echo "✗ The installed Node runtime does not run on this computer: $NODE_BIN" >&2
    exit 1
  fi
  # Recorded last, and only once the binary has answered `--version`: this file is what Desktop
  # Harness reads to pick its runtime, so it must never name something that does not work.
  printf '%s\n' "$NODE_BIN" > "$CURRENT_NODE_FILE"
  chmod 600 "$CURRENT_NODE_FILE" 2>/dev/null || true
  echo "  ✓ installed Node $node_version → $node_target"
fi

# 3. Fetch manifest, download + sha256-verify cli.js & notify.mjs, install them + the launcher (all in
#    Node so it works identically on macOS/Linux without shasum/sha256sum差异).
echo "▸ Installing harness from $METADATA_URL"
HARNESS_METADATA_URL="$METADATA_URL" HARNESS_KEY="$HARNESS_KEY" HARNESS_NODE_BINARY="$NODE_BIN" "$NODE_BIN" <<'HARNESSJS'
const fs = require('fs'), os = require('os'), path = require('path'), crypto = require('crypto')
const META = process.env.HARNESS_METADATA_URL, KEY = process.env.HARNESS_KEY, NODE = process.env.HARNESS_NODE_BINARY
const dir = path.join(os.homedir(), '.harness', 'cli')
const bin = path.join(os.homedir(), '.local', 'bin')
;(async () => {
  const res = await fetch(META)
  if (!res.ok) throw new Error('could not fetch manifest (HTTP ' + res.status + ')')
  const entry = (await res.json())[KEY]
  if (!entry || !entry.version || !entry.cli || !entry.notify) throw new Error("manifest has no valid '" + KEY + "' entry")
  fs.mkdirSync(dir, { recursive: true })
  fs.mkdirSync(bin, { recursive: true })
  const fetchVerified = async (ref, name) => {
    const r = await fetch(ref.url)
    if (!r.ok) throw new Error('HTTP ' + r.status + ' for ' + ref.url)
    const buf = Buffer.from(await r.arrayBuffer())
    const got = crypto.createHash('sha256').update(buf).digest('hex')
    if (got.toLowerCase() !== String(ref.sha256).toLowerCase()) throw new Error('sha256 mismatch for ' + name)
    fs.writeFileSync(path.join(dir, name), buf)
  }
  await fetchVerified(entry.cli, 'cli.js')
  await fetchVerified(entry.notify, 'notify.mjs')
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ type: 'module' }) + '\n')
  const shellQuote = value => "'" + value.replaceAll("'", "'\\''") + "'"
  fs.writeFileSync(path.join(bin, 'harness'), '#!/bin/sh\nexec ' + shellQuote(NODE) + ' ' + shellQuote(path.join(dir, 'cli.js')) + ' "$@"\n', { mode: 0o755 })
  console.log('  ✓ installed harness ' + entry.version + ' → ' + dir)
  // Delegate through harness so runtime repairs and update pins apply to both commands.
  // Preserve existing entries, including dangling development links. Never write through a link
  // or infer ownership from `.harness` appearing somewhere in an executable's bytes.
  const hn = path.join(bin, 'hn')
  const staged = fs.mkdtempSync(path.join(bin, '.hn-'))
  try {
    const launcher = path.join(staged, 'hn')
    fs.writeFileSync(launcher, '#!/bin/sh\nexec ' + shellQuote(path.join(bin, 'harness')) + ' tui "$@"\n', { mode: 0o755 })
    try { fs.linkSync(launcher, hn) } catch (error) {
      if (error.code !== 'EEXIST') throw error
      console.log('  · Kept existing ' + hn + '; use harness tui, or harness tui --install to migrate an old Harness build.')
    }
  } finally { fs.rmSync(staged, { recursive: true, force: true }) }
})().catch((err) => { console.error('✗ install failed: ' + err.message); process.exit(1) })
HARNESSJS

# 3b. The managed grid — the grid CLI this release pins, beside Node and tmux: the daemon shells out
#     to it, and an agent's pane runs it by name. Optional where Node and tmux are not: the harness
#     works without it (every grid call says so, in a sentence), and the daemon follows the pin on
#     every start, so a download that fails here is retried by `harness start`. Never linked into
#     ~/.local/bin — that path is grid's own installer's (uv's, on a Mac) — the daemon puts the
#     managed grid on an agent pane's PATH itself. Host mode installs no CLI, so no grid either.
#     The desktop app's first run leaves grid to its first use (the daemon's ensureGrid installs it
#     when a grid feature is first reached): it was a 17 s download in front of a new user's first
#     harness on a fresh Mac (macOS VM, 2026-10-08), for an add-on most never open.
if [ "$INSTALL_MODE" != "host" ]; then
  if [ "$INSTALL_MODE" != "desktop" ]; then
    echo "▸ Installing the managed grid into $RUNTIME_DIR"
    install_managed_grid || echo "  · grid will be set up the first time a grid feature is used"
  fi
  "$NODE_BIN" "$HOME/.harness/cli/cli.js" dsh builtins || echo "  · Core harnesses will be prepared on the next start"
fi

# 3c. hn's binary (the standalone install; Desktop's CLI fetches it on the first `hn`).
if [ "$INSTALL_MODE" = "standalone" ]; then
  echo "▸ Installing hn"
  install_hn || echo "  · hn will download itself the first time you run it"
fi

# 4. Ensure ~/.local/bin is on PATH (per shell), idempotently — defined up with the other helpers.
ensure_path_rc

# 4b. OpenCode, on a computer with no agent at all (NEW_COMPUTER) with a person at the terminal (a
#     Docker build or a provisioning script gets no agent it did not ask for): hn's first window opens
#     on it with a task typed, so it is downloaded now rather than in front of the person in that pane.
#     The CLI's recipe (cli/src/lib/engineInstall.ts) with --no-modify-path, as the desktop app runs
#     it: Harness finds ~/.opencode/bin itself. Never under --desktop, whose app downloads it beside
#     this install. After step 4, so stopping a slow download leaves a working PATH, and never longer
#     than three minutes; optional either way: a pane installs it, with the npm fallback.
#     HARNESS_INSTALL_ATTENDED=1 says a person is there when stdout is not a terminal (the specs).
if [ "$INSTALL_MODE" = "standalone" ] && [ "$NEW_COMPUTER" = 1 ] \
    && { [ -t 1 ] || [ "${HARNESS_INSTALL_ATTENDED:-}" = 1 ]; }; then
  # The mark hn's first start takes (tui/src/first_run.rs): OpenCode with a task typed, not a shell.
  mkdir -p "$HOME/.harness/tui" && : > "$HOME/.harness/tui/first-run" || true
  echo "▸ Downloading OpenCode, the free coding agent hn starts with"
  ( curl -fsSL --connect-timeout 20 https://opencode.ai/install | bash -s -- --no-modify-path ) >/dev/null 2>&1 &
  download=$!
  waited=0
  while kill -0 "$download" 2>/dev/null && [ "$waited" -lt 180 ]; do sleep 1; waited=$((waited + 1)); done
  kill "$download" 2>/dev/null || true
  wait "$download" 2>/dev/null || true
  if [ -x "$HOME/.opencode/bin/opencode" ]; then
    # Run once, `--version` only, as the desktop does: macOS checks a new binary on its first run
    # (3.9 s against 0.4 s after, fresh VM, 2026-10-09), which would be hn's first pane's. Never
    # longer than 20 s.
    "$HOME/.opencode/bin/opencode" --version >/dev/null 2>&1 &
    warm=$!
    ( sleep 20; kill "$warm" 2>/dev/null ) >/dev/null 2>&1 &
    watchdog=$!
    wait "$warm" 2>/dev/null || true
    kill "$watchdog" 2>/dev/null || true
    echo "  ✓ OpenCode ready"
  else
    echo "  · OpenCode will be installed when hn first starts it"
  fi
fi

# 5. Final verification. Use the launcher by absolute path because this process cannot update its
#    parent shell's PATH. Installation never authenticates or starts the adapter implicitly.
"$NODE_BIN" --version >/dev/null 2>&1 || { echo "✗ Managed Node verification failed." >&2; exit 30; }
"$LAUNCHER" version >/dev/null 2>&1 || { echo "✗ Harness CLI verification failed." >&2; exit 31; }
if [ "$INSTALL_MODE" = "standalone" ]; then
  tmux -V >/dev/null 2>&1 || { echo "✗ tmux verification failed." >&2; exit 32; }
fi

# The wordmark, as the sign the install is done. `printf '%s\n'` on purpose: the art holds
# backslashes and a backtick, which `echo` eats or interprets depending on the shell behind `sh`.
print_logo() {
  printf '%s\n' \
    '' \
    '    _' \
    '   | |__   __ _ _ __ _ __   ___  ___ ___' \
    '   | '"'"'_ \ / _` | '"'"'__| '"'"'_ \ / _ \/ __/ __|' \
    '   | | | | (_| | |  | | | |  __/\__ \__ \' \
    '   |_| |_|\__,_|_|  |_| |_|\___||___/___/' \
    ''
}

# The explicit commands keep browser SSO and the long-lived daemon lifecycle understandable and
# scriptable: nothing here signs in or starts anything. Desktop mode is the app installing its own
# CLI — the app takes the person through sign-in itself, so it gets the one line and not the guide.
# One line of it: `harness version` prints the version alone today, and a notice it might add
# tomorrow must not land inside this sentence.
installed_version="$("$LAUNCHER" version 2>/dev/null | head -n 1 || true)"

# An agent that installed Harness (Claude Code, Codex, any that sets AI_AGENT) reads what to run next and what only
# the person can do, in the shape every agent command ends with (docs/design/2026-10-15-agent-experience.md). Its
# shell says it is one, as the agent commands tell it (agentClient/model.ts `outsideCaller`).
ran_by_agent() {
  [ -n "${CODEX_THREAD_ID:-}" ] || [ -n "${AI_AGENT:-}" ] || [ -n "${HARNESS_AGENT_SESSION:-}" ] || [ "${CLAUDECODE:-}" = 1 ]
}
print_for_agents() {
  echo "  For an agent (Claude Code, Codex) that installed this:"
  echo ""
  echo "      Next: harness start      (starts Harness here; no account is needed)"
  echo "      Then: harness whoami     (whether you may start harnesses, and if not, what to ask the person)"
  echo ""
  echo "  Only the person can let an agent start harnesses: they sign in (harness login, in a browser)"
  echo "  and turn on agents starting agents (harness settings agents-start-agents on, in their own"
  echo "  terminal; an agent is refused). Ask them. How: https://harness.autonomous.ai/agents"
}

if [ "$INSTALL_MODE" = "desktop" ]; then
  echo ""
  echo "  harness${installed_version:+ $installed_version} installed."
else
  print_logo
  echo "  ✓ harness${installed_version:+ $installed_version} installed."
  hn_version="$("$TUI_BIN" --version 2>/dev/null | head -n 1 | cut -d' ' -f1-2 || true)"
  if [ -n "$hn_version" ]; then echo "  ✓ ${hn_version} installed."; fi
  echo ""
  # An agent reads from the top: what is for it comes first, then the person's guide.
  if ran_by_agent; then print_for_agents; echo ""; fi
  echo "  Start here — every harness on every machine, in this terminal:"
  echo ""
  # A command that works in THIS terminal: ~/.local/bin is only on new terminals' PATH (step 6).
  case ":${ORIGINAL_PATH}:" in
    *":$BIN_DIR:"*) hn_command=hn ;;
    *) hn_command="~/.local/bin/hn" ;;
  esac
  printf '      %-31s# start locally; no login required\n' "$hn_command"
  echo ""
  echo "  Or set this computer up step by step — three commands, in this order:"
  echo ""
  echo "      harness login                  # 1. sign in with your Autonomous account (opens a browser)"
  echo "      harness start                  # 2. connect this computer as a machine (runs in the background)"
  echo "      harness remote-password set    # 3. let your OTHER machines reach this one (asked once, kept)"
  echo ""
  echo "  Then, from a Harness terminal tile on any of your machines:"
  echo ""
  echo "      harness remote                 # pick a machine — the tile becomes a terminal on it"
  echo ""
  echo "  Useful:"
  echo "      harness machines               # this account's machines and their ids"
  echo "      harness status                 # is the daemon running, and which machine this is"
  echo "      harness --help                 # everything else"
  if ! ran_by_agent; then echo ""; print_for_agents; fi
fi

# 6. Make `harness` usable by NAME. We already added ~/.local/bin to your rc for NEW terminals (step 4);
#    a piped `curl … | sh` can't touch the CURRENT shell's PATH, so print the one line that fixes it here
#    now — but ONLY when ~/.local/bin isn't already on PATH (many Linux distros add it), to avoid nagging.
case ":${ORIGINAL_PATH}:" in
  *":$BIN_DIR:"*) : ;;  # already on PATH → `harness` works immediately, nothing to do
  *)
    echo ""
    echo "  'harness' is installed in ~/.local/bin. To run it in THIS terminal now:"
    echo "      export PATH=\"\$HOME/.local/bin:\$PATH\""
    echo "  New terminals already have it (or reload this one with:  exec \$SHELL)."
    ;;
esac
