require "open3"
require "match"

module AppleSigning
  # Verify storage without installing identities, creating branches or contacting
  # Apple. An empty repository is valid for bootstrap, but cannot verify a password.
  def self.verify_storage!(env)
    url = env.fetch("MATCH_GIT_URL")
    branch = env.fetch("MATCH_GIT_BRANCH", "main")
    _output, _error, status = Open3.capture3("git", "check-ref-format", "refs/heads/#{branch}")
    raise ArgumentError, "MATCH_GIT_BRANCH must name a valid Git branch" unless status.success?

    references, _error, status = Open3.capture3("git", "ls-remote", "--", url)
    raise ArgumentError, "Cannot read the Match repository with the configured SSH identity" unless status.success?
    return 0 if references.strip.empty?

    unless references.lines.any? { |line| line.split.last == "refs/heads/#{branch}" }
      raise ArgumentError, "MATCH_GIT_BRANCH is missing from the nonempty signing repository"
    end

    storage = Match::Storage::GitStorage.new(
      git_url: url,
      branch: branch,
      git_private_key: env["MATCH_GIT_PRIVATE_KEY"],
      clone_branch_directly: true,
      skip_docs: true,
    )
    storage.download
    signing_files = storage.list_files(file_ext: "{cer,p12,mobileprovision,provisionprofile}")
    return 0 unless signing_files.any? { |path| File.file?(path) }

    encryption = Match::Encryption.for_storage_mode("git", {
      git_url: url,
      working_directory: storage.working_directory,
    })
    FastlaneCore::Helper.with_env_values("MATCH_PASSWORD" => env.fetch("MATCH_PASSWORD")) do
      encryption.decrypt_files.length
    end
  ensure
    # Failed decryption can leave some plaintext files in the temporary clone.
    storage&.clear_changes
  end
end
