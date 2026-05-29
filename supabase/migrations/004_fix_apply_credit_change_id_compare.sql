-- 004_fix_apply_credit_change_id_compare.sql
-- Make apply_credit_change tolerant of text-vs-uuid user_id comparisons.

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
