# frozen_string_literal: true

require 'test_helper'

module MergeMachines
  class LwwRegisterTest < StateTest
    def test_every_write_is_answered_by_name
      register = LwwRegister.new(1, 5, 2)

      assert_unchanged(register, :superseded) { register.set(7, 4, 9) }
      assert_unchanged(register, :superseded) { register.set(7, 5, 1) }
      assert_unchanged(register, :duplicate) { register.set(1, 5, 2) }
      assert_unchanged(register, :mismatch) { register.set(8, 5, 2) }
      assert_equal :written, register.set(8, 5, 3)
      assert_equal [8, 5, 3], [register.value, register.stamp, register.replica]
    end

    def test_merges_keep_the_last_write_and_refuse_two_values_under_one_stamp
      assert_algebra(->(register) { [register.value, register.stamp, register.replica] }) do
        [LwwRegister.new(1, 5, 2), LwwRegister.new(2, 5, 7)]
      end
      left = LwwRegister.new(1, 5, 2)

      assert_unchanged(left, :mismatch) { left.merge(LwwRegister.new(9, 5, 2)) }
      assert_unchanged(left, :merged) { left.merge(LwwRegister.new(9, 4, 2)) }
    end

    def test_a_hybrid_register_takes_timestamps_only
      register = LwwRegister.new(1, Timestamp.new(100, 0), 1)

      assert_equal :written, register.set(2, Timestamp.new(100, 1), 1)
      assert_equal Timestamp.new(100, 1), register.stamp
      assert_raises(TypeError) { register.set(3, 200, 1) }
      assert_raises(TypeError) { register.merge(LwwRegister.new(1, 5, 2)) }
      assert_raises(TypeError) { LwwRegister.new(1, 5, 2).set(3, Timestamp.new(1, 1), 1) }
      assert_raises(TypeError) { LwwRegister.new(1, 7.9, 2) }
      assert_equal 2, register.value
    end

    def test_frames_round_trip_both_kinds_and_bad_ones_raise
      logical = LwwRegister.new(1, 5, 2)
      hybrid = LwwRegister.new(1, Timestamp.new(9, 3), 2)

      assert_equal logical, LwwRegister.decode(logical.encoded)
      assert_equal hybrid, LwwRegister.decode(hybrid.encoded)
      assert_raises(RangeError) { LwwRegister.decode('') }
      assert_raises(RangeError) { LwwRegister.decode(GSet.new(64).encoded) }
    end
  end
end
