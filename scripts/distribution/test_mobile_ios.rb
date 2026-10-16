require "minitest/autorun"
require "tmpdir"
require "fileutils"
require "open3"
require_relative "../../fastlane/mobile_ios"

class MobileIOSTest < Minitest::Test
  def with_configuration(variant: "development", identifier: "app.swarmie.mobile.dev", api_url: "https://api.example.com")
    Dir.mktmpdir("mobile-ios-test") do |root|
      output = File.join(root, "dist", "mobile", "ios")
      native = File.join(root, "apps", "mobile", "ios")
      FileUtils.mkdir_p([output, File.join(native, "Swarmie.xcworkspace"), File.join(native, "Swarmie.xcodeproj")])
      manifest = {
        "ios" => { "bundleIdentifier" => identifier },
        "extra" => { "appVariant" => variant, "apiBaseUrl" => api_url },
      }
      File.write(File.join(output, "expo-config.json"), JSON.generate(manifest))
      yield root, { "APP_VARIANT" => variant }
    end
  end

  def test_unsigned_archives_compile_release_code_without_signing_or_exporting_an_ipa
    with_configuration do |root, env|
      settings = MobileIOS.settings(env, signing: "unsigned", root: root)
      options = MobileIOS.build_options(settings)
      assert_equal "app.swarmie.mobile.dev", settings.fetch(:identifier)
      assert_equal "Swarmie", options.fetch(:scheme)
      assert_equal "Release", options.fetch(:configuration)
      assert options.fetch(:skip_codesigning)
      assert options.fetch(:skip_package_ipa)
    end
  end

  def test_signed_archives_require_the_selected_identity_and_an_explicit_backend
    with_configuration do |root, env|
      options = MobileIOS.build_options(MobileIOS.settings(env, signing: "signed", root: root))
      refute options.fetch(:skip_codesigning)
      refute options.fetch(:skip_package_ipa)
    end
    with_configuration(api_url: nil) do |root, env|
      assert_raises(ArgumentError) { MobileIOS.settings(env, signing: "signed", root: root) }
    end
    with_configuration(identifier: "app.swarmie.mobile") do |root, env|
      assert_raises(ArgumentError) { MobileIOS.settings(env, signing: "signed", root: root) }
    end
  end

  def test_production_uses_its_own_identity
    with_configuration(variant: "production", identifier: "app.swarmie.mobile") do |root, env|
      settings = MobileIOS.settings(env, signing: "signed", root: root)
      assert_equal "app.swarmie.mobile", settings.fetch(:identifier)
    end
  end

  def test_invalid_modes_and_stale_configuration_fail_before_signing
    with_configuration do |root, env|
      assert_raises(ArgumentError) { MobileIOS.settings(env, signing: "automatic", root: root) }
      assert_raises(ArgumentError) { MobileIOS.settings(env.merge("APP_VARIANT" => "production"), signing: "signed", root: root) }
      assert_raises(ArgumentError) { MobileIOS.settings({}, signing: "signed", root: root) }
    end
  end

  def test_real_build_cli_rejects_invalid_or_missing_signing_modes
    [
      [["just", "mobile-ios-build", "invalid"], "Signing must be unsigned or signed"],
      [["bundle", "exec", "fastlane", "ios", "build"], "Build requires a signing:"],
    ].each do |command, message|
      output, error, status = Open3.capture3(*command, chdir: MobileIOS::ROOT)
      refute status.success?
      assert_includes output + error, message
      refute_includes output + error, "Step: build_app"
    end
  end
end
