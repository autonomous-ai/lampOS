# frozen_string_literal: true

require "minitest/autorun"
require "tmpdir"
require_relative "prepare-heldout"

class CallerHeldoutPreparationTest < Minitest::Test
  def setup
    @dir = Dir.mktmpdir("caller-heldout-test-")
  end

  def teardown
    FileUtils.remove_entry(@dir)
  end

  def test_only_independent_bare_calls_and_ordinary_test_voices
    voices = [%w[provider voice split], %w[ElevenLabs a test], ["Gemini Live", "a", "train"], %w[OpenAI a test]]
    manifest = [%w[file provider voice style text label split],
                ["bare", "elevenlabs", "a", "whispered", "Hey, Lamp!", "call", "test"],
                ["ask", "elevenlabs", "a", "normal", "Hey Lamp, what time is it?", "call", "test"],
                ["like", "elevenlabs", "a", "fast", "Hey Mom!", "not-call", "test"],
                ["ok", "elevenlabs", "a", "normal", "Okay, Lamp, stop", "not-call", "test"],
                ["mid", "elevenlabs", "a", "slow", "Turn off the lamp.", "not-call", "test"],
                ["excluded-wake", "openai", "a", "normal", "Hey Lamp.", "call", "test"],
                ["excluded-line", "openai", "a", "normal", "Tomorrow?", "not-call", "test"],
                ["train", "gemini_live", "a", "fast", "Again.", "not-call", "train"]]
    v = File.join(@dir, "voices.tsv")
    m = File.join(@dir, "manifest.tsv")
    File.write(v, voices.map { |r| r.join("\t") + "\n" }.join)
    File.write(m, manifest.map { |r| r.join("\t") + "\n" }.join)
    manifest.drop(1).each { |r| File.write(File.join(@dir, r.first), "unit-test sentinel") }
    declared = CallerHeldout.voice_list(v)
    assert_equal 3, declared.size
    inventory = Hash.new(0)
    rows = CallerHeldout.read_clips(m, declared, @dir, inventory)
    assert_equal [["bare", "wake", 2], ["mid", "followup", 4]], rows.map { |c| [c.file, c.label, c.words] }
    assert_equal "whispered", rows.first.effective_style
    assert_equal 2, inventory.fetch(["openai", "a", "test", "excluded_provider"])
    assert File.file?(File.join(@dir, "excluded-wake"))
    manifest.last[-1] = "test"
    File.write(m, manifest.map { |r| r.join("\t") + "\n" }.join)
    assert_match(/split disagree/, assert_raises(ArgumentError) { CallerHeldout.read_clips(m, declared, @dir) }.message)
  end

  def clip(provider, name, idx, role)
    text = if role == "wake"
             "Hey Lamp"
           elsif idx.even?
             "Again #{idx}"
           else
             "Please do that again #{idx}"
           end
    CallerHeldout::Clip.new("#{provider}/#{name}/#{role}#{idx}.wav", provider, name, "normal", "normal", text, text.split.size, role)
  end

  def test_provider_and_negative_strata_are_balanced_and_repeatable
    voices = [["elevenlabs", "a"], ["elevenlabs", "b"], ["elevenlabs", "c"], ["gemini", "a"], ["gemini", "b"]].to_h { |k| [k, "test"] }
    wake = voices.keys.to_h { |p, v| [[p, v], [clip(p, v, 0, "wake")]] }
    follow = voices.keys.to_h { |p, v| [[p, v], (0...12).map { |i| clip(p, v, i, "followup") }] }
    first = CallerHeldout.select_followups(voices, wake, follow, 2)
    second = CallerHeldout.select_followups(voices.to_a.reverse.to_h, wake, follow.transform_values(&:reverse), 2)
    assert_equal first, second
    assert_equal({["elevenlabs", true] => 5, ["elevenlabs", false] => 5, ["gemini", true] => 5, ["gemini", false] => 5},
                 first.group_by { |c| [c.provider, c.words <= 3] }.transform_values(&:size))
    assert_equal 5, first.map(&:speaker).uniq.size
    wake[["openai", "x"]] = [clip("openai", "x", 0, "wake")]
    matches = CallerHeldout.comparisons(first, wake)
    matches.each do |f, target, imp, _|
      assert_equal f.speaker, target.speaker
      refute_equal f.speaker, imp.speaker
      refute_equal "openai", imp.provider
      assert_equal target.effective_style, imp.effective_style
    end
    cells = matches.group_by { |f, _, _, _| [f.provider, f.words <= 3] }
    cells.each_value do |records|
      within = records.count { |row| row.last == :within }
      assert_operator (records.length - 2 * within).abs, :<=, 1
    end
  end

  def test_only_repository_source_layout_or_a_complete_flat_snapshot_can_be_imported
    nested = File.expand_path("../../training", __dir__)
    assert_equal %w[channel.py microphone.py mix.py], CallerHeldout.acoustic_files(nested).map(&:first)
    assert_raises(ArgumentError) { CallerHeldout.acoustic_files(@dir) }
    CallerHeldout.acoustic_files(nested).each { |name, src| FileUtils.copy_file(src, File.join(@dir, name)) }
    assert CallerHeldout.acoustic_files(@dir).all? { |_, src| File.dirname(src) == @dir }
  end

  def test_path_safety_and_riff_duration
    ["../escape", "a/../../b", "/private/escape", "a\\b"].each do |bad|
      assert_raises(ArgumentError) { CallerHeldout.relative(bad) }
    end
    assert_equal "audio/a.wav", CallerHeldout.relative("./audio/a.wav")
    pcm = Array.new(320, 0) + Array.new(160, 1200) + Array.new(160, 0)
    path = File.join(@dir, "test.wav")
    CallerHeldout.wav(path, pcm)
    assert_equal pcm, CallerHeldout.read_pcm(path).unpack("s<*")
    assert_equal [2], CallerHeldout.active(pcm)
    File.truncate(path, 80)
    assert_raises(ArgumentError) { CallerHeldout.read_pcm(path) }
  end

  def test_bad_digest_fails_before_output_and_valid_original_is_retained_on_error
    source = File.join(@dir, "bad-layout.tar")
    File.open(source, "wb") do |io|
      Gem::Package::TarWriter.new(io) do |tar|
        tar.add_file_simple("nothing.txt", 0o600, 1) { |w| w.write("x") }
      end
    end
    out = File.join(@dir, "run")
    assert_raises(ArgumentError) { CallerHeldout.prepare(source, "0" * 64, out) }
    refute File.exist?(out)
    assert_raises(ArgumentError) { CallerHeldout.prepare(source, CallerHeldout.sha(source), out) }
    assert File.file?(File.join(out, "provenance", "original.sha256"))
    assert File.file?(File.join(out, "INCOMPLETE.md"))
    assert File.file?(File.join(out, "partial-SHA256SUMS"))
    refute File.exist?(File.join(out, "audio-SHA256SUMS"))
    assert_raises(Errno::EEXIST) { CallerHeldout.prepare(source, CallerHeldout.sha(source), out) }
  end
end
