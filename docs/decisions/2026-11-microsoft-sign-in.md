# Microsoft sign-in path for Outlook accounts (R6-13)

- Status: decided by the Outlook spec (PR #637). The owner does the steps
  below in the release gate, after R1-07.
- Date: 2026-11-18
- Card: R6-13
- Related: `docs/specs/2026-11-17-outlook-provider.md`, section 7, and
  `docs/decisions/2026-11-google-sign-in.md`.

## Problem

Iris reads and writes Outlook mail with Microsoft Graph. Graph needs an
access token from the Microsoft identity platform. A token needs an app
registration: an application (client) ID in a Microsoft Entra tenant.

Iris is self-hosted. The project does not run a shared app for all users,
for the same reasons as path A of the Google record: a shared app makes the
project responsible for each install, and a public client of the project
puts no limit on who uses its ID.

## Decision

Each user makes an own app registration, as for Google:

1. The registration is a public client (no client secret) of the type
   "Mobile and desktop applications", with the redirect URI
   `http://localhost` [1][2].
2. The account type is "Accounts in any organizational directory and
   personal Microsoft accounts". With it, Iris uses the tenant `common`,
   for an Outlook.com account and for a work or school account [3].
3. Iris asks for `offline_access`, `Mail.ReadWrite`, `Mail.Send` and
   `User.Read`, with PKCE (S256) [4][5].
4. The user gives Iris the application (client) ID and the tenant. Iris
   accepts only `common` or a tenant ID (a GUID). The host of the endpoints
   is fixed in the code: `login.microsoftonline.com`.

The code (`iris_auth::Client::microsoft`) does these checks:

- The client ID and the tenant are GUIDs (the tenant can also be
  `common`), so a wrong value cannot move the endpoints to another host.
- The redirect URI is `http://localhost:<port>/oauth/callback`. Microsoft
  ignores the port of a `localhost` redirect [2]. The listener binds
  `127.0.0.1` only.
- Each code exchange and each refresh that names a `scope` must hold
  `Mail.ReadWrite` and `Mail.Send`. Else the sign-in fails with
  `Error::PermissionMissing`, which names the missing permission.
- Each refresh gives a new refresh token [6].
  `Client::refresh_tokens` returns it, and the caller seals it in place of
  the old one. The old token stays valid, so a crash before the write
  loses nothing.
- `invalid_grant` on a refresh gives `Error::Revoked`: the account needs a
  new sign-in.
- The token calls follow no redirect, for Google and Microsoft. A 3xx
  answer is an error, so a code, a verifier or a refresh token never goes
  to another host.
- `Client::refresh` (access token only) refuses a Microsoft client with
  `Error::RotatingRefresh`, so no caller can drop the new refresh token.

## Steps for the owner (release gate)

Do these steps on the Mac after R1-07. Do not give a password or a token to
an agent. The client ID is not a secret, but keep it out of the
repository.

### 1. Get an Entra tenant

An app registration needs an Entra tenant [1]. A personal Microsoft
account has no tenant.

1. If you have a work or school tenant where you can register apps, use it
   and go to step 2.
2. Else open <https://azure.microsoft.com/free> and sign in with your
   Microsoft account. Make a free Azure account. It makes a tenant
   ("Default Directory") with the free tier of Entra ID.
3. Check after the free trial: the tenant and the registration must stay.
   Evidence: `pending` (assumption A13 of the spec). If the tenant goes
   away, the refresh fails with `invalid_grant` and Iris asks for a new
   sign-in; no mail data is lost.

### 2. Register the app

1. Open <https://entra.microsoft.com> and sign in to the tenant.
2. Go to Entra ID, App registrations, New registration.
3. Name: `Iris` (any name; the consent screen shows it).
4. Supported account types: "Accounts in any organizational directory (Any
   Microsoft Entra ID tenant - Multitenant) and personal Microsoft accounts
   (e.g. Skype, Xbox)".
5. Redirect URI: select the platform "Public client/native (mobile &
   desktop)" and type `http://localhost`.
6. Click Register.
7. On the Overview page, copy the "Application (client) ID".

Do not make a client secret. Iris does not use one.

### 3. Add the permissions

1. Open API permissions, Add a permission, Microsoft Graph, Delegated
   permissions.
2. Select `Mail.ReadWrite`, `Mail.Send`, `User.Read` and `offline_access`.
3. Click Add permissions.

A personal account gives consent at the first sign-in. A work or school
tenant can need admin consent. An admin can also block consent to an app
from an unverified publisher [7] (assumption A12). In that case, register
the app in that tenant as single-tenant, and give Iris its directory
(tenant) ID in place of `common`.

### 4. Sign in from Iris

1. In Iris, add an Outlook account. Give the application (client) ID, and
   `common` as the tenant (or the tenant ID for a single-tenant app).
2. Sign in in the browser. Accept the permissions.
3. The screen can say that the publisher is not verified. You are the
   publisher of your own app: continue.

If the browser cannot reach `http://localhost:<port>`, tell the Outlook
thread (open question of the spec: the listener can also bind `[::1]`).

## Paths not chosen

| Path | Why not |
|---|---|
| A shared Iris app registration | The project becomes responsible for each install. Publisher verification needs a Microsoft Partner Network account [7]. |
| A client secret ("Web" platform) | A secret in a self-hosted program is not secret. A public client with PKCE is the flow for desktop programs [2][5]. |
| Device code flow | It needs "Allow public client flows" and gives a user code to type. The loopback redirect needs fewer steps. |
| IMAP with OAuth to Outlook.com | Card R6-03. Graph gives categories, flags and immutable IDs that IMAP does not. |

## Open questions

- Does the free tenant stay after the Azure free trial (A13)?
- Does consent to an unverified multitenant app work for a personal
  Outlook.com account with no extra step (A12)?

The release gate answers both.

## Sources

1. Register an application in Microsoft Entra ID (official).
   <https://learn.microsoft.com/en-us/entra/identity-platform/quickstart-register-app>
2. Redirect URI (reply URL) best practices and limitations (official).
   <https://learn.microsoft.com/en-us/entra/identity-platform/reply-url>
3. Supported account types (official).
   <https://learn.microsoft.com/en-us/entra/identity-platform/supported-accounts-validation>
4. Scopes and permissions in the Microsoft identity platform (official).
   <https://learn.microsoft.com/en-us/entra/identity-platform/scopes-oidc>
5. Microsoft identity platform and OAuth 2.0 authorization code flow
   (official).
   <https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-auth-code-flow>
6. Refresh tokens in the Microsoft identity platform (official).
   <https://learn.microsoft.com/en-us/entra/identity-platform/refresh-tokens>
7. Publisher verification (official).
   <https://learn.microsoft.com/en-us/entra/identity-platform/publisher-verification-overview>
