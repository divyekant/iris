# Google sign-in path for self-hosted users (R1-01)

- Status: decided. dk chose path A on 2026-11-15.
- Date: 2026-11-15
- Card: R1-01
- Related: `docs/spec/01-mirror-store-forms.md` section 4.9 and section 11,
  item 1.

## Problem

Iris release 1 reads Gmail with the Gmail API and the scope `gmail.readonly`.
Release 2 adds `gmail.modify`. Google puts both scopes in the "restricted"
class [6]. A restricted scope needs a verified app with an annual security
assessment, or a use that Google exempts from verification [3][4].

The target of the card is "setup takes 10 minutes or less". Iris runs on a
Mac and on a small Linux server. Each user runs an own engine.

## Recommendation

Use path A: each user makes an own Google OAuth client of the type "Desktop
app" and publishes it to "In production" without verification. Iris gives a
guided setup that does each step in the browser with the user.

Reasons:

1. Path A keeps the Gmail API and the `gmail.readonly` scope. Spec 1 stays
   correct, and Google itself enforces "release 1 changes nothing" (spec 1,
   M2).
2. Path A costs nothing and needs no review by Google. Google exempts "personal
   use (fewer than 100 users)" from verification [3][5]. Each Iris user has one
   user on the own client.
3. Path B (a verified Iris app) costs money each year, takes weeks, and makes
   the project responsible for each install. Path B also puts the client
   secret in public code against Google policy [10].
4. Path C (IMAP with an app password) is faster, but it gives Iris a
   credential with full access to all mail. It cannot be read-only. It also
   changes spec 1 (no Gmail history API). Keep path C as a later second
   provider.

The recommendation has a condition: the domain test of path A must pass.
If the test fails, Iris does not ship path A as the default. The lead brings
the decision back to dk.

Path A does not meet the 10-minute target with certainty. A first-time user
needs about 10 to 20 minutes (estimate, no source). The guided setup must
bring the time down. The release target (spec 1, M4) is the first brief in
10 minutes or less, so the measure ends at the first brief, not at sign-in.
R1-02 must measure this time with a real run on dk's Mac.

## Paths

### A. Own OAuth client for each user (recommended)

Setup steps (Google Cloud console, "Google Auth Platform" since April 2025 [9]):

1. Create a Google Cloud project.
2. Enable the Gmail API.
3. On the Branding page, give an app name, a support email, a home page URL,
   a privacy policy URL and an authorized domain.
4. On the Audience page, select "External".
5. On the Data Access page, add the scope `gmail.readonly`.
6. On the Clients page, create a client of the type "Desktop app".
7. Download the JSON file at once. Since June 2025 the console does not show
   the client secret again after the creation screen closes [9].
8. On the Audience page, click "Publish app".
9. In Iris, give the JSON file. Sign in. On the warning screen "Google hasn't
   verified this app", click "Advanced", then "Go to <app> (unsafe)".

Iris uses the loopback redirect (`http://127.0.0.1:<port>`) with PKCE (Proof
Key for Code Exchange). Google recommends this flow for desktop programs and
calls the desktop client secret not confidential [7].

Sign-in on a remote Linux server: the loopback redirect goes to the machine
that runs the browser. If the browser runs on a laptop and the engine runs on
a server, the server does not receive the authorization code. Iris gives two
ways for this case:

1. Copy the redirect address. After consent, the laptop browser shows an
   error page for `http://127.0.0.1:<port>/...`. The user copies the full
   address from the address bar and gives it to the engine in the web client.
   The engine checks the `state` value and exchanges the code with the PKCE
   verifier. This step adds about 1 minute (estimate). Evidence: pending.
   R1-02 must test that Google accepts the exchange from the server.
2. Use an SSH tunnel: `ssh -L <port>:127.0.0.1:<port> <server>`. Then the
   laptop browser reaches the engine directly. This needs SSH access and
   about 1 minute more (estimate).

| Item | Value |
|---|---|
| Setup time | About 10 to 20 minutes for the first time (estimate) |
| Cost | None |
| Google review | None, under the personal-use exemption [3] |
| Scope | `gmail.readonly`, later `gmail.modify` |
| Works on | Personal Gmail accounts. Workspace accounts if the admin permits the app. |
| Remote server | Needs one extra step: copy the redirect address, or an SSH tunnel |

Limits:

- Do not stay in the "Testing" status. In "Testing", the refresh token
  expires 7 days after consent [1][2]. The user must then sign in again
  each week.
- Google documents no 7-day expiry for "In production" [1]. No Google page
  says this for an unverified app with a restricted scope. Tools such as
  rclone tell users to publish for this reason [13]. Evidence: pending. R1-02
  must check that the token response has no `refresh_token_expires_in` field,
  and that a token still works after 8 days.
- After "Publish app", the user must sign in again. A token from the
  "Testing" status keeps its 7-day limit [17] (single source, low weight).
- "Publish app" needs a home page URL, a privacy policy URL and an authorized
  domain [36]. Google writes: "The homepage must be hosted on a verified
  domain you own" [36]. The same page links the Search Console check to app
  verification, not to "Publish app". So the page does not say if an
  unverified app needs a domain that the user owns. In 2026, rclone users
  reported a refusal for `github.com`, and other users published with no
  verification [14]. Evidence: pending. See "Domain test" below.
- Google gives no API and no `gcloud` command to make a standard OAuth client
  or the Branding and Audience settings [14]. `gcloud` can create the project
  and enable the Gmail API. The other steps stay manual in the browser.
- Google can delete a client that has no token exchange for 6 months [9][10].
  An Iris engine that runs refreshes the token each hour, so this risk is
  low.
- Google revokes a refresh token with a Gmail scope when the user changes
  the account password [1]. A token unused for 6 months also expires [1].
  Spec 1 already has the account state "needs sign-in" for this case.
- Google limits each account to 100 refresh tokens for each client ID. A new
  token over the limit makes the oldest token invalid [1].
- Advanced Protection blocks unverified apps. The sign-in fails with
  "Error 400: policy_enforced", and the user cannot click through [12].
- A Workspace admin can block unverified apps, or permit only trusted apps
  for Gmail. The admin must then mark the client ID as trusted [11]. If the
  user owns the project in the same Workspace organization, the user type
  "Internal" has no warning screen and no 7-day limit [3][8].

Domain test (R1-02 does it first, before other sign-in work):

1. Put a home page and a privacy policy page on an Iris project domain. The
   user does not own this domain and cannot verify it.
2. In a new Google Cloud project, use these URLs and this authorized domain,
   then click "Publish app".
3. If Google accepts, the guided setup uses the Iris pages. No user action
   for the domain is necessary.
4. If Google refuses, test a domain that each user can own and verify: a
   GitHub Pages site at `<user>.github.io`. The `github.io` domain is a public
   suffix, so each `<user>.github.io` is a separate domain. Iris gives a
   template for the two pages. The user verifies the site in Google Search
   Console. Measure the added time.
5. If step 4 also fails, or the total time goes far over 10 minutes, path A
   fails the test. Then the lead asks dk to choose again, with path C as the
   next option.

Risks:

- Google changes these rules often. In 2026, Google told rclone that its
  shared client must stop, and the rclone maintainer wrote "the docs for
  Google oauth stuff are a moving target" [14].
- The warning screen says "unsafe". Some users will stop at that screen. The
  guided setup must explain that the user is the developer of the own app.

### B. A verified Iris app

Setup steps for the user: download Iris, sign in, consent. The setup takes
about 2 minutes.

Steps for the project [15]:

1. Verify a domain in Google Search Console.
2. Publish a privacy policy on the same domain as the home page.
3. Get brand verification (about 2 to 3 business days).
4. Send a scope justification and an unlisted demo video.
5. Follow the Limited Use rules of the Google API Services User Data Policy
   [16].
6. Pass a CASA (Cloud Application Security Assessment) with an App Defense
   Alliance lab [18][19]. Repeat it every 12 months [15][18].

| Item | Value |
|---|---|
| User setup time | About 2 minutes |
| Project time | About 6 to 12 weeks (forum and vendor reports), then each year |
| Cost | About USD 540 to 720 each year at TAC Security (Google discount) [20]. Other labs: about USD 1,800 to 8,000 and more [21][22]. |
| Scope | `gmail.readonly`, later `gmail.modify` |

Limits:

- Google wrote in 2018 that apps that keep data only on the device need no
  full assessment [23]. The current pages do not have this text. In 2024,
  Google required CASA Tier 2 from MailMate and Mimestream, two desktop
  clients that keep mail on the device [21][24]. Do not plan on this
  exemption.
- A shared client puts one client secret in the public Iris code. Google
  policy says: "You must never commit client credentials into publicly
  available code repositories" [10]. Anyone can then use the Iris app
  identity.
- The loopback redirect works only when the browser and the engine are on the
  same machine. A remote server needs the extra step of path A.
- The project is responsible to Google for each install, and the project pays
  each year. MailMate stopped the yearly assessment in 2025 [21].

### C. IMAP with an app password

Setup steps for the user:

1. Turn on 2-Step Verification, if it is not on.
2. Open `https://myaccount.google.com/apppasswords` and make an app password.
3. Give the email address and the app password to Iris.

| Item | Value |
|---|---|
| Setup time | About 2 to 5 minutes if 2-Step Verification is on (estimate) |
| Cost | None |
| Google review | None |
| Access | Full access to all mail. No read-only option. |

Limits:

- App passwords need 2-Step Verification. They are not available with
  Advanced Protection, with security keys as the only second step, or on
  some work and school accounts [25].
- Google revokes all app passwords when the user changes the account
  password [25].
- Google writes that app passwords "aren't recommended" [25]. A vendor blog
  claims removal in 2026, with no Google source [26]. No Google source gives
  an end date.
- A Workspace admin can turn IMAP off, or permit only OAuth clients. Both
  block app passwords [27][28].
- Gmail IMAP has the extensions `X-GM-MSGID`, `X-GM-THRID` and `X-GM-LABELS`,
  so Iris can read message IDs, thread IDs and labels [29]. IMAP has no
  Gmail history API. Spec 1 sync uses `history.list`, so spec 1 needs a
  second sync design.
- The IMAP download limit is 2,500 MB each day [30]. A first copy of a big
  mailbox can take more than one day.
- Gmail permits 15 IMAP connections at a time for each account [31].

Risks:

- Iris holds a credential that can delete all mail. Release 1 promises that
  Iris changes nothing. Only the Iris code can keep this promise, not Google.
- Google can end app passwords with little notice.

### D. IMAP with OAuth (XOAUTH2)

IMAP with OAuth needs the scope `https://mail.google.com/`. Google puts this
scope in the restricted class [6][32]. Path D has the limits of path A or
path B, and the IMAP limits of path C. Do not use path D.

### E. The macOS Mail app as the data source

Iris reads the local store of Apple Mail: the SQLite file
`~/Library/Mail/V10/MailData/Envelope Index` and the `.emlx` files [33].

Limits:

- The path works only on macOS. Iris must also run on Linux.
- Apple does not document the format, and the format changes between macOS
  releases [33].
- The Iris process needs Full Disk Access. Label changes must go through
  AppleScript, which is slow and needs the Automation permission [33].

Do not use path E for Iris.

### F. Google Takeout or Gmail forwarding

Takeout gives a one-time MBOX export that expires in about 7 days [34].
Forwarding sends only new mail, with no labels, and needs a public mail
address [35]. Neither path gives live access or label changes. Do not use
these paths.

## Comparison

| Path | User setup | Cost | Read-only scope | Linux server | Spec 1 change | Main risk |
|---|---|---|---|---|---|---|
| A. Own client | 10 to 20 min (estimate) | None | Yes | Extra redirect step | None | Google rule changes, "unsafe" screen |
| B. Verified app | About 2 min | USD 540+ each year | Yes | Extra redirect step | None | Weeks of review, yearly cost, public secret |
| C. IMAP app password | 2 to 5 min | None | No | Yes | New sync design | Full-access credential, removal by Google |
| D. IMAP OAuth | As A or B | As A or B | No | Yes | New sync design | Limits of A or B, plus C |
| E. macOS Mail | About 5 min | None | Yes | No | New source | Mac only, format changes |

## What the decision changes

- Spec 1 section 4.9: the client ID and the client secret come from the JSON
  file of the user. Iris does not ship a client ID.
- R1-02 (sign-in) builds the guided setup and measures the setup time.
- The Iris documentation needs a page for the home page URL and a privacy
  policy page for path A. The privacy policy must keep two cases apart:
  - The Iris install of the user stores a local copy of each message,
    including the raw mail (spec 1). This data stays on the machine of the
    user, under the control of the user.
  - The Iris project receives no mail data and no usage data from an
    install.
- Path B stays open for a later release, when the project has users and a
  budget. Path C stays open as a second provider.

## Open questions for R1-02

1. Does the domain test of path A pass, and at which step?
2. Does a token from an unverified "In production" client with
   `gmail.readonly` still work after 8 days?
3. How long does a real setup take on dk's Mac, from the start of setup to
   the first brief? Also record the time to the end of sign-in. These real
   measures are added to the M4 measure of spec 1 (connect step to first
   brief, with the synthetic mailbox and the fake Gmail server). R1-02 must
   also run the M4 measure.
4. Does the copy of the redirect address work for an engine on a remote
   Linux server?

## Sources

All sources were read on 2026-11-15. "Official" means a Google or Apple page.

1. Using OAuth 2.0 to Access Google APIs (official).
   https://developers.google.com/identity/protocols/oauth2
2. Manage App Audience (official).
   https://support.google.com/cloud/answer/15549945
3. When is verification not needed (official).
   https://support.google.com/cloud/answer/13464323
4. Verification requirements (official).
   https://support.google.com/cloud/answer/13464321
5. Unverified apps (official).
   https://support.google.com/cloud/answer/7454865
6. Gmail API scopes (official).
   https://developers.google.com/workspace/gmail/api/auth/scopes
7. OAuth 2.0 for iOS and Desktop Apps (official).
   https://developers.google.com/identity/protocols/oauth2/native-app
8. Configure the OAuth consent screen (official).
   https://developers.google.com/workspace/guides/configure-oauth-consent
9. Usability and safety updates to Google Auth Platform, 2025-04-28
   (official). https://developers.googleblog.com/en/usability-and-safety-updates-to-google-auth-platform/
10. OAuth 2.0 Policies (official).
    https://developers.google.com/identity/protocols/oauth2/policies
11. Control which apps access Google Workspace data (official).
    https://knowledge.workspace.google.com/admin/apps/control-which-apps-access-google-workspace-data
12. Advanced Protection Program FAQ (official).
    https://support.google.com/accounts/answer/7539956
13. rclone Google Drive docs, "Making your own client_id".
    https://rclone.org/drive/
14. rclone forum, 2026 threads on the own client ID.
    https://forum.rclone.org/t/google-drive-and-google-photos-users-action-required/54005 ,
    https://forum.rclone.org/t/clientid-automation-winter-is-coming-deadline-closing-in/54145 ,
    https://forum.rclone.org/t/unable-to-publish-google-drive-client-as-docs-advise/54197
15. Restricted scope verification (official).
    https://developers.google.com/identity/protocols/oauth2/production-readiness/restricted-scope-verification
16. Google API Services User Data Policy (official).
    https://developers.google.com/terms/api-services-user-data-policy
17. Guide to non-expiring refresh tokens (community, low weight).
    https://github.com/MankhongGarden/google-oauth-nonexpiring-refresh-token
18. Security assessment (official).
    https://support.google.com/cloud/answer/13465431
19. App Defense Alliance, CASA and CASA tiering.
    https://appdefensealliance.dev/casa ,
    https://appdefensealliance.dev/casa/casa-tiering
20. TAC Security CASA prices (vendor). https://tacsecurity.com/esof/
21. MailMate mailing list, 2024 and 2025 (developer reports).
    https://lists.freron.com/mailmate/2024-June/017271.html ,
    https://lists.freron.com/mailmate/2025-April/018270.html
22. Google Apps Script community, "Casa verification", 2024-05-03.
    https://groups.google.com/g/google-apps-script-community/c/XgsHT2Fr7u8
23. Elevating user trust in our API ecosystem, 2018-10-08 (official).
    https://developers.googleblog.com/en/elevating-user-trust-in-our-api-ecosystem/
24. Mimestream, "CASA verified", 2024-06-24.
    https://mimestream.com/blog/casa-verified
25. Sign in with app passwords (official).
    https://support.google.com/accounts/answer/185833
26. Vendor claim of app password removal (low weight, no Google source).
    https://www.getmailbird.com/gmail-oauth-changes-app-password-phase-out/
27. Transition from less secure apps to OAuth (official).
    https://knowledge.workspace.google.com/admin/sync/transition-from-less-secure-apps-to-oauth
28. Turn POP and IMAP on or off for users (official).
    https://knowledge.workspace.google.com/admin/sync/turn-pop-and-imap-on-or-off-for-users
29. Gmail IMAP Extensions (official).
    https://developers.google.com/workspace/gmail/imap/imap-extensions
30. Gmail bandwidth limits (official, written for Workspace).
    https://knowledge.workspace.google.com/admin/gmail/gmail-bandwidth-limits
31. Add Gmail to another email client (official).
    https://support.google.com/mail/answer/7126229
32. IMAP, POP and SMTP OAuth protocol (official).
    https://developers.google.com/workspace/gmail/imap/xoauth2-protocol
33. Apple Mail cache structure (community).
    https://chrispatten.github.io/haven/reference/mail_app_cache_structure
34. Download your Google data (official).
    https://support.google.com/accounts/answer/3024190
35. Forward Gmail messages to another account (official).
    https://support.google.com/mail/answer/10957
36. Manage OAuth App Branding (official).
    https://support.google.com/cloud/answer/15549049
