#!/usr/bin/env ruby
# frozen_string_literal: true

require "csv"
require "digest"
require "fileutils"
require "json"
require "open3"
require "optparse"
require "rubygems/package"
require "set"
require "zlib"
require_relative "score"

# Prepare paired test-voice replays from the immutable #8907 renderer tar.
module CallerHeldout
  RATE = 16_000
  SEED = 8849
  SNR_DB = 5.0
  CLIP_BLOCK = RATE / 100
  PRE_ROLL = RATE * 3 / 10
  Clip = Struct.new(:file, :provider, :voice, :style, :effective_style, :text, :words, :label) do
    def speaker
      [provider, voice]
    end

    def id
      "c-" + Digest::SHA256.hexdigest(file)[0, 20]
    end
  end
  module_function

  def sha(path)
    Digest::SHA256.file(path).hexdigest
  end

  def rank(*fields)
    Digest::SHA256.digest(([SEED] + fields).join("\0")).unpack1("Q>")
  end

  def write(path, content)
    File.open(path, "wbx", 0o600) { |f| f.write(content) }
  end

  def write_tsv(path, header, rows)
    File.open(path, "wx:utf-8", 0o600) do |f|
      f.puts(header.join("\t"))
      rows.each { |row| f.puts(row.map { |v| v.to_s.gsub(/[\t\r\n]/, " ") }.join("\t")) }
    end
  end

  def relative(path)
    value = path.sub(%r{\A(?:\./)+}, "").sub(%r{/\z}, "")
    parts = value.split("/", -1)
    unless !value.empty? && parts.none? { |p| p.empty? || p == "." || p == ".." } && !value.match?(/[\\\0\r\n\t]/)
      raise ArgumentError, "unsafe archive member path #{path.inspect}"
    end
    value
  end

  def retain_tar(tar, destination)
    File.link(tar, destination)
  rescue Errno::EXDEV, Errno::EPERM
    FileUtils.copy_file(tar, destination)
  end

  def unpack(tar, root)
    all = []
    File.open(tar, "rb") do |file|
      magic = file.read(2)
      file.rewind
      io = magic == "\x1f\x8b".b ? Zlib::GzipReader.new(file) : file
      Gem::Package::TarReader.new(io).each do |entry|
        next if entry.full_name.match?(%r{\A(?:\./?)?\z}) && entry.directory?
        # Extended time/owner records carry no data used by the benchmark.
        next if %w[x g].include?(entry.header.typeflag)
        name = relative(entry.full_name)
        dst = File.join(root, name)
        if entry.directory?
          FileUtils.mkdir_p(dst, mode: 0o700)
        elsif entry.file?
          FileUtils.mkdir_p(File.dirname(dst), mode: 0o700)
          File.open(dst, "wbx", 0o600) { |out| IO.copy_stream(entry, out) }
          all << name
        else
          raise ArgumentError, "unsupported archive entry type for #{name}"
        end
      end
    end
    all
  end

  def rows(path, required)
    CSV.open(path, "r:bom|utf-8", headers: true, col_sep: "\t", liberal_parsing: true) do |csv|
      first = csv.shift
      raise ArgumentError, "empty TSV #{path}" unless first
      headers = first.headers
      missing = required - headers
      raise ArgumentError, "missing #{missing.join(', ')} in #{File.basename(path)}" unless missing.empty?
      raise ArgumentError, "duplicate TSV columns in #{path}" unless headers.uniq == headers
      yield first
      csv.each { |r| yield r }
    end
  end

  def tokens(text)
    text.downcase.scan(/[[:alnum:]]+(?:['’][[:alnum:]]+)*/u)
  end

  def ordinary?(words)
    return false if words.empty? || %w[hey hi hello lamp alexa siri].include?(words.first)
    return false if %w[um uh well].include?(words.first) && %w[hey hi hello].include?(words[1])
    return false if %w[ok okay].include?(words.first) && %w[lamp lamps lampshade google siri alexa].include?(words[1])
    true
  end

  def voice_list(path)
    result = {}
    rows(path, %w[provider split]) do |r|
      name = (r["voice"] || r["voice_id"] || "").strip
      key = [CallerFrameBench.provider_id(r.fetch("provider")), name]
      split = r.fetch("split").strip.downcase
      raise ArgumentError, "missing voice or invalid split in voices.tsv" if key.any?(&:empty?) || !%w[train test].include?(split)
      raise ArgumentError, "duplicate provider/voice in voices.tsv: #{key.inspect}" if result.key?(key)
      result[key] = split
    end
    result
  end

  def read_clips(path, voices, source_root, inventory = Hash.new(0))
    actual = File.realpath(source_root)
    clips = []
    seen = Set.new
    rows(path, %w[file provider voice style text label]) do |r|
      provider = CallerFrameBench.provider_id(r.fetch("provider"))
      voice = r.fetch("voice").strip
      split = voices.fetch([provider, voice]) { raise ArgumentError, "voice missing from voices.tsv: #{[provider, voice].inspect}" }
      if r["split"] && r["split"].strip.downcase != split
        raise ArgumentError, "clip/voice split disagree for #{r['file']}"
      end
      file = relative(r.fetch("file"))
      raise ArgumentError, "repeated file in manifest: #{file}" unless seen.add?(file)
      allowed = CallerFrameBench::PRIMARY_PROVIDERS.include?(provider)
      reason = allowed ? (split == "test" ? "allowed_test_provider" : "training_voice") : "excluded_provider"
      inventory[[provider, voice, split, reason]] += 1
      next unless allowed
      next unless split == "test"
      abs = File.realpath(File.join(actual, file))
      raise ArgumentError, "unsafe clip path #{file}" unless abs.start_with?(actual + "/") && File.file?(abs)
      text = r.fetch("text").strip
      words = tokens(text)
      style = r.fetch("style").strip.downcase
      effective = style
      effective = "normal" if provider == "elevenlabs" && style.match?(/whisper/)
      label = r.fetch("label").strip.downcase.tr("_", "-")
      if label == "call" && words == %w[hey lamp]
        role = "wake"
      elsif %w[not-call non-call].include?(label) && ordinary?(words)
        role = "followup"
      else
        next
      end
      clips << Clip.new(file, provider, voice, style, effective, text, words.length, role)
    end
    clips
  end

  def read_pcm(path)
    bytes = File.binread(path)
    unless bytes.byteslice(0, 4) == "RIFF" && bytes.byteslice(8, 4) == "WAVE"
      raise ArgumentError, "not a RIFF/WAVE input: #{path}"
    end
    pos = 12
    fmt = nil
    pcm = nil
    while pos + 8 <= bytes.bytesize
      type, size = bytes.byteslice(pos, 8).unpack("a4V")
      pos += 8
      raise ArgumentError, "truncated WAV chunk: #{path}" if pos + size > bytes.bytesize
      fmt = bytes.byteslice(pos, size).unpack("vvVVvv") if type == "fmt "
      pcm = bytes.byteslice(pos, size) if type == "data"
      pos += size + (size % 2)
    end
    unless fmt && fmt.values_at(0, 1, 2, 4, 5) == [1, 1, RATE, 2, 16] && pcm && !pcm.empty? && pcm.length.even?
      raise ArgumentError, "source must be 16 kHz mono PCM16: #{path}"
    end
    pcm
  end

  def wav(path, samples)
    pcm = samples.pack("s<*")
    header = ["RIFF", pcm.size + 36, "WAVE", "fmt ", 16, 1, 1, RATE, RATE * 2, 2, 16, "data", pcm.size].pack("a4Va4a4VvvVVvva4V")
    write(path, header + pcm)
  end

  def active(samples)
    powers = samples.each_slice(CLIP_BLOCK).map { |s| s.sum { |x| x * x }.fdiv(s.size) }
    threshold = [powers.max * 0.0001, 20**2].max
    indices = powers.each_index.select { |i| powers[i] >= threshold }
    raise ArgumentError, "source has no active speech energy" if indices.empty?
    indices
  end

  # Prefer a new text, then underrepresented short word counts and styles.
  def ordered_followups(clips, limit)
    used_words = Hash.new(0)
    used_styles = Hash.new(0)
    selected = []
    by_text = clips.group_by { |c| tokens(c.text).join(" ") }
    until by_text.empty? || selected.size == limit
      choices = by_text.values.map do |takes|
        takes.min_by { |c| [used_styles[c.effective_style], rank("take", c.file)] }
      end
      clip = choices.min_by { |c| [c.words <= 3 ? used_words[c.words] : 0, used_styles[c.effective_style], rank("text", c.file)] }
      by_text.delete(tokens(clip.text).join(" "))
      used_words[clip.words] += 1
      used_styles[clip.effective_style] += 1
      selected << clip
    end
    selected
  end

  def select_followups(voices, wake, follow, per_length)
    providers = voices.keys.select do |v|
      voices[v] == "test" && CallerFrameBench::PRIMARY_PROVIDERS.include?(v.first) && !wake.fetch(v, []).empty?
    end.group_by(&:first)
    raise ArgumentError, "fewer than two test voices have an independent bare Hey Lamp" if providers.values.flatten(1).size < 2
    wanted = (providers.values.sum(&:size) * per_length).fdiv(providers.size).ceil
    picks = []
    [:short, :long].each do |length|
      providers.sort.each do |_, pool|
        lists = pool.sort_by { |v| rank("speaker", length, *v) }.map do |speaker|
          eligible = follow.fetch(speaker, []).select { |c| (c.words <= 3) == (length == :short) }
          ordered_followups(eligible, wanted)
        end
        count = 0
        until count >= wanted
          progress = false
          lists.each do |list|
            next if list.empty?
            picks << list.shift
            count += 1
            progress = true
            break if count == wanted
          end
          break unless progress
        end
      end
    end
    picks
  end

  def comparisons(followups, wake)
    usage = Hash.new(0)
    relations = Hash.new(0)
    followups.map do |follow|
      speaker = follow.speaker
      stratum = [follow.provider, follow.words <= 3]
      desired = relations[[stratum, :within]] <= relations[[stratum, :across]] ? :within : :across
      alternatives = wake.keys.select { |s| s != speaker && CallerFrameBench::PRIMARY_PROVIDERS.include?(s.first) && !wake.fetch(s).empty? }
      candidate_voices = alternatives.select { |s| (s.first == speaker.first) == (desired == :within) }
      candidate_voices = alternatives if candidate_voices.empty?
      choices = candidate_voices.flat_map do |other|
        wake.fetch(speaker).product(wake.fetch(other))
      end
      same, impostor = choices.min_by do |a, b|
        [a.effective_style == b.effective_style ? 0 : 1, usage[["impostor", b.speaker]], usage[["same", a.file]] + usage[["different", b.file]],
         rank("enrollment", follow.file, a.file, b.file)]
      end
      raise ArgumentError, "no eligible impostor for #{follow.file}" unless same && impostor
      relation = impostor.provider == follow.provider ? :within : :across
      relations[[stratum, relation]] += 1
      usage[["impostor", impostor.speaker]] += 1
      usage[["same", same.file]] += 1
      usage[["different", impostor.file]] += 1
      [follow, same, impostor, relation]
    end
  end
end

module CallerHeldout
  module_function

  def run(*args)
    output, status = Open3.capture2e(*args)
    raise ArgumentError, "channel preparation failed: #{output}" unless status.success?
    output
  end

  def acoustic_files(dir)
    nested = %w[name/channel.py name/microphone.py stop/mix.py]
    names = %w[channel.py microphone.py mix.py]
    paths = nested.map { |file| File.join(dir, file) }
    paths = names.map { |file| File.join(dir, file) } unless paths.all? { |file| File.file?(file) }
    raise ArgumentError, "expected channel.py, microphone.py and mix.py in #{dir}" unless paths.all? { |f| File.file?(f) }
    names.zip(paths)
  end

  # Import the exact archived module regardless of the caller's installed modules or working directory.
  def import_acoustic(util, name, path)
    spec = util.spec_from_file_location("caller_frame_8849_#{name}", path)
    mod = util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    raise ArgumentError, "loaded unexpected #{name} module" unless File.realpath(PyCall.getattr(mod, :__file__)) == File.realpath(path)
    mod
  end

  def prepare_channel(out, python, calibration, clips, seeds, acoustic_dir)
    prov = File.join(out, "provenance")
    acoustic_files(acoustic_dir).each { |name, src| FileUtils.copy_file(src, File.join(prov, name)) }
    ENV["PYTHON"] = python
    Gem.use_paths(Gem.dir, [File.expand_path(ENV.fetch("CALLER_FRAME_GEMS")), *Gem.path]) if ENV["CALLER_FRAME_GEMS"]
    begin
      gem "pycall", "= 1.5.5"
      require "pycall"
    rescue LoadError
      raise ArgumentError, "install PyCall 1.5.5 for this Ruby or set CALLER_FRAME_GEMS to the isolated gem directory (see scoring.md)"
    end
    sys = PyCall.import_module("sys")
    PyCall.setattr(sys, :dont_write_bytecode, true)
    util = PyCall.import_module("importlib.util")
    channel = import_acoustic(util, "channel", File.join(prov, "channel.py"))
    microphone = import_acoustic(util, "microphone", File.join(prov, "microphone.py"))
    mix = import_acoustic(util, "mix", File.join(prov, "mix.py"))
    np = PyCall.import_module("numpy")
    npz = File.join(prov, "channel.npz")
    if calibration
      FileUtils.copy_file(calibration, npz)
      channel_kind = "supplied impulse response and noise-floor spectrum"
    else
      impulse = np.zeros(RATE / 2)
      impulse[0] = 1.0
      ir = mix.room(impulse, np.random.default_rng(SEED))
      floor = channel.pink(1.0, seed: SEED + 1)
      np.savez(npz, ir: ir, floor_power: np.square(np.abs(np.fft.rfft(floor))))
      channel_kind = "simulated lamp training room and pink floor"
    end
    plan = clips.map { |c| [c.id, seeds.fetch(c.id)] }
    write(File.join(prov, "channel-seeds.json"), JSON.pretty_generate(plan) + "\n")
    mic_paths = plan.map do |id, seed|
      name = id + ".wav"
      signal = channel.read_wav(File.join(out, "capture-input", name))
      rng = np.random.default_rng(seed)
      signal = microphone.loudspeaker(signal, rng) unless calibration
      mic_path = File.join(out, "microphone", name)
      channel.write_wav(mic_path, microphone.clip_onsets(signal, rng))
      n = PyCall.len(signal)
      pink = channel.pink((n + 0.5).fdiv(RATE), seed: seed + 1)
      raise ArgumentError, "noise length differs for #{name}" unless PyCall.len(pink) == n
      channel.write_wav(File.join(out, "pink-input", name), pink)
      mic_path
    end
    output = run({"PYTHONDONTWRITEBYTECODE" => "1"}, python, File.join(prov, "channel.py"), "apply", "--channel", npz, "--floor-db", "-60", *mic_paths, "--out", File.join(out, "channel-floor"))
    py_version = PyCall.import_module("platform").python_version
    scipy_version = PyCall.getattr(PyCall.import_module("scipy"), :__version__)
    numpy_version = PyCall.getattr(np, :__version__)
    write(File.join(prov, "channel-run.txt"), "Python #{py_version}, NumPy #{numpy_version}, SciPy #{scipy_version}\nRuby #{RUBY_DESCRIPTION}, PyCall #{PyCall::VERSION}\n#{channel_kind}\n#{output}")
    channel_kind
  end

  def make_conditions(out, clip)
    name = clip.id + ".wav"
    quiet = read_pcm(File.join(out, "channel-floor", name)).unpack("s<*")
    pink = read_pcm(File.join(out, "pink-input", name)).unpack("s<*")
    raise ArgumentError, "channel/stem length mismatch" unless quiet.size == pink.size
    ranges = active(read_pcm(File.join(out, "microphone", name)).unpack("s<*")).map do |i|
      (i * CLIP_BLOCK)...[(i + 1) * CLIP_BLOCK, quiet.size].min
    end
    power = ->(x) { ranges.sum { |span| span.sum { |i| x[i]**2 } } }
    pink_gain = Math.sqrt(power.call(quiet).fdiv(power.call(pink) * 10**(SNR_DB / 10)))
    peak = quiet.each_index.map do |i|
      n = pink[i] * pink_gain
      [quiet[i].abs, n.abs, (quiet[i] + n).abs].max
    end.max
    common = [1.0, 32_000.0 / peak].min
    clean = quiet.map { |s| (s * common).round }
    additive = pink.map { |s| (s * pink_gain * common).round }
    noisy = clean.zip(additive).map { |a, b| a + b }
    raise ArgumentError, "PCM clipping #{clip.id}" unless noisy.all? { |s| s.between?(-32768, 32767) }
    snr = 10 * Math.log10(power.call(clean).fdiv(power.call(additive)))
    raise ArgumentError, "unexpected achieved SNR #{snr} for #{clip.id}" if (snr - SNR_DB).abs > 0.05
    wav(File.join(out, "quiet", name), clean)
    wav(File.join(out, "noise", name), additive)
    wav(File.join(out, "noisy", name), noisy)
    [format("%.5f", snr), format("%.8f", common)]
  end

  def inventory(out, name)
    paths = Dir.glob(File.join(out, "**", "*"), File::FNM_DOTMATCH).select { |f| File.file?(f) }.sort
    File.open(File.join(out, name), "wx", 0o600) do |f|
      paths.each { |s| f.puts("#{sha(s)}  #{s.delete_prefix(out + '/')}") }
    end
    paths.size
  end

  def prepare(tar, expected, destination, calibration: nil, per_length: 12, python: "python3", acoustic_dir: nil)
    raise ArgumentError, "expected the published 64-hex tar SHA-256" unless expected.match?(/\A[0-9a-f]{64}\z/)
    raise ArgumentError, "positive follow-up quota required" unless per_length.positive?
    raise ArgumentError, "original tar digest mismatch" unless sha(tar) == expected
    File.umask(0o077)
    out = File.expand_path(destination)
    Dir.mkdir(out, 0o700)
    created = true
    %w[source provenance capture-input microphone pink-input channel-floor quiet noise noisy].each { |d| Dir.mkdir(File.join(out, d), 0o700) }
    source = File.join(out, "source")
    original = File.join(out, "provenance", File.basename(tar))
    retain_tar(tar, original)
    write(File.join(out, "provenance", "original.sha256"), "#{expected}  #{File.basename(tar)}\n")
    write(File.join(out, "provenance", "prepare-heldout.rb"), File.binread(__FILE__))
    write(File.join(out, "provenance", "score.rb"), File.binread(File.join(__dir__, "score.rb")))
    names = unpack(original, source)
    find_one = lambda do |basename|
      matches = names.select { |f| File.basename(f).downcase == basename }
      raise ArgumentError, "expected one #{basename} in tar, found #{matches.size}" unless matches.size == 1
      File.join(source, matches.first)
    end
    manifest = find_one.call("manifest.tsv")
    voices = voice_list(find_one.call("voices.tsv"))
    folder = File.dirname(manifest)
    source_counts = Hash.new(0)
    clips = read_clips(manifest, voices, folder, source_counts)
    write_tsv(File.join(out, "source-scope.tsv"), %w[provider voice split policy original_clip_count],
              source_counts.sort.map { |identity, n| [*identity, n] })
    wake = clips.select { |c| c.label == "wake" }.group_by(&:speaker)
    follow = clips.select { |c| c.label == "followup" }.group_by(&:speaker)
    pcm_ids = wake.values.flatten.group_by { |c| Digest::SHA256.hexdigest(read_pcm(File.join(folder, c.file))) }
    excluded = pcm_ids.values.select { |samples| samples.map(&:speaker).uniq.size > 1 }.flatten
    wake.transform_values! { |pool| pool - excluded }
    write_tsv(File.join(out, "provenance", "duplicate-wakes.tsv"), %w[provider voice file reason], excluded.map { |c| [c.provider, c.voice, c.file, "identical decoded wake attributed to multiple speakers"] })
    selected = select_followups(voices, wake, follow, per_length)
    raise ArgumentError, "no eligible ordinary test follow-ups" if selected.empty?
    pairs = comparisons(selected, wake)
    selected_clips = pairs.map { |f, a, b, _| [f, a, b] }.flatten.uniq.sort_by(&:file)
    raise ArgumentError, "opaque clip ID collision" unless selected_clips.map(&:id).uniq.size == selected_clips.size
    raw_pcms = {}
    seeds = {}
    audio_rows = {}
    selected_clips.each do |c|
      bytes = read_pcm(File.join(folder, c.file))
      digest = Digest::SHA256.hexdigest(bytes)
      if raw_pcms[digest] && raw_pcms[digest] != c.speaker
        raise ArgumentError, "one selected PCM is labelled as two different test voices: #{c.file}"
      end
      raw_pcms[digest] = c.speaker
      samples = bytes.unpack("s<*")
      blocks = active(samples)
      first = blocks.first * CLIP_BLOCK
      last = [(blocks.last + 1) * CLIP_BLOCK, samples.size].min
      wav(File.join(out, "capture-input", c.id + ".wav"), Array.new(PRE_ROLL, 0) + samples[first...last])
      seeds[c.id] = rank("channel", c.file) % (2**32)
      audio_rows[c.id] = [c.id, c.provider, c.voice, "test", c.label, c.style, c.effective_style, c.text, c.words, c.file,
                          sha(File.join(folder, c.file)), digest, samples.size, first, last, seeds[c.id]]
    end
    acoustic_dir ||= File.file?(File.join(__dir__, "channel.py")) ? __dir__ : File.expand_path("../../training", __dir__)
    kind = prepare_channel(out, python, calibration, selected_clips, seeds, acoustic_dir)
    selected_clips.each { |c| audio_rows.fetch(c.id).concat(make_conditions(out, c)) }
    write_tsv(File.join(out, "clips.tsv"),
              %w[id provider voice split purpose style effective_style text words source_file source_sha256 decoded_pcm_sha256 samples first_active last_active_exclusive mic_seed noisy_snr_db common_gain], audio_rows.values)
    truth = []
    matches = []
    pairs.each do |f, a, b, relation|
      %w[quiet noisy].each do |condition|
        [["same", a], ["different", b]].each do |label, reference|
          id = format("pvad-8907-%06d", truth.length + 1)
          truth << [id, "#{condition}/#{reference.id}.wav", "#{condition}/#{f.id}.wav", label, f.words, condition]
          matches << [id, f.provider, f.voice, f.id, reference.provider, reference.voice, reference.id, label, f.style, reference.style,
                      a.effective_style == b.effective_style, relation, condition]
        end
      end
    end
    write_tsv(File.join(out, "pairs.tsv"), %w[case_id reference followup label words condition], truth)
    write_tsv(File.join(out, "pair-provenance.tsv"), %w[case_id followup_provider followup_voice followup_id enrollment_provider enrollment_voice enrollment_id label followup_style enrollment_style target_impostor_enrollment_styles_match impostor_provider_relation condition], matches)
    coverage = voices.select { |v, s| s == "test" && CallerFrameBench::PRIMARY_PROVIDERS.include?(v.first) }.keys.sort.map do |v|
      x = follow.fetch(v, [])
      used = selected.select { |c| c.speaker == v }
      [*v, wake.fetch(v, []).size, x.count { |c| c.words <= 3 }, x.count { |c| c.words > 3 }, used.count { |c| c.words <= 3 }, used.count { |c| c.words > 3 }]
    end
    write_tsv(File.join(out, "test-voice-coverage.tsv"), %w[provider voice eligible_bare_wakes eligible_short_clips eligible_long_clips picked_short picked_long], coverage)
    report = <<~DOC
      # Multi-provider held-out caller-voice replay

      The source tar `#{File.basename(tar)}` has SHA-256 `#{expected}`. The original archive, every regular member, the two renderer manifests and all benchmark waveforms remain here. `audio-SHA256SUMS` pins every retained file. Check it locally with `shasum -a 256 -c audio-SHA256SUMS`. [Renderer specification and provider limitations](https://github.com/autonomous-ai/lampOS/issues/8907)

      The owner limited the primary benchmark to **ElevenLabs and Gemini test voices**. The accepted provider spellings, ignoring case and outer whitespace, are `elevenlabs`, `gemini`, `gemini live`, `gemini-live` and `gemini_live`. OpenAI and all other providers are excluded from enrollment and follow-ups even when the renderer marks a voice as test. The original bytes and manifests stay in this archive. `source-scope.tsv` records the original clip counts and inclusion or exclusion reason for every provider, voice and split. [Owner's provider ruling](https://github.com/autonomous-ai/lampOS/issues/8907#issuecomment-7221499098)

      Source splits come from `voices.tsv` and must agree with any per-clip split. Identities are (provider, voice). Of #{coverage.size} eligible declared test voices, #{selected.map(&:speaker).uniq.size} supplied follow-ups paired with an independent, separately rendered call whose entire normalized text is “Hey Lamp”. `test-voice-coverage.tsv` records eligible test voices, including unrepresented ones. `duplicate-wakes.tsv` in `provenance/` lists wake waveforms that were identical under different voice identities. No model output enters the selection.

      The #{truth.size} comparisons reuse #{selected.size} original follow-ups and #{(selected_clips - selected).size} original wake files. Each ordinary follow-up is paired with the caller and a different speaker on the identical follow-up waveform in each condition. Selection aims for #{per_length} unique texts per length per test voice on average, with equal provider targets and round-robin coverage within each provider. Short texts have 1–3 words, long texts have 4 or more. The picker spreads eligible word counts and speaking styles. Calls, greetings and assistant look-alike wake phrases are excluded from ordinary lines. For impostors it targets equal within-provider and across-provider examples per length and originating provider, preferring matching enrollment styles and balancing enrollment use. When a stratum is unavailable it uses an available impostor. `pair-provenance.tsv` discloses each choice. The publisher reports only Gemini supplied genuine whispered speech; voiced ElevenLabs “whispered” renders count as normal for enrollment matching.

      The channel is **#{kind}**. Sources are signed 16-bit mono PCM at 16 kHz. The capture-input derivatives keep the active span and prepend 300 ms. The 10 ms activity blocks must exceed 20 PCM units RMS and −40 dB relative to the highest block power. The repository's `microphone.clip_onsets` models onset attenuation per source. With the simulated room, `microphone.loudspeaker` also models the playback band. A measured channel already includes the source loudspeaker. `channel.py apply` convolves every clip with the saved impulse response and adds its shaped floor at −60 dBFS. Quiet clips contain this floor. Noisy clips add separately seeded `channel.pink` noise at +5 dB measured on active blocks. Both use the same headroom adjustment and `noise/` retains the exact additive PCM. Per-file seeds, acoustic bounds, measured SNRs, source digests and gain are in `clips.tsv`. This is a simulated input path. Physical acoustic and device latency require microphone captures.

      `provenance/` contains the original tar, the precise acoustic functions, channel response, producer and software versions. `source/` preserves every extracted member. `capture-input/`, `microphone/`, `pink-input/`, `channel-floor/`, `quiet/`, `noise/` and `noisy/` preserve the successive input and output samples. The provider corpus is synthetic. Keep the exact archive for future comparisons.
    DOC
    write(File.join(out, "README.md"), report)
    count = inventory(out, "audio-SHA256SUMS")
    puts "#{truth.size} comparisons, #{selected.size} follow-ups, #{selected_clips.size} selected recordings, #{count} archived files"
  rescue StandardError => e
    if created
      write(File.join(out, "INCOMPLETE.md"), "# Incomplete caller-voice preparation\n\nThe run stopped with #{e.class}: #{e.message}. All inputs and partial outputs from this attempt are retained. See `partial-SHA256SUMS`.\n")
      inventory(out, "partial-SHA256SUMS")
    end
    raise
  end
end

if $PROGRAM_NAME == __FILE__
  options = {per_length: 12, python: ENV.fetch("PYTHON", "python3")}
  parser = OptionParser.new do |p|
    p.banner = "usage: ruby prepare-heldout.rb [--channel CALIBRATION.npz] [--per-length N] SOURCE_TAR PUBLISHED_SHA256 NEW_DIR"
    p.on("--channel FILE") { |file| options[:calibration] = File.realpath(file) }
    p.on("--acoustic-dir DIR") { |dir| options[:acoustic_dir] = File.realpath(dir) }
    p.on("--per-length N", Integer) { |n| options[:per_length] = n }
  end
  begin
    parser.parse!
    abort(parser.to_s) unless ARGV.size == 3
    CallerHeldout.prepare(*ARGV, **options)
  rescue ArgumentError, KeyError, CSV::MalformedCSVError, SystemCallError, Gem::Package::TarInvalidError, Zlib::Error => e
    warn "caller-voice held-out corpus: #{e.message}"
    exit 2
  end
end
