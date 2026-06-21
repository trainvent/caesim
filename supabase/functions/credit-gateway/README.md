Credit Gateway (credit-gateway) — deployment & local testing

Overview

This Edge Function mediates authoritative credit operations (balance, consume, grant,
Checkout, and payment intake). It authenticates the client via their Supabase JWT
(Authorization: Bearer <token>) and uses the Supabase service-role key to call
transactional database functions for credit mutations.

Required environment variables (set in Supabase dashboard or local .env):

- SERVICE_ROLE_KEY (service role) — required for credit mutation RPCs and auth lookups in production
- STRIPE_SECRET_KEY — required to create Stripe Checkout Sessions
- STRIPE_WEBHOOK_SECRET — required to verify Stripe webhook signatures

Optional:

- SUPABASE_URL: https://<project>.supabase.co
- CREDIT_PACK_PRICE_CENTS: price for 1000 credits, defaults to 169
- SITE_URL: used to build default Checkout success/cancel URLs
- STRIPE_CHECKOUT_SUCCESS_URL and STRIPE_CHECKOUT_CANCEL_URL: explicit Checkout redirect URLs

Deploying

Using the Supabase CLI (recommended):

1. Authenticate and set project ref if needed:

```bash
supabase login
supabase link --project-ref <your-project-ref>
```

2. Deploy the function:

```bash
supabase functions deploy credit-gateway --no-verify-jwt
```

3. Set the cloud config in the Supabase dashboard or via the CLI. Only the service-role key is required in production:

```bash
supabase secrets set \
  SERVICE_ROLE_KEY="$SERVICE_ROLE_KEY" \
  STRIPE_SECRET_KEY="$STRIPE_SECRET_KEY" \
  STRIPE_WEBHOOK_SECRET="$STRIPE_WEBHOOK_SECRET" \
  CREDIT_PACK_PRICE_CENTS=169
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

Create Stripe Checkout Session:

```bash
curl -s -X POST "$GATEWAY_URL" \
  -H "Authorization: Bearer $SESSION" \
  -H "Content-Type: application/json" \
  -d '{"action":"checkout","credits_granted":1000}'
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

- The function expects the Supabase service-role key to call transactional RPCs and auth lookups in production. Never embed this key in the client.
- The CLI delegates balance and consume operations to this gateway when `CREDIT_GATEWAY_URL` is set.
- Register a Stripe webhook endpoint for `checkout.session.completed` pointing at the deployed credit gateway URL. The function is deployed with JWT verification disabled so Stripe can call it; Stripe webhook requests are verified with `STRIPE_WEBHOOK_SECRET`.
- The default price is 169 cents per 1000 credits.
- Deploy and secrets management should be done using Supabase's dashboard or the CLI secrets management.
