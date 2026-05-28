Credit Gateway (credit-gateway) — deployment & local testing

Overview

This Edge Function mediates authoritative credit operations (balance, consume, grant,
and payment intake). It authenticates the client via their Supabase JWT
(Authorization: Bearer <token>) and uses the Supabase service-role key to call
transactional database functions for credit mutations.

Required environment variables (set in Supabase dashboard or local .env):

- SUPABASE_URL: https://<project>.supabase.co
- SERVICE_ROLE_KEY (service role) — required for credit mutation RPCs and auth lookups

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

3. Set the cloud config in the Supabase dashboard or via the CLI. Only the service-role key is sensitive; the project URL can be copied from your `.env` file:

```bash
supabase secrets set SERVICE_ROLE_KEY="$SERVICE_ROLE_KEY" SUPABASE_URL="$PROJECT_URL"
```

Local testing

1. Create a local `.env` file with the required variables (never commit this file):

```
SUPABASE_URL=https://<project>.supabase.co
SERVICE_ROLE_KEY=<service role key>
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
  -H "Authorization: Bearer $SESSION" \
  -H "Content-Type: application/json" \
  -d '{"action":"grant","user_id":"<uuid>","email":"user@example.com","amount":100}'
```

Payment intake for later Stripe webhooks:

```bash
curl -s -X POST "$GATEWAY_URL" \
  -H "Authorization: Bearer $SESSION" \
  -H "Content-Type: application/json" \
  -d '{"action":"payment","provider":"stripe","provider_event_id":"evt_123","user_id":"<uuid>","amount":2000,"credits_granted":100,"currency":"usd","status":"succeeded"}'
```

Notes

- The function expects the Supabase service-role key to call transactional RPCs and auth lookups. Never embed this key in the client.
- The CLI delegates balance and consume operations to this gateway when `CREDIT_GATEWAY_URL` is set.
- Deploy and secrets management should be done using Supabase's dashboard or the CLI secrets management.
