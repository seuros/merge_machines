# frozen_string_literal: true

$LOAD_PATH.unshift File.expand_path('../lib', __dir__)

require 'merge_machines'
require 'minitest/autorun'

module MergeMachines
  class StateTest < Minitest::Test
    # Merging must not depend on the order two states arrive in, or on how many
    # times one of them arrives. `pair` hands back two fresh replicas and
    # `reading` is what they have to agree on.
    def assert_algebra(reading, &pair)
      left, right = pair.call
      assert_equal :merged, left.merge(right)
      forwards = reading.call(left)

      left, right = pair.call
      assert_equal :merged, right.merge(left)
      backwards = reading.call(right)

      left, right = pair.call
      2.times { assert_equal :merged, left.merge(right) }
      redelivered = reading.call(left)

      assert_equal forwards, backwards
      assert_equal forwards, redelivered
    end

    # Runs the block, which must answer `answer`, and checks the state's frame
    # did not move: a refusal or an honoured repeat changes nothing.
    def assert_unchanged(state, answer)
      before = state.encoded
      assert_equal answer, yield
      assert_equal before, state.encoded
    end
  end
end
