require "openssl"
require_relative "match_storage"

module AppleSigning
  TARGETS = {
    "development" => { identifier: "app.swarmie.mobile.dev", name: "Swarmie Dev" }.freeze,
    "production" => { identifier: "app.swarmie.mobile", name: "Swarmie" }.freeze,
  }.freeze

  PROFILES = %w[appstore development adhoc].freeze
  OPERATIONS = %w[inspect sync].freeze

  # Match exposes its default storage commit message through this method. Keep
  # generated signing commits consistent with the repository's commit policy.
  module ConventionalCommits
    def generate_commit_message
      "chore(signing): sync #{type} profiles for #{platform}"
    end
  end

  def self.selection(operation:, variant:, profile:)
    raise ArgumentError, "Operation must be inspect or sync" unless OPERATIONS.include?(operation)
    raise ArgumentError, "Variant must be development or production" unless TARGETS.key?(variant)
    raise ArgumentError, "Profile must be appstore, development or adhoc" unless PROFILES.include?(profile)

    { operation: operation, target: TARGETS.fetch(variant), profile: profile }
  end

  def self.required(env, name)
    value = env.fetch(name, "")
    raise ArgumentError, "#{name} is required" if value.strip.empty?

    value
  end

  def self.validate_storage!(env)
    required(env, "MATCH_GIT_URL")
    required(env, "MATCH_PASSWORD")
    key_path = required(env, "MATCH_GIT_PRIVATE_KEY")
    raise ArgumentError, "MATCH_GIT_PRIVATE_KEY must point to a readable file" unless File.file?(key_path) && File.readable?(key_path)

    team_id = required(env, "APPLE_TEAM_ID")
    raise ArgumentError, "APPLE_TEAM_ID must contain 10 uppercase letters or digits" unless /\A[A-Z0-9]{10}\z/.match?(team_id)
  end

  def self.api_options(env)
    key_id = required(env, "ASC_SIGNING_API_KEY_ID").strip
    issuer_id = required(env, "ASC_SIGNING_API_ISSUER_ID").strip
    key_content = required(env, "ASC_SIGNING_API_KEY_P8")
    raise ArgumentError, "ASC_SIGNING_API_KEY_ID must contain 10 uppercase letters or digits" unless /\A[A-Z0-9]{10}\z/.match?(key_id)
    raise ArgumentError, "ASC_SIGNING_API_ISSUER_ID must be a UUID" unless /\A[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}\z/i.match?(issuer_id)

    begin
      key = OpenSSL::PKey.read(key_content)
      valid_key = key.is_a?(OpenSSL::PKey::EC) && key.private? && key.group.curve_name == "prime256v1"
      raise ArgumentError unless valid_key
    rescue OpenSSL::OpenSSLError, ArgumentError
      raise ArgumentError, "ASC_SIGNING_API_KEY_P8 must contain the complete Apple P-256 private key"
    end

    { key_id: key_id, issuer_id: issuer_id, key_content: key_content, in_house: false }
  end

  # Inspection never registers identifiers. Synchronization only creates the
  # selected identifier when it is missing; existing capabilities stay intact.
  def self.bundle_id(registry, target, create:)
    existing = registry.find(target.fetch(:identifier))
    return existing if existing || !create

    registry.create(name: target.fetch(:name), identifier: target.fetch(:identifier), platform: "IOS")
  end
end
