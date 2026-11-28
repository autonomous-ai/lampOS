# Shared synthetic avatar recipe

This source recipe gives the head-facing and speaking-face studies the same
A00–A23 identities. It follows the Reviewer's
[vision-public-synthetic-20261128-v1 recipe](https://github.com/autonomous-ai/lampOS/blob/3a8934944acba1c17f3cb0267aae778410f5809e/lampOS/tools/sensors/vision-freeze-20261128-v1.json)
for #8964 and #8847. The materialized images, accessory member hashes and
realized neutral-geometry hashes are a later seal. This commit does not
authorize fitting or claim a completed dataset or accuracy result.

## Fixed identities

`identity-parameters-v1.json` contains the exact morphology values and target
application order. A00–A11 are training identities, A12–A17 tuning identities,
and A18–A23 test identities. Every variant and source family keeps that role
across studies. Do not create different morphologies under the same Axx names.

Each block of six uses six skin materials, two identities in each hair class
(none, short, long), and three with glasses. These are authored rendering
attributes; they are not labels inferred about people. All avatars use the
same clothing asset. This small synthetic family is not a population study.

`shared_identity_v1.py` applies the parameters through MPFB's services. It
performs no inference, fitting, network access or random sampling. Initialize
the pinned MPFB extension in Blender, pass its `HumanService` and
`TargetService`, its `data` directory, the local core/glasses asset roots and
one identity row to `construct_identity`. Preserve JSON insertion order for
the facial targets. The returned human and rig use scale 0.1, the default rig,
GAMEENGINE materials and subdivision level zero. Camera, independent head and
shoulder posing, lighting and rendering belong to each separately sealed
scene producer.

The anatomical face origin is zero-based MakeHuman base vertex 297 (nose tip).
The source manifest gives the neutral basis and geometry-hash definition.
Preserve neutral vertex indexing and membership; record actual evaluated
geometry and platform versions before using output. Matching names or source
parameters alone do not prove matching realized mesh bytes.

## Sources and rights

`shared-identity-source-v1.json` pins Blender 4.5.17, MPFB 2.0.18 commit
`d9648f3257a0b9712281d466245f30d656250154`, source hashes, material/accessory
names and their publisher links. MPFB's application code is GPL; its core
[asset terms](https://github.com/makehumancommunity/mpfb2/blob/d9648f3257a0b9712281d466245f30d656250154/LICENSE.ASSETS.md)
identify the mesh and morphology assets as CC0. The selected system assets
and glasses have separate CC0 records. Do not extend those terms to every
community download. Two glasses with conflicting headers were excluded.

The official full-pack mirror failed or was too slow. The producer retains
those attempts and obtains selected ZIP members using checked byte ranges,
local headers, sizes and CRCs, then hashes the extracted bytes. Full asset
member and rendered-media inventories must be sealed before fitting. Keep
assets, renders, previews and model files outside git. Each dataset must stay
below 5,000,000,000 bytes, including shared assets accounted for once.

## Verification and current scope

The source check verified all 24 identity IDs, seeds and roles, appearance
balance in every six-identity block, every referenced facial target in the
pinned MPFB tree, JSON/source hashes and Python syntax. A00's 36 head/shoulder
geometry controls agreed with the planned camera/ray labels within the
declared float32 tolerances. One excluded A00 preview exercised real skin,
eyes and clothing. These are construction checks; test images, trained
weights and lamp measurements are not part of this source handoff.
The extracted shared helper also reproduced A00's neutral geometry and
vertex-membership hashes exactly against the preview constructor.

The owner's current order is YuNet five-point `solvePnP` first, then neural
training only if the frozen geometry bars fail. Nothing here changes product
services or switches. The lead operates all board tests; no merge or product
deployment is authorized by an offline result.
