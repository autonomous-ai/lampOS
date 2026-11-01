# A lamp for a third-party agent, through the cloud (proposal)

**Status: a design for the owner's yes, before any code.** It changes one of
control.md's four decisions, "Local only: nothing of it crosses the cloud",
for one narrow door. Everything else in control.md stands. The person's
lamp-mcp on their own computer ([sdk/mcp](../sdk/mcp/README.md), #4215) is
unchanged.

## What it is for

An agent that isn't on the person's network, such as fo or Devin Cloud, can:
- ask the person's lamp to say or do something ("tell them the build
  passed", a nod, a light);
- hear back only what the person agreed to share (someone arrived, a phrase
  it registered, whether its action was done);
- be added and removed by the person, as a skill is.

## The shape

```
agent ──MCP over HTTPS──▶ backend ──MQTT (lamp-cloud's channel)──▶ lamp-cloud ──local socket──▶ lamp-control ──▶ the parts
```

1. **No new door on the lamp.** The backend serves each added agent an MCP
   endpoint with lamp-mcp's own tools. A call goes to the lamp over the device
   channel lamp-cloud already keeps (cloud.md, "The device API"), as a new kind,
   `agent.call`. lamp-cloud hands it to lamp-control on a local Unix socket, as
   a client of kind `cloud`. Nothing listens on the network that doesn't today.
2. **One vocabulary.** Behind lamp-control, an agent's call is the same op as
   a paired computer's. So it gets the same grants, leases, rates, quiet hours,
   "the person always wins", and journal line (control.md, "The journal"),
   naming the agent's id. The body's limits are the body's own, as for every
   sender.
3. **The lamp is the authority.** The lamp keeps each added agent's grant
   record, as it keeps a paired computer's. It checks every call against that
   record, whatever the backend says, so a backend fault can't widen what the
   person agreed to.

## How an agent is bound to one lamp

- **One credential for each agent and each lamp**, issued by the backend at
  the person's Allow. It is never shared across lamps or agents, and it is
  revoked with the agent. Two lamps that allow the same agent give it two
  credentials.
- **The backend refuses a call naming another lamp.** The credential carries
  the lamp it was issued for, and a call for any other lamp is refused at
  the endpoint, before anything reaches a broker.
- **The lamp still checks for itself.** `agent.call` carries the agent's id,
  and lamp-control checks every call against its own grant record for that
  agent. One the lamp holds no record for, or has revoked, is refused, whatever
  the backend passed on.
- **Rates are each agent's own**, the same table as a computer's (control.md,
  "Limits, rates and quiet hours"), counted by the lamp. One agent can't use up
  another's, or a paired computer's.
- **The journal line names the agent** by its id, and by its name when that is
  a plain word, as for a computer (control.md, "The journal").

## The person's yes

- **Added like a skill.** The app's Skills page gets an *Agents* list. Adding
  one shows its manifest in plain words, the way an installed skill's
  permissions are shown:
  - who made it, and that it is signed by that key;
  - what it may do: speak, the light, gestures;
  - what it may hear: that someone is there, and the phrases its manifest
    names. The consent line says that a phrase's match then goes through the
    cloud to that agent, where today it goes only to a computer on the
    person's own network. Only the match goes, never the words around it
    (control.md, "words never leave the lamp").

  The person taps Allow. Removing it is a revoke: its calls and any lease it
  holds end at once, as a computer's do.
- **Narrow by default, in control.md's grants** (control.md, "One vocabulary under every door"):
  - `speak` and `light`, always asked for;
  - `body` only by opt-in, and then for `gesture` alone: never the `body`
    grant's `move`, `play`, `look` (the head turning) or `joints`, which stay
    with paired computers;
  - `hear` for the phrases its manifest names, by opt-in;
  - for presence, not the `look` grant but less: a new, reduced grant,
    `presence`, that says only that someone arrived or left. That means no
    positions, no facing, no standing up and no hand in view, and never who;
  - never the camera's picture, audio, transcripts, or who someone is (as for
    lamp-mcp today).
- **Signed.** The manifest is signed with an author key, the same scheme the
  lamp already checks for a shared skill package (`lamp-skills pack
  --author`, #3931 and #3938). An update with
  another key is refused until the agent is removed. A signature says whose
  bytes these are; what the agent may do is still only what the person allowed.
- **The lamp says whose words.** The first line an agent has the lamp say in
  a sitting begins with its name ("A message from fo: …"), so the person knows
  who speaks through their lamp. This is the living-thing rule (rule 8 of the
  repository's root [CLAUDE.md](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/main/CLAUDE.md)) applied to speech that isn't the lamp's own.

## Offline, asleep, private or quiet: refused, never queued

A call while the lamp is offline, asleep, paused or private is refused, with
the same refusal a computer gets. So is a `say` or `sound` in the lamp's
quiet hours, with `quiet_hours` (control.md, "Limits, rates and quiet
hours"), which is the refusal a cloud agent's message will meet most. It is never queued: a "tell them the build
passed" said hours later, or the moment privacy is lifted, would break "the
person always wins". The agent's caller hears the refusal at once.

## Why not through Harness

The person's lamp is already a signed-in Harness machine on their account
(`docs/tasks.md`), and fo and Devin Cloud already run as Harness cloud
harnesses on that same account. openharness has a machine link that asks the
owner (`OWNER_REQUIRED`) and names the asking machine from its pairing record
(openharness#1717). The repository's root [CLAUDE.md](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/main/CLAUDE.md)
("Harness integration context") asks lampOS not to invent another pairing flow,
transport or credential beside Harness's. So there are two routes for the
owner to choose between:

| | through the backend (above) | through Harness |
|---|---|---|
| path | agent → backend's MCP endpoint → MQTT → lamp-cloud → lamp-control | cloud harness → Harness machine link → the lamp's Harness daemon → lamp-control's local socket |
| credential | a new one for each agent and lamp | none new: the person's Harness account already binds agent and lamp |
| the person's yes | the app's *Agents* list | Harness's owner prompt for the link, then the lamp's own grant record |
| lamps it reaches | every lamp, through lamp-cloud | only lamps with Harness on and signed in: not yet every home lamp (#3693) |
| agents it reaches | any agent the backend lists | only agents that run as Harness harnesses |
| who builds the path | the backend team | the Harness team: relaying a cloud harness's call to a machine's local socket |

**What this page recommends.** Through Harness first, if the Harness team
can relay a cloud harness's call to the lamp's lamp-control socket. It adds no
credential, and it keeps to that Harness rule. Through the backend only
for what Harness can't reach: a lamp without Harness, or an agent that isn't a
harness. Either way:
- lamp-control's side is the same: a `cloud` client kind on a local socket,
  checked against the lamp's own grant record;
- the person's yes, the narrow default, the named first line and the journal
  are as above.

The Harness team should be asked, as an openharness issue, whether that relay
fits their machine link.

## What it is not

- It isn't a way to drive the lamp in real time: an MQTT round trip is
  hundreds of milliseconds, so `joints` and `move` aren't offered.
- It isn't remote access for the person: the app already does that.
- It isn't a phase-3 runtime on the lamp: no agent code runs there.

## Pieces, if the owner says yes

| piece | where | who |
|---|---|---|
| `cloud` client kind on a local socket, the grant record, the name on first speech | `crates/control` | Nine (its owner), Eighth |
| the new `presence` grant, and a gesture-only `body` grant for cloud clients (a paired computer's `body` grant stays as it is): a protocol change, so control.md's grants table, the schema's `grant` enum (all three copies), both SDKs' grant lists and fakes, lamp-mcp's tool offering and its tests, and the debug log's field sets | `crates/control`, `docs/control.schema.json`, `sdk/` | Nine, with the `cloud` client kind; blue for the SDKs |
| `agent.call` and `agent.event` kinds, the hand-over to lamp-control | `crates/cloud` | Eighth |
| the MCP endpoint per agent, the consent record, the manifest check | the backend | the backend team |
| the *Agents* list and the Allow screen | the app | Mobile |
| fo's and Devin's sides: an MCP client pointed at the endpoint | theirs | coordinated by the first builder |

## Open questions

1. **The owner's yes** to this one cloud door, with decision 2 kept for every
   other door.
2. ~~Who runs the endpoint~~: **answered by the backend team** on #4293. The
   backend team runs it, beside the part that already talks to devices, with
   the backend's existing scoped keys for the credential and no new credential
   system. It would start narrow: one agent (fo) on an allowlist, `say` and the
   light only. And before customers, it waits on a change by the people who
   run the broker.
3. ~~The latency budget for `say`~~: **answered by the backend team.** About
   2 s from the call to the lamp speaking: a few hundred ms on the device
   channel, plus the lamp making the speech. Agents should treat 3 s as the
   limit.
4. Through Harness or through the backend, or both ("Why not through
   Harness"); the Harness team's answer on the relay decides how much of the
   backend route is needed.
