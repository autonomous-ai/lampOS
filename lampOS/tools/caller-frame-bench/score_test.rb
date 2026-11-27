# frozen_string_literal: true

require "fileutils"
require "minitest/autorun"
require "tmpdir"
require_relative "reviewer_replay"
require_relative "reviewer_raw_pairs"

class CallerFrameScoreTest < Minitest::Test
  def setup
    @root = File.realpath(Dir.mktmpdir("caller-frame-contract-"))
    @pairs = File.join(@root, "pairs.tsv")
    @whole = File.join(@root, "whole.tsv")
    @frame = File.join(@root, "frame.tsv")
    @out = File.join(@root, "out")
    # Byte sentinels check provenance and joins. This test performs no inference.
    {"s1/call.wav" => "enrollment", "s1/other.wav" => "impostor", "s2/short.wav" => "short", "s2/long.wav" => "long"}.each do |path, bytes|
      FileUtils.mkdir_p(File.dirname(File.join(@root, path)))
      File.binwrite(File.join(@root, path), bytes)
    end
    write_pairs([
      ["a", "s1/call.wav", "s2/short.wav", "same", 3, "quiet"],
      ["b", "s1/other.wav", "s2/short.wav", "different", 1, "quiet"],
      ["c", "s1/call.wav", "s2/long.wav", "same", 4, "noisy"],
      ["d", "s1/other.wav", "s2/long.wav", "different", 8, "noisy"]
    ])
    write_run(@whole, [["a", "same", "0.73", 50, "first"], ["b", "same", "0.75", 10, "warm"],
                       ["c", "unknown", "", "", "unmeasured"]])
    write_run(@frame, [["a", "unknown", "0.44", 50, "first"], ["b", "different", "0.21", 10, "warm"],
                       ["c", "same", "0.98", 30, "warm"], ["d", "different", "0.02", 20, "warm"]])
  end

  def teardown
    FileUtils.remove_entry(@root)
  end

  def write_pairs(rows)
    File.write(@pairs, ([CallerFrameBench::PAIR_COLUMNS] + rows).map { |r| r.join("\t") + "\n" }.join)
    names = %w[pairs.tsv s1/call.wav s1/other.wav s2/short.wav s2/long.wav]
    File.write(File.join(@root, "audio-SHA256SUMS"), names.map { |n| "#{CallerFrameBench.sha(File.join(@root, n))}  #{n}\n" }.join)
  end

  def write_run(path, rows)
    data = ([CallerFrameBench::PREDICTION_COLUMNS] + rows).map { |r| r.join("\t") + "\n" }.join
    File.write(path, "# pairs_sha256=#{CallerFrameBench.sha(@pairs)}\n" + data)
  end

  def report
    CallerFrameBench.compare("synthetic", @pairs, @whole, @frame, @out)
  end

  def test_missing_unknown_wrong_and_cell_denominators
    r = report
    assert_equal [1, 2, 1, 0, 0], r["whole"]["all"]["same"].values_at("correct", "total", "unknown", "missing", "wrong")
    assert_equal [0, 2, 0, 1, 1], r["whole"]["all"]["different"].values_at("correct", "total", "unknown", "missing", "wrong")
    assert_equal 100.0, r["frame"]["all"]["different"]["percent"]
    assert_equal 0.0, r["frame"]["short/quiet"]["same"]["percent"]
    assert_nil r["whole"]["long/quiet"]["same"]["percent"]
    assert_equal [3, 20, 30], r["frame"]["all"]["host_inference"]["warm"].values_at("n", "p50_us", "p95_us")
    assert_equal 1, r["whole"]["all"]["host_inference"]["first"]["n"]
    assert_nil r["lamp_ready_us"]
    assert_equal CallerFrameBench.sha(@frame), r["inputs"]["frame_predictions_sha256"]
    assert_raises(Errno::EEXIST) { report }
  end

  def test_invalid_predictions_rejected_before_creating_output
    invalid = [
      [["unplanned", "same", "0.5", "1", "warm"]],
      [["a", "same", "0.5", "1", "warm"], ["a", "different", "0.4", "1", "warm"]],
      [["a", "unknown", "1e10000", "1", "warm"]],
      [["a", "different", "", "1", "first"]],
      [["a", "maybe", "0.3", "1", "warm"]],
      [["a", "same", "0.5", "", "warm"]],
      [["a", "same", "0.5", "-1", "first"]],
      [["a", "same", "0.5", "1", "unmeasured"]]
    ]
    invalid.each do |rows|
      write_run(@frame, rows)
      assert_raises(ArgumentError) { report }
      refute File.exist?(@out)
    end
    File.write(@frame, File.read(@frame).sub(CallerFrameBench.sha(@pairs), "0" * 64))
    assert_match(/different pairs TSV/, assert_raises(ArgumentError) { report }.message)
  end

  def test_raw_integrity_and_path_containment
    File.binwrite(File.join(@root, "s2", "long.wav"), "changed")
    assert_match(/checksum mismatch/, assert_raises(ArgumentError) { report }.message)
    File.symlink(File.expand_path(__FILE__), File.join(@root, "outside"))
    assert_raises(ArgumentError) { CallerFrameBench.private_file(@root, "outside") }
    assert_raises(ArgumentError) { CallerFrameBench.private_file(@root, "../other") }
  end

  def test_duplicate_truth_and_unhashed_file_rejected
    row = ["a", "s1/call.wav", "s2/long.wav", "same", 2, "quiet"]
    write_pairs([row, row])
    assert_match(/duplicate case/, assert_raises(ArgumentError) { report }.message)
    write_pairs([row, row.dup.tap { |r| r[0] = "b" }])
    assert_match(/repeated audio pair/, assert_raises(ArgumentError) { report }.message)
    write_pairs([row.dup.tap { |r| r[2] = "undeclared.wav" }])
    assert_match(/checksummed clip/, assert_raises(ArgumentError) { report }.message)
  end

  def test_primary_scorer_rejects_excluded_test_voices_at_either_end
    planned = {"t" => CallerFrameBench::Pair.new("t", "quiet/w.wav", "quiet/f.wav", "different", 2, "quiet")}
    sums = {"clips.tsv" => "", "pair-provenance.tsv" => ""}
    prov = File.join(@root, "pair-provenance.tsv")
    clips = File.join(@root, "clips.tsv")
    File.write(prov, "case_id\tfollowup_id\tenrollment_id\tfollowup_provider\tenrollment_provider\tfollowup_voice\tenrollment_voice\tlabel\tcondition\nt\tf\tw\tGemini Live\tElevenLabs\tv\tv\tdifferent\tquiet\n")
    header = "id\tprovider\tvoice\tsplit\n"
    File.write(clips, header + "f\tgemini_live\tv\ttest\nw\televenlabs\tv\ttest\n")
    assert_equal ["gemini"], CallerFrameBench.primary_groups(@pairs, planned, sums).keys
    ["f\topenai\tv\ttest\nw\televenlabs\tv\ttest\n",
     "f\tgemini\tv\ttest\nw\topenai\tv\ttest\n",
     "f\tgemini-imitation\tv\ttest\nw\televenlabs\tv\ttest\n",
     "f\tgemini\tv\ttrain\nw\televenlabs\tv\ttest\n"].each do |bad|
      File.write(clips, header + bad)
      assert_raises(ArgumentError) { CallerFrameBench.primary_groups(@pairs, planned, sums) }
    end
  end

  def test_reviewers_replay_requires_a_real_clock_binding
    write_pairs([["voice-v", "s1/call.wav", "s2/short.wav", "same", 3, "quiet"]])
    sums, = CallerFrameBench.verify_archive(@pairs)
    planned = CallerFrameBench.pairs(@pairs, sums)
    candidate = {"voice-v" => CallerFrameBench::Prediction.new("same", 0.7, 29, "warm")}
    cap = {"sessions" => [{"id" => "s1", "split" => "holdout"}, {"id" => "s2", "split" => "holdout", "clock_id" => "b", "sync_error_us" => 10}],
           "trials" => [{"id" => "c", "session_id" => "s1"}, {"id" => "v", "session_id" => "s2", "start_us" => 1000}],
           "cases" => [{"id" => "voice-v", "sensor" => "voice_match", "fault" => false, "trial_id" => "v", "reference_trial_id" => "c", "expected" => "same"}]}
    side = [{"case_id" => "voice-v", "trial_id" => "v", "reference_trial_id" => "c", "worker_epoch" => "w", "request_id" => "r",
             "clock_id" => "b", "input" => {"audio_relpath" => "s2/short.wav", "reference_audio_relpath" => "s1/call.wav"}}]
    audit = [{"case_id" => "voice-v", "input_end_us" => 10_000, "held_line" => false, "safety_violation" => false, "added_wait_us" => 0}]
    join = -> { CallerFrameReviewer.join(planned, candidate, cap, side, audit, @root) }
    p = join.call.fetch(0)
    assert_equal [10_000, 29, nil], p.values_at("input_end_us", "replay_elapsed_us", "ready_us")
    audit[0]["input_end_us"] = nil
    assert_empty join.call
    audit[0]["input_end_us"] = 900
    assert_match(/predates/, assert_raises(ArgumentError) { join.call }.message)
    audit[0]["input_end_us"] = 10_000
    audit[0]["request_id"] = "previous-request"
    assert_match(/audit request_id mismatch/, assert_raises(ArgumentError) { join.call }.message)
    audit[0].delete("request_id")
    side[0]["clock_id"] = "wrong"
    assert_match(/clock mismatch/, assert_raises(ArgumentError) { join.call }.message)
    side[0]["clock_id"] = "b"
    side[0]["input"]["audio_relpath"] = "s2/long.wav"
    assert_match(/bound capture/, assert_raises(ArgumentError) { join.call }.message)
    side[0]["input"]["audio_relpath"] = "s2/short.wav"
    cap["sessions"].each { |s| s["split"] = "development" }
    assert_equal 1, join.call.size
    cap["sessions"].last["sync_error_us"] = nil
    assert_equal [10_000, 29, nil], join.call.first.values_at("input_end_us", "replay_elapsed_us", "ready_us")
    cap["cases"] << cap["cases"].first.merge("id" => "extra")
    assert_match(/every nonfault/, assert_raises(ArgumentError) { join.call }.message)
  end

  def test_v2_bytes_bind_exact_or_patched_producer_and_candidate
    artifact = {"producer_binary" => @whole, "model" => @pairs, "config" => @frame}
    h = artifact.to_h { |k, f| [k + "_sha256", CallerFrameBench.sha(f)] }.merge("build_sha" => "1" * 40)
    digest = CallerFrameBench.sha(@whole)
    b = artifact.merge("build_sha" => h["build_sha"], "prediction_sha256" => digest, "exact_commit" => true)
    assert_equal digest, CallerFrameReviewer.verify_artifacts(h, b, digest)["producer_binary_sha256"]
    assert_raises(ArgumentError) { CallerFrameReviewer.verify_artifacts(h, b, "0" * 64) }
    assert_raises(ArgumentError) { CallerFrameReviewer.verify_artifacts(h.merge("model_sha256" => digest), b, digest) }
    assert_raises(ArgumentError) { CallerFrameReviewer.verify_artifacts(h.merge("source_patch_sha256" => digest), b, digest) }
    refute_equal CallerFrameBench.sha(@pairs), digest
    b["exact_commit"] = false
    assert_raises(KeyError) { CallerFrameReviewer.verify_artifacts(h, b, digest) }
    b["source_patch"] = @pairs
    h["source_patch_sha256"] = CallerFrameBench.sha(@pairs)
    assert_equal h["source_patch_sha256"], CallerFrameReviewer.verify_artifacts(h, b, digest)["source_patch_sha256"]
  end

  def test_v2_capture_retention_and_recording_must_match_verified_inventory
    # The verification subprocess is stubbed here. The integration command uses the official executable.
    records = {"s1" => {"root" => @root, "inventory" => File.join(@root, "inventory.json")}}
    File.write(File.join(@root, "README.md"), "fixture")
    raw = CallerFrameBench.sha(@pairs)
    inventory = records["s1"]["inventory"]
    File.write(inventory, JSON.generate({files: [{sha256: raw}]}))
    s = {"id" => "s1", "consent" => true, "retention" => "keep_all", "inventory_sha256" => CallerFrameBench.sha(inventory),
         "readme_sha256" => CallerFrameBench.sha(File.join(@root, "README.md")), "recording_sha256" => raw}
    verify = lambda do |session, program|
      CallerFrameReviewer.verify_sessions([session], records, program, Dir.mktmpdir("result-", @root))
    end
    assert verify.call(s, "/usr/bin/true")
    external = s.merge("split" => "development", "data_origin" => "public", "consent" => nil)
    assert verify.call(external, "/usr/bin/true")
    assert_raises(ArgumentError) { verify.call(external.merge("split" => "holdout"), "/usr/bin/true") }
    assert_raises(ArgumentError) { verify.call(external.merge("data_origin" => "real_people"), "/usr/bin/true") }
    ["consent", "retention", "inventory_sha256", "readme_sha256", "recording_sha256"].each do |field|
      replacement = field == "consent" ? false : "wrong"
      assert_raises(ArgumentError) { verify.call(s.merge(field => replacement), "/usr/bin/true") }
    end
    assert_raises(ArgumentError) { verify.call(s.merge("delete_after_unix_s" => 123), "/usr/bin/true") }
    assert_raises(ArgumentError) { verify.call(s, "/usr/bin/false") }
  end
end

class CallerRawPairContractTest < Minitest::Test
  def setup
    @root = File.realpath(Dir.mktmpdir("caller-digital-contract-"))
    Dir.mkdir(File.join(@root, "quiet"))
    @sums = {}
    @meta = "id\tvoice_id\nref-a\ta\nref-b\tb\nprobe\ta\n"
    File.write(File.join(@root, "clips.tsv"), @meta)
    @sums["clips.tsv"] = CallerFrameBench.sha(File.join(@root, "clips.tsv"))
    %w[ref-a ref-b probe].each_with_index do |name, i|
      rel = "quiet/#{name}.wav"
      CallerHeldout.wav(File.join(@root, rel), Array.new(17 + i, 100 + i))
      @sums[rel] = CallerFrameBench.sha(File.join(@root, rel))
    end
    @pairs = {"sa" => CallerFrameBench::Pair.new("sa", "quiet/ref-a.wav", "quiet/probe.wav", "same", 2, "quiet"),
              "di" => CallerFrameBench::Pair.new("di", "quiet/ref-b.wav", "quiet/probe.wav", "different", 2, "quiet")}
  end

  def teardown
    FileUtils.remove_entry(@root)
  end

  def manifest
    CallerRawPairs.manifest("synthetic", @root, @pairs, @sums,
                            {"created_unix_s" => 1234, "readme_sha256" => "1" * 64}, "2" * 64)
  end

  def test_correlated_pairs_use_real_file_offsets_and_preserve_abstentions
    doc, ends = manifest
    assert_equal "raw_development_pairs", doc[:evaluation_mode]
    assert_equal 3, doc[:trials].size
    assert_equal 1, doc[:cases].map { |c| c[:trial_id] }.uniq.size
    assert_equal 2, doc[:cases].map { |c| c[:reference_trial_id] }.uniq.size
    assert_equal @sums.values_at("quiet/ref-a.wav", "quiet/ref-b.wav", "quiet/probe.wav").sort, doc[:sessions].map { |s| s[:recording_sha256] }.sort
    assert doc[:sessions].all? { |s| s[:started_unix_s].nil? && s[:consent].nil? && s[:sync_error_us].nil? && s[:clock_verified] == false && s[:split] == "development" }
    assert doc[:trials].all? { |t| t[:recipient] == "unknown" && t[:call_end_us].nil? && t[:start_us] == 0 }
    rows = CallerRawPairs.predictions(@pairs, ends, {"sa" => CallerFrameBench::Prediction.new("unknown", nil, 27, "first")})
    assert_equal 1, rows.size
    row = rows.first
    assert_equal ["sa", "unknown", 1187, 27, nil], row.values_at(:case_id, :output, :input_end_us, :replay_elapsed_us, :ready_us)
    assert_equal ends.fetch("quiet/probe.wav")[:session_id], row[:clock_id]
  end

  def test_aliases_self_comparisons_and_conflicting_speaker_labels
    ref = "quiet/ref-a.wav"
    alias_name = "quiet/alias.wav"
    FileUtils.cp(File.join(@root, ref), File.join(@root, alias_name))
    @sums[alias_name] = @sums.fetch(ref)
    File.write(File.join(@root, "clips.tsv"), @meta + "alias\ta\n")
    @pairs["alias"] = @pairs.fetch("sa").dup
    @pairs["alias"].id = "alias"
    @pairs["alias"].reference = alias_name
    assert_match(/duplicated/, assert_raises(ArgumentError) { manifest }.message)
    File.write(File.join(@root, "clips.tsv"), @meta + "alias\tc\n")
    assert_match(/identical WAV/, assert_raises(ArgumentError) { manifest }.message)
    @pairs.delete("alias")
    @pairs.fetch("sa").reference = @pairs.fetch("sa").followup
    assert_match(/overlapping/, assert_raises(ArgumentError) { manifest }.message)
    @pairs.fetch("sa").reference = ref
    @pairs.fetch("sa").label = "different"
    assert_match(/mislabelled/, assert_raises(ArgumentError) { manifest }.message)
  end

  def test_provider_and_public_speaker_identities_are_namespaced
    id = {"provider" => "Gemini Live", "voice" => "shared"}
    assert_equal %w[gemini shared], CallerRawPairs.identity("synthetic-heldout", id)
    refute_equal CallerRawPairs.identity("synthetic-heldout", id), CallerRawPairs.identity("synthetic-heldout", id.merge("provider" => "ElevenLabs"))
    assert_equal ["reader"], CallerRawPairs.identity("public-speech", {"speaker_id" => "reader"})
    assert_raises(ArgumentError) { CallerRawPairs.identity("public-speech", {"speaker_id" => ""}) }
  end

  def test_original_run_seal_rejects_score_or_revision_substitution
    run = File.join(@root, "run")
    Dir.mkdir(run)
    revision = "a" * 40
    fixed = {"source-revision.txt" => revision + "\n", "source.patch" => "", "run/provenance.tsv" => "receipt",
             "run/whole.tsv" => "whole", "run/frame.tsv" => "frame"}
    fixed.each { |f, bytes| File.write(File.join(@root, f), bytes) }
    seal = File.join(@root, "SHA256SUMS")
    File.write(seal, fixed.keys.map { |f| "#{CallerFrameBench.sha(File.join(@root, f))}  #{f}\n" }.join)
    bindings = {"build_sha" => revision, "exact_commit" => true}
    checksum, outputs = CallerRawPairs.verify_run(run, bindings)
    assert_equal CallerFrameBench.sha(seal), checksum
    assert_equal CallerFrameBench.sha(File.join(run, "frame.tsv")), outputs["frame"]
    assert_match(/source revision/, assert_raises(ArgumentError) { CallerRawPairs.verify_run(run, bindings.merge("build_sha" => "b" * 40)) }.message)
    File.write(File.join(run, "whole.tsv"), "substitute")
    assert_match(/run seal/, assert_raises(ArgumentError) { CallerRawPairs.verify_run(run, bindings) }.message)
  end
end
