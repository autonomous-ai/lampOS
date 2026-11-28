# frozen_string_literal: true

require "minitest/autorun"
require "tmpdir"
require_relative "strong-score"

class StrongCallerReplayTest < Minitest::Test
  def test_every_defined_frame_and_the_strict_float32_cutoff
    assert_equal [nil, "unknown"], StrongScore.frame_vote([0.9] * 7)
    assert_equal [0.25, "different"], StrongScore.frame_vote([0.1, 0.2, 0.23, 0.25, 0.25, 0.3, 0.4, 0.5].map { |v| StrongScore.float32(v) })
    assert_equal [0.5, "same"], StrongScore.frame_vote([0.0] * 4 + [1.0] * 4)
    assert_equal [-0.5, "different"], StrongScore.frame_vote([-1.0] * 4 + [0.0] * 4)
  end

  def test_collection_seal_rejects_extra_missing_and_damaged_inputs
    Dir.mktmpdir do |dir|
      StrongReplay.write(File.join(dir, "source"), "original")
      StrongReplay.seal(dir)
      assert_equal ["source"], StrongReplay.verify_seal(dir).keys
      StrongReplay.write(File.join(dir, "unexpected"), "other")
      assert_raises(ArgumentError) { StrongReplay.verify_seal(dir) }
      File.rename(File.join(dir, "unexpected"), File.join(dir, "source"))
      assert_raises(ArgumentError) { StrongReplay.verify_seal(dir) }
      File.rename(File.join(dir, "source"), File.join(dir, "gone"))
      assert_raises(SystemCallError) { StrongReplay.verify_seal(dir) }
    end
  end

  def test_collection_hashes_must_be_distinct_and_original
    Dir.mktmpdir do |dir|
      sum = "a" * 64
      file = File.join(dir, "audio-SHA256SUMS")
      File.write(file, "#{sum}  ./file.wav\n")
      assert_equal({"file.wav" => sum}, StrongReplay.source_members(file))
      File.open(file, "a") { |f| f.puts "#{sum}  file.wav" }
      assert_raises(ArgumentError) { StrongReplay.source_members(file) }
      assert_raises(ArgumentError) { StrongReplay.verify_corpus("physical", file) }
    end
  end

  def test_physical_folder_and_inner_audio_are_bound_separately
    prefix = "09-corpus-primary"
    wavs = {"one.wav" => "1" * 64, "two.wav" => "2" * 64}
    pairs = {"case" => CallerFrameBench::Pair.new("case", "one.wav", "two.wav", "same", 2, "quiet")}
    outer = wavs.to_h { |name, hash| [File.join(prefix, name), hash] }
    audit = StrongReplay::PHYSICAL_AUDIO
    pair_hash = StrongReplay::PHYSICAL_PAIRS
    outer[File.join(prefix, "audio-SHA256SUMS")] = audit
    outer[File.join(prefix, "pairs.tsv")] = pair_hash
    assert_equal [StrongReplay::PHYSICAL_FOLDER], StrongReplay::SOURCE_SEALS.fetch("physical")
    refute_includes StrongReplay::SOURCE_SEALS.fetch("physical"), audit
    StrongReplay.physical_members(outer, prefix, audit, pair_hash, wavs, pairs)
    {File.join(prefix, "audio-SHA256SUMS") => "3" * 64, File.join(prefix, "pairs.tsv") => "4" * 64,
     File.join(prefix, "one.wav") => "0" * 64}.each do |path, changed|
      assert_raises(ArgumentError) { StrongReplay.physical_members(outer.merge(path => changed), prefix, audit, pair_hash, wavs, pairs) }
    end
    assert_raises(KeyError) { StrongReplay.physical_members(outer, "other-folder", audit, pair_hash, wavs, pairs) }
  end

  def sample(dir)
    probe = "p"
    hashes = {"a" => ["w1", probe], "b" => ["w2", probe], "c" => ["mute", probe]}
    pairs = hashes.to_h { |id, (r, p)| [id, CallerFrameBench::Pair.new(id, r, p, id == "a" ? "same" : "different", 2, "quiet")] }
    pred = {"a" => CallerFrameBench::Prediction.new("same", 0.5, 14, "first"),
            "b" => CallerFrameBench::Prediction.new("different", 0.25, nil, "unmeasured"),
            "c" => CallerFrameBench::Prediction.new("unknown", nil, 1, "warm")}
    File.write(File.join(dir, "load.tsv"), "host_model_load_us\n58\n")
    File.write(File.join(dir, "frames.tsv"), "case_id\tstart_sample\tend_sample_exclusive\tcosine\n" +
      %w[a b].flat_map { |id| 9.times.map { |i| [id, i * 160, i * 160 + 400, i == 2 ? "" : pred[id].score].join("\t") + "\n" } }.join)
    File.write(File.join(dir, "status.tsv"), <<~TSV)
      case_id	enrollment_available	probe_available	active_frames	total_frames	cached_compare_us	fresh_verified
      a	true	true	8	9	1	true
      b	true	true	8	9	0	false
      c	false	false	0	0	0	true
    TSV
    File.write(File.join(dir, "fresh-times.tsv"), <<~TSV)
      case_id	phase	enroll_us	probe_us	decide_us	total_us
      a	first	5	7	1	14
      c	warm	1		0	1
    TSV
    File.write(File.join(dir, "feature-times.tsv"), <<~TSV)
      role	wav_sha256	execution	available	host_us
      enrollment	w1	fresh	true	5
      probe	p	fresh	true	7
      enrollment	mute	fresh	false	1
      probe	p	skipped_no_enrollment	false\t
      enrollment	w2	cache_fill	true	6
    TSV
    [pairs, pred, %w[a c], hashes, {probe => 9}]
  end

  def test_reused_probe_depends_on_reference_and_uncached_timings
    Dir.mktmpdir do |dir|
      args = sample(dir)
      result = StrongScore.check_native(dir, *args)
      assert_equal [2, 1, 1, 16], result.values_at(:uncached_pairs, :cached_only_pairs, :unavailable_enrollments, :defined_frame_scores)
      assert_equal({"fresh" => 3, "skipped_no_enrollment" => 1, "cache_fill" => 1}, result.fetch(:feature_work))
      summary = CallerFrameBench.summarize(args[0], args[1])
      assert_equal [1, 2, 1], summary["all"]["different"].values_at("correct", "total", "unknown")
      assert_nil summary["short/noisy"]["different"]["percent"]
      assert_nil args[1]["b"].elapsed
      args[1]["b"].output = "same"
      assert_raises(ArgumentError) { StrongScore.check_native(dir, *args) }
    end
  end

  def test_rejects_partial_frames_phantom_timings_and_invented_feature_work
    Dir.mktmpdir do |dir|
      args = sample(dir)
      args.last["p"] = 8
      assert_raises(ArgumentError) { StrongScore.check_native(dir, *args) }
      args.last["p"] = 9
      args[1]["b"].elapsed = 0
      assert_raises(ArgumentError) { StrongScore.check_native(dir, *args) }
      args[1]["b"].elapsed = nil
      File.open(File.join(dir, "feature-times.tsv"), "a") { |f| f.puts "enrollment\tw1\tcache_fill\ttrue\t2" }
      assert_raises(ArgumentError) { StrongScore.check_native(dir, *args) }
    end
  end

  def test_physical_manifest_keeps_one_probe_clock_for_two_callers_and_checks_source_identity
    Dir.mktmpdir do |dir|
      root = File.realpath(dir)
      waves = %w[registered.wav other.wav recorded.wav]
      waves.each_with_index { |name, i| CallerHeldout.wav(File.join(root, name), Array.new(1920) { |n| (n * (i + 1)) % 8192 }) }
      StrongReplay.write(File.join(root, "source.tsv"), "Invented fixture speaker mapping for a unit test only.\n")
      sums = (waves + ["source.tsv"]).to_h { |name| [name, StrongReplay.sha(File.join(root, name))] }
      inventory = {"files" => sums.map { |n, s| {"path" => n, "sha256" => s} }, "created_unix_s" => 1, "readme_sha256" => "a" * 64}
      pairs = {"a" => CallerFrameBench::Pair.new("a", waves.first, waves.last, "same", 2, "quiet"),
               "b" => CallerFrameBench::Pair.new("b", waves[1], waves.last, "different", 2, "quiet")}
      map = File.join(root, "map.tsv")
      StrongReplay.write(map, "wav_sha256\tspeaker_id\tsource_path\tsource_sha256\n" + waves.map.with_index { |n, i| [sums[n], i == 1 ? "second" : "first", "source.tsv", sums.fetch("source.tsv")].join("\t") + "\n" }.join)
      doc = JSON.parse(JSON.generate(StrongScore.physical_manifest(map, sums, pairs, root, inventory, "b" * 64)))
      endpoints = StrongScore.official_row_ids(doc, pairs, sums, root, inventory, "b" * 64)
      assert_equal endpoints["a"], endpoints["b"]
      assert_equal 120_000, endpoints["a"].last
      assert doc["sessions"].all? { |s| s["clock_verified"] == false && s["started_unix_s"].nil? && s["consent"].nil? }
      doc["trials"].detect { |t| t["id"].end_with?(sums.fetch(waves[1])) }["speaker_id"] = "first"
      assert_raises(ArgumentError) { StrongScore.official_row_ids(doc, pairs, sums, root, inventory, "b" * 64) }
    end
  end
end
