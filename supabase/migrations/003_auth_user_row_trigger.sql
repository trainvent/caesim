-- Auto-create the public.users row whenever a new Supabase auth user signs up.
-- This keeps CLI signup working without requiring a separate edge function.

CREATE OR REPLACE FUNCTION public.handle_new_auth_user()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public
AS $$
DECLARE
  v_now bigint := extract(epoch from coalesce(NEW.created_at, now()))::bigint;
BEGIN
  INSERT INTO public.users (
    id,
    email,
    auth_provider,
    plan,
    created_at,
    last_seen_at,
    account_status,
    credit_balance
  )
  VALUES (
    NEW.id,
    NULLIF(btrim(lower(COALESCE(NEW.email, ''))), ''),
    'email',
    'beta',
    v_now,
    v_now,
    'active',
    0
  )
  ON CONFLICT (id) DO UPDATE
  SET
    email = COALESCE(EXCLUDED.email, public.users.email),
    last_seen_at = GREATEST(COALESCE(public.users.last_seen_at, 0), EXCLUDED.last_seen_at),
    account_status = COALESCE(public.users.account_status, EXCLUDED.account_status);

  RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS on_auth_user_created ON auth.users;

CREATE TRIGGER on_auth_user_created
AFTER INSERT ON auth.users
FOR EACH ROW
EXECUTE FUNCTION public.handle_new_auth_user();
