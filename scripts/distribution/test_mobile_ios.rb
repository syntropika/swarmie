require "minitest/autorun"
require "tmpdir"
require "fileutils"
require "open3"
require_relative "../../fastlane/mobile_ios"

class MobileIOSTest < Minitest::Test
  def with_configuration(variant: "development", identifier: "app.swarmie.mobile.dev", api_url: "https://api.example.com", info_plist: {})
    Dir.mktmpdir("mobile-ios-test") do |root|
      output = File.join(root, "dist", "mobile", "ios")
      native = File.join(root, "apps", "mobile", "ios")
      FileUtils.mkdir_p([output, File.join(native, "Swarmie.xcworkspace"), File.join(native, "Swarmie.xcodeproj")])
      manifest = {
        "ios" => { "bundleIdentifier" => identifier, "infoPlist" => info_plist },
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

  def test_development_uploads_require_the_prepared_documentation_exemption
    with_configuration(info_plist: { "ITSAppUsesNonExemptEncryption" => false }) do |root, env|
      settings = MobileIOS.settings(env, signing: "signed", root: root)
      assert_nil MobileIOS.validate_upload!(settings)
    end
    [nil, true].each do |non_exempt|
      with_configuration(info_plist: { "ITSAppUsesNonExemptEncryption" => non_exempt }) do |root, env|
        settings = MobileIOS.settings(env, signing: "signed", root: root)
        assert_raises(ArgumentError) { MobileIOS.validate_upload!(settings) }
      end
    end
  end

  def test_upload_waits_for_processing_and_distributes_internally_without_beta_review
    options = MobileIOS.upload_options
    refute options.fetch(:skip_submission)
    refute options.fetch(:skip_waiting_for_build_processing)
    refute options.fetch(:distribute_external)
    refute options.fetch(:submit_beta_review)
    assert_equal 1200, options.fetch(:wait_processing_timeout_duration)
    require "pilot"
    FastlaneCore::Configuration.create(Pilot::Options.available_options, options)
  end

  def test_group_selection_requires_an_existing_unambiguous_internal_group
    group = Struct.new(:id, :name, :is_internal_group)
    app = Struct.new(:get_beta_groups)
    target = group.new("internal-id", "Release team", true)
    assert_equal "internal-id", MobileIOS.internal_group!(app.new([target]), "Release team")
    [nil, app.new([]), app.new([group.new("external-id", "Release team", false)]),
     app.new([target, target])].each do |invalid|
      assert_raises(ArgumentError) { MobileIOS.internal_group!(invalid, "Release team") }
    end
  end

  def test_production_uploads_require_the_approval_in_the_prepared_configuration
    [nil, "", " ", 123].each do |code|
      with_configuration(variant: "production", identifier: "app.swarmie.mobile",
                         info_plist: { "ITSAppUsesNonExemptEncryption" => true,
                                       "ITSEncryptionExportComplianceCode" => code }) do |root, env|
        settings = MobileIOS.settings(env, signing: "signed", root: root)
        error = assert_raises(ArgumentError) { MobileIOS.validate_upload!(settings) }
        assert_includes error.message, "IOS_PRODUCTION_ENCRYPTION_EXPORT_COMPLIANCE_CODE"
      end
    end
    [false, true].each do |non_exempt|
      with_configuration(variant: "production", identifier: "app.swarmie.mobile",
                         info_plist: { "ITSAppUsesNonExemptEncryption" => non_exempt,
                                       "ITSEncryptionExportComplianceCode" => "fixture-approval-reference" }) do |root, env|
        settings = MobileIOS.settings(env, signing: "signed", root: root)
        if non_exempt
          assert_nil MobileIOS.validate_upload!(settings)
        else
          assert_raises(ArgumentError) { MobileIOS.validate_upload!(settings) }
        end
      end
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
