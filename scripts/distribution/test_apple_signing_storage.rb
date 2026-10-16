require "minitest/autorun"
require "tmpdir"
require "fileutils"
require "fastlane"
require_relative "../../fastlane/apple_signing"

class AppleSigningStorageTest < Minitest::Test
  PASSWORD = "signing-test-only".freeze
  ROOT = File.expand_path("../..", __dir__)

  def git(*arguments)
    output, error, status = Open3.capture3("git", *arguments)
    raise "Git fixture failed: #{error}" unless status.success?

    output
  end

  def with_repository(empty: false, material: true, corrupt: false)
    Dir.mktmpdir("signing-storage-test") do |root|
      repository = File.join(root, "repository")
      git("init", "--quiet", "--initial-branch=main", repository)
      git("-C", repository, "config", "user.name", "signing test")
      git("-C", repository, "config", "user.email", "signing@example.invalid")
      unless empty
        if material
          directory = File.join(repository, "certs", "distribution")
          FileUtils.mkdir_p(directory)
          %w[cer p12].each do |extension|
            path = File.join(directory, "fixture.#{extension}")
            File.write(path, "test-only signing content")
            Match::Encryption::MatchFileEncryption.new.encrypt(file_path: path, password: PASSWORD)
          end
          git("-C", repository, "add", ".")
        end
        git("-C", repository, "commit", "--quiet", "--allow-empty", "-m", "chore: initialize signing fixture")
      end
      corrupt_repository(repository) if corrupt
      env = { "MATCH_GIT_URL" => repository, "MATCH_GIT_BRANCH" => "main", "MATCH_PASSWORD" => PASSWORD }
      before = git("-C", repository, "show-ref", "--head") unless empty
      snapshots = Dir.glob(File.join(repository, "certs", "**", "*"))
        .select { |path| File.file?(path) }.to_h { |path| [path, File.binread(path)] }
      temporary_clones = File.join(root, "clones")
      FileUtils.mkdir_p(temporary_clones)
      FastlaneCore::Helper.with_env_values("TMPDIR" => temporary_clones) { yield repository, env }
      assert_empty Dir.children(temporary_clones), "Preflight must remove decrypted clones, including on failure"
      assert_equal before, git("-C", repository, "show-ref", "--head") unless empty
      snapshots.each { |path, bytes| assert_equal bytes, File.binread(path), "Storage must stay encrypted" }
    end
  end

  def test_empty_repository_reports_uninitialized_storage_without_creating_a_branch
    with_repository(empty: true) do |repository, env|
      assert_equal 0, AppleSigning.verify_storage!(env)
      assert_empty git("-C", repository, "for-each-ref")
    end
  end

  def test_existing_branch_without_signing_files_does_not_claim_to_verify_the_password
    with_repository(material: false) do |_repository, env|
      assert_equal 0, AppleSigning.verify_storage!(env.merge("MATCH_PASSWORD" => "unverifiable"))
    end
  end

  def test_real_match_storage_decrypts_existing_files_without_changing_the_repository
    with_repository do |_repository, env|
      assert_equal 2, AppleSigning.verify_storage!(env)
    end
  end

  def test_wrong_password_fails_and_removes_the_temporary_clone
    with_repository do |_repository, env|
      error = assert_raises(FastlaneCore::Interface::FastlaneError) do
        AppleSigning.verify_storage!(env.merge("MATCH_PASSWORD" => "incorrect"))
      end
      assert_includes error.message, "Invalid password"
    end
  end

  def test_missing_branch_in_an_existing_repository_is_not_bootstrapped
    with_repository do |_repository, env|
      error = assert_raises(ArgumentError) do
        AppleSigning.verify_storage!(env.merge("MATCH_GIT_BRANCH" => "missing"))
      end
      assert_includes error.message, "missing from the nonempty signing repository"
    end
  end

  def corrupt_repository(repository)
    path = File.join(repository, "certs", "distribution", "fixture.p12")
    File.write(path, "corrupt encrypted content")
    git("-C", repository, "add", ".")
    git("-C", repository, "commit", "--quiet", "-m", "test: corrupt encrypted fixture")
  end

  def test_corrupt_storage_fails_and_removes_partially_decrypted_material
    with_repository(corrupt: true) do |_repository, env|
      assert_raises(FastlaneCore::Interface::FastlaneError) { AppleSigning.verify_storage!(env) }
    end
  end

  def test_inspect_and_sync_stop_before_apple_actions_when_storage_is_invalid
    Fastlane.load_actions
    [false, true].each do |corrupt|
      with_repository(corrupt: corrupt) do |repository, storage_env|
        key_path = File.join(File.dirname(repository), "fixture.p8")
        File.write(key_path, OpenSSL::PKey::EC.generate("prime256v1").private_to_pem)
        env = storage_env.merge(
          "APPLE_TEAM_ID" => "ABCD123456",
          "MATCH_GIT_PRIVATE_KEY" => key_path,
          "ASC_SIGNING_API_KEY_ID" => "ABCD123456",
          "ASC_SIGNING_API_ISSUER_ID" => "12345678-1234-1234-1234-123456789abc",
          "ASC_SIGNING_API_KEY_P8" => File.read(key_path),
        )
        if corrupt
          assert_lane_stops_before_apple(env, "Invalid password")
        else
          assert_lane_stops_before_apple(env.merge("MATCH_PASSWORD" => "incorrect"), "Invalid password")
          assert_lane_stops_before_apple(env.merge("MATCH_GIT_BRANCH" => "missing"), "missing from the nonempty")
        end
      end
    end
  end

  def assert_lane_stops_before_apple(env, expected_error)
    %w[inspect sync].each do |operation|
      apple_calls = []
      fastfile = Fastlane::FastFile.new(File.join(ROOT, "fastlane", "Fastfile"))
      fastfile.define_singleton_method(:app_store_connect_api_key) do |**_options|
        apple_calls << :authenticate
        raise "Apple must not be contacted before storage is verified"
      end
      FastlaneCore::Helper.with_env_values(env) do
        error = assert_raises(FastlaneCore::Interface::FastlaneError) do
          fastfile.runner.execute(:signing, :ios, { operation: operation })
        end
        assert_includes error.message, expected_error
      end
      assert_empty apple_calls
    end
  end
end
