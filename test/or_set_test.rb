# frozen_string_literal: true

require 'test_helper'

module MergeMachines
  class OrSetTest < StateTest
    # Bytes a 64-bit field takes in a blob.
    FIELD_BYTES = 8

    # The frame header: the tag, the capacity, how many adds and how many
    # tombstones the frame carries.
    HEADER_BYTES = 7

    # Where the first add record's counter sits: after the header, the element
    # and the tag's replica.
    COUNTER_AT = HEADER_BYTES + (2 * FIELD_BYTES)

    def test_every_supported_capacity_is_monomorphised
      [64, 256, 1024].each do |capacity|
        assert_equal capacity, OrSet.new(capacity: capacity).capacity
      end
    end

    def test_an_unsupported_capacity_is_rejected
      [1, 32, 128, 512, 2048].each do |capacity|
        assert_raises(ArgumentError) { OrSet.new(capacity: capacity) }
      end
      assert_raises(ArgumentError) { OrSet.new }
    end

    def test_merge_through_the_binding_keeps_the_laws
      assert_algebra(->(set) { set.members.sort }) do
        left = OrSet.new(capacity: 64)
        right = OrSet.new(capacity: 64)
        %w[a b].each { |key| left.add(key, replica: 1) }
        left.remove('a')
        %w[a c].each { |key| right.add(key, replica: 2) }
        [left, right]
      end
    end

    def test_a_remove_only_takes_out_the_adds_it_has_seen
      shared = OrSet.new(capacity: 64)
      shared.add('k', replica: 1)
      remover = OrSet.load(shared.dump)
      adder = OrSet.load(shared.dump)

      remover.remove('k')
      adder.add('k', replica: 2)

      refute remover.include?('k')
      assert remover.merge(adder).include?('k')
    end

    def test_re_adding_a_removed_key_brings_it_back
      set = OrSet.new(capacity: 64)
      set.add('k', replica: 1)
      set.remove('k')

      refute set.include?('k')

      set.add('k', replica: 1)

      assert set.include?('k')
      assert_equal %w[k], set.members
    end

    def test_a_blob_round_trips_the_keys_as_strings
      set = OrSet.new(capacity: 64)
      %w[a-1 héllo gone].each { |key| set.add(key, replica: 1) }
      set.remove('gone')
      blob = set.dump

      assert_equal Encoding::BINARY, blob.encoding
      assert_equal 2_567, OrSet.new(capacity: 64).dump.bytesize

      restored = OrSet.load(blob)

      assert_equal [64, 2], [restored.capacity, restored.size]
      assert_equal %w[a-1 héllo], restored.members.sort
      refute restored.include?('gone')
    end

    def test_load_rejects_a_blob_it_did_not_write
      assert_raises(ArgumentError) { OrSet.load('') }
      assert_raises(ArgumentError) { OrSet.load(GSet.new(capacity: 1024).dump) }
      assert_raises(ArgumentError) { OrSet.load(OrSet.new(capacity: 64).dump.byteslice(0, 100)) }
      assert_raises(ArgumentError) { OrSet.load("\x82\x80\x00".b + ("\x00".b * 6_000)) }
    end

    def test_load_rejects_a_blob_whose_keys_went_missing
      set = OrSet.new(capacity: 64)
      set.add('k', replica: 1)

      assert_raises(ArgumentError) { OrSet.load(set.dump.byteslice(0, 2_567)) }
    end

    def test_merge_rejects_another_capacity
      assert_raises(ArgumentError) { OrSet.new(capacity: 64).merge(OrSet.new(capacity: 256)) }
    end

    def test_a_key_with_no_slot_left_reports_the_set_full
      set = OrSet.new(capacity: 64)
      64.times { |index| set.add("k-#{index}", replica: 1) }

      assert_raises(Full) { set.add('one-too-many', replica: 1) }
      assert_equal 64, set.size
    end

    def test_a_merge_that_overran_capacity_names_what_it_took_and_forgets_the_rest
      target = OrSet.new(capacity: 64)
      62.times { |index| target.add("base#{index}", replica: 1) }
      source = OrSet.new(capacity: 64)
      10.times { |index| source.add("extra#{index}", replica: 2) }

      assert_raises(Full) { target.merge(source) }
      assert_equal 64, target.members.size
      assert_equal 64, OrSet.load(target.dump).members.size

      assert_drops_rejected_keys(target) do |key|
        OrSet.new(capacity: 64).tap { |rejected| rejected.add(key, replica: 3) }
      end
    end

    def test_a_replica_that_ran_out_of_counters_reports_it_instead_of_dropping_the_key
      set = OrSet.new(capacity: 64)
      set.add('a', replica: 1)
      blob = set.dump
      blob[COUNTER_AT, FIELD_BYTES] = [(2**64) - 1].pack('Q<')
      saturated = OrSet.load(blob)

      assert_raises(Exhausted) { saturated.add('b', replica: 1) }
      assert_equal %w[a], saturated.members
    end

    def test_a_key_that_is_not_utf_8_is_rejected
      assert_raises(EncodingError) { OrSet.new(capacity: 64).add("\xff\xfe".b, replica: 1) }
    end
  end
end
