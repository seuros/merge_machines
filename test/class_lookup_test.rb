# frozen_string_literal: true

require_relative 'test_helper'
require 'open3'
require 'rbconfig'

# The classes of the states the extension wraps (LogicalClock, HybridClock,
# GCounter, PNCounter, LwwRegister, GSet, OrSet) are fixed when it loads.
# Nothing a caller does later, redefining a constant or `Object.const_get`,
# reaches a state being built. Each case runs in a fresh process: the class is
# fixed once per process, at load.
class ClassLookupTest < Minitest::Test
  LIB = File.expand_path('../lib', __dir__)

  # Each wrapped class: `new`'s arguments, and a use of a fresh state `s` by
  # the gem's own methods with what it answers.
  STATES = {
    LogicalClock: ['', 's.tick', '1'],
    HybridClock: ['', 's.tick(10).wall_ns', '10'],
    GCounter: ['4', 's.merge(M::GCounter.new(4))', 'merged'],
    PNCounter: ['4', 's.merge(M::PNCounter.new(4))', 'merged'],
    LwwRegister: ['1, 5, 2', 's.merge(M::LwwRegister.new(3, 9, 1))', 'merged'],
    GSet: ['64', 's.merge(M::GSet.new(64))', 'merged'],
    OrSet: ['64', 's.merge(M::OrSet.new(64))', 'merged']
  }.freeze

  # Every class's builder and use, as source for the fresh Ruby.
  PRELUDE = <<~RUBY
    require 'merge_machines'
    M = MergeMachines
    BUILDERS = { #{STATES.map { |name, (args, _, _)| "#{name}: -> { M::#{name}.new(#{args}) }" }.join(', ')} }
    USES = { #{STATES.map { |name, (_, use, _)| "#{name}: ->(s) { #{use} }" }.join(', ')} }
  RUBY

  # Runs `body` after PRELUDE in a new Ruby; answers its last line of output.
  def isolated(body)
    out, err, status = Open3.capture3(RbConfig.ruby, '-I', LIB, '-e', PRELUDE + body)
    assert status.success?, "exited #{status.exitstatus}: #{err}"
    out.lines.last&.chomp
  end

  # Bug: a constant redefined before the first state made it an instance of
  # the new class, without any of the gem's methods.
  STATES.each do |name, (args, use, answer)|
    define_method(:"test_a_redefined_#{name}_constant_does_not_change_the_state_class") do
      assert_equal "true true true #{answer}", isolated(<<~RUBY)
        original = M::#{name}
        M.send(:remove_const, :#{name})
        M.const_set(:#{name}, Class.new)
        s = original.new(#{args})
        copy = original.decode(s.encoded)
        M.send(:remove_const, :#{name})
        M.const_set(:#{name}, original)
        puts [s.instance_of?(original), copy.instance_of?(original), copy == s, #{use}].join(' ')
      RUBY
    end
  end

  # Bug: building the first state of a class ran `Object.const_get`, a
  # caller's code in the middle of a native call.
  def test_building_a_state_runs_no_const_get
    assert_equal "0 false #{STATES.size * 2}", isolated(<<~RUBY)
      hits = 0
      log = []
      Object.define_singleton_method(:const_get) do |*args|
        hits += 1
        log.freeze
        super(*args)
      end
      built = BUILDERS.flat_map { |name, build| [s = build.call, M.const_get(name).decode(s.encoded)] }
      puts [hits, log.frozen?, built.count { |s| BUILDERS.key?(s.class.name.split('::').last.to_sym) }].join(' ')
    RUBY
  end

  # Bug: a redefined `Object.const_get` that raised hit an `unwrap` in the
  # extension: an uncatchable fatal, and the process exited.
  def test_a_raising_const_get_does_not_kill_the_process
    assert_equal STATES.values.map(&:last).join(' '), isolated(<<~RUBY)
      Object.define_singleton_method(:const_get) { |*| raise IOError, 'lookup ran mid-call' }
      puts BUILDERS.map { |name, build| USES.fetch(name).call(build.call) }.join(' ')
    RUBY
  end
end
