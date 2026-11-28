# frozen_string_literal: true

require_relative "replay"
require "minitest/autorun"
require "stringio"

class OverlapStopTest < Minitest::Test
  def test_only_complete_cues_match
    ["stop", "please wait", "lamp hold on please", "okay okay never mind",
     "wait wait", "stop stop now", "nevermind"].each { |text| assert OverlapStop.cue?(text), text }
    ["", "okay", "do not stop", "the bus stop", "wait for me", "stopped",
     "okay okay okay never mind", "hold"].each { |text| refute OverlapStop.cue?(text), text }
  end

  def test_revisions_and_refractory_do_not_duplicate_a_vote
    trigger = OverlapStop::Trigger.new
    assert trigger.observe("stop", false, 640)
    refute trigger.observe("stopped", false, 960)
    refute trigger.observe("stop", true, 1280)
    refute trigger.observe("wait", false, 1600)
    refute trigger.observe("wait", true, 17_000)
    assert trigger.observe("hold on", true, 17_320)
  end

  class Decoder
    attr_reader :sizes

    def initialize
      @sizes = []
    end

    def vosk_recognizer_accept_waveform(_recognizer, pcm, bytes)
      @sizes << pcm.bytesize
      raise "bad byte count" unless pcm.bytesize == bytes
      @sizes.size == 2 ? 1 : 0
    end

    def vosk_recognizer_partial_result(_recognizer)
      '{"partial":"wait"}'
    end

    def vosk_recognizer_result(_recognizer)
      '{"text":"wait"}'
    end
  end

  def test_uses_the_available_audio_clock_and_never_flushes_at_eof
    decoder = Decoder.new
    trace = StringIO.new
    OverlapStop.replay(decoder, nil, StringIO.new("\0" * 1300), trace)
    rows = trace.string.lines.map { |line| JSON.parse(line) }
    assert_equal [640, 640, 20], decoder.sizes
    assert_equal [320, 640, 650], rows[0...-1].map { |r| r.fetch("end_sample") }
    assert_equal [true, false, false], rows[0...-1].map { |r| r.fetch("cue") }
    assert_equal [false, true, false], rows[0...-1].map { |r| r.fetch("endpoint") }
    assert_operator rows.first.fetch("ready_us"), :>=, 20_000
    assert_equal 650, rows.last.fetch("samples")
    assert rows.last.fetch("complete")
  end

  def test_rejects_missing_and_partial_samples
    ["", "\0"].each do |pcm|
      assert_raises(RuntimeError) { OverlapStop.replay(Decoder.new, nil, StringIO.new(pcm), StringIO.new) }
    end
  end
end
