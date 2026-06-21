Supabase setup for caesim

This document outlines the minimal steps to set up the Supabase schema, RLS policies, deploy the credit gateway, and test the end-to-end flow.

1) Apply database migration

Use psql or the Supabase SQL editor to run the migration:

```bash
# from repo root
psql "$(supabase db connection-string)" -f supabase/migrations/001_credit_tables.sql
psql "$(supabase db connection-string)" -f supabase/migrations/002_payments.sql
psql "$(supabase db connection-string)" -f supabase/migrations/003_auth_user_row_trigger.sql
# or paste the SQL into the Supabase SQL editor
```

2) Deploy the credit gateway function

See `supabase/functions/credit-gateway/README.md` for deployment and local testing instructions. Make sure to set the following cloud config in your project:

- `SERVICE_ROLE_KEY` (service role) — used by the gateway to mutate rows and validate user sessions in production
- `STRIPE_SECRET_KEY` — used by the gateway to create Stripe Checkout Sessions
- `STRIPE_WEBHOOK_SECRET` — used by the gateway to verify Stripe webhooks
- `CREDIT_PACK_PRICE_CENTS=169` — charges $1.69 per 1000-credit pack

`SUPABASE_URL` is optional for local testing only; the deployed function derives the project URL from the Supabase function host.

2b) Deploy the AI assist gateway function (for production Backboard key handling)

See `supabase/functions/ai-assist-gateway/README.md` for details.

Set these Supabase function secrets:

- `SERVICE_ROLE_KEY` (service role)
- `BACKBOARD_API_KEY_CAESIM` (or `BACKBOARD_API_KEY`)

Deploy:

```bash
supabase functions deploy ai-assist-gateway --no-verify-jwt
supabase secrets set SERVICE_ROLE_KEY="$SERVICE_ROLE_KEY" BACKBOARD_API_KEY_CAESIM="$BACKBOARD_API_KEY_CAESIM"
```

The payment foundation is also prepared here:

- `public.users` stores `stripe_customer_id`, billing status, and balance metadata.
- `public.credit_ledger` keeps an append-only audit trail for every credit delta.
- `public.payment_events` records Stripe-ready payment events so a webhook can replay safely later.
- `public.apply_credit_change(...)` is the atomic database helper the gateway uses for grants and consumes.
- `public.record_payment_event(...)` is the Stripe-ready entry point for a future verified webhook.

3) Configure client CLI

Locally, set the gateway URL so the CLI uses it for balance/consume operations:

```bash
export CREDIT_GATEWAY_URL="https://<project>.functions.supabase.co/credit-gateway"
export BACKBOARD_GATEWAY_URL="https://<project>.functions.supabase.co/ai-assist-gateway"
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
  -H "Authorization: Bearer $SESSION_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"action":"grant","user_id":"<uuid>","email":"user@example.com","amount":100}' | jq
```

Stripe-ready payment event:

```bash
curl -X POST "$GATEWAY_URL" \
  -H "Authorization: Bearer $SESSION_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"action":"payment","provider":"stripe","provider_event_id":"evt_123","user_id":"<uuid>","amount":2000,"credits_granted":100,"currency":"usd","status":"succeeded"}' | jq
```

6) Run CLI flow

With `CREDIT_GATEWAY_URL` set, the CLI will automatically use the gateway for balance checks and consume operations.

```bash
caesim login --email test@example.com --otp
caesim credits balance
caesim credits buy --credits 1000
caesim cut <path> --rule duplicates --find cars
```

Notes and security

- Never check service-role keys or `.env` files into version control.
- The gateway should be deployed in Supabase and secrets configured there.
- The database migration above is intentionally minimal; adapt column types and constraints for your production needs.
