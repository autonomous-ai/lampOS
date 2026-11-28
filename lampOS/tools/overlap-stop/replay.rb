# frozen_string_literal: true

require "digest"
require "fiddle/import"
require "json"

# Replay one continuous stretch of 16 kHz, mono, little-endian PCM. The input
# contains no playback schedule, scene identifiers or ground-truth words.
module OverlapStop
  RATE = 16_000
  CHUNK = 320
  MODEL_INVENTORY = "5c9c563ec18e9a7d176eacdb18788b1c7dde7decf40c92ee2c8374668ade9656"
  CUES = ["stop", "stop stop", "wait", "wait wait", "hold on", "never mind", "nevermind"].freeze
  LEADING = %w[okay ok please lamp].freeze
  TRAILING = %w[please now].freeze

  def self.cue?(words)
    tokens = words.downcase.split
    2.times { tokens.shift if LEADING.include?(tokens.first) }
    tokens.pop if TRAILING.include?(tokens.last)
    CUES.include?(tokens.join(" "))
  end

  class Trigger
    def initialize
      @matched = false
      @next_sample = 0
    end

    def observe(text, endpoint, samples)
      matches = OverlapStop.cue?(text)
      vote = matches && !@matched && samples >= @next_sample
      @matched ||= matches
      @next_sample = samples + RATE if vote
      @matched = false if endpoint
      vote
    end
  end

  def self.verify_model(directory)
    root = File.realpath(directory)
    files = Dir.glob(File.join(root, "**", "*")).select { |path| File.file?(path) }.sort
    manifest = files.map do |path|
      "#{Digest::SHA256.file(path).hexdigest}  #{path.delete_prefix(root + File::SEPARATOR)}\n"
    end.join
    abort "model differs from the frozen Vosk small English US 0.15 release" unless Digest::SHA256.hexdigest(manifest) == MODEL_INVENTORY
    root
  end

  def self.api_for(library)
    Module.new do
      extend Fiddle::Importer
      dlload library
      extern "void vosk_set_log_level(int)"
      extern "void *vosk_model_new(const char *)"
      extern "void vosk_model_free(void *)"
      extern "void *vosk_recognizer_new(void *, float)"
      extern "void vosk_recognizer_free(void *)"
      extern "int vosk_recognizer_accept_waveform(void *, const char *, int)"
      extern "char *vosk_recognizer_partial_result(void *)"
      extern "char *vosk_recognizer_result(void *)"
    end
  end

  def self.replay(api, recognizer, input, output)
    trigger = Trigger.new
    samples = 0
    ready_us = 0
    input = input.binmode
    while (pcm = input.read(2 * CHUNK))
      raise "truncated PCM sample" if pcm.bytesize.odd?
      samples += pcm.bytesize / 2
      end_us = samples * 1_000_000 / RATE
      began = Process.clock_gettime(Process::CLOCK_MONOTONIC, :microsecond)
      status = api.vosk_recognizer_accept_waveform(recognizer, pcm, pcm.bytesize)
      raise "decoder rejected PCM" unless [0, 1].include?(status)
      endpoint = status == 1
      result = endpoint ? api.vosk_recognizer_result(recognizer) : api.vosk_recognizer_partial_result(recognizer)
      words = JSON.parse(result.to_s).fetch(endpoint ? "text" : "partial")
      vote = trigger.observe(words, endpoint, samples)
      cost_us = Process.clock_gettime(Process::CLOCK_MONOTONIC, :microsecond) - began
      # Availability on a paced single worker, excluding offline file I/O.
      ready_us = [ready_us, end_us].max + cost_us
      output.puts JSON.generate(end_sample: samples, endpoint: endpoint, text: words,
                                cue: vote, compute_us: cost_us, ready_us: ready_us)
    end
    raise "empty PCM" if samples.zero?
    output.puts JSON.generate(complete: true, samples: samples, ready_us: ready_us)
  end

  def self.run(library, directory, input, output)
    root = verify_model(directory)
    library = File.realpath(library)
    api = api_for(library)
    api.vosk_set_log_level(-1)
    model = api.vosk_model_new(root)
    raise "could not load Vosk model" if model.null?
    recognizer = api.vosk_recognizer_new(model, RATE.to_f)
    raise "could not create Vosk recognizer" if recognizer.null?
    output.puts JSON.generate(model_inventory_sha256: MODEL_INVENTORY,
                              library_sha256: Digest::SHA256.file(library).hexdigest,
                              sample_rate_hz: RATE, chunk_samples: CHUNK)
    replay(api, recognizer, input, output)
  ensure
    api.vosk_recognizer_free(recognizer) if recognizer && !recognizer.null?
    api.vosk_model_free(model) if model && !model.null?
  end
end

if __FILE__ == $PROGRAM_NAME
  abort "usage: ruby replay.rb VOSK_LIBRARY MODEL_DIR < audio.s16le > new-trace.jsonl" unless ARGV.size == 2
  OverlapStop.run(ARGV[0], ARGV[1], $stdin, $stdout)
end
