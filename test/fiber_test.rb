# frozen_string_literal: true

require 'test_helper'

module MergeMachines
  # The largest shape of every state, built, encoded and decoded inside a
  # fiber's machine stack.
  class FiberTest < Minitest::Test
    def test_the_largest_shape_of_every_state_fits_a_fiber
      readings = Fiber.new do
        [largest_counter, largest_signed_counter, largest_set, largest_removable_set]
      end.resume

      assert_equal [3, -3, [7], [7]], readings
    end

    private

    def largest_counter
      counter = GCounter.new(32)
      counter.increment(31, 3)
      GCounter.decode(counter.encoded).value
    end

    def largest_signed_counter
      counter = PNCounter.new(32)
      counter.decrement(31, 3)
      PNCounter.decode(counter.encoded).value
    end

    def largest_set
      set = GSet.new(1024)
      set.insert(7)
      GSet.decode(set.encoded).to_a
    end

    def largest_removable_set
      set = OrSet.new(1024)
      set.add(7, Tag.new(1, 1))
      other = OrSet.new(1024)
      other.merge(set)
      OrSet.decode(other.encoded).to_a
    end
  end
end
