# frozen_string_literal: true

require 'test_helper'

module MergeMachines
  # What the native boundary refuses before the core is called: a frozen
  # receiver, and a value whose class is not exactly the one it claims.
  class BoundaryTest < Minitest::Test
    def test_a_frozen_state_refuses_every_mutating_call
      calls = {
        GCounter.new(4) => [->(s) { s.increment(1, 1) }, ->(s) { s.merge(GCounter.new(4)) }],
        PNCounter.new(4) => [->(s) { s.increment(1, 1) }, ->(s) { s.decrement(1, 1) },
                             ->(s) { s.merge(PNCounter.new(4)) }],
        GSet.new(64) => [->(s) { s.insert(1) }, ->(s) { s.merge(GSet.new(64)) }],
        OrSet.new(64) => [->(s) { s.add(1, Tag.new(1, 1)) }, ->(s) { s.remove(1) },
                          ->(s) { s.merge(OrSet.new(64)) }],
        LwwRegister.new(1, 5, 2) => [->(s) { s.set(2, 6, 2) }, ->(s) { s.merge(LwwRegister.new(3, 9, 1)) }]
      }
      calls.each do |state, mutations|
        before = state.encoded
        state.freeze
        mutations.each { |mutate| assert_raises(FrozenError) { mutate.call(state) } }
        assert_equal before, state.encoded
      end
    end

    # Sealing stops accidents, not reflection: Ruby binds a named subclass's
    # constant before `inherited` raises, so the class leaks and can be
    # reopened. Both forms raise TypeError all the same.
    def test_values_are_sealed_and_a_named_subclass_leaks_its_constant
      [Timestamp, Entry, Tag, Tagged].each do |value|
        assert_raises(TypeError) { Class.new(value) }
      end
      assert_raises(TypeError) do
        self.class.module_eval('class LeakedTag < MergeMachines::Tag; end', __FILE__, __LINE__)
      end
      assert self.class.const_defined?(:LeakedTag, false), 'the constant is bound before inherited raises'
    end

    # A forged value reaches no core: a leaked subclass whose instances skip
    # the check, or a Marshal frame with a field edited, raises.
    def test_a_forged_value_is_refused_at_the_boundary
      stamp = forged(Timestamp, wall_ns: 1, counter: 2)
      tag = forged(Tag, replica: 1, counter: 1)

      assert_kind_of Timestamp, stamp
      assert_raises(TypeError) { HybridClock.new.observe(5, stamp) }
      assert_raises(TypeError) { LwwRegister.new(1, stamp, 2) }
      assert_raises(TypeError) { OrSet.new(64).add(1, tag) }
      assert_raises(TypeError) { Tagged.new(1, tag) }

      negative = Marshal.load(Marshal.dump(Timestamp.new(5, 7)).sub("i\x0A".b, "i\xFA".b))
      assert_equal(-1, negative.wall_ns)
      clock = HybridClock.new
      assert_raises(RangeError) { clock.observe(1, negative) }
      assert_equal Timestamp.new(0, 0), clock.last
    end

    # The classes are resolved at init and pinned, so a compaction moves none
    # of them: every path that builds or checks a value still works after it.
    def test_every_value_path_survives_gc_compact
      skip 'GC.compact is not available' unless GC.respond_to?(:compact)
      GC.compact
      clock = HybridClock.new
      assert_equal Timestamp.new(5, 0), clock.tick(5)
      assert_equal Timestamp.new(9, 1), clock.observe(1, Timestamp.new(9, 0))
      assert_equal Timestamp.new(9, 1), clock.last
      counter = GCounter.new(4)
      counter.increment(1, 2)
      assert_equal [Entry.new(1, 2)], counter.entries
      set = OrSet.new(64)
      set.add(3, Tag.new(1, 1))
      set.remove(3)
      assert_equal [[Tagged.new(3, Tag.new(1, 1))], [Tag.new(1, 1)]], [set.adds, set.removes]
      assert_equal Timestamp.new(2, 0), LwwRegister.new(1, Timestamp.new(2, 0), 1).stamp
    end

    private

    # An instance of a leaked subclass of `value` that never ran its check.
    def forged(value, **fields)
      name = "Forged#{value.name.split('::').last}"
      unless self.class.const_defined?(name, false)
        assert_raises(TypeError) { self.class.module_eval("class #{name} < #{value}; end", __FILE__, __LINE__) }
        self.class.const_get(name).class_eval do
          def initialize(**fields) = Data.instance_method(:initialize).bind_call(self, **fields)
        end
      end
      self.class.const_get(name).new(**fields)
    end
  end
end
