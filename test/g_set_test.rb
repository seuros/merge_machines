# frozen_string_literal: true

require 'test_helper'

module MergeMachines
  class GSetTest < StateTest
    def test_every_supported_capacity_is_monomorphised
      [64, 256, 1024].each do |capacity|
        assert_equal capacity, GSet.new(capacity: capacity).capacity
      end
    end

    def test_an_unsupported_capacity_is_rejected
      [1, 32, 128, 512, 2048].each do |capacity|
        assert_raises(ArgumentError) { GSet.new(capacity: capacity) }
      end
      assert_raises(ArgumentError) { GSet.new }
    end

    def test_merge_through_the_binding_keeps_the_laws
      assert_algebra(->(set) { set.members.sort }) do
        left = GSet.new(capacity: 64)
        right = GSet.new(capacity: 64)
        %w[a b c].each { |key| left.add(key) }
        %w[c d].each { |key| right.add(key) }
        [left, right]
      end
    end

    def test_adding_the_same_key_twice_changes_nothing
      set = GSet.new(capacity: 64)
      2.times { set.add('k') }

      assert_equal %w[k], set.members
      assert_equal 1, set.size
      refute_predicate set, :empty?
      refute set.include?('j')
    end

    def test_a_blob_round_trips_the_keys_as_strings
      set = GSet.new(capacity: 64)
      %w[a-1 b-2 héllo].each { |key| set.add(key) }
      blob = set.dump

      assert_equal Encoding::BINARY, blob.encoding
      assert_equal 517, GSet.new(capacity: 64).dump.bytesize

      restored = GSet.load(blob)

      assert_equal [64, 3], [restored.capacity, restored.size]
      assert_equal %w[a-1 b-2 héllo], restored.members.sort
      assert restored.include?('héllo')
    end

    def test_load_rejects_a_blob_it_did_not_write
      assert_raises(ArgumentError) { GSet.load('') }
      assert_raises(ArgumentError) { GSet.load(OrSet.new(capacity: 64).dump) }
      assert_raises(ArgumentError) { GSet.load(GSet.new(capacity: 64).dump.byteslice(0, 100)) }
      assert_raises(ArgumentError) { GSet.load("\x81\x80\x00".b + ("\x00".b * 2_000)) }
    end

    def test_load_rejects_a_blob_whose_keys_went_missing
      set = GSet.new(capacity: 64)
      set.add('k')

      assert_raises(ArgumentError) { GSet.load(set.dump.byteslice(0, 517)) }
    end

    def test_merge_rejects_another_capacity
      assert_raises(ArgumentError) { GSet.new(capacity: 64).merge(GSet.new(capacity: 256)) }
    end

    def test_a_key_with_no_slot_left_reports_the_set_full
      set = GSet.new(capacity: 64)
      64.times { |index| set.add("k-#{index}") }

      assert_raises(Full) { set.add('one-too-many') }
      assert_equal 64, set.size
    end

    def test_a_key_that_is_not_utf_8_is_rejected
      assert_raises(EncodingError) { GSet.new(capacity: 64).add("\xff\xfe".b) }
    end
  end
end
