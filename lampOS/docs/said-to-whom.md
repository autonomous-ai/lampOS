# Whom is it said to?

Task 14 research by Fourth, 14 October 2026. Its repository observations below
refer to `origin/lamp-voice` at `8335dfbce`. That task changed documentation
only: no classifier was built, no model was run, and no lamp measurement was
made. The later [task 17 implementation](#task-17-local-judge-and-busy-rule-comparison)
is described separately; it does not turn the published research results into
measurements of this lamp.

## What exists and what we take

Device-directed speech detection asks whether an utterance is meant for the
device. Addressee detection asks which participant it is meant for. Neither
is the same as recognizing the wake word or identifying the speaker.

The recommendation from this study is to combine the lamp's existing name,
conversation and room evidence in one **turn-bound, local, initially shadow
decision**, with fresh face information as a weak additional cue. Typed
questions can add semantic evidence when their answers arrive in time. They
cannot be the only route to a decision: this lamp's transcripts sometimes
arrive late or never arrive. Face orientation cannot stand in for identifying
which visible person spoke. These are design conclusions, not measured gains.

Start with reliable turn attribution and an independently labelled evaluation.
Only then compare a small acoustic classifier or a richer multimodal model.
There is no published percentage in this study that can be applied directly
to an Orange Pi on a Vietnamese office desk.

## Different problems, different denominators

| Problem | What the decision covers | What success does not establish |
|---|---|---|
| Wake-word detection | Whether a sound resembles a specified name or phrase | Whether the name addresses this device, or whether a nameless follow-up does |
| False-trigger mitigation | Whether an invocation already admitted by a wake word or button was intended | Continuous rejection of all conversation in a room |
| Follow-up DDSD | Whether speech after an established device interaction continues it | Starting an unsolicited interaction correctly all day |
| Open-room DDSD | Whether any candidate speech is intended for the device | Which person spoke or whether it is safe to interrupt them |
| Multiparty addressee detection | Which person, device or group an utterance addresses | Speaker identity, eye contact or permission to join every group conversation |
| Selecting the robot's next listener | Whom the robot should address in its own reply | Detecting whether incoming human speech addressed the robot |

For this study, **false acceptance** means accepting non-device-directed
speech; **false rejection** means rejecting device-directed speech. A paper's
FAR may count errors per negative utterance, while a product wake-word test
may count false events per hour. State the denominator. Equal error rate
(EER) is the threshold where those two conditional error rates match; it is
not the operating point a quiet office necessarily needs. Accuracy depends
on the positive/negative mixture. A relative reduction from 10% to 6% is
40%, but the absolute change is four percentage points.

The research and product sections report **published results**, not Fourth's
experiments. Their inputs, invocation conditions, languages and scoring units
differ; the figures are not a leaderboard.

## Research with useful comparisons

The first six studies below use **private data**. They offer designs and
evaluation methods, not a reproducible lamp benchmark or ready-to-ship
classifier. Page numbers refer to the linked PDF, counted from one.

| Study and inputs | Published result and conditions | What to take; what remains missing |
|---|---|---|
| [Amazon, Device-directed Utterance Detection, 2018](https://arxiv.org/pdf/1808.02504v1), Table 4, p. 3. Acoustic embeddings, recognized text and ASR decoder uncertainty. | EER: acoustic 10.9%, text 20.1%, decoder 9.3%, combined 5.2%. Test: 50,000 far-field utterances, 38,000 directed and 12,000 undirected, about 30 hours. | Combining evidence beats each source in this experiment. End-of-utterance and decoder features do not establish early decisions; the lamp does not receive these decoder internals. |
| [Amazon, Streaming ResLSTM with Causal Mean Aggregation, 2021](https://cdn.amazon.science/5c/57/36d64cbb495db5f8293819ab2d87/streaming-reslstm-with-causal-mean-aggregation-for-device-directed-utterance-detection.pdf), Tables 1–4, pp. 4–5. Audio only; causal recurrent state and accumulated mean. | A 0.9M-parameter ResLSTM reduces EER **41.1% relatively** against a 0.3M LSTM, both using attention aggregation for this comparison. Absolute EER is not disclosed. Six million training utterances; 35,000 test utterances. Causal-mean early-prefix comparisons are reported separately. | A plausible small streaming architecture to investigate without text. No target-device milliseconds, low-FAR operating point or furnished trained DDSD weights; parameter count does not establish lamp speed. |
| [Apple, Streaming On-device Detection from Voice and Touch Invocation, 2022](https://arxiv.org/pdf/2110.04656v1), Tables 2–3, pp. 3–4. Streaming acoustic transformer and TCN. | For joint training, voice-trigger FAR at 1% FRR: TCN 6.4%, streaming LSTM 4.2%, non-streaming A2A 3.2%. Touch-trigger FAR at 3% FRR: 25.1%, 28.7%, 24.3%. Test sizes 7,300/38,000 respectively. First block 1.92 s, then 0.96 s shifts. On a 2019 smartphone, s-TCN on one four-second directed touch-invocation utterance: 33.3% lower peak memory and 41.0% lower runtime latency than A2A. | Streaming trades accuracy, waiting and resources; TCN is not uniformly more accurate. No absolute milliseconds/bytes or lamp result. Table 2 gives 24.3% for A2A touch while the figure says 24.4%; this row uses the table. |
| [Apple, Modality Dropout for Multimodal DDSD, 2023](https://arxiv.org/pdf/2310.15261v1), Tables 1, 4–5, p. 4. Audio, text, ASR uncertainty and prosody. | At 10% FRR, FAR falls 7.1% → 6.5% with prosody, then 5.7% with dropout training. With 30% randomly missing inputs, dropout changes the four-modality FAR from 11.46% to 10.61%. English task conversations; speaker-disjoint splits; test 3,100 directed/17,000 undirected queries. | Train for missing evidence, not just the ideal full-input case. The last change is 0.85 percentage points, about 7.4% relative. Randomly absent inputs do not model a late transcript assigned to the wrong turn. No inference timing is supplied. |
| [Apple, DDSD for Follow-up Conversations Using LLMs, 2024](https://arxiv.org/pdf/2411.00023v2), Table 2, p. 4. Initial query, follow-up transcript and ASR alternatives. | Trained ClassHead FAR at **10% FRR**: follow-up alone 11.4%, with initial query 6.1%, with context and eight hypotheses 4.8%. At **5% FRR** those values are 29.9%, 18.3%, 16.7%. About 245,000 English utterance pairs; speaker-disjoint 70/10/20 split; initial query always directed. Vicuna-7B inference used four A100 GPUs. | Context matters, but this is a tuned classifier. Unadapted follow-up-only prompting gave 46.7% FAR/13.8% FRR; malformed outputs were counted as directed. No latency result. LoRA's 4.1M trainable parameters are not the full model size. |
| [Apple, Adaptive Knowledge Distillation for DDSD, 2025](https://arxiv.org/pdf/2508.02801v1), Table 1, p. 3. A 79M-parameter teacher and approximately 5M-parameter students. | Conformer baseline → adaptive distillation EER: `Hey Agent` 1.57% → 1.05%; `Agent` 5.24% → 4.19%; keyword-free follow-up 10.87% → 8.70%. Training: 2,700 hours plus room-response/echo augmentation; internal evaluation. | A route to smaller acoustic judges. Global utterance summarization does not demonstrate causal streaming; test counts and hardware milliseconds are absent. Use the EER pairs rather than transferring an abstract's rounded relative gain. |

A larger-model comparison reinforces fusion without establishing a local
implementation: [A Multimodal Approach to DDSD with LLMs, 2024, Table 1,
p. 3](https://arxiv.org/pdf/2403.14438v2) reports EER 12.15% for text-only
GPT-2 versus 7.45% with text, CLAP audio and decoder signals, both using 500,000
extra in-domain text examples. Its private test has about 14,000 directed
and 23,000 undirected utterances. The 124.4M GPT-2 backbone and roughly 153M
CLAP encoder are used with mapping networks and token prediction; this is
not a small classifier alone. No lamp runtime is reported. This citation
identifies that exact paper/version, rather than
mixing its table with a similarly titled company page's headline percentages.

[WearVox, 2026, Tables 2–4,
pp. 7–10](https://arxiv.org/pdf/2601.02391v1) brings public egocentric
multichannel cases. Its side-talk test combines 582 negatives with 500 valid
tool queries. Published Gemini 2.5 Flash accuracy is 88.2%, or 91.4% with
thinking; corresponding time to first token is 1,306.62/2,176.97 ms. These
are overall binary accuracy and model-output timing, not FAR or audible
response latency. The custom Wearllama single-/multichannel comparison is
85.4%/93.9% accuracy in Table 4; the introduction's 85.6% differs. Wearer
geometry and large models do not transfer to the lamp's mono desk capture.
Its data licence is addressed below.

The September 2026 preprint [Talking to Me or Someone Else?, Tables 1–2
and sections 4–5](https://arxiv.org/html/2609.14118v1) reformulates Ego4D
into four online states: talking-to-me, talking-to-others, self-talk and
background. It reports 75.5% talking-to-me frame F1 on 406 retained clips
split 334/21/51; the 885,866 frames were rebalanced. Model-only time is
4.49 ms/frame versus 48.1 ms/frame including preprocessing; the inspected
text does not specify the timing hardware. Its face choice can use the
annotated interaction target, and text features are pre-extracted from
Whisper transcripts. Thus its causal temporal mask is not proof that every
production input becomes available causally at that speed. No usable data
download/licence route was established in this audit. Its useful idea is
to distinguish types of negative speech; its F1 is not an office false-answer
rate or a measured lamp budget.

## What shipped assistants expose

Product documentation was checked on 14 October 2026. It establishes
advertised conditions and controls, not a public confusion matrix. Historical
engineering descriptions are dated separately from current support pages.

**Google Look and Talk.** Current support limits it to Nest Hub Max in
English, within about 1.5 m, with camera sensing, Face Match and Voice Match.
The user maintains attention while talking; visible states distinguish
attention, listening and processing. Media, calls, camera streaming and Guest
Mode exclude it. It is not a general claim that anyone facing any camera
can start a conversation. [Google support](https://support.google.com/googlehome/answer/11410414?hl=en).

Google's 2022 implementation account combines proximity, enrolled face,
smoothed gaze and **audiovisual active-speaker detection**, then voice match,
local ASR and acoustic/text intent. The active-speaker input covers five
frames and 0.5 seconds of audio. Entry conditions are stricter than those
after engagement. The disclosed evaluation involved more than 3,000 people,
but supplies no reproducible absolute FAR/FRR operating point. The useful
transfer is matching the facing person to the voice and processing partial
speech locally. [Google Research](https://research.google/blog/look-and-talk-natural-conversations-with-google-assistant/).

**Google continuation.** Current documentation describes roughly ten seconds
of listening after an initial wake-word interaction, with silence, ending
phrases and speech to someone else able to end it. It distinguishes legacy
Assistant's US-English setting from Gemini for Home's supported languages
and whole-home setting. Neither this interval nor another product's timeout
is a measured lamp threshold. [Continued Conversation support](https://support.google.com/googlehome/answer/7685981?hl=en).

**Siri.** The versioned iOS 26 guide permits immediate further requests and
interruption without a new invocation, subject to region availability. It
does not provide a complete current device/language matrix. Apple's 2023
back-to-back demonstration describes an on-device directed-speech detector
using acoustic, ASR-system and lexical information, including while Siri is
speaking; it provides no reusable accuracy or latency benchmark. This is
evidence of a shipped multimodal detector, not evidence that an untuned text
prompt is equivalent. [iPhone guide](https://support.apple.com/en-ae/guide/iphone/iph83aad8922/26/ios/26),
[Apple's NeurIPS demonstration](https://machinelearning.apple.com/updates/apple-at-neurips-2023).

**Alexa.** Current optional Follow-Up Mode documentation, including Alexa+,
describes wake-free interruption on compatible Echo Shows when the person
is in frame and facing the screen. Camera/microphone and activity conditions
apply. The cited support does not enumerate every current model/language or
promise a five-second window. Ready-to-listen, listening and processing are
distinct visible states. [Follow-Up Mode](https://digprjsurvey.amazon.com/csad/help/node/GX7EJ9WHEPYBV94J),
[conditions](https://digprjsurvey.amazon.com/csad/help/node/G202201630),
[Alexa+ indicators](https://digprjsurvey.amazon.com/csad/help/node/T2Xd07kMXxJPkWIgXD).

The **2021** Conversation Mode engineering account combined head direction
with acoustic activity, including rejection of ambient noise and Alexa's
own output. It reported 83% relative false-rejection reduction from adding
the audio detector to visual-only detection, without the baseline error,
sample count or FAR operating point needed to assess lamp transfer. The
associated launch page now says the feature was updated; its old Echo model
list is not a current compatibility promise. [Amazon Science](https://www.amazon.science/blog/new-alexa-feature-enables-natural-multiparty-interactions),
[historical launch](https://www.aboutamazon.com/news/devices/conversation-mode-helps-interactions-with-alexa-feel-more-natural).

## What robots add to the picture

An early human–human–robot study used 18 English sessions of about ten
minutes, with two people and a robot-facing camera setup. Its combined
head-pose/speech classifier reported 92% overall accuracy but only **0.65
precision and 0.81 recall** for robot-directed speech. In the observed
frames, looking at the robot corresponded to addressing it only 65% of the
time. This is a concrete reason not to equate facing with directedness.
The controlled scene and class mixture matter. [Katzenmaier, Stiefelhagen
and Schultz, 2004, Tables 2 and 7, pp. 3 and 6](https://isl.iar.kit.edu/downloads/Identifying_the_Addressee_in_Human-Human-Robot_Interactions_based_on_Head_Pose_and_Speech.pdf).

Furhat's SDK explicitly says its default speech attribution assumes the
currently attended user is speaking. Its engagement radius, hysteresis and
temporary face tracks are policies, not proof of speaker identity. Creator's
attention requirement suppresses replies while still listening and
remembering: reply gating and capture/retention are separate contracts.
[Furhat users and attention](https://docs.furhat.io/skill-development/reference/users),
[Creator parameters](https://docs.furhat.io/furhat-ai-creator/parameters).

A 2026 Furhat study of 30 people reports 79.3% correct addressee in group
conversation and 92.6% in parallel conversations. **Its scored unit is a
robot utterance directed to the correct human**, not the rejection of
incoming side talk. It uses audio direction, face/voice identification and
dialogue handling; the percentages must not be compared with DDSD FAR or
quoted as room-speech rejection accuracy. [Abbo et al., section 4.2 and
Table 2](https://www.frontiersin.org/journals/robotics-and-ai/articles/10.3389/frobt.2026.1766383/full).

The 2025 XAE study estimates robot/left/right addressees from face images
and body pose; its three-class task excludes group/no-label cases. It reports
weighted F1 79.40 ± 1.06% (standard deviation; five initializations per test
set) in interaction-level Vernissage evaluation, but 66.31% **accuracy**
when transferring to iCub. The subsequent live model was trained on both
Vernissage and iCub. In that live study, robot-class
recall rose from 70.89% to 79.75% with spatial-memory correction. An
experimenter supplied sound-source identification, participants were asked
to avoid overlapping speech, and the authors say the system was not ready
for real-world exploitation. Those conditions and the different metrics
matter when interpreting the within-corpus and transfer results.
[Bečková et al., sections 4.1.1, 4.3 and 5.4](https://arxiv.org/html/2407.03340v2).

The JAMES bartender study illustrates the role of social setting: position
at the bar plus looking toward it indicated service initiation in its
105 observed initiations and subsequent judgment experiments. The study
favored inviting a possible customer over ignoring one. A quiet desk lamp
has different costs for those mistakes. [Loth et al., 2013](https://www.frontiersin.org/journals/psychology/articles/10.3389/fpsyg.2013.00557/full).

## Public recordings: labels, access and reuse

The immediate recommendation is **AMI for annotation tooling and human
conversation context**, with explicit limits on its use as negative data.
Ego4D Social and Vernissage are stronger target-task leads but require access
work outside this assignment. No source found in this bounded search supplies
an immediately downloadable, permissively licensed, real English/Vietnamese
lamp-view benchmark with all necessary labels.

| Corpus | Relevant recordings and labels | Access, licence and use for lampOS |
|---|---|---|
| **AMI** | About 100 hours of English meetings with native and non-native speakers, synchronized cameras and close/far microphones. Released XML includes individual/group addressees for some meetings, plus dialogue acts and some attention labels. | Official corpus/annotation licence is **CC BY 4.0**. Public audio and annotation URLs were reachable without registration. Useful for tooling and contextual human addressee work; there are no robot-directed positives. Calling every meeting utterance a lamp negative would be an assumption. Intersect label coverage with the published meeting-series splits; missing annotations are not negative labels. [Corpus](https://groups.inf.ed.ac.uk/ami/corpus/), [licence](https://groups.inf.ed.ac.uk/ami/corpus/license.shtml), [coverage](https://groups.inf.ed.ac.uk/ami/corpus/annotation.shtml), [partitions](https://groups.inf.ed.ac.uk/ami/corpus/datasets.shtml). |
| **Ego4D Social** | Egocentric audio/video: looking-at-me and talking-to-me are separate labels, associated with visible face tracks and speaking intervals. The original Social split has 389/50/133 five-minute clips. | Signed agreements and emailed download credentials are required; none were requested. The official linked draft allows defined commercial model development but restricts database transfer/product inclusion; check the executed institutional agreements. Code's MIT licence does not license video. Camera-wearer geometry and supplied speaker information differ from lamp inputs. [Task](https://ego4d-data.org/docs/benchmarks/social/), [access](https://ego4d-data.org/docs/start-here/), [draft data agreements](https://ego4d-data.org/pdfs/Ego4D-Licenses-Draft.pdf). |
| **Vernissage** | Thirteen approximately eleven-minute NAO/two-person sessions; robot and external audio/video, motion capture, and robot/other-person/group/no-label addressees. Requested English with some German side talk. | Original recordings are research-community **request-only**, with commercial rights unresolved. Public derived labels/checkpoints do not supply original video. The 2023-derived release's dataset directories are empty; the 2025 release separately requires agreement for original iCub recordings. Strong robot-domain lead, not an acquired training corpus. [Original report](https://publications.idiap.ch/attachments/reports/2012/Jayagopi_Idiap-RR-33-2012.pdf), [access page](http://vernissage.humavips.eu/data.html), [derived release](https://zenodo.org/records/10711588), [2025 release](https://zenodo.org/records/14833239). |
| **EasyCom** | 5h18m11s of usable noisy restaurant-style conversation; six-channel glasses audio, egocentric video, face boxes, headsets and pose. `Target_of_Speech` identifies a participant, group or none. Recommended train sessions 1,2,3,5,6,7,8,9; validation 4,12; test 10,11. | Public multipart recordings, **CC BY-NC 4.0**. Not a ready commercial-training source. Do not give the lamp baseline oracle headsets/pose or wearer geometry; no Vietnamese subset was verified. [Data/schema/splits](https://raw.githubusercontent.com/facebookresearch/EasyComDataset/main/README.md), [actual licence](https://raw.githubusercontent.com/facebookresearch/EasyComDataset/main/LICENSE). |
| **WearVox** | 3,842 egocentric multichannel examples spanning tool invocation and side talk; the side-talk experiment is described above. | Public card declares **CC BY-NC 4.0**. Research comparison only under those terms, not a commercial-training recommendation. Preserve original tasks/splits and the reused valid-query examples when avoiding leakage. [Dataset card](https://huggingface.co/datasets/zlinao/WearVox). |
| **MISP 2021** | Mandarin home/TV audio/video at several capture distances; room-/speaker-independent splits. Task 1 labels presence of `Xiao T Xiao T`, not intended addressee. | Signed registration and noncommercial research terms; no access requested. Useful recording/evaluation design, but wake-word labels do not become DDSD ground truth. [Task/data](https://mispchallenge.github.io/task1_data.html), [access](https://mispchallenge.github.io/download.html), [data agreement](https://mispchallenge.github.io/doc/Non-commercial%20Data%20License%20Agreement%20for%20MISP%20Challenge%202021.pdf). |
| **MPIIGroupInteraction / MultiMediate** | Twenty-two German group discussions with 78 people; eye contact, speaking, backchannels and other behavior. Those auxiliary labels do not identify the addressee of each sentence. | Registration and EULA; internal noncommercial scientific research only, explicitly excluding commercial testing. Do not quietly adopt it for product qualification. [Dataset](https://multimediate-challenge.org/datasets/Dataset_MPII/), [EULA](https://multimediate-challenge.org/assets/pdf/EULA-MPIIGroupInteraction-full-version.pdf). |
| **TCL Intelligent Wakeup** | English synthetic conversations with scenario-grouped variants, including no-assistant negatives. The published schema has conversation-level `has_va`, not an explicit addressee field for every turn. | Public card declares **CC BY-NC 4.0** for data; Apache-2.0 code does not override that. Paper/release details are still marked TBA. Useful stress-case ideas, not direct human evidence or an established ready-made turn-labelled benchmark. [Author card](https://huggingface.co/datasets/TCLResearchEurope/intelligent_wakeup). |

For Ego4D, the original published talking-to-me baseline has validation/test
accuracy 64.31%/49.75% and mAP 56.50%/55.06%. Those are **talking** results;
the higher looking-at-me score is not an addressee result. The original
offline task permits future context. [CVPR 2022 supplement, Appendix I,
Tables 30–31, PDF pp. 48–50](https://www.cs.utexas.edu/~grauman/papers/ego4d-supp-cvpr2022.pdf).
The [released TTM code](https://raw.githubusercontent.com/EGO4D/social-interactions/ttm/README.md)
reports different checkpoint results and notes an original-versus-cropped
data distinction. Keep the implementation/version attached to a benchmark;
do not mix one release's labels with another's scores.

For the immediate AMI lead, one audit on Fourth's development Mac
(macOS 26.6.2, Intel Core i7-10700K) used **bounded HTTP byte ranges**
on the public [manual-annotation ZIP](https://groups.inf.ed.ac.uk/ami/AMICorpusAnnotations/ami_public_manual_1.6.2.zip).
The three sampled `dialogueActs/IS1003d.{D,A,C}.dialog-act.xml` files
contained 142, 217 and 156 `addressee` attributes, including individual and
multiple-participant values. These are sample counts from one meeting, not
whole-corpus coverage or a detector result. Archive response bodies totaled
612,715 bytes; network headers were not counted. The
[IS1003d headset mix](https://groups.inf.ed.ac.uk/ami/AMICorpusMirror/amicorpus/IS1003d/audio/IS1003d.Mix-Headset.wav)
returned HTTP 200 to a header request; its waveform was not downloaded or
heard. Coverage must be established before selecting a training/evaluation
subset.

Other robot corpora are not automatic substitutes. [UE-HRI](https://adasp.telecom-paris.fr/resources/2017-05-18-ue-hri/)
has robot-view engagement data but registration and
[noncommercial terms](https://adasp.telecom-paris.fr/rc/datasets/ue-hri/Copyright.pdf);
engagement is not utterance addressee. [PInSoRo's current access page](https://freeplay-sandbox.github.io/get-dataset)
says full raw video has not been distributable since 2024. Old paper links,
sample videos, model licences and code licences do not establish rights or
availability for the original recordings.

## What the lamp's own records establish

The following are the **first builder's reported lamp observations**, read
from the repository. Fourth did not repeat them. Played voices and simulated
features are identified because neither is evidence from an ordinary day of
direct human conversation.

| Existing evidence | Consequence for the next decision |
|---|---|
| [Attention](attention.md): a hard transcript-name gate rejected two of two genuine calls in the test; earlier seven-of-seven successes had other permissions already open. No literal `Lamp` appeared in 310 examined transcript lines. | Missing a name in text is missing evidence, not evidence of an unaddressed turn. Test the gate with each other permission closed. |
| [Name](name.md): at threshold 0.9, played English `Hey Lamp` was detected in 17/20 lines, near names in 5/10, ordinary office lines in 0/10, and Vietnamese `Lamp ơi` in 1/2. Eight bare `Lamp` trials were missed. One unprompted firing occurred during a separate five-minute interval. | The name score is not a calibrated probability of device address. Keep near names, bare names and Vietnamese as separate slices; these small replay samples do not establish an hourly false-answer rate. |
| [Policy](policy.md): the typed judge is shadow only. One fitted comparison used 182 fitting and 164 held-out examples; the name-fusion improvement used simulated name observations, not captured name detections. | Do not advertise the simulated fusion result as live accuracy. Preserve a speaker/session split and real missing modalities in a new evaluation. |
| [Policy](policy.md): in the reported eight-turn live run, four turns had no text; three of four deliberately addressed calls had no text although they received answers. Another transcript arrived 14.7 seconds late and became associated with a later name event. | Every input needs the original turn and capture time. Arrival order and a recent-name window cannot safely repair a late transcript. |
| [Policy](policy.md): the reused judge connection had a 536 ms median request time in the reported sample. Its decision arrived after first sound in 9/12 audible answers, by 40–616 ms; it preceded sound in only three. | A cloud text verdict cannot guard playback it follows. Record late decisions, but never apply one to a different turn or issue a second answer merely because it was late. |
| [Group talk](group-talk.md): a seven-minute group run recorded 47 turns, 21 replaced before provider reply, and two unwanted answers. Nearby conversation and addressed speech overlapped in digital level. The later floor-hold tests were staged scenes. | Volume is not an addressee label. Measure interruptions and missed calls as well as unwanted answers; a long protected answer can hide rejection mistakes by making the lamp hard to stop. |

[Attention](attention.md) also records an in-turn note and a second-look
recovery. The four note-enabled rounds and four earlier rounds both answered,
so those runs do not establish a causal gain from the note. A name-shaped
sound can still mean a colleague such as Lan; a busy-room nameless question
can still get an unwanted answer. The evidence calls for one decision owner,
not another independently permissive gate.

## Which cues this lamp actually has

| Cue | Available now | Limit and additional work |
|---|---|---|
| Sound and prosody | Mono voice PCM, activity timing, level and the name listener | Prosody can supplement words without waiting for ASR. It needs labelled positive and negative examples, calibrated scores and native timing. Quiet/far is not equivalent to unaddressed. Echo, suppression and replay alter the sound. |
| Words and typed questions | Provider text and the shadow policy's typed semantic questions | Text can omit names, arrive late or be absent. ASR confidence, lattices or n-best alternatives used in some papers are not established inputs of this interface. A small classifier's published result does not transfer to an arbitrary prompted LLM. |
| Conversation and last speaker | Voice state, previous exchanges, recent name and room state | Recent device speech is weaker than an explicitly established human continuation. An erroneous lamp reply must not grant itself a long chain of permissions. Without speaker binding, the next person may be talking to someone else. |
| Face orientation | Passing face tracks and an estimate that a face points toward the lamp | [Eyes](eyes.md) uses landmarks and head yaw, not eye gaze. A listener can face the lamp while a different person talks. Track IDs are temporary, not identities. No visible face, stale data and a visible face turned away are distinct observations. |
| Proximity | Face-box size and uncalibrated digital sound level | The policy's 15%-of-image-height near-face threshold is not a distance in metres. Camera geometry, posture, gain and automatic processing confound these cues. Do not import a product's 1.5 m limit as a measured lamp threshold. |
| Direction and active speaker | Not verified | The [hardware record](microphone-topology.md) identifies **three capsules but two mono USB streams**: a central C-Media input and an already-combined Jieli dual-microphone input. They have independent clocks. Separate raw left/right channels, a direction output and a usable vendor interface are unverified. These streams are not a synchronized software array. |

Faces are sampled at about 4 Hz when active, less than 2 Hz when idle; the
policy currently records face summaries only when at most two seconds old
and does not use them in its fitted decision. A two-second-old face is useful
context, not proof of where the current speaker looked. Visual active-speaker
detection would need timed mouth evidence associated with the speech, suitable
camera views and additional computation. A direction estimate would need a
verified vendor output or synchronized raw channels plus physical calibration.
Neither is supplied by simply comparing the two existing USB levels.

## A sequence the first builder could qualify

These were task 14's proposals for later tasks. Task 17 begins the local shadow
comparison described below; the remaining collection and qualification steps
still apply. These proposals themselves enable no feature or default.

1. **Make the evidence belong to a turn.** Give each candidate speech turn,
   including rejected candidates, a stable ID and monotonic capture interval.
   Carry that ID through transcript, name,
   face summary, judge request, cancellation and playback decision. Track
   source time separately from delivery time. Test missing text, a 15-second
   late transcript, reused connections, cancelled turns, overlapping people
   and a cue arriving after playback. The current time-based joins are not
   ground truth for training.
2. **Evaluate one local owner in shadow.** Start with the existing features
   and explicit unknown values. Fit and calibrate on labelled real cues,
   holding out whole sessions and speakers. Compare name only, existing
   attention, typed judge, local fusion and each added modality under the
   same admission policy. Name and facing should support a decision rather
   than being universal requirements. A glance at an otherwise occupied
   person should not make the lamp repeatedly prompt them.
3. **Keep slow semantics out of the immediate path.** Update local evidence
   during speech. An on-time typed verdict may contribute; a late one is
   logged for that turn and cannot reopen a replaced turn. Bound queues and
   cancellation. Keep the current behavior when the new switch is off;
   select and document the unknown-evidence fallback before enabling it.
   A fallback that always accepts is itself an operating point to measure.
4. **Try a small acoustic model only against that baseline.** A streaming
   acoustic encoder could cover nameless or missing-text speech. Compare
   accuracy at a fixed false-reject rate and false answers per hour, including
   out-of-domain voices. Missing-modality training is relevant. A larger
   model can label or teach only after checking its labels independently;
   training on the lamp's own answer decisions would reproduce its mistakes.
5. **Add speaker-to-face binding only if it earns its cost.** First prove the
   available channels, camera timing and compute budget. Do not require face
   enrollment or a known identity merely to answer a visitor. Motion to seek
   a speaker should be a small natural glance, not a mechanical room scan.
   Stop and physical mute must remain available independently of this judge.

**Proposed development latency targets:** existing-feature local fusion at
p95 ≤20 ms and p99 ≤50 ms; a usable final decision within 100 ms of the
chosen speech-end boundary, with continuous updates before that boundary.
These targets are not measurements. Time the first decision after startup,
repeated decisions, missing inputs and slow/stalled dependencies on the lamp.
Report the audio window needed by a model separately from its compute time.
The externally visible boundary is actual human speech end to first audible
reply, including endpointing and any hold introduced by this gate.

The decision must be final for the current turn before its first audible
sample if it is to prevent that answer. Do not announce listening readiness
until input is accepted. A late reject cannot erase speech already played.

## Task 17: local judge and busy-rule comparison

Task 17 adds an optional local implementation of the Policy's seven typed
questions, an authored weights file and offline fitting, alongside the slow
judge. It remains **shadow only**: a Policy choice does not grant Voice,
speaker or motor permission. The [Policy documentation](policy.md) owns the
run command, deadline and combining contract, fitting-row schema, gates and
machine timings. The local signs cannot establish the semantic answer to
every question; the initial weights are explicitly unfitted priors. They
are not a calibrated probability of intended address.

The comparison in
[`policy/baseline.rs`](../crates/policy/src/policy/baseline.rs) mirrors the
binary result of Voice's `first_output_busy`, `called` and
`decide_busy(strict = true)`, audited at `e07449e13` after rebasing onto
`e93b96849`. It has no dependency on the Voice crate. In a busy room it
permits proposed output for an own turn, an ongoing conversation, a
transcript name match, a fresh facing face, or the per-request `named_sure`
latch. Voice sets that latch for an eligible acoustic name score ≥0.999;
a later weaker hearing does not clear it. Quiet and unknown rooms permit
output. An ordinary 0.993 acoustic detection still cannot open the busy
gate. Request freshness affects the explanation, not this permission.
`Play` is permission for a proposed output, not proof that an answer occurred.

The original task 17 eight-case evidence was pinned to `574c2e5ed`, before
First added the sure-name exception. It remains historical evidence of that
rule. Current comparisons require the new latch field explicitly and identify
the newer source; no old record is silently assigned a false latch.

Two tracks remain separate:

| Track | Evidence needed | Claim it supports |
|---|---|---|
| `same_snapshot` | Explicit inputs at a declared comparison point; the runtime uses the Policy's completed text, 20-second conversation and near-face view | What the two decision rules choose on those supplied inputs |
| `voice_first_output` | Actual inputs used when Voice first evaluates proposed output: its transcript prefix, own-turn flag, retained `named_sure` bit, 45-second conversation flag, room and supplied **any-face** argument | Reproduction of that strict gate's permission, provided the input provenance and time join are established independently |

Do not substitute one track for the other. Policy counts only facing faces
whose boxes are at least 15% of the image height; Voice accepts any facing
face. Voice uses the latest message's expiry, normally the eyes' two-second
lifespan with the bus's 50 ms grace; Policy's existing summary uses receipt
age. Policy and Voice can also see different text at their different decision
times. None of these differences is fixed by labelling a row
`voice_first_output`.

The optional `baseline` member of a fitting row has this complete shape:

```json
{"kind":"same_snapshot","room":"busy","own_turn":false,"going_on":false,"named_sure":false,"text":"And tomorrow?","facing":null}
```

This is an **authored schema example**, not a captured utterance. Every
member is required; unknown members are rejected. For `same_snapshot`,
`facing: null` declares an unknown supplied cue. For `voice_first_output`,
it records the actual `turned: None` argument: Voice queries eyes only in a
busy room under the strict rule, so quiet and unknown rooms pass `None`
without a query. A missing, expired or unreadable eyes result can also give
`None`. With a valid fresh message, Voice passes `false` if no face has
`facing: true`, including an empty list or all unknown poses; this is not
proof of a measured turned-away speaker. If the argument itself was not
recorded, omit the whole baseline snapshot rather than inventing null.
`named_sure` is a required boolean, not an optional score. For
`voice_first_output`, record Voice's actual retained bit; a missing bit
is not false. For `same_snapshot`, declare the bit from the supplied
score at the 0.999 threshold, which does not reconstruct Voice's name
window. Older snapshots lacking this member are rejected. `text` is valid
UTF-8, at most 2,048 bytes; oversized input is rejected, not truncated by
the comparison. The containing fitting input has its own
file, row and count limits. Source and rights metadata do not independently
prove labels, clock alignment or first-output provenance.

The [proposed first-output capture contract](verification/local-judge-fit-20261014/voice-first-output.md)
specifies where to capture these existing fields and a separate sidecar for
their provenance, request outcomes and Policy row joins. It is a documentation
proposal, not an implemented Voice event or recorder. The schema reproduces
the binary `strict = true` rule; ordinary attention mode also depends on
freshness and the ordinary name latch, which are not in this snapshot.

Counts retain separate 2×3 tables for the two tracks. The columns are the
supplied Policy choices `answer`, `look_up` and `stay_quiet`; rows are the
baseline's `play` and `hold`. Permission disagrees for `play/look_up`,
`play/stay_quiet` and `hold/answer`. `hold/look_up` and `hold/stay_quiet` agree
only about speech permission: the Voice gate makes no corresponding movement
choice. Missing or invalid snapshots are counted and excluded from both
decisions' paired error denominators. Unknown labels can show disagreement
but create no error. To report rates for independently labelled eligible
rows, divide false-answer counts by not-for-lamp rows and missed-call counts
by for-lamp rows; a zero denominator has no rate.
Here those names mean **counterfactual permission errors**, not audible
events or false answers per hour. The fitter compares held-out test rows;
session groups must remain separate from training and calibration.

The [comparison tests](../crates/policy/tests/it/baseline.rs) contain authored
truth tables, all exact name aliases and nonmatches, a late name appearing
only in the completed transcript, a small facing face, a 30-second-old
exchange, and independent counting examples. These test cases are neither
First's working-morning and meeting rows nor the earlier 346-example corpus.
The repository's 42-scenario Voice evaluation plan supplies useful scene
ideas, not captured attention features or independent addressee labels for
this comparison. No accuracy gain follows from agreement on authored cases.

The current collection has specific unknowns. Face direction is head yaw,
not gaze or association with the speaker; a missing pose must not become a
measured turned-away face. The live local cue's acoustic `duration_ms` stays
null: `Gather` measures first-to-last transcript receipt, which is zero for
a single-piece transcript and is not speech duration. `voice/heard` can have
`request: null`, and the current Voice publisher uses that form. Policy's
existing receipt-time joins discard bus source timestamps; a delayed name
or face update can therefore attach to the wrong speech. The local
[`lamp_ipc::monotonic_us`](../crates/ipc/src/lib.rs) clock is host-wide
`CLOCK_MONOTONIC`: timestamps from that clock are comparable across local
processes within the same OS boot, not across boots or machines. Record the
machine, OS boot and clock source; the bus's publisher `boot` nonce identifies
a publisher instance, not an OS boot. Cross-machine scene timestamps need
explicit alignment. A log snapshot and a fitting program make these
limitations visible; they do not repair the attribution.

The [played-scene dataset](verification/policy-scenes-lamp-4ace-20261014/README.md)
reports both counterfactual `same_snapshot` errors and a separate audible
result for the two-people scenes. They are different cohorts and stages.
Voice evaluates attention only for proposed output; when the provider
proposes none, there is no first-output gate verdict. The dataset contains
no actual `voice_first_output` snapshots, so it cannot establish a paired comparison
with the live gate. The capture contract keeps silent requests in coverage
accounting without turning their missing gate decisions into holds.

## A bounded first lamp observation

After the first builder obtains consent from everyone involved, a finite
metadata capture can inspect present cue timing. It does not validate a new
model. With the existing publishers running and `lamp-bus` on the lamp's
PATH, this uses the current [bus CLI](bus.md#the-tool):

```sh
umask 077
trial_dir=$(mktemp -d /var/tmp/lamp-addressee.XXXXXX)
lamp-bus record "$trial_dir" \
  voice/state voice/heard voice/said room/state name/heard \
  humans/faces/tracked policy/decisions --seconds 600 --max-mb 16
```

This creates a fresh private directory, listens for ten minutes and caps
each topic file at 16 MiB. It does not record PCM or images, but `voice/heard`
and `voice/said` **contain private spoken words**. Use a consented staged
session, handle the files accordingly and agree their retention in advance.
It starts no publishers; absence of a topic is an observation to record,
not evidence of a negative classifier decision. Current events do not yet
provide the stable end-to-end turn attribution proposed above. The recorder
keeps publisher timestamps, not subscriber receipt timestamps; this capture
cannot by itself establish delivery latency or consumer arrival order.

Alongside it, an independent observer records intended addressee, actual
speaker, speech start/end, overlap and audible response, with clock alignment
stated. Do not label a turn "for lamp" merely because the lamp answered it.
Keep lamp, another person, group including lamp, self-talk and uncertain as
separate labels; preserve annotator disagreement rather than forcing a binary
answer for an ambiguous utterance.
Use matched prompts across these scenes:

- The same question to the lamp and to a colleague, including Lamp/Lan/Liam
  confusions, bare names, English, Vietnamese and code switching.
- A nameless follow-up after an explicit call, then a question to a colleague
  immediately after the lamp's reply; a phone call and speech from a screen.
- A person addressing the lamp while looking at their own screen, a visible
  listener looking at the lamp while someone else speaks, an off-camera
  caller, an occluded camera and two people speaking together.
- A call and a stop during the lamp's own speech, rapid replacement of a
  turn, room noise, and a genuine pause in the caller's sentence.

Ten minutes checks capture and attribution, not product readiness. The
subsequent evaluation should hold out people, room sessions and days; keep
direct human speech, loudspeaker replay and synthetic tests separate.
Publish counts and conditional false accepts/rejects by scene and language,
false audible answers per hour, missed calls, interrupted calls, missing or
late cues, and decision/playback latency at first use, median, p95 and worst.
Report how often a verdict was available before first sound. Include an
explicit ablation of face evidence and of conversation carryover.

For scale, if a later target were fewer than one false answer per eight
hours, zero errors in eight hours would not demonstrate it. Under a simple
Poisson assumption, a zero-count one-sided 95% upper rate is
`-ln(0.05) / observed_hours`, about `3 / observed_hours`; approximately 24
zero-error hours would be needed for that bound to reach one per eight hours.
This is an illustrative calculation, not a selected product target or a
claim that clustered office conversations follow that model. Finite negative
utterance tests and independent positive-call tests are also necessary.

## Not shown

Task 14 performed no lamp trial, runtime change, new model training, acoustic
benchmark, power measurement or end-to-end latency measurement. Task 17's
local model and comparison do not establish lamp accuracy, calibrated real
probabilities or causal improvement over `LAMP_ATTENTION=busy`. The separately
recorded played-scene file supplies script labels, but no actual first-output
snapshots or exact joins to Voice's gate. The proposed capture contract changes
no Voice behavior and supplies no new recording or latency measurement.
Published private-corpus results were not reproduced. No account, key or paid
service was obtained, and no restricted corpus was downloaded. Public access
does not by itself permit training a model for distribution; record the
actual data and model licences before any later training task.

The remaining uncertainty is greatest for genuine simultaneous office
speakers, Vietnamese and near names, stale cue attribution, preserved stop
behavior, and the camera's ability to associate a facing person with speech.
Native cost and privacy-preserving collection still need qualification. None
of the research comparisons establishes that this lamp now knows its addressee.
