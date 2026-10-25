#!/bin/sh
# Exercise only the output target with text fixtures; no image build or device.
# An optional Makefile path permits a controlled comparison with an old recipe.
# MAKE may select a concrete make executable rather than a platform launcher.
set -eu

if [ "$#" -gt 1 ]; then
    echo "usage: $0 [Makefile]" >&2
    exit 2
fi
script_dir=$(CDPATH= cd "$(dirname "$0")" && pwd)
makefile=${1:-"$script_dir/../Makefile"}
make_command=${MAKE:-make}
case "$makefile" in
    /*) ;;
    *) makefile="$(pwd)/$makefile" ;;
esac
[ -f "$makefile" ] || { echo "Makefile not found: $makefile" >&2; exit 2; }
fixture_root=$(mktemp -d "${TMPDIR:-/tmp}/v2-image-output.XXXXXX")
trap 'rm -rf "$fixture_root"' 0
trap 'exit 129' 1
trap 'exit 130' 2
trap 'exit 143' 15
passed=0
# The full Makefile eagerly probes Git for its unrelated release version.
# Refuse that probe locally so the output-only fixtures need no repository.
mkdir "$fixture_root/bin"
printf '#!/bin/sh\nexit 1\n' > "$fixture_root/bin/git"
chmod +x "$fixture_root/bin/git"

fail() {
    echo "FAIL: $case_name: $*" >&2
    cat "$case_dir/stdout" "$case_dir/stderr" >&2
    exit 1
}

new_case() {
    case_name=$1
    case_dir="$fixture_root/$case_name"
    mkdir -p "$case_dir/output/lamp-v2"
}

artifact() {
    printf '%s\n' "$2" > "$case_dir/output/lamp-v2/$1"
}

expect_artifact() {
    printf '%s\n' "$2" > "$case_dir/expected"
    cmp -s "$case_dir/expected" "$case_dir/output/lamp-v2/$1" ||
        fail "$1 missing or changed"
}

run_make() {
    (
        cd "$case_dir"
        # Prevent inherited make flags or removed options affecting the fixture.
        unset MAKEFLAGS MFLAGS MAKEOVERRIDES MAKEFILES CASE_COLOR
        PATH="$fixture_root/bin:$PATH"
        export PATH
        # Supply a fixed release identifier; no image-output assertion uses it.
        "$make_command" --no-print-directory -f "$makefile" v2-image-output \
            VERSION=fixture TARGET=opi DEVICE_TYPE=lamp-v2 "$@"
    ) > "$case_dir/stdout" 2> "$case_dir/stderr"
}

expect_success() {
    run_make "$@" || fail "output target failed"
    [ ! -s "$case_dir/stderr" ] || fail "successful target wrote an error"
}

expect_report() {
    printf '%s\n' "$1" > "$case_dir/expected"
    cmp -s "$case_dir/expected" "$case_dir/stdout" || fail "wrong output report"
}

expect_refusal() {
    expected_error=$1
    shift
    if run_make "$@"; then
        fail "missing or empty selected artifact was accepted"
    fi
    grep -F "$expected_error" "$case_dir/stderr" >/dev/null ||
        fail "failure did not identify the selected missing artifact"
    [ ! -s "$case_dir/stdout" ] || fail "failure reported a built image"
}

pass() {
    passed=$((passed + 1))
    echo "PASS: $case_name"
}

new_case raw_pro_preserves_compressed_outputs
artifact lamp-os-v2.img current-raw
artifact lamp-os-v2.img.xz previous-compressed
artifact lamp-os-v2-pro.img.xz previous-pro
expect_success COMPRESS=0 VARIANT=pro
expect_artifact lamp-os-v2.img current-raw
expect_artifact lamp-os-v2.img.xz previous-compressed
expect_artifact lamp-os-v2-pro.img.xz previous-pro
expect_report '==> Built raw image: output/lamp-v2/lamp-os-v2.img'
pass

new_case compressed_pro_renames_current_output
artifact lamp-os-v2.img previous-raw
artifact lamp-os-v2.img.xz current-compressed
artifact lamp-os-v2-pro.img.xz previous-pro
expect_success COMPRESS=1 VARIANT=pro
[ ! -e "$case_dir/output/lamp-v2/lamp-os-v2.img.xz" ] || fail "source was not renamed"
expect_artifact lamp-os-v2-pro.img.xz current-compressed
expect_artifact lamp-os-v2.img previous-raw
expect_report '==> Built image: output/lamp-v2/lamp-os-v2-pro.img.xz'
pass

new_case compressed_standard_keeps_same_path
artifact lamp-os-v2.img.xz current-compressed
expect_success COMPRESS=1 VARIANT=standard
expect_artifact lamp-os-v2.img.xz current-compressed
expect_report '==> Built image: output/lamp-v2/lamp-os-v2.img.xz'
pass

new_case default_compression_keeps_same_path
artifact lamp-os-v2.img.xz current-compressed
expect_success COMPRESS= VARIANT=
expect_artifact lamp-os-v2.img.xz current-compressed
expect_report '==> Built image: output/lamp-v2/lamp-os-v2.img.xz'
pass

new_case missing_raw_rejects_previous_compressed
artifact lamp-os-v2.img.xz previous-compressed
expect_refusal 'ERROR: raw V2 image missing' COMPRESS=0 VARIANT=pro
expect_artifact lamp-os-v2.img.xz previous-compressed
[ ! -e "$case_dir/output/lamp-v2/lamp-os-v2-pro.img.xz" ] || fail "old output was renamed"
pass

new_case missing_compressed_rejects_previous_outputs
artifact lamp-os-v2.img previous-raw
artifact lamp-os-v2-pro.img.xz previous-pro
expect_refusal 'ERROR: compressed V2 image missing' COMPRESS=1 VARIANT=pro
expect_artifact lamp-os-v2.img previous-raw
expect_artifact lamp-os-v2-pro.img.xz previous-pro
pass

new_case empty_raw_is_rejected
: > "$case_dir/output/lamp-v2/lamp-os-v2.img"
artifact lamp-os-v2.img.xz previous-compressed
expect_refusal 'ERROR: raw V2 image missing' COMPRESS=0 VARIANT=pro
expect_artifact lamp-os-v2.img.xz previous-compressed
[ -f "$case_dir/output/lamp-v2/lamp-os-v2.img" ] &&
    [ ! -s "$case_dir/output/lamp-v2/lamp-os-v2.img" ] || fail "empty raw output changed"
pass

new_case empty_compressed_is_rejected
: > "$case_dir/output/lamp-v2/lamp-os-v2.img.xz"
artifact lamp-os-v2.img previous-raw
expect_refusal 'ERROR: compressed V2 image missing' COMPRESS=1 VARIANT=pro
expect_artifact lamp-os-v2.img previous-raw
[ -f "$case_dir/output/lamp-v2/lamp-os-v2.img.xz" ] &&
    [ ! -s "$case_dir/output/lamp-v2/lamp-os-v2.img.xz" ] || fail "empty compressed output changed"
pass

echo "$passed passed; 0 failed"
