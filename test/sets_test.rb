# frozen_string_literal: true

require 'test_helper'

module MergeMachines
  class SetsTest < StateTest
    def test_a_gset_answers_every_insert_by_name
      set = GSet.new(64)

      assert_equal :inserted, set.insert(3)
      assert_unchanged(set, :duplicate) { set.insert(3) }
      assert_equal [[3], 1, false], [set.to_a, set.size, set.empty?]
      assert set.include?(3)
      refute set.include?(4)
    end

    def test_a_full_gset_refuses_and_a_refused_merge_lands_nothing
      set = GSet.new(64)
      64.times { |element| set.insert(element) }
      other = GSet.new(64)
      other.insert(100)

      assert_unchanged(set, :full) { set.insert(64) }
      assert_unchanged(set, :full) { set.merge(other) }
      assert_equal :duplicate, set.insert(0)
    end

    def test_gset_merges_obey_the_laws
      assert_algebra(->(set) { set.to_a.sort }) do
        left = GSet.new(64)
        right = GSet.new(64)
        [1, 2].each { |element| left.insert(element) }
        [2, 3].each { |element| right.insert(element) }
        [left, right]
      end
    end

    def test_a_concurrent_add_survives_a_remove_that_never_saw_it
      shared = OrSet.new(64)
      assert_equal :added, shared.add(1, Tag.new(1, 1))
      remover = OrSet.decode(shared.encoded)
      adder = OrSet.decode(shared.encoded)

      assert_equal :removed, remover.remove(1)
      assert_equal :added, adder.add(1, Tag.new(2, 1))
      assert_equal :merged, remover.merge(adder)

      assert remover.include?(1)
      assert_equal [Tag.new(1, 1)], remover.removes
      assert_equal [Tagged.new(1, Tag.new(1, 1)), Tagged.new(1, Tag.new(2, 1))],
                   remover.adds.sort_by { |add| add.tag.replica }
    end

    def test_orset_repeats_and_refusals_change_nothing
      set = OrSet.new(64)
      set.add(1, Tag.new(1, 1))
      set.remove(1)

      assert_unchanged(set, :duplicate) { set.add(1, Tag.new(1, 1)) }
      assert_unchanged(set, :mismatch) { set.add(2, Tag.new(1, 1)) }
      assert_unchanged(set, :not_held) { set.remove(1) }
      assert_unchanged(set, :not_held) { set.remove(9) }

      other = OrSet.new(64)
      other.add(2, Tag.new(1, 1))
      assert_unchanged(set, :mismatch) { set.merge(other) }
    end

    def test_a_full_orset_refuses_and_a_refused_merge_lands_no_tombstone
      set = OrSet.new(64)
      64.times { |counter| set.add(counter, Tag.new(1, counter)) }
      other = OrSet.decode(set.encoded)
      other.remove(0)
      fresh = OrSet.new(64)
      fresh.add(500, Tag.new(2, 1))
      other.merge(OrSet.new(64))

      assert_unchanged(set, :full) { set.add(64, Tag.new(1, 64)) }
      crowded = OrSet.new(64)
      64.times { |counter| crowded.add(counter, Tag.new(3, counter)) }
      assert_unchanged(crowded, :full) { crowded.merge(other) }
      assert crowded.include?(0)
    end

    def test_orset_merges_obey_the_laws
      assert_algebra(->(set) { [set.to_a.sort, set.removes.size] }) do
        left = OrSet.new(64)
        right = OrSet.new(64)
        left.add(1, Tag.new(1, 1))
        left.remove(1)
        right.add(1, Tag.new(1, 1))
        right.add(2, Tag.new(2, 1))
        [left, right]
      end
    end

    def test_frames_round_trip_and_bad_ones_raise
      set = OrSet.new(256)
      set.add(5, Tag.new(1, 1))
      set.remove(5)

      assert_equal set, OrSet.decode(set.encoded)
      assert_equal 256, OrSet.decode(set.encoded).capacity
      assert_raises(RangeError) { OrSet.decode(GSet.new(256).encoded) }
      assert_raises(RangeError) { GSet.decode('x') }
      assert_raises(RangeError) { GSet.new(65) }
    end

    def test_arguments_are_checked_before_anything_moves
      set = OrSet.new(64)

      assert_raises(TypeError) { set.add(1, [1, 1]) }
      assert_raises(TypeError) { set.add('1', Tag.new(1, 1)) }
      assert_raises(RangeError) { set.add(-1, Tag.new(1, 1)) }
      assert_raises(TypeError) { set.merge(OrSet.new(256)) }
      assert_raises(TypeError) { GSet.new(64).merge(set) }
      assert_empty set
    end
  end
end
