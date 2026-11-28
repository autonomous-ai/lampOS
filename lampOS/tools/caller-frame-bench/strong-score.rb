# frozen_string_literal: true

require_relative "strong-replay"
require_relative "reviewer_raw_pairs"

# Independently reconstruct the native vote, compare matched cases, and invoke #8885.
module StrongScore
  PROVIDER_MANIFEST = "e5a095c707c04fbea587a20ca847f38603cad88e4ba9c07af304981daa10b5c4"
  PROVIDER_OLD = "a4e6234ecdccac65f066b4723dedc3d9da8ffec57fc63ef7add76cd3da157d1d"
  MODEL = "d8c32b0dbc17e31c7b13c9d7910f0adaca2ea0b0b3351da05041c9168e741994"
  module_function

  def nonnegative(text)
    raise ArgumentError, "expected an unsigned integer" unless text.match?(/\A\d+\z/)
    Integer(text, 10)
  end

  def boolean(text)
    {"true" => true, "false" => false}.fetch(text)
  end

  def float32(number)
    [number].pack("e").unpack1("e")
  end

  def frame_vote(numbers)
    return [nil, "unknown"] if numbers.size < 8
    values = numbers.sort
    left = values[(values.size - 1) / 2]
    right = values[values.size / 2]
    score = float32(float32(left + right) * 0.5)
    [score, score > 0.25 ? "same" : "different"]
  end

  def load_frames(path, ids)
    frames = {}
    File.open(path, "r:utf-8") do |file|
      raise ArgumentError, "unexpected frame header" unless file.gets&.chomp == "case_id\tstart_sample\tend_sample_exclusive\tcosine"
      file.each_line(chomp: true) do |line|
        id, start, stop, cosine = line.split("\t", -1)
        raise ArgumentError, "unexpected frame row" unless ids.key?(id) && cosine && line.count("\t") == 3
        row = (frames[id] ||= [])
        unless nonnegative(start) == row.size * 160 && nonnegative(stop) == row.size * 160 + 400
          raise ArgumentError, "invalid or repeated 25 ms frame at #{id}"
        end
        value = cosine.empty? ? nil : float32(Float(cosine))
        raise ArgumentError, "invalid frame cosine" if value && !(value.finite? && value.between?(-1, 1))
        row << value
      end
    end
    frames
  end

  def check_features(path, selected, case_hashes, states, times)
    counts = Hash.new(0)
    retained = {"enrollment" => {}, "probe" => {}}
    File.open(path, "r:utf-8") do |io|
      raise ArgumentError, "unexpected feature timing header" unless io.gets&.chomp == "role\twav_sha256\texecution\tavailable\thost_us"
      selected.each do |id|
        s = states.fetch(id)
        t = times.fetch(id)
        wake, probe = case_hashes.fetch(id)
        [["enrollment", wake, "fresh", s[:enroll], t[:enroll]], ["probe", probe, s[:enroll] ? "fresh" : "skipped_no_enrollment", s[:probe], t[:probe]]].each do |role, wave, stage, available, us|
          expected = [role, wave, stage, available.to_s, us.to_s]
          raise ArgumentError, "fresh feature record differs from #{id}" unless io.gets&.chomp&.split("\t", -1) == expected
          if stage == "fresh"
            seen = retained.fetch(role)
            raise ArgumentError, "identical fresh inputs disagreed about feature availability" if seen.key?(wave) && seen.fetch(wave) != available
            seen[wave] = available
          end
          counts[stage] += 1
        end
      end
      case_hashes.each do |id, (wake, probe)|
        s = states.fetch(id)
        [["enrollment", wake, s[:enroll]], ["probe", probe, s[:probe]]].each do |role, wave, available|
          next if role == "probe" && !s[:enroll]
          seen = retained.fetch(role)
          if seen.key?(wave)
            raise ArgumentError, "duplicate WAV yielded contradictory features" unless seen.fetch(wave) == available
            next
          end
          r = io.gets&.chomp&.split("\t", -1)
          unless r&.size == 5 && r.first(4) == [role, wave, "cache_fill", available.to_s]
            raise ArgumentError, "missing or incorrect cached feature provenance for #{id}"
          end
          nonnegative(r.last)
          seen[wave] = available
          counts["cache_fill"] += 1
        end
      end
      raise ArgumentError, "unplanned native feature work" unless io.eof?
    end
    counts
  end

  def check_native(native, planned, predictions, selected, case_hashes, probe_lengths)
    unless predictions.keys == planned.keys
      raise ArgumentError, "native predictions must cover every ordered opportunity, including unknown"
    end
    frames = load_frames(File.join(native, "frames.tsv"), planned)
    selected_ids = selected.to_h { |id| [id, true] }
    first_id = planned.each_key.first
    states = CallerFrameBench.table(File.join(native, "status.tsv"), %w[case_id enrollment_available probe_available active_frames total_frames cached_compare_us fresh_verified]) do |id, en, pr, active, count, elapsed, fresh|
      ref_available, probe_available = [en, pr].map { |item| boolean(item) }
      values = frames.fetch(id, [])
      defined = values.compact
      wanted = probe_available ? probe_lengths.fetch(case_hashes.fetch(id).last) : 0
      raise ArgumentError, "invalid frame count for the original WAV or input availability" unless nonnegative(count) == values.size && nonnegative(active) == defined.size && (!probe_available || ref_available) && values.size == wanted
      raw, output = frame_vote(defined)
      p = predictions.fetch(id)
      unless p.output == output && raw == (p.score && float32(p.score)) && boolean(fresh) == selected_ids.key?(id) && boolean(fresh) == !p.elapsed.nil?
        raise ArgumentError, "independent frame decision or timing differs for #{id}"
      end
      {enroll: ref_available, probe: probe_available, active: defined.size, total: values.size, compare_us: nonnegative(elapsed)}
    end
    raise ArgumentError, "native status differs from opportunities" unless states.keys == planned.keys
    times = CallerFrameBench.table(File.join(native, "fresh-times.tsv"), %w[case_id phase enroll_us probe_us decide_us total_us]) do |id, phase, enroll, probe, decide, total|
      p = predictions.fetch(id)
      stats = states.fetch(id)
      raise ArgumentError, "missing or invented probe timing for #{id}" unless probe.empty? == !stats.fetch(:enroll)
      parts = [enroll, decide, probe].reject(&:empty?).map { |t| nonnegative(t) }
      us = nonnegative(total)
      # Each timestamp difference is floored individually; a skipped branch still costs a few clock ticks.
      consistent = probe.empty? ? parts.sum <= us : (us - parts.sum).between?(0, 2)
      unless p.phase == phase && p.elapsed == us && consistent && phase == (id == first_id ? "first" : "warm")
        raise ArgumentError, "fresh host latency mismatch for #{id}"
      end
      {total: us, enroll: nonnegative(enroll), probe: probe.empty? ? nil : nonnegative(probe)}
    end
    raise ArgumentError, "fresh rows differ from the selected timing plan" unless times.keys == selected
    load_file = File.readlines(File.join(native, "load.tsv"), chomp: true)
    raise ArgumentError, "unexpected cold load record" unless load_file.size == 2 && load_file.first == "host_model_load_us"
    {model_load_us: nonnegative(load_file.last), uncached_pairs: times.size, cached_only_pairs: planned.size - times.size,
     feature_work: check_features(File.join(native, "feature-times.tsv"), selected, case_hashes, states, times),
     fresh_cache_trace_equal: times.size, unavailable_enrollments: states.values.count { |v| !v[:enroll] },
     unavailable_probes_with_enrollment: states.values.count { |v| v[:enroll] && !v[:probe] },
     frame_positions: states.values.sum { |v| v[:total] }, defined_frame_scores: states.values.sum { |v| v[:active] },
     cached_comparison_only_us: CallerFrameBench.timing(states.values.map { |v| v[:compare_us] })}
  end

  def official_row_ids(manifest, planned, sums, corpus, inventory, inventory_sha)
    raise ArgumentError, "only development-pair replays are supported" unless manifest.fetch("evaluation_mode") == "raw_development_pairs"
    sessions = CallerFrameReviewer.index(manifest.fetch("sessions"), "id")
    trials = CallerFrameReviewer.index(manifest.fetch("trials"), "id")
    cases = CallerFrameReviewer.index(manifest.fetch("cases"), "id")
    raise ArgumentError, "official opportunities differ from the original" unless cases.keys.sort == planned.keys.sort
    members = inventory.fetch("files").to_h { |f| [f.fetch("sha256"), true] }
    lengths = {}
    planned.to_h do |id, pair|
      c = cases.fetch(id)
      unless c.values_at("sensor", "expected", "fault") == ["voice_match", pair.label, false]
        raise ArgumentError, "unexpected official label or opportunity"
      end
      wake, probe = [[pair.reference, c.fetch("reference_trial_id")], [pair.followup, c.fetch("trial_id")]].map do |rel, trial_id|
        t = trials.fetch(trial_id)
        s = sessions.fetch(t.fetch("session_id"))
        hash = sums.fetch(rel)
        length = lengths[hash] ||= CallerHeldout.read_pcm(CallerFrameBench.private_file(corpus, rel)).bytesize * 1_000_000 / 32_000
        unless s.values_at("recording_sha256", "inventory_sha256", "readme_sha256", "split", "data_origin", "retention", "clock_verified") ==
               [hash, inventory_sha, inventory.fetch("readme_sha256"), "development", "generated", "keep_all", false] &&
               members.key?(hash) && t.values_at("start_us", "end_us", "noise") == [0, length, pair.condition == "quiet" ? "quiet" : "background"]
          raise ArgumentError, "official source or exclusive file endpoint differs for #{id}"
        end
        [s, t]
      end
      unless (wake.last.fetch("speaker_id") == probe.last.fetch("speaker_id")) == (pair.label == "same")
        raise ArgumentError, "source speaker identity contradicts #{id}"
      end
      [id, [probe.first.fetch("clock_id"), probe.last.fetch("end_us")]]
    end
  end

  # When Mac 2 has no existing v2 manifest, each speaker mapping must cite retained original collection metadata.
  def physical_manifest(map_path, sums, planned, corpus, inventory, inventory_sha)
    metadata = inventory.fetch("files").to_h { |r| [r.fetch("path"), r.fetch("sha256")] }
    speakers = CallerFrameBench.table(map_path, %w[wav_sha256 speaker_id source_path source_sha256]) do |hash, speaker, file, proof|
      unless hash.match?(/\A[0-9a-f]{64}\z/) && !speaker.empty? && metadata.fetch(file) == proof
        raise ArgumentError, "physical speaker mapping requires an inventoried original source record"
      end
      speaker
    end
    pairs = planned.values
    paths = pairs.flat_map { |p| [p.reference, p.followup] }.uniq
    wanted = paths.map { |rel| sums.fetch(rel) }.uniq
    raise ArgumentError, "speaker map differs from the selected WAVs" unless wanted.sort == speakers.keys.sort
    sessions = {}
    trials = {}
    paths.each do |rel|
      hash = sums.fetch(rel)
      next if sessions.key?(hash)
      us = CallerHeldout.read_pcm(CallerFrameBench.private_file(corpus, rel)).bytesize * 1_000_000 / 32_000
      id = "w-" + hash
      sessions[hash] = {id: id, split: "development", data_origin: "generated", consent: nil, retention: "keep_all", started_unix_s: nil,
                        recording_sha256: hash, inventory_sha256: inventory_sha, readme_sha256: inventory.fetch("readme_sha256"),
                        clock_id: id, clock_verified: false, sync_error_us: nil}
      trials[hash] = {id: id, session_id: id, speaker_id: speakers.fetch(hash), source: "person", start_us: 0, end_us: us,
                      call_end_us: nil, recipient: "unknown", script_id: id, look_target: "unknown", head_target: "unknown", bearing_deg: nil,
                      distance_mm: nil, light: "unknown", noise: "quiet", visible_faces: [], subject_visible: false,
                      overlap: false, first_use: false, condition: "normal"}
    end
    cases = pairs.map { |p| {id: p.id, sensor: "voice_match", trial_id: trials.fetch(sums.fetch(p.followup)).fetch(:id),
                              reference_trial_id: trials.fetch(sums.fetch(p.reference)).fetch(:id), expected: p.label, fault: false} }
    {version: 2, rubric_id: CallerFrameReviewer::RUBRIC, evaluation_mode: "raw_development_pairs",
     collection_id: "physical-playback-" + inventory_sha[0, 16], frozen_unix_s: inventory.fetch("created_unix_s"),
     sessions: sessions.values, trials: trials.values, cases: cases}
  end

  def run_official(corpus, inventory_file, manifest_arg, scorer, kind, planned, sums, predictions, freeze, out)
    executed = ->(args, name) { CallerFrameReviewer.invoke(scorer, args, File.join(out, name + ".json")) }
    scorer_sha = StrongReplay.sha(scorer)
    unless freeze.fetch("scorer_revision") == CallerRawPairs::REVISION && freeze.fetch("scorer_sha256") == scorer_sha
      raise ArgumentError, "scorer differs from the source and executable in the freeze"
    end
    inventory_sha = StrongReplay.sha(inventory_file)
    raise ArgumentError, "official inventory verification failed" unless executed.call(["verify-archive", corpus, inventory_file], "verify-inventory") == 0
    inventory = StrongReplay.read(inventory_file)
    manifest_path = File.join(out, "manifest.json")
    map_sha = nil
    if manifest_arg.start_with?("@")
      raise ArgumentError, "only physical playback needs a local source speaker map" unless kind == "physical"
      map_path = File.realpath(manifest_arg.delete_prefix("@"))
      map_sha = StrongReplay.sha(map_path)
      FileUtils.cp(map_path, File.join(out, "local-speaker-map.tsv"))
      StrongReplay.json(manifest_path, physical_manifest(map_path, sums, planned, corpus, inventory, inventory_sha))
    else
      FileUtils.cp(manifest_arg, manifest_path)
    end
    manifest_sha = StrongReplay.sha(manifest_path)
    raise ArgumentError, "provider must use the original official manifest byte for byte" if kind == "provider" && manifest_sha != PROVIDER_MANIFEST
    endpoints = official_row_ids(StrongReplay.read(manifest_path), planned, sums, corpus, inventory, inventory_sha)
    raise ArgumentError, "#8885 rejected the pair manifest" unless executed.call(["validate", manifest_path], "validate") == 0
    rows = predictions.map do |id, p|
      clock, ending = endpoints.fetch(id)
      {case_id: id, clock_id: clock, output: p.output, input_end_us: ending, ready_us: nil, replay_elapsed_us: p.elapsed,
       added_wait_us: 0, held_line: false, safety_violation: false}
    end
    run_file = File.join(out, "run.json")
    header = freeze.slice("build_sha", "source_patch_sha256", "producer_binary_sha256", "model_sha256", "config_sha256")
    StrongReplay.json(run_file, header.merge(version: 2, sensor: "voice_match", evidence: "replay", location: "local", manifest_sha256: manifest_sha, predictions: rows))
    path = File.join(out, "reviewer")
    exit_code = executed.call(["score", "voice_match", manifest_path, run_file, path], "score-invocation")
    raise ArgumentError, "expected a completed development-only evaluation, got exit #{exit_code}" unless exit_code == 1
    report_file = File.join(path, "report.json")
    report = StrongReplay.read(report_file)
    digests = {"manifest" => manifest_sha, "run" => StrongReplay.sha(run_file), "scorer" => scorer_sha}
    CallerRawPairs.validate_report(report, planned, predictions, digests)
    {revision: CallerRawPairs::REVISION, scorer_sha256: scorer_sha, inventory_sha256: inventory_sha,
     inventory_readme_sha256: inventory.fetch("readme_sha256"), manifest_sha256: manifest_sha, run_sha256: digests["run"],
     report_sha256: StrongReplay.sha(report_file), source_speaker_map_sha256: map_sha, raw_counts: report.fetch("raw_by_split"),
     opportunities: report.fetch("development_pairs"), qualified: report.fetch("qualified")}
  end

  def score(plan, corpus, run, old_file, scorer, inventory, manifest, dest)
    plan, corpus, run = [plan, corpus, run].map { |p| File.realpath(p) }
    inputs = StrongReplay.read(File.join(plan, "input.json"))
    kind = inputs.fetch("kind")
    if kind == "physical" && inputs.fetch("original_source_seal_sha256") != StrongReplay::PHYSICAL_FOLDER
      raise ArgumentError, "prepare with the original outer Mac 2 folder SHA256SUMS to bind the physical source"
    end
    plan_files, run_files = [plan, run].map { |p| StrongReplay.verify_seal(p) }
    sums, audio_sha = StrongReplay.verify_corpus(kind, File.join(corpus, "pairs.tsv"))
    pair_hash = plan_files.fetch("pairs.tsv")
    unless audio_sha == inputs.fetch("audio_inventory_sha256") && sums.fetch("pairs.tsv") == pair_hash && inputs.fetch("pairs_sha256") == pair_hash
      raise ArgumentError, "original WAV set or manifest differs from the sealed plan"
    end
    planned = CallerFrameBench.pairs(File.join(plan, "pairs.tsv"), sums)
    by_provider = kind == "provider" ? CallerFrameBench.primary_groups(File.join(corpus, "pairs.tsv"), planned, sums) : {}
    freeze = StrongReplay.read(File.join(run, "freeze.json"))
    bindings = StrongReplay.read(File.join(run, "bindings.json"))
    unless freeze.fetch("model_sha256") == MODEL && %w[build_sha producer_binary_sha256 model_sha256 config_sha256].all? { |k| bindings.fetch(k) == freeze.fetch(k) } &&
           run_files.fetch("config") == freeze.fetch("config_sha256") && bindings.fetch("plan_seal_sha256") == StrongReplay.sha(File.join(plan, "SHA256SUMS")) &&
           bindings.fetch("freeze_sha256") == run_files.fetch("freeze.json")
      raise ArgumentError, "candidate or plan differs from the sealed native replay"
    end
    run_files.fetch("native/COMPLETE")
    verified = StrongReplay.read(File.join(run, "native/inputs.json"))
    %w[pairs cases assets].each do |key|
      raise ArgumentError, "native #{key} binding mismatch" unless verified.fetch(key + "_sha256") == plan_files.fetch(key + ".tsv")
    end
    %w[model_sha256 producer_binary_sha256].each { |key| raise ArgumentError, "native binding mismatch" unless verified.fetch(key) == freeze.fetch(key) }
    ordered = CallerFrameBench.table(File.join(plan, "cases.tsv"), %w[case_id reference_sha256 followup_sha256 time_fresh], pair_hash) do |id, ref, probe, fresh|
      p = planned.fetch(id)
      raise ArgumentError, "plan changed source waveform" unless [ref, probe] == [sums.fetch(p.reference), sums.fetch(p.followup)] && %w[0 1].include?(fresh)
      fresh == "1"
    end
    raise ArgumentError, "row order differs from the original pair list" unless ordered.keys == planned.keys
    selection = {"all" => planned.keys, "sample" => ordered.select { |_, v| v }.keys}.fetch(bindings.fetch("timing"))
    old_sha = StrongReplay.sha(old_file)
    raise ArgumentError, "baseline is not the original provider frame run" if kind == "provider" && old_sha != PROVIDER_OLD
    before = CallerFrameBench.predictions(old_file, pair_hash, planned)
    after = CallerFrameBench.predictions(File.join(run, "native/predictions.tsv"), pair_hash, planned)
    case_hashes = planned.transform_values { |p| [sums.fetch(p.reference), sums.fetch(p.followup)] }
    probe_lengths = {}
    planned.values.each do |p|
      probe_lengths[sums.fetch(p.followup)] ||= begin
        samples = CallerHeldout.read_pcm(CallerFrameBench.private_file(corpus, p.followup)).bytesize / 2
        samples < 400 ? 0 : 1 + (samples - 400) / 160
      end
    end
    native = check_native(File.join(run, "native"), planned, after, selection, case_hashes, probe_lengths)
    baseline = CallerFrameBench.summarize(planned, before)
    candidate = CallerFrameBench.summarize(planned, after)
    if kind == "physical"
      expected = {"short/quiet" => [[9, 9, 0, 0], [1, 18, 1, 0]], "long/quiet" => [[7, 7, 0, 0], [0, 10, 1, 0]]}
      expected.each do |group, values|
        observed = CallerFrameBench::CLASSES.map { |c| baseline.fetch(group).fetch(c).values_at("correct", "total", "unknown", "missing") }
        raise ArgumentError, "physical baseline differs from the original source receipt" unless observed == values
      end
    end
    Dir.mkdir(dest, 0o700)
    out = File.realpath(dest)
    if scorer == "-" || inventory == "-" || manifest == "-"
      unless kind == "physical" && [scorer, inventory, manifest].all? { |x| x == "-" }
        raise ArgumentError, "local-only mode requires physical evidence and three dashes"
      end
      official = {performed: false, reason: "Original private physical inventory and official manifest or auditable source-speaker map were not supplied."}
    else
      official = run_official(corpus, File.realpath(inventory), manifest, File.realpath(scorer), kind, planned, sums, after, freeze, out)
    end
    result = {kind: kind, evidence: kind == "provider" ? "independent provider test voices, digital microphone model" : "synthetic speech played through a physical lamp microphone",
              cases: planned.size, unique_wavs: inputs.fetch("unique_input_waveforms"), unique_enrollments: inputs.fetch("distinct_reference_waveforms"),
              unique_probes: inputs.fetch("distinct_probe_waveforms"), pairs_sha256: pair_hash, original_source_seal_sha256: inputs.fetch("original_source_seal_sha256"),
              audio_inventory_sha256: audio_sha, original_readme_sha256: sums["README.md"], old_predictions_sha256: old_sha,
              source: freeze.reject { |k, _| k == "source_patch" }, source_plan_sha256: StrongReplay.sha(File.join(plan, "SHA256SUMS")), source_run_sha256: StrongReplay.sha(File.join(run, "SHA256SUMS")),
              producer_config_sha256: run_files.fetch("config"), native_prediction_sha256: run_files.fetch("native/predictions.tsv"),
              frame_trace_sha256: run_files.fetch("native/frames.tsv"), scoring_program_sha256: StrongReplay.sha(__FILE__),
              before: baseline, after: candidate, host: native, reviewer: official, physical_caller_qualification_pairs: 0, lamp_ready_us: nil}
    if by_provider.any?
      result[:by_probe_provider] = by_provider.transform_values { |group| {before: CallerFrameBench.summarize(group, before), after: CallerFrameBench.summarize(group, after)} }
    else
      result[:physical_source_gaps] = {missing_planned_followups: 28, missing_constructed_impostors: 25, uncertain_own_reference_followups: 12,
                                      verified_same: 16, constructed_impostors: 28, recorded_background_pairs: 0}
    end
    StrongReplay.json(File.join(out, "summary.json"), result)
    FileUtils.cp(__FILE__, File.join(out, "strong-score.rb"))
    StrongReplay.write(File.join(out, "README.md"), <<~TEXT)
      Private matched replay of the pre-registered 28 November speaker model. The baseline and candidate use the identical original pair list. Each reference is the caller's WAV. Each case, including reused probes, wrong outputs and abstentions, stays in its class denominator. Short means one to three words. All original and derived evidence is retained locally.
      The reader rechecks every source hash and all frame positions, classes, raw signed cosines, median rules and timing statuses. Only true uncached enrollment plus probe processing and decision supply host case timings. Cold model initialization and cache-only comparisons are separate. No on-lamp end-to-end or clock readiness was measured by this replay.
      The official raw-pair report records development-only counts when its original provenance is available. Per-case predictions, scores, frames, run/report JSON and source data stay on their collecting machine. summary.json contains aggregate counts, provenance and host timing distributions.
    TEXT
    StrongReplay.seal(out)
    result
  end
end

if $PROGRAM_NAME == __FILE__
  File.umask(0o077)
  abort "usage: strong-score.rb PLAN CORPUS RUN OLD_FRAME_TSV SCORER INVENTORY EXISTING_MANIFEST|@PHYSICAL_SPEAKER_MAP NEW_OUT" unless ARGV.size == 8
  begin
    StrongScore.score(*ARGV)
  rescue ArgumentError, SystemCallError, KeyError, JSON::ParserError => e
    warn "strong speaker score: #{e.message}"
    exit 2
  end
end
