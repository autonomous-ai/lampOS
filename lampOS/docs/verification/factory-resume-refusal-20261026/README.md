# Factory resume refusal

PR #3546, head e96296097 on cced42943. Native Mac stand-ins only; no lamp,
system service, motor, installation or physical acceptance.

`bash lampOS/deploy/factory/steps-test.sh`: new tests against unchanged
production fail exactly the active/inactive refused-resume controls (84 pass,
2 fail). The production guard passes all 86 checks. Both scripts pass
`bash -n`; the final patch passes `git diff --check`.

The separate resume and pause failure stubs preserve the existing pause
control. After resume returns failure, no body start, Expression stop, mood,
self-test or pause is attempted. No wait is added. Missing/non-executable
keeper behavior and self-test exit 3 behavior remain unchanged.

Raw logs and exact patches are retained beside the source and log hashes in
[gate.json](gate.json). Documentation was clarified while the fixed suite ran;
the tested scripts did not change. ARM64 and Linux Clippy are separate CI
gates, not these local shell executions.
