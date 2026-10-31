# frozen_string_literal: true

require 'test_helper'

module MergeMachines
  class PNCounterTest < StateTest
    def test_every_supported_replica_count_is_monomorphised
      [4, 8, 16, 32].each do |replicas|
        assert_equal replicas, PNCounter.new(replicas: replicas).replicas
      end
    end

    def test_an_unsupported_replica_count_is_rejected
      [1, 2, 6, 64, 1024].each do |replicas|
        assert_raises(ArgumentError) { PNCounter.new(replicas: replicas) }
      end
      assert_raises(ArgumentError) { PNCounter.new }
    end

    def test_merge_through_the_binding_keeps_the_laws
      assert_algebra(->(counter) { counter.value }) do
        left = PNCounter.new(replicas: 8)
        right = PNCounter.new(replicas: 8)
        left.increment(replica: 1, by: 10)
        left.decrement(replica: 2, by: 3)
        right.decrement(replica: 1, by: 4)
        right.increment(replica: 3, by: 6)
        [left, right]
      end
    end

    def test_the_value_goes_negative
      counter = PNCounter.new(replicas: 4)
      counter.decrement(replica: 1, by: 5)

      assert_equal(-5, counter.value)
      assert_equal [0, 5], [counter.added, counter.removed]
    end

    def test_a_blob_round_trips_both_directions
      counter = PNCounter.new(replicas: 4)
      counter.increment(replica: 1, by: 10)
      counter.decrement(replica: 2, by: 4)
      blob = counter.dump

      assert_equal 135, blob.bytesize

      restored = PNCounter.load(blob)

      assert_equal [4, 6, 10, 4], [restored.replicas, restored.value, restored.added, restored.removed]
    end

    def test_load_rejects_a_blob_it_did_not_write
      assert_raises(ArgumentError) { PNCounter.load('') }
      assert_raises(ArgumentError) { PNCounter.load(GCounter.new(replicas: 32).dump) }
      assert_raises(ArgumentError) { PNCounter.load(PNCounter.new(replicas: 4).dump.byteslice(0, 100)) }
      assert_raises(ArgumentError) { PNCounter.load("\x62\x06\x00".b + ("\x00".b * 400)) }
    end

    def test_merge_rejects_another_replica_count
      assert_raises(ArgumentError) { PNCounter.new(replicas: 4).merge(PNCounter.new(replicas: 8)) }
    end

    def test_a_replica_without_a_slot_reports_the_state_full
      counter = PNCounter.new(replicas: 4)
      (1..4).each { |replica| counter.decrement(replica: replica) }

      assert_raises(Full) { counter.decrement(replica: 5) }
      assert_equal(-4, counter.value)
    end
  end
end
