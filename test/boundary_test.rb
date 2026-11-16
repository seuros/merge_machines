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

    # Frozen is checked before any argument is read: a bad argument to a
    # frozen state is FrozenError, not TypeError or RangeError.
    def test_a_frozen_state_raises_before_reading_arguments
      calls = {
        GCounter.new(4) => [->(s) { s.increment('1', 1) }, ->(s) { s.merge(nil) }],
        PNCounter.new(4) => [->(s) { s.increment(1, -1) }, ->(s) { s.decrement(1.5, 1) }],
        GSet.new(64) => [->(s) { s.insert(nil) }],
        OrSet.new(64) => [->(s) { s.add(1, [1, 1]) }, ->(s) { s.remove(-1) }],
        LwwRegister.new(1, 5, 2) => [->(s) { s.set(2, '6', 2) }],
        LogicalClock.new => [->(s) { s.observe(nil) }],
        HybridClock.new => [->(s) { s.tick(1.0) }, ->(s) { s.observe(1, :stamp) }]
      }
      calls.each do |state, mutations|
        state.freeze
        mutations.each { |mutate| assert_raises(FrozenError) { mutate.call(state) } }
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

    # A singleton class opened on a genuine value does not change what it is.
    def test_an_opened_singleton_class_is_still_the_gems_value
      stamp = Timestamp.new(9, 0).tap(&:singleton_class)
      assert_equal Timestamp.new(9, 1), HybridClock.new.observe(1, stamp)
      tag = Tag.new(1, 1).tap(&:singleton_class)
      assert_equal Tag.new(1, 1), Tagged.new(3, tag).tag
      set = OrSet.new(64)
      assert_equal :added, set.add(3, tag.clone)
      assert_equal Timestamp.new(2, 0), LwwRegister.new(1, Timestamp.new(2, 0).tap(&:singleton_class), 1).stamp
    end

    # Fields are read from the struct itself: a reader redefined on a genuine
    # value answers nothing to the binding.
    def test_a_redefined_reader_is_ignored
      readers(Timestamp, wall_ns: -> { 2**70 }, counter: -> { raise 'the binding ran a reader' }) do
        assert_equal Timestamp.new(9, 1), HybridClock.new.observe(1, Timestamp.new(9, 0))
      end
      set = OrSet.new(64)
      readers(Tag, replica: -> { -1 }) { assert_equal :added, set.add(3, Tag.new(1, 1)) }
      assert_equal [Tagged.new(3, Tag.new(1, 1))], set.adds
    end

    # A state class cannot have an instance of a subclass: `new` builds the
    # class itself and the allocator is gone, so the partner check, which
    # refuses any value not exactly the class, only ever meets the real one.
    def test_a_state_subclass_builds_the_state_itself
      [[GCounter, 4], [PNCounter, 4], [GSet, 64], [OrSet, 64]].each do |state, shape|
        sub = Class.new(state)
        assert_instance_of state, sub.new(shape)
        assert_raises(TypeError) { sub.allocate }
        assert_equal :merged, state.new(shape).merge(sub.new(shape))
      end
      assert_raises(TypeError) { GCounter.new(4).merge(Object.new) }
      refute_equal GCounter.new(4), Object.new
    end

    # A partner of the wrong class is refused with a message naming the
    # class it was.
    def test_a_wrong_partner_is_named_by_its_class
      error = assert_raises(TypeError) { GCounter.new(4).merge(PNCounter.new(4)) }
      assert_equal 'expected a MergeMachines::GCounter, got MergeMachines::PNCounter', error.message
      error = assert_raises(TypeError) { GSet.new(64).merge(5) }
      assert_equal 'expected a MergeMachines::GSet, got Integer', error.message
      error = assert_raises(TypeError) { LwwRegister.new(1, 5, 2).merge(nil) }
      assert_equal 'expected a MergeMachines::LwwRegister, got NilClass', error.message
      opened = PNCounter.new(4).tap { |counter| def counter.opened = true }
      error = assert_raises(TypeError) { GCounter.new(4).merge(opened) }
      assert_equal 'expected a MergeMachines::GCounter, got MergeMachines::PNCounter', error.message
    end

    # A frame is exactly a String: a subclass, an object answering `to_str`
    # and a String with a redefined `#bytes` change nothing about that. The
    # TypeError names the class received: the real class, never an opened
    # singleton nor what a redefined `#class` claims.
    def test_a_frame_is_exactly_a_string
      subclass = Class.new(String).new(GCounter.new(4).encoded)
      stringly = Object.new.tap { |o| o.define_singleton_method(:to_str) { GCounter.new(4).encoded } }
      posing = Object.new.tap { |o| def o.class = String }
      bad = { subclass => subclass.class.to_s, stringly => 'Object', posing => 'Object',
              frame: 'Symbol', 1 => 'Integer', nil => 'NilClass' }
      [GCounter.new(4), PNCounter.new(4), GSet.new(64), OrSet.new(64), LwwRegister.new(1, 5, 2),
       LogicalClock.new, HybridClock.new].each do |held|
        state = held.class
        frame = held.encoded
        bad.each do |value, received|
          error = assert_raises(TypeError) { state.decode(value) }
          assert_equal "frame must be a String, not #{received}", error.message
        end
        lying = frame.dup.tap { |f| f.define_singleton_method(:bytes) { raise 'the binding ran #bytes' } }
        assert_equal state.decode(frame), state.decode(lying)
      end
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

    # Runs the block with each reader of `value` redefined, then restores it.
    def readers(value, **bodies)
      bodies.each_key { |name| value.alias_method(:"__original_#{name}", name) }
      bodies.each { |name, body| value.define_method(name, &body) }
      yield
    ensure
      bodies.each_key do |name|
        value.alias_method(name, :"__original_#{name}")
        value.remove_method(:"__original_#{name}")
      end
    end

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
