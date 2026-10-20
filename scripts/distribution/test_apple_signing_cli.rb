require "minitest/autorun"
require "open3"

class AppleSigningCliTest < Minitest::Test
  ROOT = File.expand_path("../..", __dir__)

  def run_command(*arguments)
    env = ENV.keys.grep(/\A(?:MATCH_|ASC_SIGNING_|APPLE_TEAM_ID\z)/).to_h { |name| [name, nil] }
    env.merge!(
      "FASTLANE_SKIP_UPDATE_CHECK" => "1",
      "FASTLANE_OPT_OUT_USAGE" => "1",
    )
    output, error, status = Open3.capture3(env, *arguments, chdir: ROOT)
    [output + error, status]
  end

  def test_root_command_forwards_each_option_to_the_real_fastlane_cli
    [
      ["invalid", "development", "appstore", "Operation must be inspect or sync"],
      ["sync", "invalid", "appstore", "Variant must be development or production"],
      ["sync", "production", "invalid", "Profile must be appstore, development or adhoc"],
    ].each do |operation, variant, profile, expected_error|
      output, status = run_command("just", "mobile-signing", operation, variant, profile)
      refute status.success?
      assert_includes output, expected_error
      refute_includes output, "app_store_connect_api_key"
    end
  end

  def test_sync_and_nondefault_options_are_logged_without_falling_back_to_inspect
    output, status = run_command("just", "mobile-signing", "sync", "production", "adhoc")
    refute status.success?
    assert_includes output, "Signing request: sync / production / adhoc"
    assert_includes output, "MATCH_GIT_URL is required"
    refute_includes output, "app_store_connect_api_key"
  end

  def test_root_command_keeps_its_explicit_inspection_defaults
    output, status = run_command("just", "mobile-signing")
    refute status.success?
    assert_includes output, "Signing request: inspect / development / appstore"
    assert_includes output, "MATCH_GIT_URL is required"
  end

  def test_raw_fastlane_rejects_missing_or_incorrectly_formatted_options
    [[], ["operation=sync", "variant=production", "profile=adhoc"], ["operation:sync"]].each do |options|
      output, status = run_command("bundle", "exec", "fastlane", "ios", "signing", *options)
      refute status.success?
      assert_includes output, "Signing requires operation:, variant: and profile: parameters"
      refute_includes output, "app_store_connect_api_key"
    end
  end
end
