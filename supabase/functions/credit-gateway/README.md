Credit Gateway (credit-gateway) — deployment & local testing

Overview

This Edge Function mediates authoritative credit operations (balance, consume, grant).
It authenticates the client via their Supabase JWT (Authorization: Bearer <token>) and
uses the Supabase service-role key to perform upserts / mutations on the public users table.

Required environment variables (set in Supabase dashboard or local .env):

- SUPABASE_URL: https://<project>.supabase.co
- SERVICE_ROLE_KEY (service role) — required for upserts
- PUBLISHABLE_KEY (publishable key) — used for auth lookups
- CREDIT_ADMIN_TOKEN — an admin token used by the CLI/CI to perform grants

Deploying

Using the Supabase CLI (recommended):

1. Authenticate and set project ref if needed:

```bash
supabase login
supabase link --project-ref <your-project-ref>
```

2. Deploy the function:

```bash
supabase functions deploy credit-gateway --no-verify
```

3. Set the secrets in the Supabase dashboard or via the CLI:

```bash
supabase secrets set SERVICE_ROLE_KEY="$SERVICE_ROLE_KEY" PUBLISHABLE_KEY="$PUBLISHABLE_KEY" CREDIT_ADMIN_TOKEN="$CREDIT_ADMIN_TOKEN"
```

Local testing

1. Create a local `.env` file with the required variables (never commit this file):

```
SUPABASE_URL=https://<project>.supabase.co
PUBLISHABLE_KEY=<anon key>
SERVICE_ROLE_KEY=<service role key>
CREDIT_ADMIN_TOKEN=<admin token for grant>
```

2. Serve the function locally:

```bash
supabase functions serve credit-gateway --env-file .env
```

3. Example curl calls (replace $GATEWAY_URL with the local server address, and $SESSION with a real user session token):

Balance:

```bash
curl -s -X POST "$GATEWAY_URL" \
  -H "Authorization: Bearer $SESSION" \
  -H "Content-Type: application/json" \
  -d '{"action":"balance"}'
```

Consume:

```bash
curl -s -X POST "$GATEWAY_URL" \
  -H "Authorization: Bearer $SESSION" \
  -H "Content-Type: application/json" \
  -d '{"action":"consume","amount":10}'
```

Grant (admin only):

```bash
curl -s -X POST "$GATEWAY_URL" \
  -H "x-caesim-admin-token: $CREDIT_ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"action":"grant","user_id":"<uuid>","email":"user@example.com","amount":100}'
```

Notes

- The function expects the Supabase service-role key to perform upserts. Never embed this key in the client.
- The CLI delegates balance and consume operations to this gateway when `CREDIT_GATEWAY_URL` is set.
- Deploy and secrets management should be done using Supabase's dashboard or the CLI secrets management.
