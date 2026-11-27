# frozen_string_literal: true

require_relative "score"
require "open3"

# Join a replay to the lead's actual #8885 annotations and capture audit.
# Missing source timestamps stay missing. The shared scorer owns qualification.
module CallerFrameReviewer
  RUBRIC = "sensor-team-2026-11-27-v2"
  module_function

  def read_json(path)
    JSON.parse(File.read(path))
  end

  def index(records, key)
    records.each_with_object({}) do |record, by_id|
      id = record.fetch(key)
      raise ArgumentError, "duplicate #{key} #{id}" if by_id.key?(id)
      by_id[id] = record
    end
  end

  def clip_belongs_to?(root, relative, session_id)
    return false unless relative.start_with?(session_id + "/")
    expected_session = File.join(root, session_id)
    File.realpath(expected_session) == expected_session &&
      CallerFrameBench.private_file(root, relative).start_with?(expected_session + "/")
  end

  def join(planned, predicted, manifest, entries, audit, root)
    trials = index(manifest.fetch("trials"), "id")
    sessions = index(manifest.fetch("sessions"), "id")
    cases = index(manifest.fetch("cases"), "id")
    side = index(entries, "case_id")
    obs = index(audit, "case_id")
    raise ArgumentError, "unrecognized audit or sidecar case" unless ((obs.keys | side.keys) - cases.keys).empty?
    requests = entries.map { |e| [e.fetch("worker_epoch"), e.fetch("request_id")] }
    unless requests.uniq.length == requests.length && requests.flatten.all? { |id| id.is_a?(String) && !id.empty? }
      raise ArgumentError, "duplicate or empty sidecar worker/request ID"
    end
    split_by_trial = trials.transform_values { |t| sessions.fetch(t.fetch("session_id")).fetch("split") }
    splits = planned.keys.map { |id| split_by_trial.fetch(cases.fetch(id).fetch("trial_id")) }.uniq
    selected = cases.values.select do |c|
      c.fetch("sensor") == "voice_match" && !c.fetch("fault") && splits.include?(split_by_trial.fetch(c.fetch("trial_id")))
    end
    unless selected.map { |c| c.fetch("id") }.sort == planned.keys.sort
      raise ArgumentError, "pairs must cover every nonfault voice-match case in their splits"
    end
    rows = []
    selected.each do |c|
      id = c.fetch("id")
      pair = planned.fetch(id)
      raise ArgumentError, "label mismatch for #{id}" unless pair.label == c.fetch("expected")
      e = side[id]
      next unless e
      trial = trials.fetch(c.fetch("trial_id"))
      ref = trials.fetch(c.fetch("reference_trial_id"))
      session = sessions.fetch(trial.fetch("session_id"))
      ref_session = sessions.fetch(ref.fetch("session_id"))
      unless e.fetch("trial_id") == trial.fetch("id") && e.fetch("reference_trial_id") == ref.fetch("id") && e.fetch("clock_id") == session.fetch("clock_id")
        raise ArgumentError, "sidecar trial, reference or clock mismatch for #{id}"
      end
      input = e.fetch("input")
      rel = input["audio_relpath"]
      ref_rel = input["reference_audio_relpath"]
      next unless rel && ref_rel
      unless pair.followup == rel && pair.reference == ref_rel &&
             clip_belongs_to?(root, rel, session.fetch("id")) && clip_belongs_to?(root, ref_rel, ref_session.fetch("id"))
        raise ArgumentError, "replay audio differs from the bound capture for #{id}"
      end
      a = obs[id]
      p = predicted[id]
      next unless a && p && a["input_end_us"]
      %w[clock_id worker_epoch request_id].each do |key|
        raise ArgumentError, "audit #{key} mismatch for #{id}" if a.key?(key) && a[key] != e.fetch(key)
      end
      submitted = a.fetch("input_end_us")
      error = session.fetch("sync_error_us")
      unless submitted.is_a?(Integer) && submitted >= 0 && (error.nil? || submitted >= trial.fetch("start_us") - error)
        raise ArgumentError, "submission predates trial #{id}"
      end
      rows << {"case_id" => id, "clock_id" => session.fetch("clock_id"), "output" => p.output,
               "input_end_us" => submitted, "ready_us" => nil, "replay_elapsed_us" => p.elapsed,
               "added_wait_us" => a.fetch("added_wait_us"), "held_line" => a.fetch("held_line"),
               "safety_violation" => a.fetch("safety_violation")}
    end
    rows
  end

  # Persist failed attempts too. A non-qualifying but well-formed replay exits 1.
  def invoke(scorer, args, path)
    stdout, stderr, status = Open3.capture3(File.realpath(scorer), *args)
    File.open(path, "wx", 0o600) do |f|
      f.puts JSON.pretty_generate({argv: args, exit: status.exitstatus, stdout: stdout, stderr: stderr})
    end
    status.exitstatus
  end

  def verify_sessions(sessions, archives, scorer, out)
    checked = {}
    sessions.each_with_index do |s, i|
      external = s["split"] == "development" && %w[generated public invented].include?(s["data_origin"])
      consent = s.fetch("consent")
      unless (consent == true || (external && consent.nil?)) && s.fetch("retention") == "keep_all" && s["delete_after_unix_s"].nil?
        raise ArgumentError, "session origin, consent or keep_all mismatch for #{s.fetch('id')}"
      end
      record = archives.fetch(s.fetch("id"))
      raw = File.realpath(record.fetch("root"))
      inventory = File.realpath(record.fetch("inventory"))
      key = [raw, inventory]
      checked[key] ||= begin
        status = invoke(scorer, ["verify-archive", raw, inventory], File.join(out, "verify-#{i}.json"))
        raise ArgumentError, "official inventory check failed for #{s.fetch('id')}" unless status == 0
        doc = read_json(inventory)
        {inventory: CallerFrameBench.sha(inventory), readme: CallerFrameBench.sha(File.join(File.dirname(inventory), "README.md")),
         members: doc.fetch("files").to_h { |row| [row.fetch("sha256"), true] }}
      end
      result = checked.fetch(key)
      unless result[:inventory] == s.fetch("inventory_sha256") && result[:readme] == s.fetch("readme_sha256") &&
             result[:members].key?(s.fetch("recording_sha256"))
        raise ArgumentError, "session inventory, README or raw recording hash mismatch for #{s.fetch('id')}"
      end
    end
  end

  def verify_artifacts(header, bindings, prediction_digest)
    unless header.fetch("build_sha").match?(/\A[0-9a-f]{40}\z/) && header.fetch("build_sha") == bindings.fetch("build_sha") &&
           prediction_digest == bindings.fetch("prediction_sha256") && [true, false].include?(bindings.fetch("exact_commit"))
      raise ArgumentError, "producer revision, source state or prediction digest differs from the bindings"
    end
    fields = %w[producer_binary model config]
    exact = bindings.fetch("exact_commit")
    if exact
      raise ArgumentError, "exact source must not claim a patch" if header["source_patch_sha256"] || bindings["source_patch"]
    else
      fields << "source_patch"
    end
    fields.to_h do |field|
      hash = CallerFrameBench.sha(bindings.fetch(field))
      raise ArgumentError, "wrong #{field} digest" unless header.fetch(field + "_sha256") == hash
      [field + "_sha256", hash]
    end
  end

  def replay(pairs, predictions, manifest_path, sidecar_path, header_path, audit_path, bindings_path, scorer, out)
    manifest = read_json(manifest_path)
    sidecar = read_json(sidecar_path)
    header = read_json(header_path)
    audit = read_json(audit_path)
    bindings = read_json(bindings_path)
    manifest_hash = CallerFrameBench.sha(manifest_path)
    unless [manifest, header].all? { |doc| doc.fetch("version") == 2 } && manifest.fetch("rubric_id") == RUBRIC &&
           [1, 2].include?(sidecar.fetch("version")) &&
           [sidecar, header].all? { |doc| doc.fetch("manifest_sha256") == manifest_hash } &&
           header.fetch("predictions") == [] && header.fetch("evidence") == "replay" &&
           header.fetch("sensor") == "voice_match"
      raise ArgumentError, "expected a manifest-bound sidecar and empty voice-match replay header"
    end

    artifacts = verify_artifacts(header, bindings, CallerFrameBench.sha(predictions))
    root = File.dirname(File.realpath(pairs))
    checksums, = CallerFrameBench.verify_archive(pairs)
    planned = CallerFrameBench.pairs(pairs, checksums)
    classified = CallerFrameBench.predictions(predictions, checksums.fetch(File.basename(pairs)), planned)
    header["predictions"] = join(planned, classified, manifest, sidecar.fetch("entries"), audit, root)
    Dir.mkdir(out, 0o700)
    hashes = [pairs, predictions, manifest_path, sidecar_path, header_path, audit_path, bindings_path, scorer, __FILE__, File.join(__dir__, "score.rb")].map { |path| CallerFrameBench.sha(path) }
    source_hashes = %w[pairs predictions manifest sidecar run_header capture_audit bindings shared_scorer adapter paired_scorer].zip(hashes).to_h.merge(artifacts)
    File.write(File.join(out, "source-hashes.json"), JSON.pretty_generate(source_hashes) + "\n")
    status = invoke(scorer, ["validate", File.realpath(manifest_path)], File.join(out, "validate.json"))
    raise ArgumentError, "shared manifest validation failed (exit #{status})" unless status == 0
    verify_sessions(manifest.fetch("sessions"), bindings.fetch("sessions"), scorer, out)
    run_file = File.join(out, "replay.json")
    File.write(run_file, JSON.pretty_generate(header) + "\n")
    args = ["score", "voice_match", File.realpath(manifest_path), File.realpath(run_file), File.join(File.realpath(out), "shared")]
    status = invoke(scorer, args, File.join(out, "score.json"))
    raise ArgumentError, "shared scorer rejected replay (exit #{status})" unless status == 1
    report = read_json(File.join(out, "shared", "report.json"))
    unless report.fetch("evidence") == "replay" && report.fetch("qualified") == false && report.fetch("raw_by_split").key?("development")
      raise ArgumentError, "expected an unqualified v2 replay"
    end
    digests = %w[replay.json shared/report.json shared/trials.tsv]
    File.write(File.join(out, "result-hashes.json"), JSON.pretty_generate(digests.to_h { |f| [f, CallerFrameBench.sha(File.join(out, f))] }) + "\n")
    report
  end
end

if $PROGRAM_NAME == __FILE__
  abort "usage: ruby reviewer_replay.rb PAIRS TSV V2_MANIFEST SIDECAR EMPTY_V2_REPLAY_HEADER AUDIT BINDINGS SHARED_SCORER_BINARY NEW_OUT_DIR" unless ARGV.size == 9
  begin
    CallerFrameReviewer.replay(*ARGV)
  rescue ArgumentError, SystemCallError, JSON::ParserError, KeyError => e
    warn "caller-frame reviewer replay: #{e.message}"
    exit 2
  end
end
