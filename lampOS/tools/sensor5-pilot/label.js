/* Local-only annotation state and browser UI. No network or persistent storage. */
(function (root) {
  'use strict';
  const HASH = /^[a-f0-9]{64}$/;
  const ID = /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/;
  const TRACK = /^T[0-9]{2,3}$/;
  const SCENES = ['unlabelled', 'faces', 'no_face', 'uncertain'];
  const SPEECH = ['not_speaking', 'speaking_audible', 'speaking_inaudible', 'uncertain'];
  const VISIBILITY = ['visible', 'occluded', 'out_of_view'];
  const SOURCES = ['visible', 'offscreen', 'playback', 'none', 'uncertain'];
  const copy = value => JSON.parse(JSON.stringify(value));
  function check(ok, message) { if (!ok) throw new Error(message); }
  function object(value) { return value !== null && typeof value === 'object' && !Array.isArray(value); }
  function safePath(value) {
    return typeof value === 'string' && value.length < 240 &&
      /^[A-Za-z0-9_.\/-]+$/.test(value) && !value.startsWith('/') &&
      value.split('/').every(part => part && part !== '.' && part !== '..');
  }
  function timingMetadata(raw) {
    const result = {};
    for (const key of ['alignment', 'scoring_allowed', 'timestamp_boundary', 'physical_sync_validated']) {
      if (raw[key] !== undefined) result[key] = copy(raw[key]);
    }
    return result;
  }
  function frameIdentity(frame) {
    const identity = { frame_index: frame.index, timestamp_us: frame.timestamp_us };
    for (const key of ['original_index', 'source_record_index', 'native_frame']) {
      if (frame[key] !== undefined) identity[key] = copy(frame[key]);
    }
    return identity;
  }
  function alignmentNotice(manifest) {
    if (manifest.alignment?.status === 'unverified_estimate' || manifest.scoring_allowed === false) {
      return 'UNVERIFIED A/V ALIGNMENT — estimated preview only. These labels are not eligible for scoring. Native source timestamps remain in the manifest and export; preview timing is separate.';
    }
    return 'Playback does not verify physical A/V alignment. Check the separate alignment evidence before scoring.';
  }
  function validateManifest(raw) {
    check(object(raw) && raw.schema_version === 1, 'Unsupported prepared manifest.');
    check(ID.test(raw.clip_id || ''), 'Clip ID must be a short pseudonymous identifier.');
    check(HASH.test(raw.capture_sha256 || ''), 'Missing original capture SHA256.');
    check(safePath(raw.audio_file) && HASH.test(raw.audio_sha256 || ''), 'Audio path/hash is invalid.');
    check(Number.isSafeInteger(raw.audio_start_us), 'Audio origin must be integer microseconds.');
    if (raw.alignment !== undefined) check(object(raw.alignment) && typeof raw.alignment.status === 'string' && raw.alignment.status.length < 80, 'Invalid alignment metadata.');
    if (raw.scoring_allowed !== undefined) check(typeof raw.scoring_allowed === 'boolean', 'Invalid scoring eligibility metadata.');
    if (raw.physical_sync_validated !== undefined) check(typeof raw.physical_sync_validated === 'boolean', 'Invalid physical alignment metadata.');
    if (raw.timestamp_boundary !== undefined) check(typeof raw.timestamp_boundary === 'string' && raw.timestamp_boundary.length > 0 && raw.timestamp_boundary.length < 160, 'Invalid timestamp boundary.');
    if (raw.alignment?.status === 'unverified_estimate') {
      check(raw.scoring_allowed === false && raw.physical_sync_validated !== true && raw.timestamp_boundary, 'Estimated alignment must prohibit scoring and name its timestamp boundary.');
    }
    check(Array.isArray(raw.frames) && raw.frames.length > 0 && raw.frames.length <= 10000, 'Expected 1–10000 native frames.');
    const paths = new Set([raw.audio_file]);
    let previous = -Infinity;
    const frames = raw.frames.map((f, i) => {
      check(object(f) && f.index === i, 'Frame indices must be contiguous, starting at zero.');
      check(Number.isSafeInteger(f.timestamp_us) && f.timestamp_us > previous, 'Native frame timestamps must increase strictly.');
      check(safePath(f.image_file) && !paths.has(f.image_file), 'Frame paths must be safe and unique.');
      check(HASH.test(f.sha256 || ''), 'Each prepared frame needs its SHA256.');
      if (f.width_px !== undefined) check(Number.isSafeInteger(f.width_px) && f.width_px > 0, 'Invalid frame width.');
      if (f.height_px !== undefined) check(Number.isSafeInteger(f.height_px) && f.height_px > 0, 'Invalid frame height.');
      for (const key of ['original_index', 'source_record_index']) {
        if (f[key] !== undefined) check(Number.isSafeInteger(f[key]) && f[key] >= 0 && (i === 0 || f[key] > raw.frames[i - 1][key]), `Invalid or reordered ${key}.`);
      }
      if (f.native_frame !== undefined) check(object(f.native_frame), 'Invalid native frame provenance.');
      paths.add(f.image_file);
      previous = f.timestamp_us;
      return copy(f);
    });
    return { schema_version: 1, clip_id: raw.clip_id, capture_sha256: raw.capture_sha256,
      audio_file: raw.audio_file, audio_sha256: raw.audio_sha256, audio_start_us: raw.audio_start_us, frames, ...timingMetadata(raw) };
  }
  function blank(frame) {
    return { ...frameIdentity(frame), scene: 'unlabelled', faces: [], audible_sources: [] };
  }
  function validateRow(row, frame) {
    check(object(row) && row.frame_index === frame.index && row.timestamp_us === frame.timestamp_us, 'Label frame identity does not match the prepared media.');
    for (const key of ['original_index', 'source_record_index', 'native_frame']) {
      check(JSON.stringify(row[key]) === JSON.stringify(frame[key]), `Label ${key} differs from the original source.`);
    }
    check(SCENES.includes(row.scene), 'Invalid scene label.');
    check(Array.isArray(row.faces) && row.faces.length <= 32, 'Expected at most 32 face tracks.');
    check(Array.isArray(row.audible_sources) && row.audible_sources.every(s => SOURCES.includes(s)), 'Invalid audible-source label.');
    check(new Set(row.audible_sources).size === row.audible_sources.length, 'Duplicate audible-source label.');
    check(!row.audible_sources.some(s => (s === 'none' || s === 'uncertain') && row.audible_sources.length !== 1), 'None/uncertain cannot be combined with other sources.');
    const ids = new Set();
    for (const face of row.faces) {
      check(object(face) && TRACK.test(face.track_id || '') && !ids.has(face.track_id), 'Use unique track IDs such as T01; reuse the same ID across frames.');
      ids.add(face.track_id);
      check(VISIBILITY.includes(face.visibility) && SPEECH.includes(face.speaking), 'Invalid face visibility or speaking label.');
      if (face.visibility === 'out_of_view') check(face.box === null, 'Out-of-view tracks must not have a box.');
      else {
        check(Array.isArray(face.box) && face.box.length === 4 && face.box.every(Number.isFinite), 'Draw a face box before applying labels.');
        const [x, y, w, h] = face.box;
        check(x >= 0 && y >= 0 && w > 0 && h > 0 && x + w <= 1.000001 && y + h <= 1.000001, 'Face box must fit inside the image.');
      }
    }
    if (row.scene === 'unlabelled') {
      check(row.faces.length === 0 && row.audible_sources.length === 0, 'Unlabelled frames cannot contain inferred labels.');
    } else {
      check(row.audible_sources.length > 0, 'Choose the audible source, including none or uncertain.');
      const present = row.faces.filter(f => f.visibility !== 'out_of_view');
      if (row.scene === 'faces') check(present.length > 0, 'Faces-in-view needs at least one visible or occluded track.');
      if (row.scene === 'no_face') check(present.length === 0, 'No-face conflicts with a visible face box.');
      const visibleSpeech = present.some(f => f.speaking === 'speaking_audible');
      check(!visibleSpeech || row.audible_sources.includes('visible') || row.audible_sources.includes('uncertain'), 'An audible visible speaker needs the visible source label.');
      check(!row.audible_sources.includes('visible') || visibleSpeech, 'Visible-source speech needs a matching audible visible face.');
    }
    return { ...frameIdentity(frame), scene: row.scene,
      faces: row.faces.map(f => ({ track_id: f.track_id, box: f.box === null ? null : [...f.box], visibility: f.visibility, speaking: f.speaking })),
      audible_sources: [...row.audible_sources] };
  }
  function createProject(manifest, rater, manifestSha256) {
    const m = validateManifest(manifest);
    check(ID.test(rater || ''), 'Use a rater pseudonym such as R01, without spaces.');
    check(HASH.test(manifestSha256 || ''), 'Prepared manifest SHA256 is required.');
    return { manifest: m, prepared_manifest_sha256: manifestSha256, rater_id: rater, rows: m.frames.map(blank) };
  }
  function applyRange(project, first, last, row, overwrite = false) {
    check(Number.isSafeInteger(first) && Number.isSafeInteger(last) && first >= 0 && last >= first && last < project.rows.length, 'Range must stay within this clip (inclusive frame indices).');
    const next = copy(project);
    for (let i = first; i <= last; i += 1) {
      check(overwrite || project.rows[i].scene === 'unlabelled', 'Range contains existing labels; explicitly allow replacement.');
      next.rows[i] = validateRow({ ...copy(row), ...frameIdentity(project.manifest.frames[i]) }, project.manifest.frames[i]);
    }
    return next;
  }
  function clearFrame(project, index) {
    check(Number.isSafeInteger(index) && index >= 0 && index < project.rows.length, 'Invalid frame index.');
    const next = copy(project);
    next.rows[index] = blank(project.manifest.frames[index]);
    return next;
  }
  function summary(project) {
    const unlabelled = project.rows.filter(r => r.scene === 'unlabelled').length;
    const uncertain = project.rows.filter(r => r.scene === 'uncertain' || r.audible_sources.includes('uncertain') || r.faces.some(f => f.speaking === 'uncertain')).length;
    return { frames: project.rows.length, reviewed: project.rows.length - unlabelled, unlabelled, uncertain, unknown: unlabelled + uncertain };
  }
  function exportProject(project, createdUtc = new Date().toISOString()) {
    check(ID.test(project.rater_id || ''), 'Invalid rater pseudonym.');
    return { schema_version: 1, kind: 'sensor5.frame_labels', clip_id: project.manifest.clip_id,
      capture_sha256: project.manifest.capture_sha256, prepared_manifest_sha256: project.prepared_manifest_sha256,
      rater_id: project.rater_id, created_utc: createdUtc, audio_start_us: project.manifest.audio_start_us,
      audio_sha256: project.manifest.audio_sha256, image_sha256: project.manifest.frames.map(f => f.sha256),
      frames: project.rows.map((row, i) => validateRow(row, project.manifest.frames[i])), summary: summary(project), ...timingMetadata(project.manifest) };
  }
  function importProject(project, input) {
    check(object(input) && input.schema_version === 1 && input.kind === 'sensor5.frame_labels', 'Unsupported annotation file.');
    check(input.clip_id === project.manifest.clip_id && input.capture_sha256 === project.manifest.capture_sha256 &&
      input.prepared_manifest_sha256 === project.prepared_manifest_sha256, 'Annotation file belongs to different capture or prepared media.');
    check(input.audio_start_us === project.manifest.audio_start_us && input.audio_sha256 === project.manifest.audio_sha256,
      'Annotation audio identity/origin differs.');
    check(Array.isArray(input.image_sha256) && JSON.stringify(input.image_sha256) === JSON.stringify(project.manifest.frames.map(f => f.sha256)), 'Annotation frame images differ.');
    check(JSON.stringify(timingMetadata(input)) === JSON.stringify(timingMetadata(project.manifest)), 'Annotation alignment/scoring metadata differs from the prepared media.');
    check(ID.test(input.rater_id || ''), 'Invalid rater pseudonym.');
    check(Array.isArray(input.frames) && input.frames.length === project.rows.length, 'Annotation frame count differs.');
    const next = copy(project);
    next.rater_id = input.rater_id;
    next.rows = input.frames.map((row, i) => validateRow(row, project.manifest.frames[i]));
    if (input.summary !== undefined) check(JSON.stringify(input.summary) === JSON.stringify(summary(next)), 'Annotation summary differs from its frame labels.');
    return next;
  }
  function frameAtTime(frames, timestampUs) {
    check(Number.isFinite(timestampUs), 'Invalid playback time.');
    let low = 0, high = frames.length - 1;
    while (low < high) { const mid = Math.ceil((low + high) / 2); if (frames[mid].timestamp_us <= timestampUs) low = mid; else high = mid - 1; }
    return low;
  }
  function audioTime(timestampUs, audioStartUs) { return (timestampUs - audioStartUs) / 1000000; }
  const api = { validateManifest, validateRow, createProject, applyRange, clearFrame, summary, exportProject, importProject, frameAtTime, audioTime, safePath, alignmentNotice };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  root.Sensor5Labels = api;
  if (typeof document === 'undefined') return;

  document.addEventListener('DOMContentLoaded', () => {
    const $ = id => document.getElementById(id);
    const canvas = $('frame'), ctx = canvas.getContext('2d'), audio = $('audio');
    let project = null, index = 0, draft = null, files = new Map(), urls = new Map(), image = null;
    let revision = 0, confirmedRevision = 0, exportedRevision = -1, draftDirty = false, drawTrack = null, drag = null;
    let loading = false, frameLoaded = false, renderToken = 0, playing = false, playbackStart = 0, playbackClock = 0, audioStarted = false;
    let audioPending = false, animation = null, currentImageUrl = null, playbackGeneration = 0;
    const pretty = value => value.replaceAll('_', ' ');
    const dirty = () => revision !== confirmedRevision || draftDirty;
    function message(text, error = false) { $('message').textContent = text; $('message').className = error ? 'message error' : 'message'; }
    function guarded(fn) { return async (...args) => { try { await fn(...args); } catch (e) { message(e.message, true); } }; }
    function canLeave() { return !dirty() || window.confirm('Unconfirmed labels or editor changes remain. Leave them without exporting?'); }
    function stop() { playbackGeneration += 1; playing = false; audioStarted = false; audioPending = false; audio.pause(); cancelAnimationFrame(animation); $('play').textContent = 'Play from frame'; $('editor').disabled = !frameLoaded; }
    function changed() { revision += 1; updateStatus(); }
    function updateStatus() {
      if (!project) return;
      const s = summary(project);
      $('counts').textContent = `${s.reviewed}/${s.frames} reviewed · ${s.unlabelled} unlabelled · ${s.uncertain} uncertain · ${s.unknown} unknown`;
      $('saveState').textContent = draftDirty ? 'Editor changes are not applied.' : revision === confirmedRevision ? 'No unconfirmed applied changes.' : exportedRevision === revision ? 'Download requested. Confirm that you kept the file.' : 'Applied labels need a JSON export.';
      $('confirmExport').disabled = exportedRevision !== revision || draftDirty || revision === confirmedRevision;
      $('export').disabled = !project || draftDirty;
    }
    function editChanged() { draftDirty = true; updateStatus(); }
    function sourceControls() {
      for (const input of document.querySelectorAll('[name="source"]')) input.checked = draft.audible_sources.includes(input.value);
    }
    function select(options, selected, label, change) {
      const el = document.createElement('select'); el.setAttribute('aria-label', label);
      for (const value of options) { const option = document.createElement('option'); option.value = value; option.textContent = pretty(value); el.append(option); }
      el.value = selected; el.addEventListener('change', () => change(el.value)); return el;
    }
    function renderFaces() {
      const panel = $('faces'); panel.replaceChildren();
      for (const face of draft.faces) {
        const row = document.createElement('div'); row.className = 'face-row';
        const title = document.createElement('strong'); title.textContent = face.track_id; row.append(title);
        row.append(select(VISIBILITY, face.visibility, `${face.track_id} visibility`, value => {
          face.visibility = value; if (value === 'out_of_view') face.box = null;
          editChanged(); renderFaces(); draw();
        }));
        row.append(select(SPEECH, face.speaking, `${face.track_id} speaking`, value => { face.speaking = value; editChanged(); draw(); }));
        const box = document.createElement('button'); box.type = 'button'; box.textContent = face.box ? 'Replace box' : 'Draw box';
        box.disabled = face.visibility === 'out_of_view'; box.addEventListener('click', () => { stop(); drawTrack = face.track_id; message(`Drag a box around ${face.track_id} on this frame.`); }); row.append(box);
        const remove = document.createElement('button'); remove.type = 'button'; remove.textContent = 'Remove track';
        remove.addEventListener('click', () => { draft.faces = draft.faces.filter(f => f !== face); editChanged(); renderFaces(); draw(); }); row.append(remove);
        const desc = document.createElement('span'); desc.className = 'box-desc';
        desc.textContent = face.box ? `Box: ${face.box.map(x => x.toFixed(3)).join(', ')}` : face.visibility === 'out_of_view' ? 'No box: out of view' : 'Box required'; row.append(desc); panel.append(row);
      }
      const ids = new Set(project.rows.flatMap(r => r.faces.map(f => f.track_id)));
      const list = $('knownTracks'); list.replaceChildren();
      for (const id of [...ids].sort()) { const option = document.createElement('option'); option.value = id; list.append(option); }
    }
    function draw() {
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      if (!image || !frameLoaded) return;
      ctx.drawImage(image, 0, 0, canvas.width, canvas.height);
      for (const [i, face] of draft.faces.entries()) {
        if (!face.box) continue;
        const [x, y, w, h] = face.box;
        ctx.strokeStyle = i % 2 ? '#efbb6a' : '#77e0d1'; ctx.lineWidth = Math.max(2, canvas.width / 400);
        ctx.strokeRect(x * canvas.width, y * canvas.height, w * canvas.width, h * canvas.height);
        ctx.font = `${Math.max(14, canvas.width / 45)}px sans-serif`; ctx.fillStyle = ctx.strokeStyle;
        ctx.fillText(`${face.track_id}: ${pretty(face.speaking)}`, x * canvas.width + 4, Math.max(18, y * canvas.height - 4));
      }
      if (drag) { ctx.strokeStyle = '#fff'; ctx.strokeRect(drag.x * canvas.width, drag.y * canvas.height, (drag.endX - drag.x) * canvas.width, (drag.endY - drag.y) * canvas.height); }
    }
    function showFrame(next, fromPlayback = false) {
      if (!project) return;
      if (!fromPlayback && draftDirty && !window.confirm('Discard unapplied editor changes on this frame?')) return;
      index = Math.max(0, Math.min(project.rows.length - 1, next));
      draft = copy(project.rows[index]); draftDirty = false; drawTrack = null; drag = null;
      $('position').value = index; $('jump').value = index; $('rangeFirst').value = index; $('rangeLast').value = index;
      const frame = project.manifest.frames[index];
      const boundary = project.manifest.timestamp_boundary || 'native PTS';
      const source = frame.original_index === undefined ? '' : ` · original frame ${frame.original_index}`;
      $('frameInfo').textContent = `Frame ${index} / ${project.rows.length - 1}${source} · ${boundary} ${frame.timestamp_us} µs · audio ${audioTime(frame.timestamp_us, project.manifest.audio_start_us).toFixed(3)} s`;
      $('scene').value = draft.scene; sourceControls(); renderFaces(); updateStatus();
      frameLoaded = false; $('editor').disabled = true; draw();
      const token = ++renderToken, img = new Image();
      const newUrl = URL.createObjectURL(files.get(frame.image_file));
      img.onload = () => {
        if (token !== renderToken) { URL.revokeObjectURL(newUrl); return; }
        if ((frame.width_px !== undefined && img.naturalWidth !== frame.width_px) ||
            (frame.height_px !== undefined && img.naturalHeight !== frame.height_px)) {
          URL.revokeObjectURL(newUrl); stop();
          message(`Frame ${index} dimensions do not match the prepared manifest; labels are disabled.`, true); return;
        }
        if (currentImageUrl) URL.revokeObjectURL(currentImageUrl);
        currentImageUrl = newUrl; image = img; canvas.width = img.naturalWidth; canvas.height = img.naturalHeight;
        frameLoaded = true; $('editor').disabled = playing; draw();
      };
      img.onerror = () => { URL.revokeObjectURL(newUrl); if (token === renderToken) { stop(); message(`Cannot decode frame ${index}; labels are disabled.`, true); } };
      img.src = newUrl;
    }
    function currentDraft() {
      return validateRow(draft, project.manifest.frames[index]);
    }
    async function hash(file) {
      check(globalThis.crypto?.subtle, 'This browser needs local Web Crypto support to verify media.');
      check(file.size <= 512 * 1024 * 1024, 'A prepared file is too large (512 MiB limit).');
      const bytes = await crypto.subtle.digest('SHA-256', await file.arrayBuffer());
      return Array.from(new Uint8Array(bytes), x => x.toString(16).padStart(2, '0')).join('');
    }
    async function openFiles(selected, synthetic = false) {
      if (loading || !canLeave()) return;
      loading = true; stop(); $('openFolder').disabled = $('practice').disabled = true;
      try {
        const nextFiles = new Map();
        for (const file of selected) {
          const rel = file.webkitRelativePath ? file.webkitRelativePath.split('/').slice(1).join('/') : file.name;
          check(!nextFiles.has(rel), 'Duplicate selected file path.'); nextFiles.set(rel, file);
        }
        const source = nextFiles.get('manifest.json');
        check(source && source.size < 16 * 1024 * 1024, 'Choose the prepared folder containing one manifest.json.');
        const manifest = validateManifest(JSON.parse(await source.text()));
        const wanted = [[manifest.audio_file, manifest.audio_sha256], ...manifest.frames.map(f => [f.image_file, f.sha256])];
        for (const [i, [name, expected]] of wanted.entries()) {
          const file = nextFiles.get(name); check(file, `Missing prepared file: ${name}`);
          check(await hash(file) === expected, `SHA256 mismatch: ${name}`);
          if (i % 10 === 0 || i === wanted.length - 1) message(`Verifying local files: ${i + 1}/${wanted.length}…`);
        }
        const nextProject = createProject(manifest, $('rater').value.trim(), await hash(source));
        for (const url of urls.values()) URL.revokeObjectURL(url);
        urls = new Map(); files = nextFiles; project = nextProject;
        const audioUrl = URL.createObjectURL(files.get(manifest.audio_file)); urls.set('audio', audioUrl); audio.src = audioUrl;
        revision = 0; confirmedRevision = 0; exportedRevision = -1; draftDirty = false;
        $('workspace').hidden = false; $('clipTitle').textContent = `${synthetic ? 'Synthetic practice · ' : ''}${manifest.clip_id}`;
        $('hashInfo').textContent = `Capture ${manifest.capture_sha256} · prepared media hashes verified locally`;
        $('alignmentInfo').textContent = alignmentNotice(manifest);
        $('position').max = $('jump').max = $('rangeFirst').max = $('rangeLast').max = manifest.frames.length - 1;
        $('import').disabled = false; showFrame(0);
        message('Clip opened. Every frame starts unlabelled. Keep separate files for each rater.');
      } finally { loading = false; $('openFolder').disabled = $('practice').disabled = false; }
    }
    $('folder').addEventListener('change', guarded(async event => { const selected = [...event.target.files]; event.target.value = ''; await openFiles(selected); }));
    $('openFolder').addEventListener('click', () => $('folder').click());
    $('rater').addEventListener('change', guarded(() => { check(ID.test($('rater').value.trim()), 'Use a rater pseudonym without spaces.'); if (project) { project.rater_id = $('rater').value.trim(); changed(); } }));
    $('previous').addEventListener('click', () => { stop(); showFrame(index - 1); });
    $('next').addEventListener('click', () => { stop(); showFrame(index + 1); });
    $('position').addEventListener('input', () => { stop(); showFrame(Number($('position').value)); });
    $('jumpButton').addEventListener('click', guarded(() => { const n = Number($('jump').value); check(Number.isSafeInteger(n) && n >= 0 && n < project.rows.length, 'Frame is outside this clip.'); stop(); showFrame(n); }));
    $('scene').addEventListener('change', () => {
      draft.scene = $('scene').value;
      if (draft.scene === 'unlabelled') { draft.faces = []; draft.audible_sources = []; sourceControls(); renderFaces(); draw(); }
      editChanged();
    });
    for (const input of document.querySelectorAll('[name="source"]')) input.addEventListener('change', () => {
      if (input.checked && ['none', 'uncertain'].includes(input.value)) for (const other of document.querySelectorAll('[name="source"]')) if (other !== input) other.checked = false;
      if (input.checked && !['none', 'uncertain'].includes(input.value)) for (const other of document.querySelectorAll('[name="source"]')) if (['none', 'uncertain'].includes(other.value)) other.checked = false;
      draft.audible_sources = [...document.querySelectorAll('[name="source"]:checked')].map(x => x.value); editChanged();
    });
    $('addFace').addEventListener('click', guarded(() => {
      const id = $('trackId').value.trim(); check(TRACK.test(id), 'Use a stable track ID such as T01.'); check(!draft.faces.some(f => f.track_id === id), 'That track is already on this frame.');
      draft.faces.push({ track_id: id, box: null, visibility: 'visible', speaking: 'uncertain' }); draft.scene = 'faces'; $('scene').value = 'faces'; drawTrack = id;
      editChanged(); renderFaces(); message(`Drag ${id}'s face box. Reuse this ID for the same person in later frames.`);
    }));
    function point(event) { const rect = canvas.getBoundingClientRect(); return { x: Math.max(0, Math.min(1, (event.clientX - rect.left) / rect.width)), y: Math.max(0, Math.min(1, (event.clientY - rect.top) / rect.height)) }; }
    canvas.addEventListener('pointerdown', event => { if (!drawTrack || !frameLoaded || playing) return; const p = point(event); drag = { ...p, endX: p.x, endY: p.y }; canvas.setPointerCapture(event.pointerId); });
    canvas.addEventListener('pointermove', event => { if (!drag) return; const p = point(event); drag.endX = p.x; drag.endY = p.y; draw(); });
    canvas.addEventListener('pointerup', () => {
      if (!drag) return;
      const x = Math.min(drag.x, drag.endX), y = Math.min(drag.y, drag.endY), w = Math.abs(drag.x - drag.endX), h = Math.abs(drag.y - drag.endY);
      if (w * canvas.width >= 2 && h * canvas.height >= 2) {
        draft.faces.find(f => f.track_id === drawTrack).box = [x, y, w, h]; editChanged(); message('Box set in the editor. Apply this frame to keep it.');
      }
      drag = null; drawTrack = null; renderFaces(); draw();
    });
    canvas.addEventListener('pointercancel', () => { drag = null; draw(); });
    $('apply').addEventListener('click', guarded(() => { project = applyRange(project, index, index, currentDraft(), true); draftDirty = false; changed(); showFrame(index); message('Frame labels applied. Export JSON to keep them.'); }));
    $('clear').addEventListener('click', () => { if (!window.confirm('Clear this frame back to unlabelled?')) return; project = clearFrame(project, index); draftDirty = false; changed(); showFrame(index); });
    $('copyPrevious').addEventListener('click', guarded(() => { check(index > 0, 'This is the first frame.'); draft = { ...copy(project.rows[index - 1]), ...frameIdentity(project.manifest.frames[index]) }; $('scene').value = draft.scene; sourceControls(); renderFaces(); editChanged(); draw(); message('Previous labels copied only into the editor. Check boxes and speech before applying.'); }));
    $('applyRange').addEventListener('click', guarded(() => {
      const first = Number($('rangeFirst').value), last = Number($('rangeLast').value), overwrite = $('overwrite').checked;
      const next = applyRange(project, first, last, currentDraft(), overwrite);
      if (!window.confirm(`Apply these exact labels and boxes to frames ${first}–${last} (${last - first + 1} frames)? No tracking or interpolation is performed.`)) return;
      project = next; draftDirty = false; changed(); showFrame(index); message('Explicit range applied. Review boxes and speech on every frame; export to keep it.');
    }));
    $('play').addEventListener('click', guarded(async () => {
      if (playing) { stop(); $('editor').disabled = !frameLoaded; return; }
      check(!draftDirty, 'Apply or discard editor changes before playback.');
      check(Number.isFinite(audio.duration), 'Wait for the audio to load before playback.');
      playing = true; $('editor').disabled = true; $('play').textContent = 'Pause';
      const generation = ++playbackGeneration;
      playbackStart = project.manifest.frames[index].timestamp_us; playbackClock = performance.now(); audioStarted = false;
      const frames = project.manifest.frames, last = frames.at(-1).timestamp_us;
      const tail = frames.length > 1 ? Math.min(1000000, last - frames.at(-2).timestamp_us) : 40000;
      const tick = () => {
        if (!playing || generation !== playbackGeneration) return;
        let time = playbackStart + (performance.now() - playbackClock) * 1000;
        const local = audioTime(time, project.manifest.audio_start_us);
        if (!audioStarted && !audioPending && local >= 0 && local < audio.duration) {
          audio.currentTime = local; audioPending = true;
          audio.play().then(() => {
            if (generation !== playbackGeneration) return;
            audioPending = false; if (playing) audioStarted = true;
          }).catch(e => {
            if (generation !== playbackGeneration) return;
            stop(); message(`Playback failed: ${e.message}`, true);
          });
        }
        if (audioStarted && !audio.ended) time = project.manifest.audio_start_us + audio.currentTime * 1000000;
        if (audioStarted && audio.ended) { audioStarted = false; playbackStart = project.manifest.audio_start_us + audio.duration * 1000000; playbackClock = performance.now(); time = playbackStart; }
        const next = frameAtTime(frames, time); if (next !== index) showFrame(next, true);
        if (time >= last + tail) { stop(); $('editor').disabled = !frameLoaded; return; }
        animation = requestAnimationFrame(tick);
      };
      tick();
    }));
    $('export').addEventListener('click', guarded(() => {
      check(!draftDirty, 'Apply editor changes before exporting.');
      check(ID.test($('rater').value.trim()) && $('rater').value.trim() === project.rater_id, 'Fix the rater pseudonym before exporting.');
      const output = exportProject(project), blob = new Blob([JSON.stringify(output, null, 2) + '\n'], { type: 'application/json' });
      const url = URL.createObjectURL(blob), a = document.createElement('a');
      a.href = url; a.download = `${project.manifest.clip_id}-${project.rater_id}-${output.created_utc.replaceAll(':', '-')}.labels.json`; a.click(); setTimeout(() => URL.revokeObjectURL(url), 60000);
      exportedRevision = revision; updateStatus(); message('JSON download requested. Confirm the file exists in your private archive before closing.');
    }));
    $('confirmExport').addEventListener('click', () => { if (exportedRevision === revision && !draftDirty) { confirmedRevision = revision; updateStatus(); message('You confirmed the exported file was kept. Original media remain unchanged.'); } });
    $('import').addEventListener('click', () => $('labelsFile').click());
    $('labelsFile').addEventListener('change', guarded(async event => {
      const file = event.target.files[0]; event.target.value = ''; if (!file || !canLeave()) return;
      check(file.size < 32 * 1024 * 1024, 'Annotation file is too large.');
      const next = importProject(project, JSON.parse(await file.text()));
      if (next.rater_id !== project.rater_id && !window.confirm(`This file belongs to rater ${next.rater_id}. Importing changes your rater identity and reveals those labels. Continue only when the independent pass is complete.`)) return;
      stop(); project = next; $('rater').value = project.rater_id; revision = confirmedRevision = 0; exportedRevision = -1; draftDirty = false; showFrame(index); message('Existing annotation file imported after media-identity checks.');
    }));
    window.addEventListener('beforeunload', event => { if (dirty()) { event.preventDefault(); event.returnValue = ''; } });
    document.addEventListener('keydown', event => {
      if (!project || /INPUT|SELECT|TEXTAREA|BUTTON/.test(event.target.tagName) || event.ctrlKey || event.metaKey || event.altKey) return;
      if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') { event.preventDefault(); stop(); showFrame(index + (event.key === 'ArrowLeft' ? -1 : 1)); }
      if (event.code === 'Space') { event.preventDefault(); $('play').click(); }
      if (event.key.toLowerCase() === 's' && !playing && frameLoaded) { event.preventDefault(); $('apply').click(); }
    });
    $('practice').addEventListener('click', guarded(async () => {
      const selected = [], frames = [], surface = document.createElement('canvas'); surface.width = 640; surface.height = 360;
      const c = surface.getContext('2d');
      for (const [i, pts] of [1000000, 1040000, 1120000, 1160000, 1240000].entries()) {
        c.fillStyle = '#243947'; c.fillRect(0, 0, 640, 360); c.fillStyle = '#fff'; c.font = '20px sans-serif'; c.fillText(`Synthetic practice — frame ${i}`, 20, 32);
        c.fillStyle = '#77e0d1'; c.beginPath(); c.arc(210 + i * 5, 175, 65, 0, Math.PI * 2); c.fill(); c.fillStyle = '#243947'; c.fillRect(185 + i * 5, 185, 50, 6 + i * 3);
        c.fillStyle = '#efbb6a'; c.beginPath(); c.arc(430, 175, 65, 0, Math.PI * 2); c.fill(); c.fillStyle = '#243947'; c.fillRect(405, 185, 50, 6);
        const blob = await new Promise(resolve => surface.toBlob(resolve, 'image/png'));
        const file = new File([blob], `frame-${i}.png`, { type: 'image/png' }); selected.push(file);
        frames.push({ index: i, timestamp_us: pts, image_file: file.name, sha256: await hash(file), width_px: 640, height_px: 360 });
      }
      const samples = 8000, wav = new ArrayBuffer(44 + samples * 2), view = new DataView(wav);
      const str = (at, value) => [...value].forEach((ch, i) => view.setUint8(at + i, ch.charCodeAt(0)));
      str(0, 'RIFF'); view.setUint32(4, 36 + samples * 2, true); str(8, 'WAVE'); str(12, 'fmt '); view.setUint32(16, 16, true); view.setUint16(20, 1, true); view.setUint16(22, 1, true); view.setUint32(24, 8000, true); view.setUint32(28, 16000, true); view.setUint16(32, 2, true); view.setUint16(34, 16, true); str(36, 'data'); view.setUint32(40, samples * 2, true);
      const sound = new File([wav], 'audio.wav', { type: 'audio/wav' }); selected.push(sound);
      const manifest = { schema_version: 1, clip_id: 'synthetic-practice', capture_sha256: '0'.repeat(64), frames, audio_file: sound.name, audio_sha256: await hash(sound), audio_start_us: 1080000 };
      selected.push(new File([JSON.stringify(manifest)], 'manifest.json', { type: 'application/json' })); await openFiles(selected, true);
    }));
  });
})(typeof globalThis !== 'undefined' ? globalThis : this);
