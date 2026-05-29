AI Assist Gateway (ai-assist-gateway)

Overview

This Edge Function proxies calls from the CLI to Backboard so the Backboard API key can stay in Supabase secrets.
The client sends a normal Supabase bearer token and prompt payload; the function validates the user session and forwards
request data to Backboard with `X-API-Key`.

Required Supabase function secrets (production):

- `SERVICE_ROLE_KEY` (required) — validates bearer tokens using Supabase Auth.
- `BACKBOARD_API_KEY_CAESIM` or `BACKBOARD_API_KEY` (required) — Backboard API key.

Optional:

- `BACKBOARD_API_BASE` (default: `https://app.backboard.io/api`)
- `SUPABASE_URL` (only needed for local function serving outside `*.functions.supabase.co`)

Deploy

```bash
supabase functions deploy ai-assist-gateway --no-verify-jwt
supabase secrets set SERVICE_ROLE_KEY="$SERVICE_ROLE_KEY" BACKBOARD_API_KEY_CAESIM="$BACKBOARD_API_KEY_CAESIM"
```

Client configuration

Set this in the CLI environment:

```bash
export BACKBOARD_GATEWAY_URL="https://<project>.functions.supabase.co/ai-assist-gateway"
```

With `BACKBOARD_GATEWAY_URL` set, `caesim ai-assist` uses this proxy and does not require local Backboard API keys.
