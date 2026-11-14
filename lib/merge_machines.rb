# frozen_string_literal: true

require_relative 'merge_machines/version'

module MergeMachines
  # The core's `HybridTimestamp`: a wall-clock reading in Integer nanoseconds
  # since the Unix epoch (an event-time stamp, not a monotonic instant) and the
  # counter that breaks its ties. Both are unsigned 64-bit Integers, checked by
  # `HybridTimestamp::new`: TypeError for a non-Integer, RangeError outside
  # 0..2**64-1.
  Timestamp = Data.define(:wall_ns, :counter) do
    def initialize(wall_ns:, counter:)
      MergeMachines.__send__(:__timestamp, wall_ns, counter)
      super
    end
  end

  # The core's `Entry`: one replica's count in a GCounter, checked by
  # `Entry::new` like a Timestamp.
  Entry = Data.define(:replica, :count) do
    def initialize(replica:, count:)
      MergeMachines.__send__(:__entry, replica, count)
      super
    end
  end

  # The core's `Tag`: the replica that made an OrSet add and that replica's
  # counter when it made it. The caller picks both; checked by `Tag::new`.
  Tag = Data.define(:replica, :counter) do
    def initialize(replica:, counter:)
      MergeMachines.__send__(:__tag, replica, counter)
      super
    end
  end

  # The core's `Tagged`: an OrSet element and the Tag it was added under.
  # TypeError unless `tag` is a Tag; checked by `Tagged::new`.
  Tagged = Data.define(:element, :tag) do
    def initialize(element:, tag:)
      MergeMachines.__send__(:__tagged, element, tag)
      super
    end
  end
end

# GCounter, PNCounter, LwwRegister, GSet, OrSet, LogicalClock and HybridClock
# are defined by the native extension; there is no pure-Ruby implementation to
# fall back to. It validates and builds the Data values above, so they come
# first.
require 'merge_machines_native/merge_machines_native'

module MergeMachines
  private_class_method :__timestamp, :__entry, :__tag, :__tagged

  # The values are final. `initialize` is where each validates, and a subclass
  # could override it, or `allocate` build an instance that never ran it:
  # subclassing raises TypeError (as subclassing Class does) and `allocate`
  # is gone. `new`, keywords, `with` and `dup` all still run the check.
  [Timestamp, Entry, Tag, Tagged].each do |value|
    value.singleton_class.undef_method(:allocate)
    value.define_singleton_method(:inherited) { |_| raise TypeError, "#{value} cannot be subclassed" }
  end
end
