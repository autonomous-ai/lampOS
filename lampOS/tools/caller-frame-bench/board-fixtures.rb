# frozen_string_literal: true
require "csv"
require "digest"
require "fileutils"
require "rubygems/package"

module BoardFixtures
  TAR_SHA = "4fe9cfc9b8ce12f5e3dfcb316904e843861738adbd6ccd970b7b426ed563cc34"
  RATE = 16_000
  BLOCK = RATE / 100
  module_function

  def source_bytes(source, needed)
    if File.directory?(source)
      return needed.to_h { |name| [name, File.binread(File.join(source, name))] }
    end
    raise "unexpected official source tar SHA-256" unless Digest::SHA256.file(source).hexdigest == TAR_SHA
    files = {}
    File.open(source, "rb") do |input|
      Gem::Package::TarReader.new(input).each do |item|
        name = item.full_name.delete_prefix("./")
        files[name] = item.read if item.file? && needed.include?(name)
        break if files.size == needed.size
      end
    end
    raise "official source is missing expected members" unless files.keys.sort == needed.sort
    files
  end

  def pcm(wav)
    raise "not a WAV" unless wav.start_with?("RIFF") && wav.byteslice(8, 4) == "WAVE"
    at = 12
    data = nil
    correct_format = false
    while at + 8 <= wav.bytesize
      tag, n = wav.byteslice(at, 8).unpack("a4V")
      at += 8
      raise "truncated WAV" if at + n > wav.bytesize
      body = wav.byteslice(at, n)
      correct_format = body.unpack("vvVVvv") == [1, 1, RATE, RATE * 2, 2, 16] if tag == "fmt "
      data = body if tag == "data"
      at += n + (n % 2)
    end
    raise "expected 16 kHz mono signed PCM16" unless correct_format && data && data.bytesize.positive? && data.bytesize.even?
    data.unpack("s<*")
  end

  def write_wav(path, samples)
    data = samples.pack("s<*")
    header = ["RIFF", 36 + data.bytesize, "WAVE", "fmt ", 16, 1, 1, RATE, RATE * 2, 2, 16, "data", data.bytesize]
    File.binwrite(path, header.pack("a4Va4a4VvvVVvva4V") + data)
  end

  def pink(seed, length)
    state = seed
    rand_pcm = lambda do
      state = (1_664_525 * state + 1_013_904_223) & 0xffff_ffff
      (state >> 16) - 32_768
    end
    octaves = Array.new(12) { rand_pcm.call }
    sum = octaves.sum
    Array.new(length) do |i|
      n = i + 1
      index = [((n & -n).bit_length - 1), octaves.length - 1].min
      sum -= octaves[index]
      octaves[index] = rand_pcm.call
      sum += octaves[index]
      (sum + rand_pcm.call).div(13)
    end
  end

  def condition(raw, name, noisy)
    powers = raw.each_slice(BLOCK).map { |s| s.sum { |v| v * v }.fdiv(s.size) }
    threshold = [400, powers.max * 0.0001].max
    blocks = powers.each_index.select { |i| powers[i] >= threshold }
    raise "no clear synthetic speech" if blocks.empty?
    first = blocks.first * BLOCK
    last = [(blocks.last + 1) * BLOCK, raw.size].min
    silence = RATE * 3 / 10
    signal = Array.new(silence, 0).concat(raw[first...last])
    return [signal, ""] unless noisy
    seed = Digest::SHA256.digest("board-s6-pink-v1\0" + name).unpack1("N")
    noise = pink(seed, signal.size)
    spans = blocks.map { |i| (i * BLOCK - first + silence)...[(i + 1) * BLOCK - first + silence, signal.size].min }
    power = ->(a) { spans.sum { |span| span.sum { |i| a[i] * a[i] } } }
    ratio = Math.sqrt(power.call(signal).fdiv(power.call(noise) * (10 ** 0.5)))
    peak = signal.each_index.map { |i| [signal[i].abs, (noise[i] * ratio).abs, (signal[i] + noise[i] * ratio).abs].max }.max
    headroom = [1.0, 32000.0 / peak].min
    dry = signal.map { |v| (v * headroom).round }
    added = noise.map { |v| (v * ratio * headroom).round }
    mixed = dry.zip(added).map { |u,v| u + v }
    achieved = 10 * Math.log10(power.call(dry).fdiv(power.call(added)))
    raise "invalid synthetic noise mix" unless (achieved - 5).abs <= 0.05 && mixed.all? { |v| (-32768..32767).cover?(v) }
    [mixed, format("%.5f", achieved)]
  end

  def generate(source, kit)
    plan_file = File.join(__dir__, "board-fixture-sources.tsv")
    raise "changed synthetic source plan" unless Digest::SHA256.file(plan_file).hexdigest == "c2e72746727368059273998e91259c15aa0f956e2090e2d201d948f872f9cea1"
    rows = CSV.read(plan_file, headers: true, col_sep: "\t")
    raise "expected eight frozen timing cases" unless rows.size == 8
    names = rows.flat_map { |r| [r.fetch("reference_source"), r.fetch("followup_source")] }.uniq
    raise "unexpected public fixture path" unless names.all? { |s| s.match?(%r{\A(?:gemini|elevenlabs)/[[:alnum:]]+/[[:alpha:]]+/[[:digit:]]+\.wav\z}) }
    bytes = source_bytes(source, names)
    FileUtils.mkdir_p(File.join(kit, "audio"))
    manifest = [%w[case_id reference followup label words condition]]
    provenance = [%w[case_id role official_case source source_sha256 samples measured_noisy_snr_db]]
    rows.each do |r|
      paths = %w[reference followup].map do |role|
        name = r.fetch(role + "_source")
        b = bytes.fetch(name)
        sha = Digest::SHA256.hexdigest(b)
        raise "unexpected source bytes for #{name}" unless sha == r.fetch(role + "_sha256")
        samples, snr = condition(pcm(b), name, r["condition"] == "noisy")
        rel = "audio/" + r.fetch("case_id") + "-" + role + ".wav"
        write_wav(File.join(kit, rel), samples)
        provenance << [r["case_id"], role, r["official_case"], name, sha, samples.size, snr]
        rel
      end
      manifest << [r["case_id"], *paths, r["label"], r["words"], r["condition"]]
    end
    BoardKit.write(File.join(kit, "cases.tsv"), manifest.map { |r| r.join("\t") }.join("\n") + "\n")
    BoardKit.write(File.join(kit, "synthetic-sources.tsv"), provenance.map { |r| r.join("\t") }.join("\n") + "\n")
  end
end
