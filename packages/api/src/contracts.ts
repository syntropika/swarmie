export type Role = "owner" | "member";

export type User = {
  id: string;
  email: string;
  email_verified: boolean;
  display_name: string | null;
  avatar_url: string | null;
  created_at: string;
  updated_at: string;
};

export type Workspace = {
  id: string;
  name: string;
  created_by_user_id: string;
  created_at: string;
  updated_at: string;
  deleted_at: string | null;
};

export type Membership = {
  id: string;
  user_id: string;
  workspace_id: string;
  role: Role;
  origin: "workspace_creation" | "invitation";
  created_at: string;
  updated_at: string;
  ended_at: string | null;
};

export type Invitation = {
  id: string;
  workspace_id: string;
  email: string;
  invited_by_user_id: string;
  created_at: string;
  expires_at: string;
  revoked_at: string | null;
  accepted_at: string | null;
  accepted_by_user_id: string | null;
};

export type CurrentSession = {
  user: User;
  csrf_token: string;
};

/** Provider-independent outcomes of a browser sign-in attempt. */
export type AuthenticationResult =
  | { status: "authenticated" }
  | { status: "email_verification_required"; challenge: string }
  | { status: "additional_verification_required" };

/** A public credential label; no credential material is exposed by account management. */
export type Passkey = {
  id: string;
  name: string;
  created_at: string;
  last_used_at: string | null;
};

/** The opaque challenge handle binds the standard WebAuthn options to this browser. */
export type PasskeyChallenge<T> = {
  challenge: string;
  public_key: T;
};

export type CreatedInvitation = {
  invitation: Invitation;
  invitation_url: string;
};
