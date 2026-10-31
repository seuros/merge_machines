# frozen_string_literal: true

require 'test_helper'

module MergeMachines
  class LwwRegisterTest < StateTest
    def test_every_supported_stamp_is_monomorphised
      assert_equal :logical, LwwRegister.new.stamp_kind
      assert_equal :logical, LwwRegister.new(stamp: :logical).stamp_kind
      assert_equal :hybrid, LwwRegister.new(stamp: :hybrid).stamp_kind
    end

    def test_an_unsupported_stamp_is_rejected
      [:vector, :wall, 'logical', 7].each do |stamp|
        assert_raises(ArgumentError) { LwwRegister.new(stamp: stamp) }
      end
    end

    def test_merge_through_the_binding_keeps_the_laws
      assert_algebra(->(register) { register.value }) do
        left = LwwRegister.new
        right = LwwRegister.new
        left.set('left', replica: 1, stamp: 5)
        right.set('right', replica: 2, stamp: 5)
        [left, right]
      end
    end

    def test_merge_through_the_binding_keeps_the_laws_with_hybrid_stamps
      assert_algebra(->(register) { register.value }) do
        left = LwwRegister.new(stamp: :hybrid)
        right = LwwRegister.new(stamp: :hybrid)
        left.set('left', replica: 1, stamp: Timestamp.new(wall_ns: 1_500_000_000, counter: 2))
        right.set('right', replica: 2, stamp: Timestamp.new(wall_ns: 1_500_000_000, counter: 2))
        [left, right]
      end
    end

    def test_a_register_nobody_wrote_holds_nothing
      register = LwwRegister.new

      assert_nil register.value
      assert_equal [0, 0], [register.stamp, register.replica]
    end

    def test_a_stamp_from_before_the_current_one_changes_nothing
      register = LwwRegister.new
      register.set('late', replica: 1, stamp: 9)
      register.set('early', replica: 1, stamp: 4)

      assert_equal 'late', register.value
      assert_equal 9, register.stamp
    end

    def test_a_stale_write_loses_to_a_value_that_arrived_by_merge
      target = LwwRegister.new
      source = LwwRegister.new
      source.set('new', replica: 1, stamp: 100)
      target.merge(source)
      restored = LwwRegister.load(target.dump)

      [target, restored].each { |register| register.set('stale', replica: 2, stamp: 1) }

      assert_equal ['new', 100, 'new', 100],
                   [target.value, target.stamp, restored.value, restored.stamp]
    end

    def test_a_write_stamped_zero_by_replica_zero_is_still_a_write
      register = LwwRegister.new
      register.set('zero', replica: 0, stamp: 0)

      assert_equal ['zero', 0, 0], [register.value, register.stamp, register.replica]

      target = LwwRegister.new
      target.merge(register)

      assert_equal 'zero', target.value
      assert_equal 'zero', LwwRegister.load(target.dump).value
    end

    def test_a_blob_round_trips_the_value_as_a_string
      register = LwwRegister.new
      register.set('héllo', replica: 3, stamp: 7)
      blob = register.dump

      assert_equal Encoding::BINARY, blob.encoding
      assert_equal 27 + 8 + 'héllo'.bytesize, blob.bytesize

      restored = LwwRegister.load(blob)

      assert_equal ['héllo', 7, 3, :logical], [restored.value, restored.stamp, restored.replica,
                                               restored.stamp_kind]
    end

    def test_a_blob_round_trips_a_hybrid_stamp
      register = LwwRegister.new(stamp: :hybrid)
      register.set('v', replica: 1, stamp: Timestamp.new(wall_ns: 2_500_000_000, counter: 3))

      restored = LwwRegister.load(register.dump)

      assert_equal :hybrid, restored.stamp_kind
      assert_equal 'v', restored.value
      assert_equal Timestamp.new(wall_ns: 2_500_000_000, counter: 3), restored.stamp
    end

    def test_load_rejects_a_blob_it_did_not_write
      assert_raises(ArgumentError) { LwwRegister.load('') }
      assert_raises(ArgumentError) { LwwRegister.load(GSet.new(capacity: 64).dump) }
      assert_raises(ArgumentError) { LwwRegister.load(LwwRegister.new.dump.byteslice(0, 20)) }
      assert_raises(ArgumentError) { LwwRegister.load("\x71\x07\x00".b + ("\x00".b * 40)) }
    end

    def test_load_rejects_a_blob_whose_key_table_lost_the_held_value
      register = LwwRegister.new
      register.set('payload', replica: 1, stamp: 7)
      frame = register.dump.byteslice(0, 27)

      assert_raises(ArgumentError) { LwwRegister.load(frame) }

      hybrid = LwwRegister.new(stamp: :hybrid)
      hybrid.set('payload', replica: 1, stamp: Timestamp.new(wall_ns: 1_000_000_000, counter: 0))

      assert_raises(ArgumentError) { LwwRegister.load(hybrid.dump.byteslice(0, 35)) }
    end

    def test_load_rejects_a_hybrid_blob_cut_to_a_logical_frames_length
      hybrid = LwwRegister.new(stamp: :hybrid)
      hybrid.set('v', replica: 1, stamp: Timestamp.new(wall_ns: 1_000_000_000, counter: 0))

      assert_raises(ArgumentError) { LwwRegister.load(hybrid.dump.byteslice(0, 27)) }
    end

    def test_merge_rejects_a_register_stamped_by_the_other_clock
      assert_raises(ArgumentError) { LwwRegister.new.merge(LwwRegister.new(stamp: :hybrid)) }
      assert_raises(ArgumentError) { LwwRegister.new(stamp: :hybrid).merge(LwwRegister.new) }
    end

    def test_a_value_that_is_not_utf_8_is_rejected
      assert_raises(EncodingError) { LwwRegister.new.set("\xff\xfe".b, replica: 1, stamp: 1) }
    end
  end
end
