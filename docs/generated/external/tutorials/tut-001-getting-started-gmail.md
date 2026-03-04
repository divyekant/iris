---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Tutorial: Set Up Iris and Connect Gmail

This tutorial walks you through setting up Iris from scratch and connecting your first Gmail account via OAuth. By the end, you will have Iris running locally with your Gmail inbox synced and viewable.

**Time required:** About 15 minutes.

## Prerequisites

- Rust 1.85 or later (`rustup update stable`)
- Node.js 20 or later
- A Gmail account
- A Google Cloud project with the Gmail API enabled (you will create one if you do not already have it)

## Step 1: Clone and Configure Iris

Clone the Iris repository and set up your environment file:

```bash
git clone https://github.com/your-org/iris.git
cd iris
cp .env.example .env
```

## Step 2: Create Google OAuth Credentials

You need OAuth credentials so Iris can authenticate with Gmail on your behalf.

1. Go to the [Google Cloud Console](https://console.cloud.google.com/).
2. Create a new project or select an existing one.
3. In the left sidebar, go to **APIs & Services > Library**.
4. Search for **Gmail API** and click **Enable**.
5. Go to **APIs & Services > Credentials**.
6. Click **Create Credentials > OAuth 2.0 Client ID**.
7. If prompted, configure the OAuth consent screen:
   - Choose **External** user type (or Internal if you have a Workspace account).
   - Fill in the app name (e.g., "Iris Email Client") and your email.
   - Add the scope `https://mail.google.com/`.
   - Save and continue.
8. Back on the credentials page, set:
   - **Application type:** Web application
   - **Name:** Iris
   - **Authorized redirect URIs:** `http://localhost:3000/auth/callback`
9. Click **Create**.
10. Copy the **Client ID** and **Client Secret**.

## Step 3: Configure Environment Variables

Open `.env` in your editor and add the Gmail credentials:

```bash
GMAIL_CLIENT_ID=your-client-id-here.apps.googleusercontent.com
GMAIL_CLIENT_SECRET=GOCSPX-your-secret-here
PUBLIC_URL=http://localhost:3000
```

Leave the other variables at their defaults for now.

## Step 4: Build and Start the Backend

```bash
cargo build --release
cargo run --release
```

You should see output like:

```
Iris listening on 127.0.0.1:3000
```

Leave this terminal running.

## Step 5: Build and Start the Frontend

Open a new terminal in the project directory:

```bash
cd web
npm install
npm run dev
```

The frontend dev server starts on `http://localhost:5173`.

## Step 6: Verify the Server Is Running

In a third terminal, check the health endpoint:

```bash
curl http://localhost:3000/api/health
```

You should see:

```json
{
  "status": "ok",
  "version": "0.1.0",
  "db": true,
  "ollama": false,
  "memories": false
}
```

It is fine that `ollama` and `memories` are `false` -- those are optional AI services.

## Step 7: Connect Your Gmail Account

1. Open your browser and navigate to [http://localhost:5173](http://localhost:5173).
2. You should see the Iris welcome page or account setup screen.
3. Click **Add Account** and select **Gmail**.
4. You are redirected to Google. Sign in with the Gmail account you want to connect.
5. Grant Iris the requested permissions. Google may show a warning about an unverified app -- click **Advanced** and **Go to Iris** to proceed.
6. After authorization, you are redirected back to Iris.

## Step 8: Wait for Initial Sync

Once your account is connected, Iris begins syncing your inbox. You can watch the backend terminal for sync progress logs:

```
IMAP sync started for your@gmail.com
Synced 247 messages for your@gmail.com
IMAP IDLE started for your@gmail.com
```

The initial sync downloads your existing messages. After that, IMAP IDLE keeps your inbox up to date in real-time.

## Step 9: Browse Your Inbox

Switch back to the browser. Your inbox should now show your Gmail messages. You can:

- Click any message to read it in the thread view
- Use the search bar to find specific emails
- Star, archive, or delete messages
- Click **Compose** to write a new email

## What's Next

Your Gmail account is connected and syncing. Here are some next steps:

- **Add more accounts** -- connect Outlook or IMAP accounts from **Settings > Accounts**
- **Enable AI features** -- install Ollama and follow the [Using AI Features](tut-002-using-ai-features.md) tutorial
- **Connect an AI agent** -- create API keys and use the Agent API with the [Connecting an Agent](tut-003-connecting-an-agent.md) tutorial
- **Explore search** -- try keyword and semantic search from the [Search](../features/feat-002-search.md) docs

## Troubleshooting

**"redirect_uri_mismatch" error from Google:**
Make sure the redirect URI in your Google Cloud Console credentials exactly matches `http://localhost:3000/auth/callback`. Include the full URL with protocol and path.

**Sync does not start:**
Check the backend terminal for error messages. Common issues include expired OAuth tokens (re-add the account) or network connectivity problems.

**Cannot reach http://localhost:5173:**
Make sure the frontend dev server is running (`npm run dev` in the `web` directory). If you are using Docker, the frontend is bundled at `http://localhost:3000` instead.

**Google shows "unverified app" warning:**
This is expected for development credentials. Click **Advanced** then **Go to Iris (unsafe)** to proceed. For production use, you would submit your app for Google verification.
