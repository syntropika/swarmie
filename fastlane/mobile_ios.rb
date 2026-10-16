require "json"
require_relative "apple_signing"

module MobileIOS
  ROOT = File.expand_path("..", __dir__)

  def self.settings(env, signing:, root: ROOT)
    raise ArgumentError, "Signing must be unsigned or signed" unless %w[unsigned signed].include?(signing)

    variant = AppleSigning.required(env, "APP_VARIANT")
    target = AppleSigning::TARGETS[variant]
    raise ArgumentError, "APP_VARIANT must be development or production" unless target

    output = File.join(root, "dist", "mobile", "ios")
    manifest = JSON.parse(File.read(File.join(output, "expo-config.json")))
    unless manifest.dig("extra", "appVariant") == variant &&
           manifest.dig("ios", "bundleIdentifier") == target.fetch(:identifier)
      raise ArgumentError, "Generated Expo configuration does not match the selected app variant"
    end
    if signing == "signed" && manifest.dig("extra", "apiBaseUrl").to_s.empty?
      raise ArgumentError, "A signed build requires MOBILE_API_BASE_URL in its Expo configuration"
    end

    workspaces = Dir[File.join(root, "apps", "mobile", "ios", "*.xcworkspace")]
    raise ArgumentError, "Run just mobile-ios-prepare to generate a single iOS workspace" unless workspaces.one?

    workspace = workspaces.first
    scheme = File.basename(workspace, ".xcworkspace")
    project = File.join(File.dirname(workspace), "#{scheme}.xcodeproj")
    raise ArgumentError, "The generated Xcode project is missing" unless File.directory?(project)

    {
      variant: variant,
      identifier: target.fetch(:identifier),
      project: project,
      workspace: workspace,
      scheme: scheme,
      output: output,
      signed: signing == "signed",
      uses_non_exempt_encryption: manifest.dig("ios", "infoPlist", "ITSAppUsesNonExemptEncryption"),
      export_compliance_code: manifest.dig("ios", "infoPlist", "ITSEncryptionExportComplianceCode"),
    }
  rescue Errno::ENOENT, JSON::ParserError
    raise ArgumentError, "Run just mobile-ios-prepare to generate the public Expo configuration"
  end

  def self.validate_upload!(settings)
    if settings.fetch(:variant) == "development"
      unless settings.fetch(:uses_non_exempt_encryption) == false
        raise ArgumentError, "Prepare the development app with its export documentation exemption before uploading"
      end
      return
    end

    code = settings.fetch(:export_compliance_code)
    unless settings.fetch(:uses_non_exempt_encryption) == true && code.is_a?(String) && !code.strip.empty?
      raise ArgumentError, "Production uploads require an Apple-approved export compliance code; set IOS_PRODUCTION_ENCRYPTION_EXPORT_COMPLIANCE_CODE before preparing the app"
    end
  end

  def self.upload_options
    {
      skip_submission: false,
      skip_waiting_for_build_processing: false,
      distribute_external: false,
      submit_beta_review: false,
      notify_external_testers: false,
      wait_processing_interval: 30,
      wait_processing_timeout_duration: 1200,
    }
  end

  def self.internal_group!(app, name)
    groups = app ? app.get_beta_groups.select { |group| group.name == name } : []
    unless groups.one? && groups.first.is_internal_group == true
      raise ArgumentError, "The selected app must have exactly one internal TestFlight group named #{name}"
    end

    groups.first.id
  end

  def self.build_options(settings)
    {
      workspace: settings.fetch(:workspace),
      scheme: settings.fetch(:scheme),
      configuration: "Release",
      destination: "generic/platform=iOS",
      clean: true,
      archive_path: File.join(settings.fetch(:output), "Swarmie.xcarchive"),
      output_directory: settings.fetch(:output),
      output_name: "swarmie.ipa",
      buildlog_path: File.join(settings.fetch(:output), "logs"),
      skip_codesigning: !settings.fetch(:signed),
      skip_package_ipa: !settings.fetch(:signed),
    }
  end
end
