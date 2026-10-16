require "minitest/autorun"
require "tempfile"
require "match"
require_relative "../../fastlane/apple_signing"

class AppleSigningTest < Minitest::Test
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
