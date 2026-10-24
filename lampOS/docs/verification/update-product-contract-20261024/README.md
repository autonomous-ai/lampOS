# Updater product contract: static audit, 24 October 2026

Second, item 6 of First's ship-critical list, claim Queue 6383009310.
**No lamp, SSH, key, release, service or switch was changed.** Second owns
no lamp. This checks repository contracts and existing native controls;
product-versus-effective-running equivalence remains unobserved.

[Exact source comparison](source-comparison.json): all **16 files** are
byte-identical at published release 6592 (`c6dfe9809`), qualified native
preview `1afef8749`, and current lamp-voice `5238a6ecd`. The sample includes
all four updater/recovery/apply product units, ten updater runtime files,
the publisher and ARM64 workflow. No static delta in this sampled contract
requires a new implementation or gate. This is not a whole-repository or
installed-unit identity assertion.

| Contract | Source result | Holder observation still required |
| --- | --- | --- |
| Normal update owner | `lamp-update.service` starts `/opt/lampOS/update/current/lamp-update serve`, HTTPS channel, installed key, `--apply on`; outside lampos.target | Exact effective argv, fragment/drop-ins, selected channel/key path and active PID/executable |
| Persistent state and sandbox | root, `ProtectSystem=strict`, `StateDirectory=lampos-update`, allowed `/opt/lampOS`, unit and skill-state paths; fixed fallback outside round's write set | Actual service sandbox/mount permissions and retained before/after state |
| Independent fallback | install copies a qualified updater to `/usr/local/lib/lampos/lamp-update-back`; `OnFailure` and fixed-path `back` unit use it | Plain executable identity/mode, exact unit/override and usable recovery access |
| Recovery timer | install enables `lamp-update-back.timer`; 2-minute boot/active intervals, 30-second accuracy; recovery readiness requires loaded service plus persistently enabled, active waiting/running timer targeting that service | Real `systemctl show` record and timer arm state; inactivity of the oneshot service alone is normal |
| Apply helper | fixed fallback `apply`, root, private network, narrow manifest path policy, update-key directory read-only under its writable `/etc` | Effective unit/overrides and real signed request/restoration; no direct manual apply |
| Optional capabilities | `LAMP_UPDATE_SELF`, `LAMP_UPDATE_MODELS`, `LAMP_UPDATE_APPLY`, `LAMP_UPDATE_IMAGE` are absent from active product assignments and accept only literal `on` in code | Exact allowlisted effective values; no activation inferred from comments or installed binary presence |
| Published payload coverage | 6592: 32 files; 16 units for 4ace, 12 for 52e6; no model/settings/image sections and no updater/back/apply unit entries | Eligible newer-than-installed target and actual selected-lamp diff; absent categories remain NOT RUN |

The normal `--apply on` argument permits ordinary part updates; it does
**not** turn on the four optional environment switches. They remain off
by policy until their separate attended acceptance. Do not enable them as
an incidental audit fix. Their definitions and pending acceptance are in
updates.md; switches.md's stated census scope excludes the updater.

The published per-lamp manifest's unit list does not carry updater/helper
service definitions. The installer embeds the product definitions and
installs the fixed fallback. A successful ordinary published round therefore
does not establish that those installed definitions or fallback were
refreshed. They must be checked against the actual installer provenance;
never run install during a baseline capture to make the comparison pass.
The fallback readiness query accepts installed service states and timer
arm state; it does not certify FragmentPath or arbitrary overrides.

## Existing controls, not a new execution

The [12 selected PASS lines](selected-existing-native-controls.txt) and
[binding](control-binding.json) come from original official ARM64
**40083987635**, tested merge `1afef8749`; sampled source is unchanged at
today's audited head. They include current-link product placement, timer
versus oneshot enablement, older-binary argv compatibility, valid recovery
and refusal of missing, masked, runtime-only or malformed recovery state.
The whole run had **11,213 PASS + 1 LEAK**, zero failed/unrun, 106 skipped;
that separate Voice LEAK is not cleared by this audit. No suite was rerun
for unchanged source and no local ARM build/test was performed.

## Minimal holder readback for item 6

After its normal access approval and slot check, the holder retains full
raw unit/configuration bytes privately, then reports only scrubbed status,
exact binary/unit/drop-in hashes and the four allowlisted on/off values.
Do not publish raw Environment fields, credentials, private URLs or records.

```sh
systemctl show --no-pager   --property=Id,LoadState,UnitFileState,ActiveState,SubState,Unit   -- lamp-update-back.service lamp-update-back.timer
systemctl show lamp-update.service   -p FragmentPath -p DropInPaths -p ExecStart -p StateDirectory   -p ProtectSystem -p ReadWritePaths -p ProtectHome -p PrivateTmp
readlink -f /opt/lampOS/update/current/lamp-update
sha256sum /opt/lampOS/update/current/lamp-update /usr/local/lib/lampos/lamp-update-back
```

The holder additionally hashes the effective fragments/drop-ins, checks
plain file/mode and original installed A/state/pending/refusal identities,
compares safe switch values and confirms no competing updater. Repository
units, an enabled timer alone or the stale LAMP.md are not live evidence.
Use the separate [attended update worksheet](../update-lamp-acceptance-20261024/README.md)
for the actual service/sandbox update, quiet boundary, per-part old-state
readback, failed-candidate put-back and restoration. #2810 physical power
loss and #1812 positive real-key trust remain separately required.
