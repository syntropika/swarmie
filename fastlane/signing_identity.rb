require "fileutils"
require "open3"
require "openssl"
require "tmpdir"

module AppleSigning
  # Verify the selected profile's identity and private-key access before the full archive.
  def self.verify_identity!(profile, keychain, execute: Open3.method(:capture3))
    certificates = profile.fetch("DeveloperCertificates", [])
    raise ArgumentError, "The signing profile has no certificates" unless certificates.is_a?(Array) && !certificates.empty?

    fingerprints = certificates.map do |data|
      encoded = data.respond_to?(:string) ? data.string : data
      certificate = OpenSSL::X509::Certificate.new(encoded)
      OpenSSL::Digest::SHA1.hexdigest(certificate.to_der).upcase
    end
    identities, _error, status = execute.call("security", "find-identity", "-v", "-p", "codesigning", keychain)
    raise ArgumentError, "Could not inspect the CI signing keychain" unless status.success?

    installed = identities.scan(/^\s*\d+\) ([[:xdigit:]]{40}) /).flatten.map(&:upcase)
    identity = fingerprints.find { |fingerprint| installed.include?(fingerprint) }
    unless identity
      raise ArgumentError, "The profile has no trusted signing identity in the CI keychain; verify Apple WWDR intermediates and the imported private key"
    end

    Dir.mktmpdir("signing-probe-") do |directory|
      probe = File.join(directory, "probe")
      FileUtils.cp("/usr/bin/true", probe)
      _output, _error, signed = execute.call("codesign", "--force", "--sign", identity, "--keychain", keychain, "--timestamp=none", probe)
      raise ArgumentError, "The CI identity cannot sign; verify Apple trust and non-interactive private-key access" unless signed.success?
    end
    identity
  rescue OpenSSL::OpenSSLError, TypeError
    raise ArgumentError, "The signing profile contains an invalid certificate"
  end
end
