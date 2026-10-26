# Reset stop ordering: source support

Read-only support for the factory onboarding residue investigation, against lampOS ba950f289b and upstream systemd v252. Third owns the repair. No systemd experiment, lamp command, reset, device mutation or physical acceptance ran here.

## Observed source

- Product `lamp-voice.service` has `PartOf=lampos.target` and `WantedBy=lampos.target`, with default dependencies enabled. Product `lampos.target` also retains default dependencies.
- Setup's product reset plan includes the entire `/var/lib/lampOS/voice/onboarding` directory. `reset::reset` records intent, removes the setup marker, runs the stop command, then wipes. A missing product wipe entry is not supported by this source.
- [systemd v252 systemctl-start-unit.c](https://github.com/systemd/systemd/blob/v252/src/systemctl/systemctl-start-unit.c#L119-L135) reads the requested unit job's object path and adds that path to its wait set. Lines 332–347 iterate explicitly named units and wait on the accumulated set. Propagated jobs are not individually inserted in that loop.
- [systemd 252 target defaults](https://www.freedesktop.org/software/systemd/man/252/systemd.target.html) order a target after its wanted services when the applicable default dependencies remain enabled. [Unit ordering](https://www.freedesktop.org/software/systemd/man/252/systemd.unit.html#Before=) reverses that relation during shutdown.

## Inference and limits

These sources support the candidate that `systemctl stop lampos.target` can finish its watched target job before Voice's propagated stop completes. A successful target stop therefore does not alone prove all data writers have quiesced before reset wipes retained directories. A surviving writer could rewrite cached progress, but this source audit does not demonstrate that the affected lamp did so.

The repair needs an explicit completion barrier for the relevant writers and a delayed-stop regression; reset must retain its failure/recovery journal behavior. The effective lamp units, plan and reset invocation must still be bound to incident evidence. Other lamp-specific maintained plans may separately omit onboarding; do not conflate that drift with this ordering candidate.

The primary-source finding was posted on Queue comment 6414697205. No source fix, runtime test or acceptance result is claimed by this record.

## Accepted keys helper: separate activation prerequisite

Third's common-helper coverage audit also applies to own-keys activation #3510. On ba950 and the 20bc carry, `lamp-setup-keys.socket` has `PartOf=lampos.target`, but `lamp-setup-keys@.service` has no target membership. In setup `main.rs::own_keys`, peer authentication precedes the request. `keys::check_and_keep` waits for the provider verdict before `Store::keep`, which can recreate the directory and write credentials. That write precedes the helper's reply to Cloud; a broken reply socket does not undo it. There is no reset-generation check between the provider result and storage.

This exposes a source-level post-wipe write opportunity if an accepted helper survives reset; no device reproduction or actual credential was used. #3510 remains draft until accepted-helper quiescence and a delayed-provider regression cover this interleaving, in addition to #2094 and current-lamp acceptance. Third owns that separate helper repair; the onboarding-only barrier must not be described as fixing it. Source finding: PR comment 6415101391 and Queue comment 6415101850.
