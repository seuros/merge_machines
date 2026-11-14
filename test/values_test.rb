# frozen_string_literal: true

require 'test_helper'

module MergeMachines
  # Every Data value is checked by the core's constructor, built by hand or
  # returned by the binding alike.
  class ValuesTest < Minitest::Test
    MAX = (2**64) - 1

    def test_values_hold_unsigned_64_bit_integers
      assert_equal [MAX, 0], Timestamp.new(MAX, 0).deconstruct
      assert_equal Tag.new(1, 2), Tag.new(replica: 1, counter: 2)
      assert_equal 5, Entry.new(1, 5).count

      [-> { Timestamp.new(-1, 0) }, -> { Tag.new(1, MAX + 1) }, -> { Entry.new(-3, 1) }].each do |build|
        assert_raises(RangeError) { build.call }
      end
      [-> { Timestamp.new(1.0, 0) }, -> { Tag.new('1', 2) }, -> { Entry.new(1, nil) }].each do |build|
        assert_raises(TypeError) { build.call }
      end
    end

    def test_a_tagged_add_needs_a_tag
      assert_equal Tag.new(1, 1), Tagged.new(9, Tag.new(1, 1)).tag
      assert_raises(TypeError) { Tagged.new(9, [1, 1]) }
      assert_raises(RangeError) { Tagged.new(-9, Tag.new(1, 1)) }
    end

    # A subclass overriding `initialize` without `super`, `allocate`, or a
    # call to the hook itself would skip the core's check.
    def test_values_are_final_and_their_hooks_private
      [Timestamp, Entry, Tag, Tagged].each do |value|
        assert_raises(TypeError) { Class.new(value) { def initialize(**) = nil } }
        assert_raises(NoMethodError) { value.allocate }
      end
      assert_raises(RangeError) { Tag.new(1, 1).with(counter: -1) }
      assert_raises(NoMethodError) { MergeMachines.__tag(1, 1) }
    end
  end
end
