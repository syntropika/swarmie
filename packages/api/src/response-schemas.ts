import * as Schema from "effect/Schema";

/** HTTP responses become trusted values only after decoding their complete wire shape. */
export const UserSchema = Schema.Struct({
  id: Schema.String,
  email: Schema.String,
  email_verified: Schema.Boolean,
  display_name: Schema.NullOr(Schema.String),
  avatar_url: Schema.NullOr(Schema.String),
  created_at: Schema.String,
  updated_at: Schema.String,
});

export const CurrentSessionSchema = Schema.Struct({
  user: UserSchema,
  csrf_token: Schema.NonEmptyString,
});

export const AuthenticationResultSchema = Schema.Union(
  Schema.Struct({ status: Schema.Literal("authenticated") }),
  Schema.Struct({
    status: Schema.Literal("email_verification_required"),
    challenge: Schema.NonEmptyString,
  }),
  Schema.Struct({ status: Schema.Literal("additional_verification_required") }),
);

export const WorkspaceSchema = Schema.Struct({
  id: Schema.String,
  name: Schema.String,
  created_by_user_id: Schema.String,
  created_at: Schema.String,
  updated_at: Schema.String,
  deleted_at: Schema.NullOr(Schema.String),
});

export const MembershipSchema = Schema.Struct({
  id: Schema.String,
  user_id: Schema.String,
  workspace_id: Schema.String,
  role: Schema.Literal("owner", "member"),
  origin: Schema.Literal("workspace_creation", "invitation"),
  created_at: Schema.String,
  updated_at: Schema.String,
  ended_at: Schema.NullOr(Schema.String),
});

export const InvitationSchema = Schema.Struct({
  id: Schema.String,
  workspace_id: Schema.String,
  email: Schema.String,
  invited_by_user_id: Schema.String,
  created_at: Schema.String,
  expires_at: Schema.String,
  revoked_at: Schema.NullOr(Schema.String),
  accepted_at: Schema.NullOr(Schema.String),
  accepted_by_user_id: Schema.NullOr(Schema.String),
});

export const CreatedInvitationSchema = Schema.Struct({
  invitation: InvitationSchema,
  invitation_url: Schema.String,
});

export const PasskeySchema = Schema.Struct({
  id: Schema.String,
  name: Schema.String,
  created_at: Schema.String,
  last_used_at: Schema.NullOr(Schema.String),
});

const PrfValues = Schema.Struct({
  first: Schema.String,
  second: Schema.optionalWith(Schema.String, { exact: true }),
});

const Extensions = Schema.Struct({
  // The pinned Rust WebAuthn adapter emits these FIDO extensions beyond lib.dom's declaration.
  credentialProtectionPolicy: Schema.optionalWith(
    Schema.Literal(
      "userVerificationOptional",
      "userVerificationOptionalWithCredentialIDList",
      "userVerificationRequired",
    ),
    { exact: true },
  ),
  enforceCredentialProtectionPolicy: Schema.optionalWith(Schema.Boolean, { exact: true }),
  uvm: Schema.optionalWith(Schema.Boolean, { exact: true }),
  minPinLength: Schema.optionalWith(Schema.Boolean, { exact: true }),
  hmacCreateSecret: Schema.optionalWith(Schema.Boolean, { exact: true }),
  hmacGetSecret: Schema.optionalWith(
    Schema.Struct({ output1: Schema.String, output2: Schema.NullOr(Schema.String) }),
    { exact: true },
  ),
  appid: Schema.optionalWith(Schema.String, { exact: true }),
  credProps: Schema.optionalWith(Schema.Boolean, { exact: true }),
  largeBlob: Schema.optionalWith(
    Schema.Struct({
      read: Schema.optionalWith(Schema.Boolean, { exact: true }),
      support: Schema.optionalWith(Schema.String, { exact: true }),
      write: Schema.optionalWith(Schema.String, { exact: true }),
    }),
    { exact: true },
  ),
  prf: Schema.optionalWith(
    Schema.Struct({
      eval: Schema.optionalWith(PrfValues, { exact: true }),
      evalByCredential: Schema.optionalWith(
        Schema.Record({ key: Schema.String, value: PrfValues }),
        { exact: true },
      ),
    }),
    { exact: true },
  ),
});

const Descriptor = Schema.Struct({
  id: Schema.String,
  type: Schema.String,
  transports: Schema.optionalWith(Schema.mutable(Schema.Array(Schema.String)), { exact: true }),
});

/** Standard JSON WebAuthn options remain independent of browser credential APIs. */
export const PasskeySignInSchema = Schema.Struct({
  challenge: Schema.NonEmptyString,
  public_key: Schema.Struct({
    challenge: Schema.String,
    allowCredentials: Schema.optionalWith(Schema.mutable(Schema.Array(Descriptor)), {
      exact: true,
    }),
    extensions: Schema.optionalWith(Extensions, { exact: true }),
    hints: Schema.optionalWith(Schema.mutable(Schema.Array(Schema.String)), { exact: true }),
    rpId: Schema.optionalWith(Schema.String, { exact: true }),
    timeout: Schema.optionalWith(Schema.Number, { exact: true }),
    userVerification: Schema.optionalWith(Schema.String, { exact: true }),
  }),
});

export const PasskeyRegistrationSchema = Schema.Struct({
  challenge: Schema.NonEmptyString,
  public_key: Schema.Struct({
    challenge: Schema.String,
    rp: Schema.Struct({
      name: Schema.String,
      id: Schema.optionalWith(Schema.String, { exact: true }),
    }),
    user: Schema.Struct({ name: Schema.String, displayName: Schema.String, id: Schema.String }),
    pubKeyCredParams: Schema.mutable(
      Schema.Array(Schema.Struct({ alg: Schema.Number, type: Schema.Literal("public-key") })),
    ),
    attestation: Schema.optionalWith(Schema.String, { exact: true }),
    attestationFormats: Schema.optionalWith(Schema.mutable(Schema.Array(Schema.String)), {
      exact: true,
    }),
    authenticatorSelection: Schema.optionalWith(
      Schema.Struct({
        authenticatorAttachment: Schema.optionalWith(Schema.Literal("platform", "cross-platform"), {
          exact: true,
        }),
        requireResidentKey: Schema.optionalWith(Schema.Boolean, { exact: true }),
        residentKey: Schema.optionalWith(Schema.Literal("discouraged", "preferred", "required"), {
          exact: true,
        }),
        userVerification: Schema.optionalWith(
          Schema.Literal("discouraged", "preferred", "required"),
          { exact: true },
        ),
      }),
      { exact: true },
    ),
    excludeCredentials: Schema.optionalWith(Schema.mutable(Schema.Array(Descriptor)), {
      exact: true,
    }),
    extensions: Schema.optionalWith(Extensions, { exact: true }),
    hints: Schema.optionalWith(Schema.mutable(Schema.Array(Schema.String)), { exact: true }),
    timeout: Schema.optionalWith(Schema.Number, { exact: true }),
  }),
});
