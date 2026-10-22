# Photo product activation: tool-budget diagnostic

Activation is **not ready**. This is a host diagnostic of the existing
product configuration, not a product activation or a result on a lamp.
No source, test, unit or runtime-policy change is retained by this checkpoint.

## Build and method

The exact parent was `8613304e47c9ea16909e16218436597fcb5cb993`.
The temporary test `photo_face_activation_tool_budget_diagnostic` was added
only to `crates/update/tests/functions.rs`, using its existing `product_with`
child-process harness. The test source was copied to private evidence and
that file restored byte for byte to its parent afterwards.

That harness loads the product's installed skill files and Actions catalog,
reads the real product Voice unit's environment, removes inherited `LAMP_`
settings, and invokes Voice's `tools::arguments_from_environment`, `take`,
`declared`, `admitted` and the second route's `chat_tools`. It substitutes
temporary catalog locations, uses a synthetic search credential, and makes
no provider request. It measures tool admission, not hearing, recognition,
consent, playback or actual service behavior on a lamp.

The diagnostic compared the product unchanged, the same environment with
`LAMP_PHOTO_FACES=off`, and with `LAMP_PHOTO_FACES=on`. Baseline and explicit
off had exactly equal declared tools, chat tools, link rejections and stderr.
The test prints every resulting name and stderr; the public
[census](census.md) retains counts, names and configuration only.

From `lampOS/`, with the pinned toolchain on the Intel macOS host:

```sh
CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 cargo test -p lamp-update --test functions photo_face_activation_tool_budget_diagnostic -- --exact --nocapture --test-threads=1
```

The run used the shared host `CARGO_TARGET_DIR`; no device was accessed.
**1 passed, 0 failed, 0 ignored, 4 filtered out**, compilation 52.68 s and
test execution 0.18 s. This is one diagnostic invocation, not the full
package, workspace or native ARM64 gate. An initial invocation from the
repository root failed before compilation because it had no `Cargo.toml`;
that failure is retained separately and is not counted as a test failure.
After formatting, `git diff --check` passed; formatting changed no behavior.

## Result

| Configuration | Live declared/admitted | Second-route chat |
|---|---|---|
| Product baseline and explicit off | 31; all 17 Actions functions and Dance | 32; all Actions, Dance and `web_search`; `fetch_page` omitted |
| Photo on | 32; `photo_face_yes` and `photo_face_no` present, **`snooze_reminder` omitted** | 32; all Actions, Dance and photo yes/no present; **`the_time`, `web_search`, `fetch_page` omitted** |

Baseline stderr reports the expected `fetch_page` cut. With photo on,
Voice also emits the error that its own and core skills' functions exceed
the 32-tool bound and names `snooze_reminder` as omitted from Live. The
second route separately names the time/search/fetch cuts. The cap and
priority policy were not changed.

The link's `left_out` list is empty in all three cases: the earlier Voice
admission stage already removed these functions. An empty list therefore
does not prove that every product capability reaches the model. The result
must not be made green by only increasing the fixture's expected count or
by silently accepting the missing product tools.

## Evidence and remaining gates

Private raw evidence is under
`/private/tmp/fourth-photo-product-checks-20261022/`:

- `tool-budget-diagnostic.log`: initial wrong-directory invocation;
- `tool-budget-diagnostic-retry.log`: successful command, stdout and stderr;
- `tool-budget-diagnostic.json`: extracted full cases including fixed diagnostic stderr;
- `functions-with-diagnostic.rs`: the exact temporary test source as formatted.

The raw logs were captured before the formatting-only pass. The public JSON
contains no face, photo, person name, credential, private catalog path or
conversation data. No picture was used by this host diagnostic.

First must decide the tool-budget treatment and settle the separate
installable-skill scope question on #2856. The present behavior is controlled
by Cloud/Voice flags and Eyes' photo-directory option, without a photo-face
skill directory; this checkpoint supplies no installable-skill migration.

After that decision, the product-unit piece still needs a cross-unit
contract regression deriving the real Cloud photo place, Eyes directory
and request socket, and Voice socket from the installed units. It must
check matching absolute paths, recogniser prerequisites, setup conditions,
tmpfiles 0700 ownership and the matching Cloud writable path. Explicit off,
own-account routing and existing consent/deletion/privacy tests remain
required, together with local affected-package checks and exact-head
native ARM64 and Clippy gates.

Mobile's authorized lamp-0c89 acceptance remains separate: a full update,
app selfie, one question and a yes that keeps the face; #2854's
left/middle/right recognition and #2855's 5-of-5 immediate answers.
Keep only counts, times and build identities in the public result. Fourth
has no lamp and performed no SSH, install, restart or person-facing test.
