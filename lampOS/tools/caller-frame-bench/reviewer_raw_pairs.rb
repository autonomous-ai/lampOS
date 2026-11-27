# frozen_string_literal: true

require_relative "prepare-heldout"
require_relative "reviewer_replay"

# Adapt correlated digital pairs to the reviewer's raw development mode.
module CallerRawPairs
  REVISION = "bfb0a95a9da96740f77923b76fd8547fecbb7ffd"
  ORIGINS = {"synthetic" => "generated", "synthetic-heldout" => "generated", "public-speech" => "public"}.freeze
  PARTITIONS = {"synthetic" => "macOS synthetic development voices", "public-speech" => "LibriSpeech test-clean",
                "synthetic-heldout" => "#8907 ElevenLabs and Gemini provider test voices only"}.freeze
  MODELS = {"whole" => "cam++", "frame" => "personal-vad"}.freeze
  module_function

  def save(path, object)
    File.open(path, "wx", 0o600) { |f| f.puts JSON.pretty_generate(object) }
  end

  def identity(kind, row)
    values = case kind
             when "synthetic" then [row.fetch("voice_id")]
             when "public-speech" then [row.fetch("speaker_id")]
             when "synthetic-heldout" then [CallerFrameBench.provider_id(row.fetch("provider")), row.fetch("voice")]
             else raise ArgumentError, "expected an external digital corpus"
             end
    raise ArgumentError, "missing source speaker" unless values.all? { |v| v.is_a?(String) && !v.empty? }
    values
  end

  def manifest(kind, root, planned, sums, inventory, inventory_sha)
    raise ArgumentError, "clip metadata must be in the verified corpus" unless sums.key?("clips.tsv")
    clips = CallerFrameReviewer.index(CSV.read(File.join(root, "clips.tsv"), headers: true, col_sep: "\t").map(&:to_h), "id")
    origin = ORIGINS.fetch(kind)
    sessions = {}
    trials = {}
    endpoint = {}
    paths = planned.values.flat_map { |p| [p.reference, p.followup] }.uniq.sort
    paths.each do |path|
      condition, file = path.split("/", 2)
      unless %w[quiet noisy].include?(condition) && file&.match?(/\A[^\/]+\.wav\z/i)
        raise ArgumentError, "expected a conditioned WAV directly under quiet/ or noisy/"
      end
      clip = clips.fetch(File.basename(file, ".*"))
      hash = sums.fetch(path)
      speaker = "sp-" + Digest::SHA256.hexdigest(JSON.generate([kind, *identity(kind, clip)]))
      pcm = CallerHeldout.read_pcm(CallerFrameBench.private_file(root, path))
      # Each source is its entire model-input WAV. The exclusive end is in integer microseconds.
      end_us = (pcm.bytesize / 2) * 1_000_000 / CallerHeldout::RATE
      id = "w-" + hash
      noise = condition == "quiet" ? "quiet" : "background"
      if trials.key?(hash)
        unless trials.fetch(hash).values_at(:speaker_id, :end_us, :noise) == [speaker, end_us, noise]
          raise ArgumentError, "identical WAV has conflicting labels"
        end
      else
        sessions[hash] = {id: id, split: "development", data_origin: origin, consent: nil, retention: "keep_all",
                          started_unix_s: nil, recording_sha256: hash, inventory_sha256: inventory_sha,
                          readme_sha256: inventory.fetch("readme_sha256"), clock_id: id, clock_verified: false, sync_error_us: nil}
        trials[hash] = {id: id, session_id: id, speaker_id: speaker, source: "person", start_us: 0, end_us: end_us,
                        call_end_us: nil, recipient: "unknown", script_id: id, look_target: "unknown", head_target: "unknown",
                        bearing_deg: nil, distance_mm: nil, light: "unknown", noise: noise, visible_faces: [],
                        subject_visible: false, overlap: false, first_use: false, condition: "normal"}
      end
      endpoint[path] = trials.fetch(hash)
    end
    used = {}
    cases = planned.values.map do |p|
      ref = endpoint.fetch(p.reference)
      probe = endpoint.fetch(p.followup)
      key = [ref[:id], probe[:id]]
      if ref[:id] == probe[:id] || used.key?(key) ||
         (ref[:speaker_id] == probe[:speaker_id]) != (p.label == "same") ||
         [p.followup, p.reference].any? { |path| path.split("/").first != p.condition }
        raise ArgumentError, "overlapping, duplicated or mislabelled source pair #{p.id}"
      end
      used[key] = true
      {id: p.id, sensor: "voice_match", trial_id: probe[:id], reference_trial_id: ref[:id], expected: p.label, fault: false}
    end
    doc = {version: 2, rubric_id: CallerFrameReviewer::RUBRIC, evaluation_mode: "raw_development_pairs",
           collection_id: "paired-" + inventory_sha[0, 24], frozen_unix_s: inventory.fetch("created_unix_s"),
           sessions: sessions.values, trials: trials.values, cases: cases}
    [doc, endpoint]
  end

  def predictions(planned, endpoints, classified)
    classified.map do |id, p|
      probe = endpoints.fetch(planned.fetch(id).followup)
      {case_id: id, clock_id: probe.fetch(:session_id), output: p.output, input_end_us: probe.fetch(:end_us),
       ready_us: nil, replay_elapsed_us: p.elapsed, added_wait_us: 0, held_line: false, safety_violation: false}
    end
  end

  def receipt(run_dir)
    path = File.join(run_dir, "provenance.tsv")
    rows = CSV.read(path, headers: true, col_sep: "\t")
    raise ArgumentError, "unexpected producer receipt header" unless rows.headers == %w[input sha256]
    CallerFrameReviewer.index(rows.map(&:to_h), "input").transform_values { |r| r.fetch("sha256") }
  end

  def verify_run(run_dir, bindings)
    dir = File.realpath(run_dir)
    root = File.dirname(dir)
    prefix = File.basename(dir)
    seal = File.join(root, "SHA256SUMS")
    hashes = {}
    File.foreach(seal, chomp: true) do |line|
      row = /\A([0-9a-f]{64})  (.+)\z/.match(line)
      raise ArgumentError, "invalid or duplicate run seal entry" unless row && !hashes.key?(row[2])
      hashes[row[2]] = row[1]
    end
    names = %w[source-revision.txt source.patch] + %w[provenance.tsv whole.tsv frame.tsv].map { |f| "#{prefix}/#{f}" }
    names.each do |name|
      raise ArgumentError, "run seal differs for #{name}" unless hashes.fetch(name) == CallerFrameBench.sha(CallerFrameBench.private_file(root, name))
    end
    patch = File.join(root, "source.patch")
    unless File.read(File.join(root, "source-revision.txt")).strip == bindings.fetch("build_sha") &&
           (bindings.fetch("exact_commit") ? File.zero?(patch) : hashes.fetch("source.patch") == bindings.fetch("source_patch_sha256"))
      raise ArgumentError, "source revision or patch differs from original benchmark run"
    end
    [CallerFrameBench.sha(seal), MODELS.keys.to_h { |name| [name, hashes.fetch("#{prefix}/#{name}.tsv")] }]
  end

  def validate_report(report, planned, classified, digests)
    unless report.fetch("evaluation_mode") == "raw_development_pairs" && !report.fetch("qualified") &&
           report.fetch("raw_by_split").fetch("holdout").fetch("counts").fetch("total").zero? &&
           report.fetch("development_pairs").fetch("planned_pairs") == planned.size &&
           report.fetch("input_sha256") == digests.slice("manifest", "run") && report.fetch("scorer_binary_sha256") == digests.fetch("scorer")
      raise ArgumentError, "unexpected official raw-pair report or input bindings"
    end
    rows = CallerFrameReviewer.index(report.fetch("rows"), "case_id")
    raise ArgumentError, "official pair denominator changed" unless rows.keys.sort == planned.keys.sort
    planned.each do |id, pair|
      row = rows.fetch(id)
      p = classified[id]
      unless row.values_at("expected", "raw_output", "classification_correct", "replay_elapsed_us", "latency_us") ==
             [pair.label, p&.output, p&.output == pair.label, p&.elapsed, nil]
        raise ArgumentError, "official row disagrees with paired result #{id}"
      end
    end
    counts = CallerFrameBench.summarize(planned, classified)
    official = report.fetch("raw_by_split").fetch("development").fetch("by_class")
    CallerFrameBench::CLASSES.each do |c|
      next if counts.fetch("all").fetch(c).fetch("total") == 0 && !official.key?(c)
      unless official.fetch(c).values_at("correct", "total") == counts.fetch("all").fetch(c).values_at("correct", "total")
        raise ArgumentError, "official class count mismatch"
      end
    end
    counts
  end

  def compare(kind, pair_file, run_dir, inventory_path, bindings_path, scorer, dest)
    ORIGINS.fetch(kind)
    root = File.dirname(File.realpath(pair_file))
    inventory_path = File.realpath(inventory_path)
    bindings = CallerFrameReviewer.read_json(bindings_path)
    scorer = File.realpath(scorer)
    if bindings.fetch("scorer_revision") != REVISION || bindings.fetch("scorer_sha256") != CallerFrameBench.sha(scorer)
      raise ArgumentError, "official scorer revision or executable digest mismatch"
    end
    Dir.mkdir(dest, 0o700)
    dest = File.realpath(dest)
    unless CallerFrameReviewer.invoke(scorer, ["verify-archive", root, inventory_path], File.join(dest, "verify.json")) == 0
      raise ArgumentError, "official source inventory verification failed"
    end
    inventory = CallerFrameReviewer.read_json(inventory_path)
    inv_digest = CallerFrameBench.sha(inventory_path)
    sums, = CallerFrameBench.verify_archive(pair_file)
    planned = CallerFrameBench.pairs(pair_file, sums)
    groups = kind == "synthetic-heldout" ? CallerFrameBench.primary_groups(pair_file, planned, sums) : {}
    run_seal, pred_hashes = verify_run(run_dir, bindings)
    source = receipt(run_dir)
    pair_sha = sums.fetch(File.basename(pair_file))
    raise ArgumentError, "run receipt differs from input pairs or executable" unless source.fetch("pairs.tsv") == pair_sha && source.fetch("producer") == CallerFrameBench.sha(bindings.fetch("producer_binary"))
    doc, endpoints = manifest(kind, root, planned, sums, inventory, inv_digest)
    mp = File.join(dest, "manifest.json")
    save(mp, doc)
    unless CallerFrameReviewer.invoke(scorer, ["validate", mp], File.join(dest, "validate.json")) == 0
      raise ArgumentError, "official raw-pair manifest validation failed"
    end
    summary = {"evidence" => kind, "source_partition" => PARTITIONS.fetch(kind), "pairs_sha256" => pair_sha,
               "corpus_inventory_sha256" => inv_digest, "corpus_readme_sha256" => inventory.fetch("readme_sha256"),
               "original_corpus_readme_sha256" => sums.fetch("README.md"), "manifest_sha256" => CallerFrameBench.sha(mp),
               "scorer_sha256" => CallerFrameBench.sha(scorer), "scorer_revision" => REVISION,
               "build_sha" => bindings.fetch("build_sha"), "producer_binary_sha256" => source.fetch("producer"),
               "bindings_sha256" => CallerFrameBench.sha(bindings_path), "producer_receipt_sha256" => CallerFrameBench.sha(File.join(run_dir, "provenance.tsv")),
               "original_run_seal_sha256" => run_seal,
               "adapter_sha256" => CallerFrameBench.sha(__FILE__), "candidates" => {}}
    MODELS.each do |name, model_key|
      args = bindings.merge(bindings.fetch(name))
      pred_file = File.join(run_dir, name + ".tsv")
      args["prediction_sha256"] = pred_hashes.fetch(name)
      header = {"build_sha" => bindings.fetch("build_sha"), "producer_binary_sha256" => source.fetch("producer"),
                "model_sha256" => source.fetch(model_key), "config_sha256" => args.fetch("config_sha256")}
      header["source_patch_sha256"] = args.fetch("source_patch_sha256") unless args.fetch("exact_commit")
      artifact_hashes = CallerFrameReviewer.verify_artifacts(header, args, args.fetch("prediction_sha256"))
      classified = CallerFrameBench.predictions(pred_file, pair_sha, planned)
      rp = File.join(dest, name + "-run.json")
      save(rp, header.merge(version: 2, sensor: "voice_match", evidence: "replay", location: "local",
                            manifest_sha256: summary["manifest_sha256"], predictions: predictions(planned, endpoints, classified)))
      out = File.join(dest, name)
      status = CallerFrameReviewer.invoke(scorer, ["score", "voice_match", mp, rp, out], File.join(dest, name + "-score.json"))
      raise ArgumentError, "expected completed, unqualified #{name} replay, got exit #{status}" unless status == 1
      official_path = File.join(out, "report.json")
      report = CallerFrameReviewer.read_json(official_path)
      run_sha = CallerFrameBench.sha(rp)
      counts = validate_report(report, planned, classified, {"manifest" => summary["manifest_sha256"], "run" => run_sha, "scorer" => summary["scorer_sha256"]})
      result = artifact_hashes.merge("run_sha256" => run_sha, "report_sha256" => CallerFrameBench.sha(official_path),
                                    "prediction_sha256" => args.fetch("prediction_sha256"), "development_pairs" => report.fetch("development_pairs"),
                                    "raw_by_split" => report.fetch("raw_by_split"), "by_length_and_noise" => counts)
      result["by_followup_provider"] = groups.transform_values { |group| CallerFrameBench.summarize(group, classified) } unless groups.empty?
      summary["candidates"][name] = result
    end
    save(File.join(dest, "summary.json"), summary)
    readme = <<~TEXT
      Private standalone 16 kHz raw development replay using frozen #8885 #{REVISION}. Original partition: #{PARTITIONS.fetch(kind)}.
      All independent source clips, raw speaker labels, recipes and derivatives stay local; retain them and every old output.
      The source inventory seal supplies the freeze epoch of this retrospective replay. Original source acquisition epochs remain unknown.
      Each file time domain is its model-input WAV, from zero to its PCM duration (exclusive end, rounded down to whole microseconds).
      Same/impostor cases can share the probe. Quiet/noisy derivatives share original speech. Unique file hashes count processed inputs.
      Source "person" denotes the voiced source class with origin #{ORIGINS.fetch(kind)}. No human collection consent, physical geometry or clock mapping is inferred.
      Offline input_end_us is the file endpoint. The producer only computes classifications on files. Zero added wait and false hold/safety flags describe that isolated replay.
      Host timings include enrollment and classifier processing and exclude disk I/O, model load, ASR and device transport. Device readiness is null.
      The official reports contain development counts only. summary.json cross-checks every case and both class denominators and gives short/long and quiet/noisy results.
    TEXT
    File.open(File.join(dest, "README.md"), "wx", 0o600) { |f| f.write(readme) }
    summary
  end
end

if $PROGRAM_NAME == __FILE__
  abort "usage: reviewer_raw_pairs.rb synthetic|synthetic-heldout|public-speech PAIRS RUN_DIR CORPUS_INVENTORY BINDINGS SCORER NEW_DIR" unless ARGV.length == 7
  begin
    CallerRawPairs.compare(*ARGV)
  rescue ArgumentError, SystemCallError, JSON::ParserError, CSV::MalformedCSVError, KeyError => e
    warn "caller-frame raw-pair replay: #{e.message}"
    exit 2
  end
end
