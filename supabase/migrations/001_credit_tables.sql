-- 001_credit_tables.sql
-- Creates a public users table for client-visible profile + credit balance,
-- and an append-only credit_ledger table. Service-role (server) can mutate.

CREATE TABLE IF NOT EXISTS public.users (
  id uuid PRIMARY KEY,
  email text,
  auth_provider text,
  plan text,
  created_at bigint,
  last_seen_at bigint,
  account_status text DEFAULT 'active',
  credit_balance bigint DEFAULT 0
);

-- Row-level security: allow authenticated users to read their own row only.
ALTER TABLE public.users ENABLE ROW LEVEL SECURITY;

CREATE POLICY select_own ON public.users
  FOR SELECT
  USING (auth.uid()::text = id::text);

-- Do NOT create UPDATE/INSERT policies for public.users; service-role keys bypass RLS
-- and are used by server-side functions to upsert and mutate credit_balance.

-- Credit ledger (append-only) for auditability
CREATE TABLE IF NOT EXISTS public.credit_ledger (
  id bigserial PRIMARY KEY,
  user_id uuid NOT NULL,
  delta bigint NOT NULL,
  reason text,
  created_at bigint DEFAULT (extract(epoch from now())::bigint)
);

ALTER TABLE public.credit_ledger ENABLE ROW LEVEL SECURITY;

CREATE POLICY select_ledger_owner ON public.credit_ledger
  FOR SELECT
  USING (auth.uid()::text = user_id::text);

-- Do NOT create INSERT policy for credit_ledger - inserts should be performed
-- only by server-side functions using the service-role key (which bypasses RLS).

-- Indexes
CREATE INDEX IF NOT EXISTS idx_credit_ledger_user_id_created_at ON public.credit_ledger (user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_public_users_email ON public.users (lower(email));
