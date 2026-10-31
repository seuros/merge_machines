# frozen_string_literal: true

require 'test_helper'

module MergeMachines
  class HybridClockTest < Minitest::Test
    # Wall-clock readings in nanoseconds, both far enough ahead of any realtime
    # reading that the clock has to keep them.
    AHEAD = 1 << 61
    LATER = 1 << 62

    def test_a_clock_needs_the_replica_it_stamps_for
      assert_equal 7, HybridClock.new(replica: 7).replica
      assert_raises(ArgumentError) { HybridClock.new }
    end

    def test_a_reading_the_clock_has_already_passed_only_moves_the_counter
      clock = HybridClock.new(replica: 1)

      assert_equal Timestamp.new(wall_ns: AHEAD, counter: 0), clock.tick(wall_ns: AHEAD)
      assert_equal Timestamp.new(wall_ns: AHEAD, counter: 1), clock.tick(wall_ns: 1)
      assert_equal Timestamp.new(wall_ns: AHEAD, counter: 1), clock.now
      assert_equal Timestamp.new(wall_ns: LATER, counter: 0), clock.tick(wall_ns: LATER)
    end

    def test_a_tick_with_no_reading_takes_one_from_the_realtime_clock
      before = Process.clock_gettime(Process::CLOCK_REALTIME, :nanosecond)
      issued = HybridClock.new(replica: 1).tick
      after = Process.clock_gettime(Process::CLOCK_REALTIME, :nanosecond)

      assert_operator issued.wall_ns, :>=, before
      assert_operator issued.wall_ns, :<=, after
    end

    def test_two_readings_one_nanosecond_apart_stay_distinct
      clock = HybridClock.new(replica: 1)

      assert_equal Timestamp.new(wall_ns: AHEAD, counter: 0), clock.tick(wall_ns: AHEAD)
      assert_equal Timestamp.new(wall_ns: AHEAD + 1, counter: 0), clock.tick(wall_ns: AHEAD + 1)
    end

    def test_an_observed_timestamp_from_the_future_is_stamped_after_it
      clock = HybridClock.new(replica: 1)
      issued = clock.observe(wall_ns: AHEAD, counter: 3)

      assert_equal Timestamp.new(wall_ns: AHEAD, counter: 4), issued
      assert_equal Timestamp.new(wall_ns: AHEAD, counter: 5), clock.observe(wall_ns: AHEAD, counter: 2)
    end

    def test_a_counter_with_nothing_left_refuses_to_reissue_its_last_stamp
      clock = HybridClock.new(replica: 1)
      last = (2**64) - 1
      full = Timestamp.new(wall_ns: AHEAD, counter: last)

      assert_equal full, clock.observe(wall_ns: AHEAD, counter: last - 1)
      assert_raises(Exhausted) { clock.tick(wall_ns: AHEAD) }
      assert_raises(Exhausted) { clock.observe(wall_ns: AHEAD, counter: 1) }
      assert_equal full, clock.now
      assert_equal Timestamp.new(wall_ns: LATER, counter: 0), clock.tick(wall_ns: LATER)
    end

    def test_a_blob_round_trips_the_last_timestamp_and_the_replica
      clock = HybridClock.new(replica: 7)
      clock.tick(wall_ns: AHEAD)
      blob = clock.dump

      assert_equal Encoding::BINARY, blob.encoding
      assert_equal 25, blob.bytesize

      restored = HybridClock.load(blob)

      assert_equal [Timestamp.new(wall_ns: AHEAD, counter: 0), 7], [restored.now, restored.replica]
      assert_equal Timestamp.new(wall_ns: AHEAD, counter: 1), restored.tick(wall_ns: 1)
    end

    def test_load_rejects_a_blob_it_did_not_write
      assert_raises(ArgumentError) { HybridClock.load('') }
      assert_raises(ArgumentError) { HybridClock.load(LogicalClock.new(replica: 1).dump) }
      assert_raises(ArgumentError) { HybridClock.load(HybridClock.new(replica: 1).dump.byteslice(0, 20)) }
    end
  end
end
