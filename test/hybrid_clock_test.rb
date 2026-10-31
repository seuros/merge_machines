# frozen_string_literal: true

require 'test_helper'

module MergeMachines
  class HybridClockTest < Minitest::Test
    # Wall-clock readings in seconds, both far enough ahead of any realtime
    # reading that the clock has to keep them, and both exact in a Float.
    AHEAD = 1 << 33
    LATER = 1 << 34

    def test_a_clock_needs_the_replica_it_stamps_for
      assert_equal 7, HybridClock.new(replica: 7).replica
      assert_raises(ArgumentError) { HybridClock.new }
    end

    def test_a_reading_the_clock_has_already_passed_only_moves_the_counter
      clock = HybridClock.new(replica: 1)

      assert_equal Timestamp.new(wall: AHEAD, counter: 0), clock.tick(wall: AHEAD)
      assert_equal Timestamp.new(wall: AHEAD, counter: 1), clock.tick(wall: 1.0)
      assert_equal Timestamp.new(wall: AHEAD, counter: 1), clock.now
      assert_equal Timestamp.new(wall: LATER, counter: 0), clock.tick(wall: LATER)
    end

    def test_a_tick_with_no_reading_takes_one_from_the_realtime_clock
      before = Process.clock_gettime(Process::CLOCK_REALTIME)
      issued = HybridClock.new(replica: 1).tick
      after = Process.clock_gettime(Process::CLOCK_REALTIME)

      assert_operator issued.wall, :>=, before
      assert_operator issued.wall, :<=, after
    end

    def test_an_observed_timestamp_from_the_future_is_stamped_after_it
      clock = HybridClock.new(replica: 1)
      issued = clock.observe(wall: AHEAD, counter: 3)

      assert_equal Timestamp.new(wall: AHEAD, counter: 4), issued
      assert_equal Timestamp.new(wall: AHEAD, counter: 5), clock.observe(wall: AHEAD, counter: 2)
    end

    def test_a_counter_with_nothing_left_refuses_to_reissue_its_last_stamp
      clock = HybridClock.new(replica: 1)
      last = (2**64) - 1
      full = Timestamp.new(wall: AHEAD, counter: last)

      assert_equal full, clock.observe(wall: AHEAD, counter: last - 1)
      assert_raises(Exhausted) { clock.tick(wall: AHEAD) }
      assert_raises(Exhausted) { clock.observe(wall: AHEAD, counter: 1) }
      assert_equal full, clock.now
      assert_equal Timestamp.new(wall: LATER, counter: 0), clock.tick(wall: LATER)
    end

    def test_a_blob_round_trips_the_last_timestamp_and_the_replica
      clock = HybridClock.new(replica: 7)
      clock.tick(wall: AHEAD)
      blob = clock.dump

      assert_equal Encoding::BINARY, blob.encoding
      assert_equal 25, blob.bytesize

      restored = HybridClock.load(blob)

      assert_equal [Timestamp.new(wall: AHEAD, counter: 0), 7], [restored.now, restored.replica]
      assert_equal Timestamp.new(wall: AHEAD, counter: 1), restored.tick(wall: 1.0)
    end

    def test_load_rejects_a_blob_it_did_not_write
      assert_raises(ArgumentError) { HybridClock.load('') }
      assert_raises(ArgumentError) { HybridClock.load(LogicalClock.new(replica: 1).dump) }
      assert_raises(ArgumentError) { HybridClock.load(HybridClock.new(replica: 1).dump.byteslice(0, 20)) }
    end
  end
end
