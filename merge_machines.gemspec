# frozen_string_literal: true

require_relative 'lib/merge_machines/version'

Gem::Specification.new do |spec|
  spec.name = 'merge_machines'
  spec.version = MergeMachines::VERSION
  spec.authors = ['Abdelkader Boudih']
  spec.email = ['oss@seuros.com']

  spec.summary = 'Conflict-free replicated data types for Ruby'
  spec.description = 'Counters, a last-writer-wins register, grow-only and observed-remove sets, and the ' \
                     'Lamport and hybrid logical clocks that stamp them, bound to Ruby from the no_std ' \
                     'merge-machines crate. Every state merges commutatively, associatively and ' \
                     'idempotently, and encodes to a byte string other languages reading the same crate ' \
                     'can decode.'
  spec.homepage = 'https://github.com/seuros/merge_machines'
  spec.license = 'MIT'
  spec.required_ruby_version = '>= 4.0.0'

  spec.metadata['source_code_uri'] = spec.homepage
  spec.metadata['changelog_uri'] = "#{spec.homepage}/blob/main/CHANGELOG.md"
  spec.metadata['bug_tracker_uri'] = "#{spec.homepage}/issues"
  spec.metadata['rubygems_mfa_required'] = 'true'
  spec.metadata['cargo_crate_name'] = 'merge_machines_native'
  spec.metadata['cargo_manifest_path'] = 'ext/merge_machines_native/ffi/Cargo.toml'

  spec.files = Dir.glob(%w[
                          lib/**/*.rb
                          sig/**/*.rbs
                          ext/**/*.{rb,rs,toml}
                          Cargo.toml
                          Cargo.lock
                          README.md
                          CHANGELOG.md
                          LICENSE.txt
                        ]).select { |path| File.exist?(path) }
                   .reject { |path| path =~ %r{/tests(/|\.rs\z)} }
  spec.require_paths = ['lib']
  spec.extensions = ['ext/merge_machines_native/extconf.rb']

  # A source install runs extconf.rb, which requires rb_sys/mkmf, and RubyGems
  # clears GEM_PATH while building the extension. Without this the build only
  # works where rb_sys happens to be installed globally.
  spec.add_dependency 'rb_sys', '~> 0.9'
end
