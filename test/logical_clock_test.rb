# frozen_string_literal: true

require 'test_helper'

module MergeMachines
  class LogicalClockTest < Minitest::Test
    def test_a_clock_needs_the_replica_it_stamps_for
      assert_equal 7, LogicalClock.new(replica: 7).replica
      assert_raises(ArgumentError) { LogicalClock.new }
    end

    def test_a_receive_is_stamped_after_the_send_it_answers
      clock = LogicalClock.new(replica: 1)

      assert_equal 1, clock.tick
      assert_equal 43, clock.observe(42)
      assert_equal 43, clock.now
      assert_equal 44, clock.observe(2)
    end

    def test_a_blob_round_trips_the_counter_and_the_replica
      clock = LogicalClock.new(replica: 7)
      clock.observe(41)
      blob = clock.dump

      assert_equal Encoding::BINARY, blob.encoding
      assert_equal 17, blob.bytesize

      restored = LogicalClock.load(blob)

      assert_equal [42, 7], [restored.now, restored.replica]
      assert_equal 43, restored.tick
    end

    def test_load_rejects_a_blob_it_did_not_write
      assert_raises(ArgumentError) { LogicalClock.load('') }
      assert_raises(ArgumentError) { LogicalClock.load(HybridClock.new(replica: 1).dump) }
      assert_raises(ArgumentError) { LogicalClock.load(LogicalClock.new(replica: 1).dump.byteslice(0, 12)) }
    end
  end
end
