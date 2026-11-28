# frozen_string_literal: true

require_relative "score"
require "fileutils"
require "open3"

# Bind an exact previously evaluated pair list to a label-free native replay.
module StrongReplay
  PROVIDER_PAIRS = "540b13ff4457aa569a9f8499b37fa8a90de9a820ded8935eb280eda205287172"
  PHYSICAL_PAIRS = "ca526e1a10942223c61736dab329894647a2a4b68e608ef0304aeb9ffa9ec0e7"
  PROVIDER_AUDIO = "b3a66082354b7b0181de89261c9d2eaadbfdca6f156c117a60e40a2e745f2dbf"
  PHYSICAL_AUDIO = "c84b0ef3499bfdf8722200512fd01f4a455a1a7f779735d3f926981c910de074"
  PHYSICAL_FOLDER = "fe2e0021f56f6512d51ab5165cd661d18440ad19846883890bb0636f06209130"
  MODEL = "d8c32b0dbc17e31c7b13c9d7910f0adaca2ea0b0b3351da05041c9168e741994"
  REVIEWER = "bfb0a95a9da96740f77923b76fd8547fecbb7ffd"
  SOURCE_SEALS = {
    "provider" => [PROVIDER_AUDIO, "907573417595c3347c56f9a152fb00a0dc5b9ac3f46014bf92071af7d16f873a"],
    "physical" => [PHYSICAL_FOLDER]
  }.freeze
  module_function

  def write(path, data)
    File.open(path, "wx", 0o600) { |f| f.write(data) }
  end

  def json(path, data)
    write(path, JSON.pretty_generate(data) + "\n")
  end

  def read(path)
    JSON.parse(File.read(path))
  end

  def sha(path)
    CallerFrameBench.sha(path)
  end

  def seal(root)
    files = Dir.glob("**/*", base: root).sort.select { |f| File.file?(File.join(root, f)) }
    write(File.join(root, "SHA256SUMS"), files.map { |f| "#{sha(File.join(root, f))}  #{f}\n" }.join)
  end

  def verify_seal(root)
    root = File.realpath(root)
    values = {}
    File.foreach(File.join(root, "SHA256SUMS"), chomp: true) do |line|
      m = /\A([0-9a-f]{64})  (.+)\z/.match(line)
      raise ArgumentError, "malformed or duplicate sealed file" unless m && !values.key?(m[2])
      raise ArgumentError, "changed sealed file #{m[2]}" unless sha(CallerFrameBench.private_file(root, m[2])) == m[1]
      values[m[2]] = m[1]
    end
    members = Dir.glob("**/*", base: root).select { |f| f != "SHA256SUMS" && File.file?(File.join(root, f)) }
    raise ArgumentError, "sealed collection has missing or additional files" unless members.sort == values.keys.sort
    values
  end

  def source_members(path)
    entries = if File.extname(path).downcase == ".json"
                read(path).fetch("files").map { |r| [r.fetch("path"), r.fetch("sha256")] }
              else
                File.readlines(path, chomp: true).map do |line|
                  m = /\A([a-f0-9]{64}) [ *](.+)\z/.match(line)
                  raise ArgumentError, "unrecognized collection seal format" unless m
                  [m[2].delete_prefix("./"), m[1]]
                end
              end
    entries.each_with_object({}) do |(file, sum), result|
      raise ArgumentError, "duplicate or invalid original inventory member" if result.key?(file) || !sum.match?(/\A[a-f0-9]{64}\z/)
      result[file] = sum
    end
  end

  def verify_corpus(kind, pair_file)
    return CallerFrameBench.verify_archive(pair_file) if kind == "provider"
    raise ArgumentError, "wrong physical pair digest" unless kind == "physical" && sha(pair_file) == PHYSICAL_PAIRS
    root = File.dirname(File.realpath(pair_file))
    checksum_file = CallerFrameBench.private_file(root, "audio-SHA256SUMS")
    raise ArgumentError, "wrong physical audio inventory" unless sha(checksum_file) == PHYSICAL_AUDIO
    sums = source_members(checksum_file)
    sums.each do |rel, expected|
      raise ArgumentError, "physical inventory mismatch for #{rel}" unless sha(CallerFrameBench.private_file(root, rel)) == expected
    end
    sums[File.basename(pair_file)] = PHYSICAL_PAIRS
    [sums, PHYSICAL_AUDIO]
  end

  def bind(revision, patch, binary, model, config, scorer, output)
    raise ArgumentError, "expected a full source revision" unless revision.match?(/\A[0-9a-f]{40}\z/)
    raise ArgumentError, "wrong pre-registered ECAPA graph" unless sha(model) == MODEL
    data = {build_sha: revision, producer_binary_sha256: sha(binary), model_sha256: MODEL,
            config_sha256: sha(config), scorer_revision: REVIEWER, scorer_sha256: sha(scorer)}
    if patch != "-"
      data[:source_patch] = File.realpath(patch)
      data[:source_patch_sha256] = sha(patch)
    end
    json(output, data)
  end

  def physical_members(outer, corpus_prefix, audio_sha, pair_sha, sums, planned)
    unless audio_sha == PHYSICAL_AUDIO && pair_sha == PHYSICAL_PAIRS &&
           outer.fetch(File.join(corpus_prefix, "audio-SHA256SUMS")) == PHYSICAL_AUDIO &&
           outer.fetch(File.join(corpus_prefix, "pairs.tsv")) == PHYSICAL_PAIRS
      raise ArgumentError, "original physical folder must bind both the waveform inventory and the pair list"
    end
    planned.each_value do |row|
      [row.reference, row.followup].each do |rel|
        raise ArgumentError, "physical folder and audio inventory disagree for #{rel}" unless outer.fetch(File.join(corpus_prefix, rel)) == sums.fetch(rel)
      end
    end
  end

  def verify_source(kind, pair_file, source_seal, audio_sha, sums, planned)
    unless SOURCE_SEALS.fetch(kind).include?(sha(source_seal))
      raise ArgumentError, "source seal differs from the original collection receipt"
    end
    if kind == "provider"
      unless sha(pair_file) == PROVIDER_PAIRS && audio_sha == PROVIDER_AUDIO && planned.size == 1108
        raise ArgumentError, "provider input must be the original 1,108 frozen pairs"
      end
      CallerFrameBench.primary_groups(pair_file, planned, sums)
    else
      strata = planned.values.group_by { |p| [p.words <= 3, p.condition, p.label] }.transform_values(&:size)
      unless sha(pair_file) == PHYSICAL_PAIRS && strata == {[true, "quiet", "same"] => 9, [true, "quiet", "different"] => 18,
                        [false, "quiet", "same"] => 7, [false, "quiet", "different"] => 10}
        raise ArgumentError, "physical inputs differ from the original 44 quiet pairs"
      end
      source_root = Pathname.new(File.dirname(File.realpath(source_seal)))
      corpus_prefix = Pathname.new(File.dirname(File.realpath(pair_file))).relative_path_from(source_root)
      raise ArgumentError, "physical corpus must be within the original sealed folder" if corpus_prefix.each_filename.any? { |part| part == ".." }
      return physical_members(source_members(source_seal), corpus_prefix.to_s, audio_sha, sha(pair_file), sums, planned)
    end
    return if sha(source_seal) == audio_sha
    members = source_members(source_seal)
    return if members.any? { |p, hash| File.basename(p) == "audio-SHA256SUMS" && hash == audio_sha }
    needed = [File.basename(pair_file)] + planned.values.flat_map { |p| [p.reference, p.followup] }.uniq
    unless needed.all? { |rel| members.any? { |p, hash| hash == sums.fetch(rel) && (p == rel || p.end_with?("/" + rel)) } }
      raise ArgumentError, "original seal does not bind the pair list and its WAVs"
    end
  end

  def sampling(kind, planned)
    return planned.keys.to_h { |id| [id, true] } if kind == "physical"
    # Select before inference, independently of scores. Five per nonempty truth/length/noise cell plus the first real case.
    groups = planned.values.group_by { |p| [p.label, p.condition, p.words <= 3] }
    rows = groups.values.flat_map { |g| g.sort_by { |p| Digest::SHA256.hexdigest("8849-strong-latency-v1/" + p.id) }.first(5) }
    ([planned.keys.first] + rows.map(&:id)).to_h { |id| [id, true] }
  end

  def make_plan(kind, pair_file, source_seal, dest)
    SOURCE_SEALS.fetch(kind)
    pair_file = File.realpath(pair_file)
    sums, audio_sha = verify_corpus(kind, pair_file)
    planned = CallerFrameBench.pairs(pair_file, sums)
    verify_source(kind, pair_file, source_seal, audio_sha, sums, planned)
    files = planned.values.flat_map { |p| [p.reference, p.followup] }.uniq.sort
    assets = files.group_by { |f| sums.fetch(f) }.sort.map { |hash, paths| [hash, paths.first] }
    chosen = sampling(kind, planned)
    Dir.mkdir(dest, 0o700)
    FileUtils.cp(pair_file, File.join(dest, "pairs.tsv"))
    header = "# pairs_sha256=#{sha(pair_file)}\n"
    write(File.join(dest, "assets.tsv"), header + "wav_sha256\trelative_path\n" + assets.map { |r| r.join("\t") + "\n" }.join)
    cases = planned.map { |id, p| [id, sums.fetch(p.reference), sums.fetch(p.followup), chosen.key?(id) ? 1 : 0] }
    write(File.join(dest, "cases.tsv"), header + "case_id\treference_sha256\tfollowup_sha256\ttime_fresh\n" + cases.map { |r| r.join("\t") + "\n" }.join)
    record = {kind: kind, pairs_sha256: sha(pair_file), audio_inventory_sha256: audio_sha, original_source_seal_sha256: sha(source_seal),
              planned_pairs: planned.size, unique_input_waveforms: assets.size, distinct_reference_waveforms: cases.map { |r| r[1] }.uniq.size,
              distinct_probe_waveforms: cases.map { |r| r[2] }.uniq.size, uncached_sample_size: chosen.size,
              note: "Private plan. Native input omits truth, text, length, condition and speaker identities. All inputs stay on the collecting computer."}
    json(File.join(dest, "input.json"), record)
    seal(dest)
    record
  end

  def run(plan, corpus, binary, model, config, freeze_file, mode, dest)
    raise ArgumentError, "timing mode must be all or sample" unless %w[all sample].include?(mode)
    plan = File.realpath(plan)
    content = verify_seal(plan)
    %w[input.json pairs.tsv assets.tsv cases.tsv].each { |f| content.fetch(f) }
    freeze = read(freeze_file)
    hashes = {"producer_binary_sha256" => sha(binary), "model_sha256" => sha(model), "config_sha256" => sha(config)}
    unless freeze.fetch("build_sha").match?(/\A[0-9a-f]{40}\z/) && hashes.all? { |k, v| freeze.fetch(k) == v }
      raise ArgumentError, "native binary, model, compiled config or build revision differs from the freeze"
    end
    if freeze["source_patch_sha256"]
      unless sha(freeze.fetch("source_patch")) == freeze["source_patch_sha256"]
        raise ArgumentError, "complete source patch differs from the freeze"
      end
    end
    Dir.mkdir(dest, 0o700)
    out = File.realpath(dest)
    FileUtils.cp(freeze_file, File.join(out, "freeze.json"))
    FileUtils.cp(config, File.join(out, "config"))
    before = hashes.merge("freeze_sha256" => sha(freeze_file), "plan_seal_sha256" => sha(File.join(plan, "SHA256SUMS")),
                          "pairs_sha256" => content.fetch("pairs.tsv"), "cases_sha256" => content.fetch("cases.tsv"),
                          "assets_sha256" => content.fetch("assets.tsv"), "build_sha" => freeze["build_sha"], "timing" => mode)
    json(File.join(out, "bindings.json"), before)
    args = [File.realpath(binary), plan, File.realpath(corpus), File.realpath(model), mode, File.join(out, "native")]
    stdout, stderr, status = Open3.capture3(*args)
    json(File.join(out, "execution.json"), {exit: status.exitstatus, stdout: stdout, stderr: stderr})
    raise ArgumentError, "native replay failed; retain this attempt" unless status.success?
    native = read(File.join(out, "native", "inputs.json"))
    %w[pairs_sha256 cases_sha256 assets_sha256 model_sha256 producer_binary_sha256].each do |k|
      raise ArgumentError, "native input digest mismatch for #{k}" unless native.fetch(k) == before.fetch(k)
    end
    raise ArgumentError, "native replay incomplete" unless File.file?(File.join(out, "native", "COMPLETE"))
    seal(out)
  end
end

if $PROGRAM_NAME == __FILE__
  File.umask(0o077)
  begin
    case ARGV.shift
    when "prepare"
      abort "usage: strong-replay.rb prepare provider|physical PAIRS ORIGINAL_SOURCE_SEAL NEW_PLAN" unless ARGV.length == 4
      puts JSON.generate(StrongReplay.make_plan(*ARGV))
    when "bind"
      abort "usage: strong-replay.rb bind BUILD_SHA SOURCE_PATCH_OR_DASH NATIVE_BINARY MODEL CONFIG SCORER NEW_FREEZE_JSON" unless ARGV.length == 7
      StrongReplay.bind(*ARGV)
    when "run"
      abort "usage: strong-replay.rb run PLAN CORPUS NATIVE_BINARY MODEL_FILE CONFIG_FILE FREEZE_JSON all|sample NEW_RUN" unless ARGV.length == 8
      StrongReplay.run(*ARGV)
    else
      abort "use prepare, bind or run"
    end
  rescue ArgumentError, SystemCallError, KeyError, JSON::ParserError => e
    warn "strong speaker replay: #{e.message}"
    exit 2
  end
end
