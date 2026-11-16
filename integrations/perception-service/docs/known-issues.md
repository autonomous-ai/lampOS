# Known Issues

Tracked model accuracy and architecture limitations that affect production quality.

## Action Recognition (UniformerV2 / X3D)

- **Degraded accuracy on moving cameras**: Temporal action models assume a
  mostly-static viewpoint. Camera ego-motion (handheld, pan/tilt, robot
  movement) introduces background motion that the model confuses with human
  actions, producing false positives or missed detections.

- **Person detection as partial mitigation**: Enabling the YOLO person detector
  (`PERSON_DETECTOR__ENABLED=true`) crops the largest person before feeding
  frames to the action recognizer, reducing background motion noise. However
  the crop quality depends on person detector confidence, bbox expand scale,
  and minimum area ratio — these parameters need per-deployment tuning for
  the specific camera setup (angle, distance, movement pattern).

- **X3D weaker than UniformerV2**: X3D (INT8 quantized) consistently
  underperforms UniformerV2 (FP32) on fine-grained actions like "drinking".
  The X3D performance test is known to fail on Mac CPU.

## Face Detection (YuNet)

- **Misclassified faces**: YuNet occasionally detects non-face objects as faces
  (posters, screens, patterns with face-like features). This propagates false
  detections into the FER pipeline — the emotion recognizer classifies emotions
  on non-face crops, producing meaningless results.

## Pose Estimation (RTMPose + RULA)

- **No spine keypoints**: RTMPose uses the COCO-17 skeleton which has no spine
  or thoracic keypoints. The graph only provides nose, eyes, ears, shoulders,
  elbows, wrists, hips, knees, ankles. Back posture (thoracic flexion, lumbar
  lordosis) cannot be directly measured.

- **RULA back assessment relies on hip proxy**: The current RULA assessor
  infers trunk flexion from the hip-to-shoulder vector. This is a rough
  approximation — a person can have a straight back with bent hips (seated) or
  a curved back with upright hips (standing slouch). The score may under- or
  over-estimate back risk depending on the posture.

- **Hip keypoints required**: The ergo assessor requires confident hip
  keypoints to compute trunk angle. If hips are occluded (desk, table edge,
  close-up framing), the assessor returns None and no ergo score is produced
  for that frame. This is common in seated desk setups where the camera is
  at monitor height. For a session that sends `ergo_mode: "desk"`, a
  frame whose only gap is the hips (shoulders, nose and both ears confident) is
  scored on the upper body instead: the trunk reference is the perpendicular of
  the shoulder line (image vertical when the shoulders overlap in a side view),
  trunk and legs are assumed neutral, and `ergo.partial` is `true`.

- **Neck flexion in 2D** (fixed in desk mode only): RULA is fed 2D keypoints
  (z = 0), where the old sign rule (`cross(trunk_up, v)[0]`) is always 0, so every
  neck angle comes out negative and scores 4 ("extension"). Default sessions (V1
  lamps) keep that rule unchanged. With `ergo_mode: "desk"` the 2D neck is the
  shoulder midpoint -> ear midpoint line, and its sign comes from the face: the
  nose's x offset from the ear midpoint gives the side the person faces, and a
  lean towards that side is flexion. Leans behind the trunk line under 5° count
  as neutral. In a frontal view (nose straight below the ears) the facing side
  is unknown and the angle is unsigned, so no extension is reported; forward
  head flexion lies along depth there and is barely visible, so a frontal
  camera mostly reads neck 1. 3D input (real z) keeps the old rule.

- **Upper-arm angle reference**: the upper arm is measured against trunk-up,
  so an arm hanging down reads about 180° and scores 2 instead of 1. Not
  changed yet.
