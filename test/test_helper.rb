# frozen_string_literal: true

$LOAD_PATH.unshift File.expand_path('../lib', __dir__)

require 'merge_machines'
require 'minitest/autorun'

module MergeMachines
  # A state whose merge has to keep its laws now that it is behind the binding.
  class StateTest < Minitest::Test
    # Bytes in a kibibyte, which is what `ps` reports resident memory in.
    KIB = 1024

    # A key big enough that one kept by mistake stands out in resident memory.
    REJECTED_KEY_BYTES = 2_000_000

    # Enough rejected merges that their keys dwarf the allocator's noise.
    REJECTED_MERGES = 30

    # A binding that kept the name of every key its core rejected grows by
    # everything that was rejected; one that kept none grows by the
    # allocator's noise. Half the rejected bytes tells the two apart.
    LEAK_SHARE = 0.5

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

    # Hands `full`, a state with no room left, one oversized key per round in a
    # source the block builds, and fails if the binding kept the keys its core
    # turned down. The names it keeps are reachable from nowhere else, so
    # resident memory is what has to be watched.
    def assert_drops_rejected_keys(full)
      keys = Array.new(REJECTED_MERGES) { |round| "#{round}-#{'k' * REJECTED_KEY_BYTES}" }
      GC.start
      before = resident_bytes
      keys.each do |key|
        source = yield(key)
        assert_raises(Full) { full.merge(source) }
        source = nil
        GC.start
      end

      assert_operator resident_bytes - before, :<, REJECTED_MERGES * REJECTED_KEY_BYTES * LEAK_SHARE
    end

    def resident_bytes
      `ps -o rss= -p #{Process.pid}`.to_i * KIB
    end
  end
end
