require "minitest/autorun"
require "tempfile"
require "fastlane"
require "match"
require_relative "../../fastlane/apple_signing"

class AppleSigningTest < Minitest::Test
  class BundleIdentifier
    attr_reader :created

    def initialize(types)
      @capabilities = types.map do |type|
        Spaceship::ConnectAPI::BundleIdCapability.new(type, "capabilityType" => type)
      end
      @created = []
    end

    def get_capabilities
      @capabilities
    end

    def create_capability(type)
      @created << type
      @capabilities << Spaceship::ConnectAPI::BundleIdCapability.new(type, "capabilityType" => type)
    end
  end

  class Registry
    attr_reader :created

    def initialize(existing = nil)
      @existing = existing
      @created = []
    end

    def find(_identifier)
      @existing
    end

    def create(**attributes)
      @created << attributes
      attributes
    end
  end

  def api_environment
    {
      "ASC_SIGNING_API_KEY_ID" => "ABCD123456",
      "ASC_SIGNING_API_ISSUER_ID" => "12345678-1234-1234-1234-123456789abc",
      "ASC_SIGNING_API_KEY_P8" => OpenSSL::PKey::EC.generate("prime256v1").private_to_pem,
    }
  end

  def test_invalid_selection_fails_before_work_can_start
    [{ operation: "delete", variant: "development", profile: "appstore" },
     { operation: "sync", variant: "all", profile: "appstore" },
     { operation: "sync", variant: "production", profile: "enterprise" }].each do |selection|
      assert_raises(ArgumentError) { AppleSigning.selection(**selection) }
    end
  end

  def test_inspection_cannot_create_a_missing_bundle_id
    registry = Registry.new
    assert_nil AppleSigning.bundle_id(registry, AppleSigning::TARGETS.fetch("development"), create: false)
    assert_empty registry.created
  end

  def test_sync_creates_only_the_selected_missing_identifier
    registry = Registry.new
    target = AppleSigning::TARGETS.fetch("development")
    AppleSigning.bundle_id(registry, target, create: true)
    assert_equal [{ name: "Swarmie Dev", identifier: "app.swarmie.mobile.dev", platform: "IOS" }], registry.created
    refute_equal target.fetch(:identifier), AppleSigning::TARGETS.fetch("production").fetch(:identifier)
  end

  def test_existing_identifier_is_preserved
    existing = Object.new
    registry = Registry.new(existing)
    assert_same existing, AppleSigning.bundle_id(registry, AppleSigning::TARGETS.fetch("production"), create: true)
    assert_empty registry.created
  end

  def test_sync_adds_push_to_an_existing_identifier_without_removing_other_capabilities
    bundle = BundleIdentifier.new(["ASSOCIATED_DOMAINS"])
    AppleSigning.sync_capabilities!(bundle)
    assert_equal ["PUSH_NOTIFICATIONS"], bundle.created
    assert_equal ["ASSOCIATED_DOMAINS", "PUSH_NOTIFICATIONS"], bundle.get_capabilities.map(&:capability_type)
  end

  def test_sync_can_configure_a_fresh_identifier_and_is_idempotent
    bundle = BundleIdentifier.new([])
    2.times { AppleSigning.sync_capabilities!(bundle) }
    assert_equal ["PUSH_NOTIFICATIONS"], bundle.created
  end

  def test_sync_preserves_an_existing_push_capability
    bundle = BundleIdentifier.new(["PUSH_NOTIFICATIONS"])
    AppleSigning.sync_capabilities!(bundle)
    assert_empty bundle.created
  end

  def test_real_sync_lane_enables_push_before_replacing_profiles_then_checks_readonly_storage
    Fastlane.load_actions
    registry = Spaceship::ConnectAPI::BundleId
    original_find = registry.method(:find)
    original_verify = AppleSigning.method(:verify_storage!)
    AppleSigning.define_singleton_method(:verify_storage!) { |_env| 1 }
    Tempfile.create("signing-lane-fixture") do |key|
      env = api_environment.merge(
        "MATCH_GIT_URL" => "git@github.com:example/signing.git", "MATCH_PASSWORD" => "test-only",
        "MATCH_GIT_PRIVATE_KEY" => key.path, "APPLE_TEAM_ID" => "ABCD123456",
      )
      AppleSigning::TARGETS.each do |variant, target|
        bundle = BundleIdentifier.new([])
        registry.define_singleton_method(:find) do |identifier|
          raise "Unexpected identifier" unless identifier == target.fetch(:identifier)

          bundle
        end
        calls = []
        fastfile = Fastlane::FastFile.new(File.expand_path("../../fastlane/Fastfile", __dir__))
        fastfile.define_singleton_method(:app_store_connect_api_key) { |**_options| { key_id: "fixture" } }
        fastfile.define_singleton_method(:setup_ci) { |**_options| nil }
        fastfile.define_singleton_method(:match) do |**options|
          raise "Push must be enabled before generating profiles" unless bundle.created == ["PUSH_NOTIFICATIONS"]

          calls << options
        end
        FastlaneCore::Helper.with_env_values(env) do
          fastfile.runner.execute(:signing, :ios, { operation: "sync", variant: variant, profile: "appstore" })
        end
        assert_equal 2, calls.length
        assert_equal [target.fetch(:identifier)], calls.first.fetch(:app_identifier)
        assert_equal false, calls.first.fetch(:readonly)
        assert_equal true, calls.first.fetch(:force), "An existing profile must be regenerated after capability changes"
        assert_equal true, calls.last.fetch(:readonly)
        refute calls.last[:force], "The verification call must consume the new profile without replacing it"
      end
    end
  ensure
    registry.define_singleton_method(:find, original_find) if original_find
    AppleSigning.define_singleton_method(:verify_storage!, original_verify) if original_verify
  end

  def test_api_configuration_accepts_a_complete_apple_private_key
    env = api_environment
    result = AppleSigning.api_options(env)
    assert_equal env.fetch("ASC_SIGNING_API_KEY_P8"), result.fetch(:key_content)
    refute result.fetch(:in_house)
  end

  def test_incomplete_or_wrong_keys_fail_without_echoing_secret_material
    ["not-a-private-key", OpenSSL::PKey::RSA.generate(2048).private_to_pem,
     OpenSSL::PKey::EC.generate("prime256v1").public_to_pem].each do |key|
      env = api_environment.merge("ASC_SIGNING_API_KEY_P8" => key)
      error = assert_raises(ArgumentError) { AppleSigning.api_options(env) }
      assert_equal "ASC_SIGNING_API_KEY_P8 must contain the complete Apple P-256 private key", error.message
    end
  end

  def test_storage_requires_the_ssh_file_and_team_id
    Tempfile.create("signing-test") do |file|
      env = { "MATCH_GIT_URL" => "git@github.com:example/signing.git", "MATCH_PASSWORD" => "test-only",
              "MATCH_GIT_PRIVATE_KEY" => file.path, "APPLE_TEAM_ID" => "ABCD123456" }
      AppleSigning.validate_storage!(env)
      assert_raises(ArgumentError) { AppleSigning.validate_storage!(env.merge("APPLE_TEAM_ID" => "")) }
      assert_raises(ArgumentError) { AppleSigning.validate_storage!(env.merge("MATCH_GIT_PRIVATE_KEY" => "missing")) }
    end
  end

  def test_real_match_storage_uses_lowercase_conventional_commits
    Match::Storage::GitStorage.prepend(AppleSigning::ConventionalCommits)
    AppleSigning::PROFILES.each do |profile|
      storage = Match::Storage::GitStorage.new(type: profile, platform: "ios")
      assert_match(/\Achore\(signing\): [a-z ]+\z/, storage.generate_commit_message)
    end
  end
end
