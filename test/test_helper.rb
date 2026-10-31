# frozen_string_literal: true

$LOAD_PATH.unshift File.expand_path('../lib', __dir__)

require 'merge_machines'
require 'minitest/autorun'

module MergeMachines
  # A state whose merge has to keep its laws now that it is behind the binding.
  class StateTest < Minitest::Test
    # Merging must not depend on the order two states arrive in, or on how many
    # times one of them arrives. `pair` hands back two fresh replicas and
    # `reading` is what they have to agree on.
    def assert_algebra(reading, &pair)
      left, right = pair.call
      forwards = reading.call(left.merge(right))

      left, right = pair.call
      backwards = reading.call(right.merge(left))

      left, right = pair.call
      redelivered = reading.call(left.merge(right).merge(right))

      assert_equal forwards, backwards
      assert_equal forwards, redelivered
    end
  end
end
