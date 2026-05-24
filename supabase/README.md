Supabase setup for caesim

This document outlines the minimal steps to set up the Supabase schema, RLS policies, deploy the credit gateway, and test the end-to-end flow.

1) Apply database migration

Use psql or the Supabase SQL editor to run the migration:

```bash
# from repo root
psql "$(supabase db connection-string)" -f supabase/migrations/001_credit_tables.sql
# or paste the SQL into the Supabase SQL editor
```

2) Deploy the credit gateway function

See `supabase/functions/credit-gateway/README.md` for deployment and local testing instructions. Make sure to set the following secrets in your project:

- `SERVICE_ROLE_KEY` (service role) — used by the gateway to mutate rows
- `PUBLISHABLE_KEY` (publishable) — used by the gateway to validate user sessions
- `CREDIT_ADMIN_TOKEN` — admin token for grant operations

3) Configure client CLI

Locally, set the gateway URL so the CLI uses it for balance/consume operations:

```bash
export CREDIT_GATEWAY_URL="https://<project>.functions.supabase.co/credit-gateway"
```

4) Create test user & obtain session token

- Use the CLI to `caesim signup --email test@example.com` and complete OTP flow, or use the Supabase auth email flow.
- After login, the CLI stores a session at the usual config path. You can extract the access token from the session file (`~/.config/caesim/session.json`).

5) Test via curl

Balance:

```bash
SESSION_TOKEN=$(jq -r .session_token ~/.config/caesim/session.json)
GATEWAY_URL=${CREDIT_GATEWAY_URL}

curl -X POST "$GATEWAY_URL" \
  -H "Authorization: Bearer $SESSION_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"action":"balance"}' | jq
```

Consume:

```bash
curl -X POST "$GATEWAY_URL" \
  -H "Authorization: Bearer $SESSION_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"action":"consume","amount":1}' | jq
```

Grant (admin):

```bash
curl -X POST "$GATEWAY_URL" \
  -H "x-caesim-admin-token: $CREDIT_ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"action":"grant","user_id":"<uuid>","email":"user@example.com","amount":100}' | jq
```

6) Run CLI flow

With `CREDIT_GATEWAY_URL` set, the CLI will automatically use the gateway for balance checks and consume operations.

```bash
caesim login --email test@example.com --otp
caesim credits balance
caesim cut <path> --rule duplicates --find cars
```

Notes and security

- Never check service-role keys or `.env` files into version control.
- The gateway should be deployed in Supabase and secrets configured there.
- The database migration above is intentionally minimal; adapt column types and constraints for your production needs.
