# frozen_string_literal: true

require 'test_helper'

module MergeMachines
  # The largest shape of every state, built, dumped and loaded inside a fiber's
  # machine stack.
  class FiberTest < Minitest::Test
    def test_the_largest_shape_of_every_state_fits_a_fiber
      readings = Fiber.new do
        [
          largest_counter,
          largest_signed_counter,
          largest_set,
          largest_removable_set
        ]
      end.resume

      assert_equal [3, -3, %w[k], %w[k]], readings
    end

    private

    def largest_counter
      counter = GCounter.new(replicas: 32)
      counter.increment(replica: 31, by: 3)
      GCounter.load(counter.dump).value
    end

    def largest_signed_counter
      counter = PNCounter.new(replicas: 32)
      counter.decrement(replica: 31, by: 3)
      PNCounter.load(counter.dump).value
    end

    def largest_set
      set = GSet.new(capacity: 1024)
      set.add('k')
      GSet.load(set.dump).members
    end

    def largest_removable_set
      set = OrSet.new(capacity: 1024)
      set.add('k', replica: 1)
      OrSet.load(set.dump).members
    end
  end
end
