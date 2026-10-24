# Provisioning DEVICE_TYPE documentation read

Exact head ee17eb8e6ce3b4800887a03d631a8a8e10e28404, PR #3286, base main. Third independently read all four changed files and posted READ-OK. No source was modified.

The English/Vietnamese quick install, scripts overview and shell Usage comment all pass DEVICE_TYPE after sudo, matching the required-variable guard in the installer. Eight exact-document pipeline controls passed: the four old forms exit at the required-variable check; the four corrected forms reach the mocked setup with DEVICE_TYPE=lamp and the default metadata URL. bash -n on exact installer source and diff whitespace checks passed.

The local shims replace curl, chmod and sudo; a bash shim intercepts the final /tmp/setup.sh call. No actual sudo, network, root command, setup script or device ran, and /tmp/setup.sh was not created or modified. The earlier README command-extraction error and its limited execution boundary are retained in controls.json. No source changed between harness attempts.

This concerns legacy provisioning on main. It is not fresh lampOS product-image installation, phone setup or device acceptance evidence.
