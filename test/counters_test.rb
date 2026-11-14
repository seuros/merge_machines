# frozen_string_literal: true

require 'test_helper'

module MergeMachines
  class CountersTest < StateTest
    MAX = (2**64) - 1

    def test_a_gcounter_sums_every_replicas_count
      counter = GCounter.new(4)

      assert_equal :incremented, counter.increment(7, 2)
      assert_equal :incremented, counter.increment(9, 5)
      assert_equal :incremented, counter.increment(7, 1)

      assert_equal [3, 5, 0, 8], [counter.count(7), counter.count(9), counter.count(11), counter.value]
      assert_equal [Entry.new(7, 3), Entry.new(9, 5)], counter.entries.sort_by(&:replica)
      assert_equal 4, counter.replicas
    end

    def test_gcounter_merges_obey_the_laws
      assert_algebra(->(counter) { [counter.value, counter.count(1)] }) do
        left = GCounter.new(8)
        right = GCounter.new(8)
        left.increment(1, 4)
        right.increment(2, 6)
        right.increment(1, 1)
        [left, right]
      end
    end

    def test_a_refused_increment_changes_nothing
      counter = GCounter.new(4)
      4.times { |replica| counter.increment(replica, 1) }

      assert_unchanged(counter, :full) { counter.increment(9, 1) }
      assert_equal :incremented, counter.increment(0, MAX - 1)
      assert_unchanged(counter, :overflow) { counter.increment(0, 1) }
      assert_unchanged(counter, :incremented) { counter.increment(9, 0) }
    end

    def test_a_refused_merge_lands_nothing
      left = GCounter.new(4)
      left.increment(1, 1)
      crowded = GCounter.new(4)
      [2, 3, 4, 5].each { |replica| crowded.increment(replica, 1) }

      assert_unchanged(left, :full) { left.merge(crowded) }
    end

    def test_a_pncounter_goes_below_zero_and_refuses_whole
      left = PNCounter.new(4)
      right = PNCounter.new(4)
      assert_equal :incremented, left.increment(1, 10)
      assert_equal :decremented, right.decrement(2, 14)
      assert_equal :merged, left.merge(right)

      assert_equal [-4, 10, 14], [left.value, left.added, left.removed]

      crowded = PNCounter.new(4)
      [3, 4, 5, 6].each { |replica| crowded.decrement(replica, 1) }
      assert_unchanged(left, :full) { left.merge(crowded) }
      assert_unchanged(left, :overflow) { left.increment(1, MAX) }
    end

    def test_pncounter_merges_obey_the_laws
      assert_algebra(->(counter) { [counter.value, counter.added, counter.removed] }) do
        left = PNCounter.new(4)
        right = PNCounter.new(4)
        left.increment(1, 4)
        right.decrement(2, 6)
        [left, right]
      end
    end

    def test_a_frame_round_trips_and_a_bad_one_raises
      counter = GCounter.new(16)
      counter.increment(3, 9)

      assert_equal counter, GCounter.decode(counter.encoded)
      assert_equal 16, GCounter.decode(counter.encoded).replicas
      assert_raises(RangeError) { GCounter.decode('') }
      assert_raises(RangeError) { GCounter.decode(PNCounter.new(16).encoded) }
      error = assert_raises(RangeError) { GCounter.decode(GSet.new(64).encoded.byteslice(0, 3)) }
      assert_match(/64 is not supported|truncated|wrong_structure/, error.message)
    end

    def test_shapes_and_arguments_are_checked
      assert_raises(RangeError) { GCounter.new(5) }
      assert_raises(TypeError) { GCounter.new(4.0) }
      counter = GCounter.new(4)

      assert_raises(TypeError) { counter.increment(1.9, 1) }
      assert_raises(RangeError) { counter.increment(-1, 1) }
      assert_raises(RangeError) { counter.increment(1, 2**64) }
      assert_raises(TypeError) { counter.merge(GCounter.new(8)) }
      assert_raises(TypeError) { counter.merge(PNCounter.new(4)) }
      assert_equal 0, counter.value
    end
  end
end
