require "minitest/autorun"
require "stringio"
require_relative "../../fastlane/signing_identity"

class SigningIdentityTest < Minitest::Test
  Status = Struct.new(:success?)

  class Executor
    attr_reader :commands

    def initialize(identities:, signing: true)
      @identities = identities
      @signing = signing
      @commands = []
    end

    def call(*command)
      @commands << command
      if command.first == "security"
        [@identities, "", Status.new(true)]
      else
        ["", "", Status.new(@signing)]
      end
    end
  end

  def profile
    key = OpenSSL::PKey::RSA.new(2048)
    certificate = OpenSSL::X509::Certificate.new
    certificate.version = 2
    certificate.serial = 1
    certificate.subject = OpenSSL::X509::Name.parse("/CN=fixture")
    certificate.issuer = certificate.subject
    certificate.public_key = key.public_key
    certificate.not_before = Time.now - 60
    certificate.not_after = Time.now + 60
    certificate.sign(key, OpenSSL::Digest::SHA256.new)
    [{ "DeveloperCertificates" => [StringIO.new(certificate.to_der)] }, OpenSSL::Digest::SHA1.hexdigest(certificate.to_der).upcase]
  end

  def test_missing_or_unrelated_identity_prevents_the_signing_probe
    value, _fingerprint = profile
    ["0 valid identities found", "  1) #{'A' * 40} \"unrelated\""].each do |identities|
      executor = Executor.new(identities: identities)
      assert_raises(ArgumentError) { AppleSigning.verify_identity!(value, "fixture.keychain", execute: executor.method(:call)) }
      assert_equal(1, executor.commands.length)
    end
  end

  def test_matching_identity_must_sign_with_the_selected_keychain
    value, fingerprint = profile
    executor = Executor.new(identities: "  1) #{fingerprint} \"fixture\"")
    assert_equal(fingerprint, AppleSigning.verify_identity!(value, "fixture.keychain", execute: executor.method(:call)))
    assert_equal(["codesign", "--force", "--sign", fingerprint, "--keychain", "fixture.keychain", "--timestamp=none"], executor.commands.last[0...-1])
    refute(File.exist?(executor.commands.last.last))
  end

  def test_inaccessible_private_key_fails_before_archive
    value, fingerprint = profile
    executor = Executor.new(identities: "  1) #{fingerprint} \"fixture\"", signing: false)
    error = assert_raises(ArgumentError) { AppleSigning.verify_identity!(value, "fixture.keychain", execute: executor.method(:call)) }
    assert_includes(error.message, "cannot sign")
  end

  def test_missing_or_invalid_profile_certificates_are_rejected
    [{}, { "DeveloperCertificates" => ["invalid"] }].each do |value|
      assert_raises(ArgumentError) { AppleSigning.verify_identity!(value, "fixture.keychain") }
    end
  end
end
