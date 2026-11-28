#!/usr/bin/env ruby
# frozen_string_literal: true
# Package prepared Linux ARM64 executables with eight authenticated synthetic timing cases.
require "csv"
require "digest"
require "fileutils"
require "optparse"
require "rubygems/package"
require "zlib"
require_relative "board-fixtures"

module BoardKit
  ROOT = "sensor6-board-v2"
  MODEL = "ecapa-voxceleb.onnx"
  MODEL_SHA = "d8c32b0dbc17e31c7b13c9d7910f0adaca2ea0b0b3351da05041c9168e741994"
  module_function

  def sha(path)
    Digest::SHA256.file(path).hexdigest
  end

  def write(path, bytes)
    File.open(path, "wbx", 0o600) { |file| file.write(bytes) }
  end

  def checked_elf(path)
    File.open(path, "rb") do |f|
      header = f.read(64)
      valid = header&.bytesize == 64 && header.start_with?("\x7FELF".b) &&
              header.getbyte(4) == 2 && header.getbyte(5) == 1 &&
              [2, 3].include?(header.byteslice(16, 2).unpack1("v")) &&
              header.byteslice(18, 2).unpack1("v") == 183
      raise "#{path} must be a 64-bit little-endian AArch64 Linux ELF" unless valid
      offset = header.byteslice(32, 8).unpack1("Q<")
      step, count = header.byteslice(54, 4).unpack("v2")
      raise "invalid program table in #{path}" unless step >= 56 && count.between?(1, 1024) && offset + step * count <= f.stat.size
      dynamic_loader = count.times.any? do |i|
        f.seek(offset + step * i)
        f.read(4).unpack1("V") == 3
      end
      dynamic_loader ? "ELF64-AArch64-with-interpreter" : "ELF64-AArch64-static"
    end
  end

  def tar(kit, target)
    # RubyGems sets each tar entry's mtime with SOURCE_DATE_EPOCH.
    ENV["SOURCE_DATE_EPOCH"] = "0"
    names = [""] + Dir.glob("**/*", base: kit).sort
    File.open(target, "wbx", 0o600) do |file|
      gzip = Zlib::GzipWriter.new(file, Zlib::BEST_SPEED)
      gzip.mtime = 0
      Gem::Package::TarWriter.new(gzip) do |writer|
        names.each do |n|
          path = n.empty? ? kit : File.join(kit, n)
          entry = n.empty? ? ROOT : ROOT + "/" + n
          if File.directory?(path)
            writer.mkdir(entry, 0o755)
          else
            mode = n.start_with?("bin/") || n == "board-run.sh" ? 0o755 : 0o644
            writer.add_file_simple(entry, mode, File.size(path)) do |io|
              File.open(path, "rb") { |input| IO.copy_stream(input, io) }
            end
          end
        end
      end
      gzip.close
    end
  end

  def make(opt)
    raise "source revision needs 40 hex characters" unless opt.fetch(:revision).match?(/\A[0-9a-f]{40}\z/)
    raise "source snapshot digest needs 64 hex characters" unless opt.fetch(:source_sha).match?(/\A[0-9a-f]{64}\z/)
    binary = opt.fetch(:binary); timer = opt.fetch(:timer); model = opt.fetch(:model)
    modelsum = sha(model)
    raise "ECAPA source model does not match the frozen asset" unless modelsum == MODEL_SHA
    elf = checked_elf(binary); timer_elf = checked_elf(timer)
    destination = File.expand_path(opt.fetch(:out))
    Dir.mkdir(destination, 0o700)
    kit = File.join(destination, ROOT)
    %w[bin model licenses].each { |dir| FileUtils.mkdir_p(File.join(kit, dir)) }
    FileUtils.copy_file(binary, File.join(kit, "bin/lamp-same-voice"))
    FileUtils.copy_file(timer, File.join(kit, "bin/board-timer"))
    FileUtils.copy_file(model, File.join(kit, "model", MODEL))
    FileUtils.copy_file(File.join(__dir__, "board-run.sh"), File.join(kit, "board-run.sh"))
    %w[bin/lamp-same-voice bin/board-timer board-run.sh].each { |name| File.chmod(0o755, File.join(kit, name)) }
    license = opt.fetch(:license)
    license_hashes = %w[cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30
                        c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4]
    raise "expected original Apache-2.0 license text" unless license_hashes.include?(sha(license))
    FileUtils.copy_file(license, File.join(kit, "licenses", "Apache-2.0.txt"))
    write(File.join(kit, "licenses", "MODEL-NOTICE.txt"), <<~NOTICE)
      SpeechBrain spkrec-ecapa-voxceleb, released under Apache License 2.0 by the SpeechBrain project.
      Source https://huggingface.co/speechbrain/spkrec-ecapa-voxceleb/tree/0f99f2d0ebe89ac095bcc5903c4dd8f72b367286
      Original embedding_model.ckpt SHA-256 0575cb64845e6b9a10db9bcb74d5ac32b326b8dc90352671d345e2ee3d0126a2
      Converted ecapa-voxceleb.onnx SHA-256 #{MODEL_SHA}
      The lampOS conversion exports the original learned speaker encoder as a single-utterance ONNX graph with normalized 80-bin filterbank input and an L2-normalized 192-value output. The trained parameters were retained. The included weight asset is this graph. The SpeechBrain source model card declares Apache-2.0. See Apache-2.0.txt.
    NOTICE
    BoardFixtures.generate(opt.fetch(:fixtures), kit)
    write(File.join(kit, "SOURCE.tsv"), "field\tvalue\nsource_checkout_head\t#{opt[:revision]}\nsource_snapshot_sha256\t#{opt[:source_sha]}\nchild_format\t#{elf}\ntimer_format\t#{timer_elf}\nmodel_sha256\t#{modelsum}\nofficial_synthetic_tar_sha256\t#{BoardFixtures::TAR_SHA}\n")
    write(File.join(kit, "README.md"), <<~NOTE)
      # Sensor 6 v2 board timing

      From the extracted kit run `sh board-run.sh /tmp/sensor6-board-timing.tsv`. The output must be new. `SHA256SUMS` verifies the two Linux ARM64 executables, frozen model, and offline synthetic fixtures before timing. The extra `.child.log` retains stderr. No speaker or microphone is opened. No installation, running lamp service, network, Ruby or Python is required.

      There are three independent process starts and first replies, then 24 warm requests on the third child. The eight labelled #8907 provider-synthetic cases cover 1–3 and 6–7 word lines, same and different synthetic voices, and quiet or fixed +5 dB pink noise over active 10 ms frames. Each carries 300 ms of synthetic pre-roll. No human or physical recording is included. Each request sends both complete PCM inputs, including enrollment. The first case stays fixed across process starts. Warm cases rotate across rounds. These tiny reuse counts measure timing and do not establish classifier quality.

      Spawn-to-ready starts immediately before spawning the actual child and ends at the completed ready line. Per-request time starts immediately before writing the first header byte and stops after reading the complete decision line. It includes two PCM transfers, flush, both embeddings and the reply. WAV reads, summaries and result writes are outside it. Spawn-to-first-reply includes the handshake and request transition. A process-cold start can have warm system page caches. Numeric times are integer microseconds from a monotonic clock, with nearest-rank p50/p95 for each recorded group. Every complete request, including unknown, counts. A request or startup without a full response after 60 seconds leaves an incomplete result.
    NOTE
    entries = Dir.glob("**/*", base: kit).select { |n| File.file?(File.join(kit,n)) }.sort
    write(File.join(kit, "SHA256SUMS"), entries.map { |n| "#{sha(File.join(kit,n))}  #{n}\n" }.join)
    target = File.join(destination, ROOT + ".tar.gz")
    tar(kit, target)
    write(File.join(destination, ROOT + ".tar.gz.sha256"), "#{sha(target)}  #{File.basename(target)}\n")
    puts "#{sha(target)}  #{File.basename(target)}"
  end
end

if $PROGRAM_NAME == __FILE__
  options = {}
  parser = OptionParser.new do |p|
    p.banner = "ruby board-kit.rb --binary ARM64_CHILD --timer ARM64_TIMER --model ECAPA_ONNX --fixtures OFFICIAL_TAR_OR_EXTRACTED_SOURCE --license APACHE_2_0_TEXT --revision GIT_COMMIT --source-sha256 SHA --out NEW_DIR"
    %i[binary timer model fixtures license revision out].each { |k| p.on("--#{k} VALUE") { |v| options[k] = v } }
    p.on("--source-sha256 SHA") { |v| options[:source_sha] = v }
  end
  parser.parse!
  abort(parser.to_s) unless ARGV.empty? && %i[binary timer model fixtures license revision source_sha out].all? { |k| options[k] }
  File.umask(0o077)
  BoardKit.make(options)
end
