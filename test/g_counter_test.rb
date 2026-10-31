# frozen_string_literal: true

require 'test_helper'

module MergeMachines
  class GCounterTest < StateTest
    def test_every_supported_replica_count_is_monomorphised
      [4, 8, 16, 32].each do |replicas|
        assert_equal replicas, GCounter.new(replicas: replicas).replicas
      end
    end

    def test_an_unsupported_replica_count_is_rejected
      [1, 2, 6, 64, 1024].each do |replicas|
        assert_raises(ArgumentError) { GCounter.new(replicas: replicas) }
      end
      assert_raises(ArgumentError) { GCounter.new }
    end

    def test_merge_through_the_binding_keeps_the_laws
      assert_algebra(->(counter) { counter.value }) do
        left = GCounter.new(replicas: 8)
        right = GCounter.new(replicas: 8)
        left.increment(replica: 1, by: 3)
        left.increment(replica: 2, by: 1)
        right.increment(replica: 2, by: 9)
        right.increment(replica: 3, by: 4)
        [left, right]
      end
    end

    def test_an_increment_defaults_to_one
      counter = GCounter.new(replicas: 4)
      2.times { counter.increment(replica: 1) }

      assert_equal 2, counter.value
      assert_equal 2, counter.count(replica: 1)
      assert_equal 0, counter.count(replica: 2)
    end

    def test_a_blob_round_trips_every_replicas_count
      counter = GCounter.new(replicas: 8)
      counter.increment(replica: 1, by: 3)
      counter.increment(replica: 7, by: 4)
      blob = counter.dump

      assert_equal Encoding::BINARY, blob.encoding
      assert_equal 133, blob.bytesize

      restored = GCounter.load(blob)

      assert_equal [8, 7], [restored.replicas, restored.value]
      assert_equal [3, 4], [restored.count(replica: 1), restored.count(replica: 7)]
    end

    def test_load_rejects_a_blob_it_did_not_write
      assert_raises(ArgumentError) { GCounter.load('') }
      assert_raises(ArgumentError) { GCounter.load(PNCounter.new(replicas: 8).dump) }
      assert_raises(ArgumentError) { GCounter.load(GCounter.new(replicas: 8).dump.byteslice(0, 100)) }
      assert_raises(ArgumentError) { GCounter.load("\x61\x06\x00".b + ("\x00".b * 200)) }
    end

    def test_merge_rejects_another_replica_count
      assert_raises(ArgumentError) { GCounter.new(replicas: 4).merge(GCounter.new(replicas: 8)) }
    end

    def test_a_replica_without_a_slot_reports_the_state_full
      counter = GCounter.new(replicas: 4)
      (1..4).each { |replica| counter.increment(replica: replica) }

      assert_raises(Full) { counter.increment(replica: 5) }
      assert_equal 4, counter.value
    end
  end
end
