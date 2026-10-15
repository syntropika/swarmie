Pod::Spec.new do |s|
  s.name = 'SwarmieClient'
  s.version = '0.1.0'
  s.summary = 'Swarmie native client binding for Expo'
  s.description = 'Thin C ABI binding to the shared Rust client.'
  s.license = { :type => 'Apache-2.0', :file => 'LICENSE' }
  s.author = 'Syntropika'
  s.homepage = 'https://github.com/syntropika/swarmie'
  s.platforms = { :ios => '16.4' }
  s.source = { :git => 'https://github.com/syntropika/swarmie.git' }
  s.swift_version = '5.9'
  s.static_framework = true
  s.dependency 'ExpoModulesCore'
  s.source_files = 'ios/**/*.swift'
  s.vendored_frameworks = 'native/CSwarmie.xcframework'
  s.libraries = 'resolv', 'c++'
  s.frameworks = 'Security', 'SystemConfiguration'
end
