#!/usr/bin/env ruby
# Build matched synthetic pairs for personal VAD on macOS. This never reads a microphone.
require 'digest'
require 'fileutils'
require 'open3'
require 'rbconfig'

RATE = 16_000
BLOCK = 160 # 10 ms
PRE_ROLL = RATE * 3 / 10
SNR_DB = 5.0
VOICES = [
  ['Daniel', 'en_GB'], ['Rishi', 'en_IN'], ['Samantha', 'en_US'], ['Tessa', 'en_ZA'],
  ['Moira', 'en_IE'], ['Karen', 'en_AU']
].freeze
PROMPTS = [
  'Again.', 'Tomorrow?', 'How much?', 'In euros?', 'And times two?', 'What about Tuesday?',
  'Please read the last part again.', 'Will it rain later this afternoon?',
  'What time is it in Hanoi?', 'Convert that total to euros for me.',
  'Could you turn the volume down?', 'How long is the trip by train?'
].freeze
FOLLOWUP_RATES = [151, 164, 178, 193, 204].freeze
FOLLOWUP_PITCH = [-0.5, 0, 0.5].freeze

def capture(*args)
  output, error, status = Open3.capture3(*args, binmode: true)
  abort "#{args.first} failed: #{error}" unless status.success?
  output
end

def write(path, contents)
  File.open(path, 'wbx', 0o600) { |file| file.write(contents) }
end

def wav(path, samples)
  pcm = samples.pack('s<*')
  header = ['RIFF', pcm.size + 36, 'WAVE', 'fmt ', 16, 1, 1, RATE, RATE * 2,
            2, 16, 'data', pcm.size].pack('a4Va4a4VvvVVvva4V')
  write(path, header + pcm)
end

def read_pcm(path, filters)
  args = ['ffmpeg', '-nostdin', '-loglevel', 'error', '-i', path, '-ac', '1', '-af', filters,
          '-ar', RATE.to_s, '-f', 's16le', '-c:a', 'pcm_s16le', 'pipe:1']
  pcm = capture(*args).unpack('s<*')
  abort "Empty or silent rendering in #{path}" unless pcm.any? { |s| s.abs > 100 }
  pcm
end

def active_blocks(samples)
  power = samples.each_slice(BLOCK).map { |s| s.sum { |v| v * v }.fdiv(s.size) }
  threshold = [power.max * 0.0001, 20**2].max # -40 dB from the loudest block
  selected = power.each_index.select { |i| power[i] >= threshold }
  abort 'No active speech energy in synthetic rendering' if selected.empty?
  selected
end

def render(root, id, text, voice, speed, pitch)
  native = "native/#{id}.aiff"
  capture('/usr/bin/say', '-v', voice, '-r', speed.to_s, '-o', File.join(root, native), text)
  converted = read_pcm(File.join(root, native), "aresample=#{RATE}")
  wav(File.join(root, "source16/#{id}.wav"), converted)
  samples = if pitch.zero?
              converted
            else
              shifted_rate = (RATE * 2**(pitch / 12.0)).round
              read_pcm(File.join(root, "source16/#{id}.wav"), "asetrate=#{shifted_rate},aresample=#{RATE}")
            end

  active = active_blocks(samples)
  # Retain all samples from first to last active block, including internal pauses.
  first = active.first * BLOCK
  last = [(active.last + 1) * BLOCK, samples.length].min
  clean = Array.new(PRE_ROLL, 0) + samples[first...last]
  active_ranges = active.map do |block|
    start = PRE_ROLL + block * BLOCK - first
    (start...[start + BLOCK, clean.size].min)
  end
  seed = Digest::SHA256.digest("sensor-6-noise-v1/#{id}").unpack1('N')
  random = Random.new(seed)
  filtered = 0.0
  phase = random.rand * 2 * Math::PI
  noise = Array.new(clean.size) do |i|
    white = 2.0 * random.rand - 1.0
    filtered = 0.90 * filtered + 0.10 * white
    0.9 * filtered + 0.12 * white + 0.045 * Math.sin(2 * Math::PI * 60 * i / RATE + phase)
  end
  energy = ->(a) { active_ranges.sum { |r| r.sum { |i| a[i]**2 } } }
  noise_gain = Math.sqrt(energy.call(clean) / (energy.call(noise) * 10**(SNR_DB / 10)))
  scaled_noise = noise.map { |v| v * noise_gain }
  peak = clean.each_index.map { |i| [clean[i].abs, scaled_noise[i].abs, (clean[i] + scaled_noise[i]).abs].max }.max
  headroom = [1.0, 32_000.0 / peak].min
  clean.map! { |s| (s * headroom).round }
  scaled_noise.map! { |s| (s * headroom).round }
  noisy = clean.each_index.map { |i| clean[i] + scaled_noise[i] }
  raise 'PCM clipping' unless noisy.all? { |v| (-32_768..32_767).cover?(v) }
  measured_snr = 10 * Math.log10(energy.call(clean).fdiv(energy.call(scaled_noise)))
  raise "Unexpected SNR #{measured_snr}" unless (measured_snr - SNR_DB).abs < 0.01
  wav(File.join(root, "quiet/#{id}.wav"), clean)
  wav(File.join(root, "noise/#{id}.wav"), scaled_noise)
  wav(File.join(root, "noisy/#{id}.wav"), noisy)
  [native, converted.length, clean.length, seed, format('%.5f', measured_snr), first, last,
   format('%.9f', headroom), active.length]
end

def print_tsv(path, header, rows)
  write(path, ([header] + rows).map { |r| r.join("\t") }.join("\n") + "\n")
end

abort 'Usage: ruby generate-fixtures.rb NEW_ARCHIVE_DIRECTORY' unless ARGV.length == 1
root = File.expand_path(ARGV.fetch(0))
File.umask(0o077)
FileUtils.mkdir_p(root, mode: 0o700)
abort 'The output path must be a private directory' unless File.directory?(root) && (File.stat(root).mode & 0o077).zero?
dirs = %w[native source16 quiet noise noisy fixture-provenance]
owned_files = %w[pairs.tsv clips.tsv README.md audio-SHA256SUMS]
abort 'Existing fixture output: choose a fresh directory. Existing data will be retained.' if (dirs + owned_files).any? { |n| File.exist?(File.join(root, n)) }
dirs.each { |n| Dir.mkdir(File.join(root, n), 0o700) }

say_list = capture('/usr/bin/say', '-v', '?')
VOICES.each { |voice, locale| abort "Unavailable voice #{voice}" unless say_list.lines.any? { |s| s.start_with?(voice) && s.include?(locale) } }
write(File.join(root, 'fixture-provenance', 'system-voices.txt'), say_list)
versions = [capture('sw_vers'), capture('ffmpeg', '-version'), RUBY_DESCRIPTION, RbConfig::CONFIG['host_os']]
write(File.join(root, 'fixture-provenance', 'tools.txt'), versions.join("\n"))
write(File.join(root, 'fixture-provenance', 'generate-fixtures.rb'), File.binread(__FILE__))

clips = []
pairs = []
wake_identity = {}
wake_hashes = []
VOICES.each_with_index do |(voice, locale), v|
  voice_id = format('v%02d', v + 1)
  2.times do |take|
    id = "#{voice_id}-wake#{take + 1}"
    speed = [153, 183][take]
    info = render(root, id, 'Hey Lamp.', voice, speed, 0)
    # say can list a voice yet silently synthesize with a fallback. Compare the actual
    # untrimmed decoded PCM at identical text, rate and pitch before accepting a label.
    digest = File.open(File.join(root, "source16/#{id}.wav"), 'rb') do |file|
      file.seek(44)
      Digest::SHA256.hexdigest(file.read)
    end
    previous = wake_identity[[speed, digest]]
    abort "Duplicate wake PCM for #{previous} and #{voice} at #{speed} wpm. Retain and reject this run." if previous
    wake_identity[[speed, digest]] = voice
    wake_hashes << [id, voice, speed, digest]
    clips << [id, voice_id, voice, locale, 'wake', 'Hey Lamp.', 2, speed, 0, *info]
  end
  PROMPTS.each_with_index do |phrase, p|
    id = format('%s-f%02d', voice_id, p + 1)
    speed = FOLLOWUP_RATES[(p + v) % FOLLOWUP_RATES.length]
    pitch = FOLLOWUP_PITCH[(p + 2 * v) % FOLLOWUP_PITCH.length]
    info = render(root, id, phrase, voice, speed, pitch)
    words = phrase.split.length
    clips << [id, voice_id, voice, locale, 'followup', phrase, words, speed, pitch, *info]
    # Each rendered follow-up is scored twice against separately rendered calls. Rotate
    # impostors through all other voices, balancing the enrollment speakers at each prompt.
    take = 1 + (p % 2)
    other = (v + 1 + p % (VOICES.length - 1)) % VOICES.length
    references = [['same', "#{voice_id}-wake#{take}"], ['different', format('v%02d-wake%d', other + 1, take)]]
    %w[quiet noisy].each do |condition|
      references.each do |label, ref|
        case_id = format('s6-%04d', pairs.length + 1)
        pairs << [case_id, "#{condition}/#{ref}.wav", "#{condition}/#{id}.wav", label, words, condition]
      end
    end
  end
  warn "Rendered #{voice_id} (#{voice})"
end
print_tsv(File.join(root, 'fixture-provenance', 'wake-pcm-sha256.tsv'), %w[id voice rate_wpm decoded_s16le_sha256], wake_hashes)
print_tsv(File.join(root, 'clips.tsv'), %w[id voice_id voice locale role text words rate_wpm pitch_semitones native resampled_samples capture_samples noise_seed achieved_snr_db first_active_sample last_active_exclusive headroom_gain active_10ms_blocks], clips)
print_tsv(File.join(root, 'pairs.tsv'), %w[case_id reference followup label words condition], pairs)

readme = <<~DOC
  # Synthetic caller-match benchmark • 27 November 2026

  This development set contains **#{pairs.size} paired trials** rendered on a Mac with #{VOICES.length} stock English system text-to-speech voices. The raw material is digital synthesis. The 16 kHz mono signed PCM matches the lamp's input format. This set measures neither room acoustics nor performance with human voices on the lamp.

  Each voice rendered two separate “Hey Lamp.” calls (153 and 183 words per minute) and 12 follow-ups. Six prompts are short, covering one, two and three words twice each. Six are longer. Each prompt is spoken by every voice. Follow-ups use a preselected 151–204 wpm rate and a pitch adjustment of −0.5, 0 or +0.5 semitones. Resampling to change pitch also changes the follow-up duration slightly. The generator picked rates, pitch and noise independently of model outputs. The source inventory, word counts, voices, individual settings, sample counts, noise seeds, applied gain and achieved SNR appear in `clips.tsv`.

  Each of the #{VOICES.size * PROMPTS.size} separately rendered follow-ups appears in quiet and noisy form. The same WAV in each condition is paired with a wake from its own voice and a wake from a different voice. The other voice rotates for each prompt. This yields **#{pairs.size / 8} trials per class × length × noise cell**, #{pairs.size / 2} rows in each class and #{VOICES.size * 2} original wake clips. There are #{clips.size} original TTS renderings and #{clips.size * 2} quiet/noisy WAVs. A fixed phrase, pitch and rate produced distinct decoded waveforms for each of the #{VOICES.size} voices at both enrollment rates. The decoded PCM checksums are in `fixture-provenance/wake-pcm-sha256.tsv`. The rows reuse audio. They cannot be counted as #{pairs.size} independent recordings or as independently held-out people. Both clips in a noisy pair contain separately seeded noise. The noise is synthetic stationary, colored noise with a 60 Hz component. TV, a second simultaneous talker, microphone response, reverberation and device timing require separate captures.

  The `native/` directory retains the original AIFF or AIFC output from `say`. `source16/` retains each entire untrimmed mono signal resampled to 16 kHz. Input `quiet/` clips keep the portion from the first to the last active 10 ms block, with 300 ms of silence prepended to represent capture pre-roll. Block power must be within 40 dB of the loudest block and above 20 signed-PCM units RMS. `noise/` preserves the exact additive signal for every file. `noisy/` is the sample-wise sum of `quiet/` and `noise/`. Noise is scaled to +5 dB SNR using the active blocks, and the same headroom gain protects all three stems. `first_active_sample` and `last_active_exclusive` refer to the 16 kHz signal after the pitch variation. The `pairs.tsv` paths are relative to this archive. Checksums cover every audio file, both manifests, this README and the full generator/tool provenance.

  Run `shasum -a 256 -c audio-SHA256SUMS` here to check the archive. On macOS, `ruby fixture-provenance/generate-fixtures.rb PATH_TO_NEW_PRIVATE_DIRECTORY` builds another set. The generator refuses to overwrite earlier output. Keep these original bits and hashes for repeat measurements because installed system synthesizer versions may change.

  Benchmark rows point only into `quiet/` and `noisy/`. Any separately retained engineering probes are outside the denominators.
DOC
write(File.join(root, 'README.md'), readme)
hash_paths = dirs.flat_map { |d| Dir.children(File.join(root, d)).map { |n| File.join(d, n) } }
hash_paths += %w[clips.tsv pairs.tsv README.md]
probes = File.join(root, 'probes')
hash_paths += Dir.children(probes).map { |n| File.join('probes', n) } if File.directory?(probes)
lines = hash_paths.sort.map { |relative| "#{Digest::SHA256.file(File.join(root, relative)).hexdigest}  #{relative}" }
write(File.join(root, 'audio-SHA256SUMS'), lines.join("\n") + "\n")
warn "#{pairs.size} pairs, #{clips.size} native sources, #{lines.size} SHA-256 entries"
