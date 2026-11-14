# frozen_string_literal: true

require 'test_helper'

module MergeMachines
  class ClocksTest < StateTest
    MAX = (2**64) - 1

    def test_a_logical_clock_stamps_receipt_after_the_send
      sender = LogicalClock.new
      receiver = LogicalClock.new
      sent_at = sender.tick
      receiver.tick
      receiver.tick

      assert_equal 3, receiver.observe(sent_at)
      assert_equal 100, receiver.observe(99)
      assert_equal 100, receiver.now
    end

    def test_an_exhausted_logical_clock_refuses_without_moving
      clock = LogicalClock.new
      assert_equal MAX, clock.observe(MAX - 1)

      assert_unchanged(clock, :exhausted) { clock.tick }
      assert_unchanged(clock, :exhausted) { clock.observe(3) }
      assert_equal clock, LogicalClock.decode(clock.encoded)
      assert_raises(RangeError) { LogicalClock.decode('') }
      assert_raises(RangeError) { clock.observe(-1) }
    end

    def test_a_hybrid_clock_never_goes_backwards_with_the_callers_reading
      clock = HybridClock.new
      first = clock.tick(10)
      second = clock.tick(10)
      third = clock.tick(4)

      assert_equal [Timestamp.new(10, 0), Timestamp.new(10, 1), Timestamp.new(10, 2)], [first, second, third]
      assert_equal Timestamp.new(11, 0), clock.tick(11)
      assert_equal Timestamp.new(20, 6), clock.observe(5, Timestamp.new(20, 5))
      assert_equal Timestamp.new(20, 6), clock.last
    end

    def test_an_exhausted_hybrid_clock_refuses_without_moving
      clock = HybridClock.new
      assert_equal Timestamp.new(7, MAX), clock.observe(0, Timestamp.new(7, MAX - 1))

      assert_unchanged(clock, :exhausted) { clock.tick(7) }
      assert_unchanged(clock, :exhausted) { clock.observe(1, Timestamp.new(7, 0)) }
      assert_equal clock, HybridClock.decode(clock.encoded)
    end

    def test_arguments_are_checked_before_the_clock_moves
      clock = HybridClock.new

      assert_raises(TypeError) { clock.tick(1.5) }
      assert_raises(TypeError) { clock.observe(1, [1, 2]) }
      assert_equal Timestamp.new(0, 0), clock.last
    end

    # The Timestamp is built from a copy of the clock that is kept only once
    # the value exists: a build that raises leaves the clock unmoved, and one
    # that stamps through the clock itself raises RuntimeError with the outer
    # stamp not applied.
    def test_a_stamp_that_cannot_be_built_moves_nothing
      clock = HybridClock.new
      seen = Timestamp.new(3, 0)
      redefining(Timestamp, ->(*) { raise IOError }) do
        assert_unchanged(clock, IOError) { assert_raises(IOError) { clock.tick(5) }.class }
        assert_unchanged(clock, IOError) { assert_raises(IOError) { clock.observe(5, seen) }.class }
      end
      calls = 0
      reentrant = ->(original, *args) { ((calls += 1) == 1 ? clock.tick(9) : nil).then { original.call(*args) } }
      redefining(Timestamp, reentrant) { assert_raises(RuntimeError) { clock.tick(5) } }
      assert_equal Timestamp.new(9, 0), clock.last
    end

    # A build that freezes the clock: FrozenError, and the stamp is not kept.
    def test_a_stamp_whose_build_freezes_the_clock_moves_nothing
      clock = HybridClock.new
      clock.tick(4)
      before = clock.encoded
      freezing = ->(original, *args) { clock.freeze.then { original.call(*args) } }
      redefining(Timestamp, freezing) { assert_raises(FrozenError) { clock.tick(5) } }
      assert_equal before, clock.encoded
    end

    def test_a_frozen_clock_refuses_every_stamp
      logical = LogicalClock.new.freeze
      hybrid = HybridClock.new.freeze

      assert_raises(FrozenError) { logical.tick }
      assert_raises(FrozenError) { logical.observe(3) }
      assert_raises(FrozenError) { hybrid.tick(1) }
      assert_raises(FrozenError) { hybrid.observe(1, Timestamp.new(2, 0)) }
      assert_equal [0, Timestamp.new(0, 0)], [logical.now, hybrid.last]
    end

    private

    def redefining(klass, body)
      singleton = klass.singleton_class
      singleton.alias_method(:__original_new, :new)
      singleton.remove_method(:new)
      original = klass.method(:__original_new)
      klass.define_singleton_method(:new) { |*args| body.call(original, *args) }
      yield
    ensure
      singleton.remove_method(:new)
      singleton.alias_method(:new, :__original_new)
      singleton.remove_method(:__original_new)
    end
  end
end
