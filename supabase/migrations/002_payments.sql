-- 002_payments.sql
-- Adds payment-ready metadata, a payment event table, and transactional
-- helpers for credit mutations so Stripe can plug into the same flow later.

ALTER TABLE public.users
  ADD COLUMN IF NOT EXISTS stripe_customer_id text,
  ADD COLUMN IF NOT EXISTS billing_email text,
  ADD COLUMN IF NOT EXISTS billing_status text DEFAULT 'inactive',
  ADD COLUMN IF NOT EXISTS billing_updated_at bigint;

CREATE UNIQUE INDEX IF NOT EXISTS idx_public_users_stripe_customer_id
  ON public.users (stripe_customer_id)
  WHERE stripe_customer_id IS NOT NULL;

ALTER TABLE public.credit_ledger
  ADD COLUMN IF NOT EXISTS source text NOT NULL DEFAULT 'manual',
  ADD COLUMN IF NOT EXISTS source_ref text,
  ADD COLUMN IF NOT EXISTS balance_before bigint,
  ADD COLUMN IF NOT EXISTS balance_after bigint,
  ADD COLUMN IF NOT EXISTS metadata jsonb NOT NULL DEFAULT '{}'::jsonb;

CREATE INDEX IF NOT EXISTS idx_credit_ledger_source_source_ref
  ON public.credit_ledger (source, source_ref);

CREATE TABLE IF NOT EXISTS public.payment_events (
  id bigserial PRIMARY KEY,
  provider text NOT NULL DEFAULT 'stripe',
  provider_event_id text NOT NULL,
  user_id uuid,
  email text,
  stripe_customer_id text,
  stripe_checkout_session_id text,
  stripe_payment_intent_id text,
  amount_cents bigint NOT NULL,
  currency text NOT NULL DEFAULT 'usd',
  credits_granted bigint NOT NULL DEFAULT 0,
  status text NOT NULL DEFAULT 'pending',
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
  created_at bigint NOT NULL DEFAULT (extract(epoch from now())::bigint),
  processed_at bigint,
  CONSTRAINT payment_events_provider_event_id_key UNIQUE (provider, provider_event_id)
);

ALTER TABLE public.payment_events ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS select_own_payment_events ON public.payment_events;
CREATE POLICY select_own_payment_events ON public.payment_events
  FOR SELECT
  USING (auth.uid()::text = user_id::text);

-- Inserts and updates happen through service-role-backed functions.

CREATE OR REPLACE FUNCTION public.apply_credit_change(
  p_user_id uuid,
  p_delta bigint,
  p_reason text DEFAULT NULL,
  p_source text DEFAULT 'manual',
  p_source_ref text DEFAULT NULL,
  p_metadata jsonb DEFAULT '{}'::jsonb,
  p_email text DEFAULT NULL,
  p_auth_provider text DEFAULT NULL,
  p_plan text DEFAULT NULL,
  p_account_status text DEFAULT NULL,
  p_stripe_customer_id text DEFAULT NULL,
  p_billing_email text DEFAULT NULL,
  p_billing_status text DEFAULT NULL,
  p_last_seen_at bigint DEFAULT NULL
)
RETURNS TABLE (
  user_id uuid,
  credit_balance bigint,
  balance_before bigint,
  balance_after bigint,
  ledger_id bigint
)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public
AS $$
DECLARE
  v_now bigint := coalesce(p_last_seen_at, extract(epoch from now())::bigint);
  v_row public.users%ROWTYPE;
  v_before bigint;
  v_after bigint;
  v_ledger_id bigint;
BEGIN
  IF p_delta = 0 THEN
    RAISE EXCEPTION 'delta must not be zero';
  END IF;

  INSERT INTO public.users (
    id,
    email,
    auth_provider,
    plan,
    created_at,
    last_seen_at,
    account_status,
    credit_balance,
    stripe_customer_id,
    billing_email,
    billing_status,
    billing_updated_at
  )
  VALUES (
    p_user_id,
    NULLIF(btrim(p_email), ''),
    COALESCE(NULLIF(btrim(p_auth_provider), ''), 'email'),
    COALESCE(NULLIF(btrim(p_plan), ''), 'beta'),
    v_now,
    v_now,
    COALESCE(NULLIF(btrim(p_account_status), ''), 'active'),
    0,
    NULLIF(btrim(p_stripe_customer_id), ''),
    NULLIF(btrim(p_billing_email), ''),
    COALESCE(NULLIF(btrim(p_billing_status), ''), 'inactive'),
    v_now
  )
  ON CONFLICT (id) DO UPDATE
  SET
    email = COALESCE(EXCLUDED.email, public.users.email),
    auth_provider = COALESCE(EXCLUDED.auth_provider, public.users.auth_provider),
    plan = COALESCE(EXCLUDED.plan, public.users.plan),
    last_seen_at = GREATEST(COALESCE(public.users.last_seen_at, 0), EXCLUDED.last_seen_at),
    account_status = COALESCE(EXCLUDED.account_status, public.users.account_status),
    stripe_customer_id = COALESCE(EXCLUDED.stripe_customer_id, public.users.stripe_customer_id),
    billing_email = COALESCE(EXCLUDED.billing_email, public.users.billing_email),
    billing_status = COALESCE(EXCLUDED.billing_status, public.users.billing_status),
    billing_updated_at = GREATEST(COALESCE(public.users.billing_updated_at, 0), EXCLUDED.billing_updated_at);

  SELECT *
  INTO v_row
  FROM public.users
  WHERE id::text = p_user_id::text
  FOR UPDATE;

  v_before := COALESCE(v_row.credit_balance, 0);
  v_after := v_before + p_delta;

  IF v_after < 0 THEN
    RAISE EXCEPTION 'insufficient credits';
  END IF;

  UPDATE public.users
  SET
    email = COALESCE(NULLIF(btrim(p_email), ''), email),
    auth_provider = COALESCE(NULLIF(btrim(p_auth_provider), ''), auth_provider),
    plan = COALESCE(NULLIF(btrim(p_plan), ''), plan),
    account_status = COALESCE(NULLIF(btrim(p_account_status), ''), account_status),
    credit_balance = v_after,
    last_seen_at = v_now,
    stripe_customer_id = COALESCE(NULLIF(btrim(p_stripe_customer_id), ''), stripe_customer_id),
    billing_email = COALESCE(NULLIF(btrim(p_billing_email), ''), billing_email),
    billing_status = COALESCE(NULLIF(btrim(p_billing_status), ''), billing_status),
    billing_updated_at = v_now
  WHERE id::text = p_user_id::text
  RETURNING * INTO v_row;

  INSERT INTO public.credit_ledger (
    user_id,
    delta,
    reason,
    source,
    source_ref,
    balance_before,
    balance_after,
    metadata,
    created_at
  )
  VALUES (
    p_user_id,
    p_delta,
    p_reason,
    p_source,
    p_source_ref,
    v_before,
    v_after,
    COALESCE(p_metadata, '{}'::jsonb),
    v_now
  )
  RETURNING id INTO v_ledger_id;

  user_id := p_user_id;
  credit_balance := v_row.credit_balance;
  balance_before := v_before;
  balance_after := v_after;
  ledger_id := v_ledger_id;
  RETURN NEXT;
END;
$$;

CREATE OR REPLACE FUNCTION public.record_payment_event(
  p_provider text,
  p_provider_event_id text,
  p_user_id uuid,
  p_amount_cents bigint,
  p_currency text DEFAULT 'usd',
  p_credits_granted bigint DEFAULT 0,
  p_status text DEFAULT 'pending',
  p_metadata jsonb DEFAULT '{}'::jsonb,
  p_email text DEFAULT NULL,
  p_stripe_customer_id text DEFAULT NULL,
  p_stripe_checkout_session_id text DEFAULT NULL,
  p_stripe_payment_intent_id text DEFAULT NULL
)
RETURNS TABLE (
  payment_event_id bigint,
  user_id uuid,
  credit_balance bigint,
  balance_before bigint,
  balance_after bigint,
  ledger_id bigint
)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public
AS $$
DECLARE
  v_now bigint := extract(epoch from now())::bigint;
  v_event public.payment_events%ROWTYPE;
  v_credit RECORD;
  v_should_credit boolean := lower(COALESCE(p_status, 'pending')) IN ('succeeded', 'paid', 'completed');
BEGIN
  IF btrim(COALESCE(p_provider, '')) = '' THEN
    RAISE EXCEPTION 'provider is required';
  END IF;

  IF btrim(COALESCE(p_provider_event_id, '')) = '' THEN
    RAISE EXCEPTION 'provider_event_id is required';
  END IF;

  IF p_amount_cents < 0 THEN
    RAISE EXCEPTION 'amount_cents must be non-negative';
  END IF;

  IF p_credits_granted < 0 THEN
    RAISE EXCEPTION 'credits_granted must be non-negative';
  END IF;

  INSERT INTO public.payment_events AS payment_events (
    provider,
    provider_event_id,
    user_id,
    email,
    stripe_customer_id,
    stripe_checkout_session_id,
    stripe_payment_intent_id,
    amount_cents,
    currency,
    credits_granted,
    status,
    metadata,
    created_at,
    processed_at
  )
  VALUES (
    p_provider,
    p_provider_event_id,
    p_user_id,
    NULLIF(btrim(p_email), ''),
    NULLIF(btrim(p_stripe_customer_id), ''),
    NULLIF(btrim(p_stripe_checkout_session_id), ''),
    NULLIF(btrim(p_stripe_payment_intent_id), ''),
    p_amount_cents,
    COALESCE(NULLIF(btrim(p_currency), ''), 'usd'),
    p_credits_granted,
    COALESCE(NULLIF(btrim(p_status), ''), 'pending'),
    COALESCE(p_metadata, '{}'::jsonb),
    v_now,
    NULL
  )
  ON CONFLICT (provider, provider_event_id) DO UPDATE
  SET
    user_id = COALESCE(EXCLUDED.user_id, payment_events.user_id),
    email = COALESCE(EXCLUDED.email, payment_events.email),
    stripe_customer_id = COALESCE(EXCLUDED.stripe_customer_id, payment_events.stripe_customer_id),
    stripe_checkout_session_id = COALESCE(EXCLUDED.stripe_checkout_session_id, payment_events.stripe_checkout_session_id),
    stripe_payment_intent_id = COALESCE(EXCLUDED.stripe_payment_intent_id, payment_events.stripe_payment_intent_id),
    amount_cents = EXCLUDED.amount_cents,
    currency = EXCLUDED.currency,
    credits_granted = EXCLUDED.credits_granted,
    status = EXCLUDED.status,
    metadata = COALESCE(EXCLUDED.metadata, payment_events.metadata),
    processed_at = payment_events.processed_at
  RETURNING * INTO v_event;

  payment_event_id := v_event.id;
  user_id := v_event.user_id;

  IF v_event.processed_at IS NOT NULL THEN
    credit_balance := NULL;
    balance_before := NULL;
    balance_after := NULL;
    ledger_id := NULL;
    RETURN NEXT;
    RETURN;
  END IF;

  IF v_should_credit AND COALESCE(v_event.credits_granted, 0) > 0 THEN
    SELECT *
    INTO v_credit
    FROM public.apply_credit_change(
      p_user_id := v_event.user_id,
      p_delta := v_event.credits_granted,
      p_reason := COALESCE('payment:' || v_event.provider, 'payment'),
      p_source := v_event.provider,
      p_source_ref := v_event.provider || ':' || v_event.provider_event_id,
      p_metadata := COALESCE(v_event.metadata, '{}'::jsonb),
      p_email := v_event.email,
      p_stripe_customer_id := v_event.stripe_customer_id,
      p_billing_email := v_event.email,
      p_billing_status := 'active',
      p_last_seen_at := v_now
    );

    UPDATE public.payment_events
    SET processed_at = v_now,
        status = COALESCE(NULLIF(btrim(p_status), ''), status)
    WHERE id = v_event.id;

    credit_balance := v_credit.credit_balance;
    balance_before := v_credit.balance_before;
    balance_after := v_credit.balance_after;
    ledger_id := v_credit.ledger_id;
    RETURN NEXT;
    RETURN;
  END IF;

  credit_balance := NULL;
  balance_before := NULL;
  balance_after := NULL;
  ledger_id := NULL;
  RETURN NEXT;
END;
$$;
