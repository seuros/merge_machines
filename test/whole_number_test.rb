# frozen_string_literal: true

require 'test_helper'

module MergeMachines
  class WholeNumberTest < StateTest
    def test_a_fractional_stamp_is_refused_rather_than_truncated_into_a_tie
      register = LwwRegister.new
      register.set('kept', replica: 1, stamp: 7)

      assert_raises(TypeError) { register.set('lost', replica: 1, stamp: 7.9) }
      assert_raises(TypeError) { register.set('lost', replica: 1.9, stamp: 8) }

      assert_equal 'kept', register.value
      assert_equal 7, register.stamp
    end
  end
end
