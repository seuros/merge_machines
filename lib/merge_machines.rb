# frozen_string_literal: true

require_relative 'merge_machines/version'

# GCounter, PNCounter, LwwRegister, GSet, OrSet, LogicalClock and HybridClock
# are defined by the native extension; there is no pure-Ruby implementation to
# fall back to.
require 'merge_machines_native/merge_machines_native'
