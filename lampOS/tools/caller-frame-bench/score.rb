# frozen_string_literal: true

require "digest"
require "csv"
require "json"
require "pathname"

# Compare two offline voice-match runs on exactly the same declared pairs.
module CallerFrameBench
  PAIR_COLUMNS = %w[case_id reference followup label words condition].freeze
  PREDICTION_COLUMNS = %w[case_id output score replay_elapsed_us phase].freeze
  CLASSES = %w[same different].freeze
  PRIMARY_PROVIDERS = %w[elevenlabs gemini].freeze
  GROUPS = ["all", "short/quiet", "short/noisy", "long/quiet", "long/noisy"].freeze
  Pair = Struct.new(:id, :reference, :followup, :label, :words, :condition)
  Prediction = Struct.new(:output, :score, :elapsed, :phase)

  module_function

  def sha(path)
    Digest::SHA256.file(path).hexdigest
  end

  def provider_id(value)
    key = value.to_s.strip.downcase
    ["gemini live", "gemini-live", "gemini_live"].include?(key) ? "gemini" : key
  end

  def primary_groups(pair_file, planned, digests)
    root = File.dirname(File.realpath(pair_file))
    %w[clips.tsv pair-provenance.tsv].each { |f| raise ArgumentError, "missing primary provenance #{f}" unless digests.key?(f) }
    clips = {}
    CSV.foreach(File.join(root, "clips.tsv"), headers: true, col_sep: "\t") do |r|
      id = r.fetch("id")
      provider = provider_id(r.fetch("provider"))
      unless PRIMARY_PROVIDERS.include?(provider) && r.fetch("split") == "test" && !r.fetch("voice").empty? && !clips.key?(id)
        raise ArgumentError, "primary clip #{id} needs an eligible held-out ElevenLabs or Gemini voice"
      end
      clips[id] = [provider, r.fetch("voice")]
    end
    by_provider = Hash.new { |h, k| h[k] = {} }
    seen = {}
    CSV.foreach(File.join(root, "pair-provenance.tsv"), headers: true, col_sep: "\t") do |r|
      id = r.fetch("case_id")
      pair = planned.fetch(id)
      target = r.fetch("followup_id")
      reference = r.fetch("enrollment_id")
      follow_voice = [provider_id(r.fetch("followup_provider")), r.fetch("followup_voice")]
      call_voice = [provider_id(r.fetch("enrollment_provider")), r.fetch("enrollment_voice")]
      unless seen[id].nil? && clips.fetch(target) == follow_voice && clips.fetch(reference) == call_voice &&
             r.fetch("condition") == pair.condition && r.fetch("label") == pair.label && (follow_voice == call_voice) == (pair.label == "same") &&
             pair.followup == "#{pair.condition}/#{target}.wav" && pair.reference == "#{pair.condition}/#{reference}.wav"
        raise ArgumentError, "primary source identity mismatch for #{id}"
      end
      seen[id] = true
      by_provider[follow_voice.first][id] = pair
    end
    raise ArgumentError, "missing primary pair provenance" unless seen.size == planned.size
    by_provider
  end

  def private_file(root, name)
    path = Pathname.new(name)
    if name.empty? || path.absolute? || path.each_filename.any? { |part| part == ".." || part == "." }
      raise ArgumentError, "expected a relative archive path: #{name.inspect}"
    end
    actual = File.realpath(File.join(root, name))
    unless actual.start_with?(root + File::SEPARATOR) && File.file?(actual)
      raise ArgumentError, "file outside the archive: #{name.inspect}"
    end
    actual
  end

  def verify_archive(pairs_path)
    root = File.dirname(File.realpath(pairs_path))
    checksum_file = private_file(root, "audio-SHA256SUMS")
    digests = {}
    File.foreach(checksum_file, chomp: true) do |line|
      entry = /\A([a-f0-9]{64})  (.+)\z/.match(line)
      raise ArgumentError, "malformed audio-SHA256SUMS entry" unless entry
      expected, name = entry.captures
      raise ArgumentError, "duplicate checksum for #{name}" if digests.key?(name)
      actual = sha(private_file(root, name))
      raise ArgumentError, "archive checksum mismatch for #{name}" unless actual == expected
      digests[name] = actual
    end
    raise ArgumentError, "pairs TSV missing from audio-SHA256SUMS" unless digests.key?(File.basename(pairs_path))
    [digests, sha(checksum_file)]
  end

  def table(path, columns, pairs_digest = nil)
    rows = {}
    File.open(path, "r:utf-8") do |io|
      if pairs_digest && io.gets&.chomp != "# pairs_sha256=#{pairs_digest}"
        raise ArgumentError, "#{File.basename(path)} was produced for a different pairs TSV"
      end
      raise ArgumentError, "unexpected columns in #{path}" unless io.gets&.chomp == columns.join("\t")
      io.each_line(chomp: true) do |line|
        fields = line.split("\t", -1)
        id = fields.first
        unless fields.size == columns.size && id.match?(/\A[a-zA-Z0-9_.:-]{1,96}\z/)
          raise ArgumentError, "invalid row at #{path}:#{io.lineno}"
        end
        raise ArgumentError, "duplicate case #{id} in #{path}" if rows.key?(id)
        rows[id] = yield(*fields)
      end
    end
    rows
  end

  def pairs(path, checksums)
    used_pairs = {}
    rows = table(path, PAIR_COLUMNS) do |id, reference, followup, label, words, condition|
      unless CLASSES.include?(label) && %w[quiet noisy].include?(condition) && words.match?(/\A[1-9]\d*\z/)
        raise ArgumentError, "invalid label, condition or word count for #{id}"
      end
      unless reference != followup && checksums.key?(reference) && checksums.key?(followup)
        raise ArgumentError, "pair #{id} needs two distinct checksummed clip paths"
      end
      files = [reference, followup].sort
      raise ArgumentError, "repeated audio pair #{id} and #{used_pairs[files]}" if used_pairs.key?(files)
      used_pairs[files] = id
      Pair.new(id, reference, followup, label, Integer(words, 10), condition)
    end
    raise ArgumentError, "empty pair manifest" if rows.empty?
    rows
  end

  def predictions(path, pairs_digest, planned)
    table(path, PREDICTION_COLUMNS, pairs_digest) do |id, output, score_text, us_text, phase|
      raise ArgumentError, "unplanned prediction #{id}" unless planned.key?(id)
      raise ArgumentError, "invalid prediction class for #{id}" unless (CLASSES + ["unknown"]).include?(output)
      score = score_text.empty? ? nil : Float(score_text)
      if (score && !score.finite?) || (score.nil? && output != "unknown")
        raise ArgumentError, "invalid score for #{id}"
      end
      measured = %w[first warm].include?(phase)
      unless (measured && us_text.match?(/\A\d+\z/)) || (phase == "unmeasured" && us_text.empty?)
        raise ArgumentError, "invalid host timing for #{id}"
      end
      Prediction.new(output, score, measured ? Integer(us_text, 10) : nil, phase)
    end
  end

  # The shared scorer uses the nearest rank for each percentile.
  def timing(values)
    sorted = values.sort
    n = sorted.size
    {"n" => n, "p50_us" => n.zero? ? nil : sorted[(n * 50 + 99) / 100 - 1],
     "p95_us" => n.zero? ? nil : sorted[(n * 95 + 99) / 100 - 1], "max_us" => sorted.last}
  end

  def tally(rows, run)
    result = {"total" => rows.size, "correct" => 0, "wrong" => 0, "unknown" => 0, "missing" => 0}
    rows.each do |pair|
      output = run[pair.id]&.output
      bucket = if output.nil?
                 "missing"
               elsif output == "unknown"
                 "unknown"
               elsif output == pair.label
                 "correct"
               else
                 "wrong"
               end
      result[bucket] += 1
    end
    result["percent"] = rows.empty? ? nil : (100.0 * result["correct"] / rows.size).round(2)
    result
  end

  def summarize(planned, run)
    summary = {}
    GROUPS.each do |key|
      rows = planned.values.select do |row|
        key == "all" || key == "#{row.words <= 3 ? 'short' : 'long'}/#{row.condition}"
      end
      summary[key] = CLASSES.to_h { |label| [label, tally(rows.select { |r| r.label == label }, run)] }
      summary[key]["host_inference"] = %w[first warm].to_h do |phase|
        times = rows.map { |r| run[r.id] }.compact.select { |p| p.phase == phase }.map(&:elapsed)
        [phase, timing(times)]
      end
    end
    summary
  end

  def fraction(count)
    return "— (0/0)" unless count["percent"]
    format("%.1f%% (%d/%d)", count["percent"], count["correct"], count["total"])
  end

  def markdown(report)
    lines = ["# Paired voice-match replay", "",
             "This is a `#{report.fetch('evidence')}` run. A short line has one to three words. A longer line has four or more.", "",
             "| Lines | Whole-line same caller kept | Frame same caller kept | Whole-line other rejected | Frame other rejected |",
             "|---|---:|---:|---:|---:|"]
    GROUPS.each do |group|
      cells = CLASSES.flat_map do |label|
        %w[whole frame].map { |name| fraction(report.fetch(name).fetch(group).fetch(label)) }
      end
      lines << "| #{([group] + cells).join(' | ')} |"
    end
    lines.concat(["", "Kept and rejected describe the classifier’s explicit same-caller and other-source decisions. The simple baseline admits ordinary in-window speech independently. Unknown, missing and wrong-direction outputs all count as misses. The JSON records those counts for every class and cell.", "",
                  "| Host inference | First n | First p50 µs | First p95 µs | Warm n | Warm p50 µs | Warm p95 µs |",
                  "|---|---:|---:|---:|---:|---:|---:|"])
    %w[whole frame].each do |name|
      elapsed = report.fetch(name).fetch("all").fetch("host_inference")
      cells = %w[first warm].flat_map { |k| elapsed.fetch(k).values_at("n", "p50_us", "p95_us") }
      lines << "| #{([name] + cells.map { |v| v.nil? ? '—' : v }).join(' | ')} |"
    end
    lines.concat(["", "Host timings use the producer’s measured inference boundaries. Initialization and enrollment measurements belong in the producer’s run log. Lamp readiness, first admission and end-of-speech to audible reply require a live capture.", "",
                  "The source WAV inventory is pinned by `audio-SHA256SUMS`. The source pair and prediction hashes are in `report.json`. The original TSV files retain the per-case scores and timings.", ""])
    lines.join("\n")
  end

  def compare(kind, pair_file, whole_file, frame_file, destination)
    evidence = {"synthetic" => "synthetic_development_replay",
                "synthetic-heldout" => "synthetic_heldout_voices_replay",
                "public-speech" => "public_off_lamp_speech_replay",
                "lamp-replay" => "labelled_lamp_audio_replay"}.fetch(kind)
    checksums, inventory = verify_archive(pair_file)
    planned = pairs(pair_file, checksums)
    pair_hash = checksums.fetch(File.basename(pair_file))
    inputs = {"pairs_sha256" => pair_hash, "audio_inventory_sha256" => inventory, "scorer_sha256" => sha(__FILE__)}
    report = {"version" => 1, "evidence" => evidence, "lamp_ready_us" => nil, "inputs" => inputs}
    groups = kind == "synthetic-heldout" ? primary_groups(pair_file, planned, checksums) : {}
    report["by_followup_provider"] = {} unless groups.empty?
    {"whole" => whole_file, "frame" => frame_file}.each do |name, path|
      inputs["#{name}_predictions_sha256"] = sha(path)
      run = predictions(path, pair_hash, planned)
      report[name] = summarize(planned, run)
      groups.each do |provider, subsample|
        (report["by_followup_provider"][provider] ||= {})[name] = summarize(subsample, run)
      end
    end
    Dir.mkdir(destination, 0o700)
    File.write(File.join(destination, "report.json"), JSON.pretty_generate(report) + "\n")
    File.write(File.join(destination, "README.md"), markdown(report))
    File.binwrite(File.join(destination, "score.rb"), File.binread(__FILE__))
    records = %w[README.md report.json score.rb].map { |f| "#{sha(File.join(destination, f))}  #{f}\n" }
    File.write(File.join(destination, "SHA256SUMS"), records.join)
    report
  end
end

if $PROGRAM_NAME == __FILE__
  begin
    unless ARGV.size == 6 && ARGV.shift == "compare"
      abort "usage: ruby score.rb compare synthetic|synthetic-heldout|public-speech|lamp-replay PAIRS WHOLE_TSV FRAME_TSV NEW_OUTPUT_DIR"
    end
    CallerFrameBench.compare(*ARGV)
  rescue ArgumentError, SystemCallError, KeyError => e
    warn "caller-frame score: #{e.message}"
    exit 2
  end
end
