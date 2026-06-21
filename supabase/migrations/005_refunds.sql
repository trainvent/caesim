-- 005_refunds.sql
-- Records Stripe refund events idempotently and reverses credits through the
-- existing credit ledger when the account has enough unused credits.

CREATE TABLE IF NOT EXISTS public.refund_events (
  id bigserial PRIMARY KEY,
  provider text NOT NULL DEFAULT 'stripe',
  provider_event_id text NOT NULL,
  user_id uuid,
  payment_event_id bigint REFERENCES public.payment_events(id),
  stripe_charge_id text,
  stripe_payment_intent_id text,
  amount_cents bigint NOT NULL,
  currency text NOT NULL DEFAULT 'usd',
  credits_reversed bigint NOT NULL DEFAULT 0,
  status text NOT NULL DEFAULT 'pending',
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
  created_at bigint NOT NULL DEFAULT (extract(epoch from now())::bigint),
  processed_at bigint,
  CONSTRAINT refund_events_provider_event_id_key UNIQUE (provider, provider_event_id)
);

ALTER TABLE public.refund_events ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS select_own_refund_events ON public.refund_events;
CREATE POLICY select_own_refund_events ON public.refund_events
  FOR SELECT
  USING (auth.uid()::text = user_id::text);

CREATE INDEX IF NOT EXISTS idx_refund_events_payment_intent
  ON public.refund_events (provider, stripe_payment_intent_id);

CREATE INDEX IF NOT EXISTS idx_refund_events_charge
  ON public.refund_events (provider, stripe_charge_id);

CREATE TABLE IF NOT EXISTS public.refund_requests (
  id bigserial PRIMARY KEY,
  user_id uuid NOT NULL,
  payment_event_id bigint NOT NULL REFERENCES public.payment_events(id),
  reason text,
  status text NOT NULL DEFAULT 'pending',
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
  created_at bigint NOT NULL DEFAULT (extract(epoch from now())::bigint),
  reviewed_at bigint,
  CONSTRAINT refund_requests_one_pending_per_payment UNIQUE (user_id, payment_event_id, status)
);

ALTER TABLE public.refund_requests ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS select_own_refund_requests ON public.refund_requests;
CREATE POLICY select_own_refund_requests ON public.refund_requests
  FOR SELECT
  USING (auth.uid()::text = user_id::text);

CREATE INDEX IF NOT EXISTS idx_refund_requests_status
  ON public.refund_requests (status, created_at);

CREATE OR REPLACE FUNCTION public.request_refund(
  p_user_id uuid,
  p_payment_event_id bigint,
  p_reason text DEFAULT NULL,
  p_metadata jsonb DEFAULT '{}'::jsonb
)
RETURNS TABLE (
  refund_request_id bigint,
  payment_event_id bigint,
  status text,
  created_at bigint
)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public
AS $$
DECLARE
  v_request public.refund_requests%ROWTYPE;
BEGIN
  IF p_payment_event_id IS NULL THEN
    RAISE EXCEPTION 'payment_event_id is required';
  END IF;

  IF NOT EXISTS (
    SELECT 1
    FROM public.payment_events
    WHERE id = p_payment_event_id
      AND user_id::text = p_user_id::text
      AND processed_at IS NOT NULL
      AND COALESCE(credits_granted, 0) > 0
  ) THEN
    RAISE EXCEPTION 'payment event is not refundable';
  END IF;

  INSERT INTO public.refund_requests AS refund_requests (
    user_id,
    payment_event_id,
    reason,
    status,
    metadata
  )
  VALUES (
    p_user_id,
    p_payment_event_id,
    NULLIF(btrim(p_reason), ''),
    'pending',
    COALESCE(p_metadata, '{}'::jsonb)
  )
  ON CONFLICT (user_id, payment_event_id, status) DO UPDATE
  SET reason = COALESCE(EXCLUDED.reason, refund_requests.reason),
      metadata = COALESCE(EXCLUDED.metadata, refund_requests.metadata)
  RETURNING * INTO v_request;

  refund_request_id := v_request.id;
  payment_event_id := v_request.payment_event_id;
  status := v_request.status;
  created_at := v_request.created_at;
  RETURN NEXT;
END;
$$;

CREATE OR REPLACE FUNCTION public.record_refund_event(
  p_provider text,
  p_provider_event_id text,
  p_stripe_charge_id text DEFAULT NULL,
  p_stripe_payment_intent_id text DEFAULT NULL,
  p_amount_refunded_cents bigint DEFAULT 0,
  p_currency text DEFAULT 'usd',
  p_status text DEFAULT 'succeeded',
  p_metadata jsonb DEFAULT '{}'::jsonb
)
RETURNS TABLE (
  refund_event_id bigint,
  payment_event_id bigint,
  user_id uuid,
  credit_balance bigint,
  balance_before bigint,
  balance_after bigint,
  ledger_id bigint,
  credits_reversed bigint,
  refund_delta_cents bigint,
  status text
)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public
AS $$
DECLARE
  v_now bigint := extract(epoch from now())::bigint;
  v_payment public.payment_events%ROWTYPE;
  v_refund public.refund_events%ROWTYPE;
  v_prior_refund_cents bigint := 0;
  v_prior_credits_reversed bigint := 0;
  v_refund_delta_cents bigint := 0;
  v_credits_to_reverse bigint := 0;
  v_credit RECORD;
BEGIN
  IF btrim(COALESCE(p_provider, '')) = '' THEN
    RAISE EXCEPTION 'provider is required';
  END IF;

  IF btrim(COALESCE(p_provider_event_id, '')) = '' THEN
    RAISE EXCEPTION 'provider_event_id is required';
  END IF;

  IF p_amount_refunded_cents < 0 THEN
    RAISE EXCEPTION 'amount_refunded_cents must be non-negative';
  END IF;

  SELECT *
  INTO v_payment
  FROM public.payment_events
  WHERE provider = COALESCE(NULLIF(btrim(p_provider), ''), 'stripe')
    AND processed_at IS NOT NULL
    AND (
      (NULLIF(btrim(p_stripe_payment_intent_id), '') IS NOT NULL
        AND stripe_payment_intent_id = NULLIF(btrim(p_stripe_payment_intent_id), ''))
      OR
      (NULLIF(btrim(p_stripe_charge_id), '') IS NOT NULL
        AND metadata->>'stripe_charge_id' = NULLIF(btrim(p_stripe_charge_id), ''))
    )
  ORDER BY processed_at DESC NULLS LAST, id DESC
  LIMIT 1;

  IF v_payment.id IS NULL THEN
    INSERT INTO public.refund_events AS refund_events (
      provider,
      provider_event_id,
      stripe_charge_id,
      stripe_payment_intent_id,
      amount_cents,
      currency,
      credits_reversed,
      status,
      metadata,
      created_at,
      processed_at
    )
    VALUES (
      COALESCE(NULLIF(btrim(p_provider), ''), 'stripe'),
      p_provider_event_id,
      NULLIF(btrim(p_stripe_charge_id), ''),
      NULLIF(btrim(p_stripe_payment_intent_id), ''),
      p_amount_refunded_cents,
      COALESCE(NULLIF(btrim(p_currency), ''), 'usd'),
      0,
      'unmatched',
      COALESCE(p_metadata, '{}'::jsonb),
      v_now,
      NULL
    )
    ON CONFLICT (provider, provider_event_id) DO UPDATE
    SET metadata = COALESCE(EXCLUDED.metadata, refund_events.metadata)
    RETURNING * INTO v_refund;

    refund_event_id := v_refund.id;
    payment_event_id := NULL;
    user_id := NULL;
    credit_balance := NULL;
    balance_before := NULL;
    balance_after := NULL;
    ledger_id := NULL;
    credits_reversed := 0;
    refund_delta_cents := 0;
    status := v_refund.status;
    RETURN NEXT;
    RETURN;
  END IF;

  SELECT
    COALESCE(MAX(amount_cents), 0),
    COALESCE(SUM(credits_reversed), 0)
  INTO v_prior_refund_cents, v_prior_credits_reversed
  FROM public.refund_events
  WHERE provider = COALESCE(NULLIF(btrim(p_provider), ''), 'stripe')
    AND processed_at IS NOT NULL
    AND refund_events.payment_event_id = v_payment.id;

  INSERT INTO public.refund_events AS refund_events (
    provider,
    provider_event_id,
    user_id,
    payment_event_id,
    stripe_charge_id,
    stripe_payment_intent_id,
    amount_cents,
    currency,
    credits_reversed,
    status,
    metadata,
    created_at,
    processed_at
  )
  VALUES (
    COALESCE(NULLIF(btrim(p_provider), ''), 'stripe'),
    p_provider_event_id,
    v_payment.user_id,
    v_payment.id,
    NULLIF(btrim(p_stripe_charge_id), ''),
    NULLIF(btrim(p_stripe_payment_intent_id), ''),
    p_amount_refunded_cents,
    COALESCE(NULLIF(btrim(p_currency), ''), 'usd'),
    0,
    COALESCE(NULLIF(btrim(p_status), ''), 'succeeded'),
    COALESCE(p_metadata, '{}'::jsonb),
    v_now,
    NULL
  )
  ON CONFLICT (provider, provider_event_id) DO UPDATE
  SET metadata = COALESCE(EXCLUDED.metadata, refund_events.metadata),
      status = refund_events.status,
      processed_at = refund_events.processed_at
  RETURNING * INTO v_refund;

  refund_event_id := v_refund.id;
  payment_event_id := v_payment.id;
  user_id := v_payment.user_id;

  IF v_refund.processed_at IS NOT NULL THEN
    credit_balance := NULL;
    balance_before := NULL;
    balance_after := NULL;
    ledger_id := NULL;
    credits_reversed := v_refund.credits_reversed;
    refund_delta_cents := 0;
    status := v_refund.status;
    RETURN NEXT;
    RETURN;
  END IF;

  v_refund_delta_cents := GREATEST(0, p_amount_refunded_cents - v_prior_refund_cents);

  IF v_refund_delta_cents = 0 THEN
    UPDATE public.refund_events
    SET processed_at = v_now,
        status = 'already_refunded'
    WHERE id = v_refund.id
    RETURNING * INTO v_refund;

    credit_balance := NULL;
    balance_before := NULL;
    balance_after := NULL;
    ledger_id := NULL;
    credits_reversed := 0;
    refund_delta_cents := 0;
    status := v_refund.status;
    RETURN NEXT;
    RETURN;
  END IF;

  v_credits_to_reverse := CEIL(
    (v_refund_delta_cents::numeric * v_payment.credits_granted::numeric)
    / GREATEST(v_payment.amount_cents, 1)::numeric
  )::bigint;
  v_credits_to_reverse := LEAST(
    v_credits_to_reverse,
    GREATEST(0, v_payment.credits_granted - v_prior_credits_reversed)
  );

  IF v_credits_to_reverse = 0 THEN
    UPDATE public.refund_events
    SET processed_at = v_now,
        status = 'already_reversed',
        credits_reversed = 0
    WHERE id = v_refund.id
    RETURNING * INTO v_refund;

    credit_balance := NULL;
    balance_before := NULL;
    balance_after := NULL;
    ledger_id := NULL;
    credits_reversed := 0;
    refund_delta_cents := v_refund_delta_cents;
    status := v_refund.status;
    RETURN NEXT;
    RETURN;
  END IF;

  BEGIN
    SELECT *
    INTO v_credit
    FROM public.apply_credit_change(
      p_user_id := v_payment.user_id,
      p_delta := -v_credits_to_reverse,
      p_reason := COALESCE('refund:' || v_payment.provider, 'refund'),
      p_source := v_payment.provider,
      p_source_ref := v_payment.provider || ':refund:' || p_provider_event_id,
      p_metadata := jsonb_build_object(
        'refund_event_id', p_provider_event_id,
        'payment_event_id', v_payment.provider_event_id,
        'refund_delta_cents', v_refund_delta_cents,
        'amount_refunded_cents', p_amount_refunded_cents
      ) || COALESCE(p_metadata, '{}'::jsonb),
      p_email := v_payment.email,
      p_stripe_customer_id := v_payment.stripe_customer_id,
      p_billing_email := v_payment.email,
      p_billing_status := 'active',
      p_last_seen_at := v_now
    );

    UPDATE public.refund_events
    SET processed_at = v_now,
        status = 'succeeded',
        credits_reversed = v_credits_to_reverse
    WHERE id = v_refund.id
    RETURNING * INTO v_refund;

    credit_balance := v_credit.credit_balance;
    balance_before := v_credit.balance_before;
    balance_after := v_credit.balance_after;
    ledger_id := v_credit.ledger_id;
    credits_reversed := v_credits_to_reverse;
    refund_delta_cents := v_refund_delta_cents;
    status := v_refund.status;
    RETURN NEXT;
    RETURN;
  EXCEPTION WHEN OTHERS THEN
    UPDATE public.refund_events
    SET status = 'requires_manual_review',
        credits_reversed = 0,
        metadata = COALESCE(metadata, '{}'::jsonb) || jsonb_build_object(
          'refund_error', SQLERRM,
          'refund_delta_cents', v_refund_delta_cents,
          'credits_to_reverse', v_credits_to_reverse
        )
    WHERE id = v_refund.id
    RETURNING * INTO v_refund;

    credit_balance := NULL;
    balance_before := NULL;
    balance_after := NULL;
    ledger_id := NULL;
    credits_reversed := 0;
    refund_delta_cents := v_refund_delta_cents;
    status := v_refund.status;
    RETURN NEXT;
    RETURN;
  END;
END;
$$;
