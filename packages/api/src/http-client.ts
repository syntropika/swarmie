import * as Effect from "effect/Effect";
import * as Schema from "effect/Schema";

import type {
  AuthenticationResult,
  CreatedInvitation,
  CurrentSession,
  Invitation,
  Membership,
  Passkey,
  PasskeyChallenge,
  Role,
  Workspace,
} from "./contracts";
import { RequestError } from "./request-error";
import {
  AuthenticationResultSchema,
  CurrentSessionSchema,
  CreatedInvitationSchema,
  InvitationSchema,
  MembershipSchema,
  PasskeySchema,
  PasskeySignInSchema,
  PasskeyRegistrationSchema,
  WorkspaceSchema,
} from "./response-schemas";

export type Fetcher = (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;

export type ClientOptions = {
  /** Omit for same-origin browser requests. */
  baseUrl?: string;
  fetch?: Fetcher;
};

/** A client belongs to one browser session; do not share it between server-side users. */
export class SwarmieClient {
  private readonly baseUrl: string;
  private readonly fetcher: Fetcher;
  private csrfToken: string | null = null;

  constructor(options: ClientOptions = {}) {
    this.baseUrl = (options.baseUrl ?? "").replace(/\/$/u, "");
    this.fetcher = options.fetch ?? globalThis.fetch.bind(globalThis);
  }

  get loginUrl(): string {
    return `${this.baseUrl}/auth/login`;
  }

  signInWithPassword(
    email: string,
    password: string,
  ): Effect.Effect<AuthenticationResult, RequestError> {
    return this.authenticate("/auth/password", { email, password });
  }

  /** Create an account without bypassing the provider's email-verification continuation. */
  signUpWithPassword(
    email: string,
    password: string,
  ): Effect.Effect<AuthenticationResult, RequestError> {
    return this.authenticate("/auth/signup", { email, password });
  }

  verifyEmail(challenge: string, code: string): Effect.Effect<AuthenticationResult, RequestError> {
    return this.authenticate("/auth/email-verification", { challenge, code });
  }

  requestMagicLink(email: string): Effect.Effect<void, RequestError> {
    return this.command("POST", "/auth/magic-link", { email }, "public");
  }

  verifyMagicLink(token: string): Effect.Effect<AuthenticationResult, RequestError> {
    return this.authenticate("/auth/magic-link/verify", { token });
  }

  passkeySignInOptions(
    email: string,
  ): Effect.Effect<PasskeyChallenge<PublicKeyCredentialRequestOptionsJSON>, RequestError> {
    return this.validatedRequest(
      PasskeySignInSchema,
      "POST",
      "/auth/passkey/options",
      { email },
      "public",
    );
  }

  verifyPasskeySignIn(
    challenge: string,
    credential: AuthenticationResponseJSON,
  ): Effect.Effect<AuthenticationResult, RequestError> {
    return this.authenticate("/auth/passkey/verify", { challenge, credential });
  }

  passkeys(): Effect.Effect<Passkey[], RequestError> {
    return this.validatedRequest(
      Schema.mutable(Schema.Array(PasskeySchema)),
      "GET",
      "/api/passkeys",
    );
  }

  passkeyRegistrationOptions(
    name: string,
  ): Effect.Effect<PasskeyChallenge<PublicKeyCredentialCreationOptionsJSON>, RequestError> {
    return this.validatedRequest(
      PasskeyRegistrationSchema,
      "POST",
      "/api/passkeys/registration/options",
      { name },
    );
  }

  verifyPasskeyRegistration(
    challenge: string,
    credential: RegistrationResponseJSON,
  ): Effect.Effect<void, RequestError> {
    return this.command("POST", "/api/passkeys/registration/verify", { challenge, credential });
  }

  revokePasskey(id: string): Effect.Effect<void, RequestError> {
    return this.command("DELETE", `/api/passkeys/${encodeURIComponent(id)}`);
  }

  requestPasswordReset(email: string): Effect.Effect<void, RequestError> {
    return this.command("POST", "/auth/password/reset", { email }, "public");
  }

  resetPassword(token: string, password: string): Effect.Effect<void, RequestError> {
    return this.command("POST", "/auth/password/reset/confirm", { token, password }, "public").pipe(
      Effect.tap(() => this.clearSession()),
    );
  }

  me(): Effect.Effect<CurrentSession, RequestError> {
    return this.validatedRequest(CurrentSessionSchema, "GET", "/api/me").pipe(
      Effect.tap((session) =>
        Effect.sync(() => {
          this.csrfToken = session.csrf_token;
        }),
      ),
    );
  }

  logout(): Effect.Effect<void, RequestError> {
    return this.command("POST", "/auth/logout").pipe(Effect.tap(() => this.clearSession()));
  }

  workspaces(): Effect.Effect<Workspace[], RequestError> {
    return this.validatedRequest(
      Schema.mutable(Schema.Array(WorkspaceSchema)),
      "GET",
      "/api/workspaces",
    );
  }

  createWorkspace(name: string): Effect.Effect<Workspace, RequestError> {
    return this.validatedRequest(WorkspaceSchema, "POST", "/api/workspaces", { name });
  }

  workspace(workspaceId: string): Effect.Effect<Workspace, RequestError> {
    return this.validatedRequest(WorkspaceSchema, "GET", workspacePath(workspaceId));
  }

  renameWorkspace(workspaceId: string, name: string): Effect.Effect<Workspace, RequestError> {
    return this.validatedRequest(WorkspaceSchema, "PATCH", workspacePath(workspaceId), { name });
  }

  deleteWorkspace(
    workspaceId: string,
    confirmWorkspaceId: string,
  ): Effect.Effect<void, RequestError> {
    return this.command("DELETE", workspacePath(workspaceId), {
      confirm_workspace_id: confirmWorkspaceId,
    });
  }

  members(workspaceId: string): Effect.Effect<Membership[], RequestError> {
    return this.validatedRequest(
      Schema.mutable(Schema.Array(MembershipSchema)),
      "GET",
      `${workspacePath(workspaceId)}/members`,
    );
  }

  changeRole(workspaceId: string, userId: string, role: Role): Effect.Effect<void, RequestError> {
    return this.command(
      "PATCH",
      `${workspacePath(workspaceId)}/members/${encodeURIComponent(userId)}`,
      { role },
    );
  }

  removeMember(workspaceId: string, userId: string): Effect.Effect<void, RequestError> {
    return this.command(
      "DELETE",
      `${workspacePath(workspaceId)}/members/${encodeURIComponent(userId)}`,
    );
  }

  leaveWorkspace(workspaceId: string): Effect.Effect<void, RequestError> {
    return this.command("POST", `${workspacePath(workspaceId)}/leave`);
  }

  transferOwnership(
    workspaceId: string,
    userId: string,
    leave = false,
  ): Effect.Effect<void, RequestError> {
    return this.command("POST", `${workspacePath(workspaceId)}/transfer`, {
      user_id: userId,
      leave,
    });
  }

  invitations(workspaceId: string): Effect.Effect<Invitation[], RequestError> {
    return this.validatedRequest(
      Schema.mutable(Schema.Array(InvitationSchema)),
      "GET",
      `${workspacePath(workspaceId)}/invitations`,
    );
  }

  invite(workspaceId: string, email: string): Effect.Effect<CreatedInvitation, RequestError> {
    return this.validatedRequest(
      CreatedInvitationSchema,
      "POST",
      `${workspacePath(workspaceId)}/invitations`,
      { email },
    );
  }

  invitationLink(
    workspaceId: string,
    invitationId: string,
  ): Effect.Effect<CreatedInvitation, RequestError> {
    return this.validatedRequest(
      CreatedInvitationSchema,
      "GET",
      `${workspacePath(workspaceId)}/invitations/${encodeURIComponent(invitationId)}/link`,
    );
  }

  revokeInvitation(workspaceId: string, invitationId: string): Effect.Effect<void, RequestError> {
    return this.command(
      "DELETE",
      `${workspacePath(workspaceId)}/invitations/${encodeURIComponent(invitationId)}`,
    );
  }

  acceptInvitation(token: string): Effect.Effect<Membership, RequestError> {
    return this.validatedRequest(MembershipSchema, "POST", "/api/invitations/accept", { token });
  }

  private validatedRequest<A, I>(
    schema: Schema.Schema<A, I>,
    method: string,
    path: string,
    body?: unknown,
    authentication: "session" | "public" = "session",
  ): Effect.Effect<A, RequestError> {
    return this.request(method, path, body, authentication).pipe(
      Effect.flatMap((response) =>
        Schema.decodeUnknown(schema)(response).pipe(
          Effect.mapError(() => new RequestError({ reason: "invalid-response", status: null })),
        ),
      ),
    );
  }

  private authenticate(
    path: string,
    body: unknown,
  ): Effect.Effect<AuthenticationResult, RequestError> {
    return this.validatedRequest(AuthenticationResultSchema, "POST", path, body, "public").pipe(
      Effect.tap((result) =>
        result.status === "authenticated" ? this.clearSession() : Effect.void,
      ),
    );
  }

  private clearSession(): Effect.Effect<void> {
    return Effect.sync(() => {
      this.csrfToken = null;
    });
  }

  private command(
    method: string,
    path: string,
    body?: unknown,
    authentication: "session" | "public" = "session",
  ): Effect.Effect<void, RequestError> {
    return this.request(method, path, body, authentication).pipe(Effect.asVoid);
  }

  /** Headers, CSRF reads, serialization, and fetch begin only when the returned Effect runs. */
  private request(
    method: string,
    path: string,
    body?: unknown,
    authentication: "session" | "public" = "session",
  ): Effect.Effect<unknown, RequestError> {
    return Effect.gen(this, function* request() {
      const headers = new Headers({ Accept: "application/json" });
      if (method !== "GET" && authentication === "session") {
        const csrfToken = this.csrfToken ?? (yield* this.me()).csrf_token;
        headers.set("X-CSRF-Token", csrfToken);
      }
      return yield* Effect.tryPromise({
        // Effect interruption aborts fetch and response consumption through the same signal.
        try: async (signal) => {
          const init: RequestInit = { method, headers, credentials: "include", signal };
          if (body !== undefined) {
            headers.set("Content-Type", "application/json");
            init.body = JSON.stringify(body);
          }
          const response = await this.fetcher(`${this.baseUrl}${path}`, init);
          if (!response.ok) {
            // Session rotation may invalidate cached protection; never replay the rejected write.
            if (response.status === 401 || response.status === 403) {
              this.csrfToken = null;
            }
            throw new RequestError({ reason: "http", status: response.status });
          }
          let payload: unknown;
          if (response.status !== 204) {
            try {
              payload = await response.json();
            } catch {
              throw new RequestError({ reason: "invalid-response", status: null });
            }
          }
          return payload;
        },
        // Never retain response bodies, arbitrary exception text, request inputs, or causes.
        catch: (error) =>
          error instanceof RequestError
            ? error
            : new RequestError({ reason: "network", status: null }),
      });
    });
  }
}

function workspacePath(id: string): string {
  return `/api/workspaces/${encodeURIComponent(id)}`;
}
