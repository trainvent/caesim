declare const Deno: {
  env: {
    get(name: string): string | undefined;
  };
  serve(handler: (request: Request) => Response | Promise<Response>): void;
};

const corsHeaders = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers": "authorization, x-client-info, apikey, content-type, x-caesim-admin-token",
  "Access-Control-Allow-Methods": "POST, OPTIONS",
};

type Action = "balance" | "consume" | "grant" | "payment";

type BalanceRequest = {
  action?: Action;
  amount?: number;
  user_id?: string;
  email?: string;
  reason?: string;
  provider?: string;
  provider_event_id?: string;
  currency?: string;
  credits_granted?: number;
  status?: string;
  metadata?: Record<string, unknown>;
  stripe_customer_id?: string;
  stripe_checkout_session_id?: string;
  stripe_payment_intent_id?: string;
};

type SupabaseUser = {
  id: string;
  email?: string;
  user_metadata?: Record<string, unknown>;
  raw_user_meta_data?: Record<string, unknown>;
};

type UserRow = {
  id: string;
  email?: string;
  auth_provider?: string;
  plan?: string;
  created_at?: number;
  last_seen_at?: number;
  account_status?: string;
  credit_balance?: number;
  stripe_customer_id?: string;
  billing_email?: string;
  billing_status?: string;
};

type RpcCreditChangeRow = {
  user_id?: string;
  credit_balance?: number;
  balance_before?: number;
  balance_after?: number;
  ledger_id?: number;
};

type RpcPaymentEventRow = {
  payment_event_id?: number;
  user_id?: string;
  credit_balance?: number | null;
  balance_before?: number | null;
  balance_after?: number | null;
  ledger_id?: number | null;
};

function jsonResponse(status: number, body: Record<string, unknown>) {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      ...corsHeaders,
      "Content-Type": "application/json; charset=utf-8",
    },
  });
}

function getServiceRoleKey(): string {
  const direct = Deno.env.get("SERVICE_ROLE_KEY");
  if (direct) return direct;
  throw new Error("missing Supabase service key: set SERVICE_ROLE_KEY");
}

function deriveProjectUrlFromRequest(request: Request): string | null {
  const url = new URL(request.url);
  const host = url.hostname;

  if (host.endsWith(".functions.supabase.co")) {
    return `https://${host.replace(/\.functions\.supabase\.co$/, ".supabase.co")}`;
  }

  return null;
}

function getProjectUrl(request?: Request): string {
  const direct = Deno.env.get("SUPABASE_URL");
  if (direct) {
    return direct.replace(/\/$/, "");
  }

  if (request) {
    const derived = deriveProjectUrlFromRequest(request);
    if (derived) return derived;
  }

  throw new Error("missing SUPABASE_URL (or run the function on a Supabase *.functions.supabase.co host)");
}

function getBearerToken(request: Request): string {
  const header = request.headers.get("Authorization") ?? "";
  const [scheme, token] = header.split(" ");
  if (scheme?.toLowerCase() !== "bearer" || !token) {
    throw new Error("missing bearer token");
  }
  return token;
}

function extractBalance(row: UserRow | undefined, user: SupabaseUser): number {
  if (typeof row?.credit_balance === "number") return row.credit_balance;

  const metadata = user.user_metadata ?? user.raw_user_meta_data ?? {};
  const value = metadata.credit_balance;
  return typeof value === "number" ? value : 0;
}

function pickUserEmail(row: UserRow | undefined, user: SupabaseUser): string {
  return row?.email ?? user.email ?? "";
}

async function getAuthenticatedUser(request: Request): Promise<SupabaseUser> {
  const projectUrl = getProjectUrl(request);
  const serviceKey = getServiceRoleKey();
  const token = getBearerToken(request);

  const response = await fetch(`${projectUrl}/auth/v1/user`, {
    headers: {
      apikey: serviceKey,
      Authorization: `Bearer ${token}`,
    },
  });

  if (!response.ok) {
    const errorText = await response.text();
    throw new Error(`auth lookup failed: ${response.status} ${errorText}`);
  }

  return await response.json() as SupabaseUser;
}

async function fetchUserRow(projectUrl: string, serviceKey: string, userId: string): Promise<UserRow | null> {
  const response = await fetch(
    `${projectUrl}/rest/v1/users?select=*&id=eq.${encodeURIComponent(userId)}&limit=1`,
    {
      headers: {
        apikey: serviceKey,
        Authorization: `Bearer ${serviceKey}`,
      },
    },
  );

  if (!response.ok) {
    const errorText = await response.text();
    throw new Error(`user row lookup failed: ${response.status} ${errorText}`);
  }

  const rows = await response.json() as UserRow[];
  return rows[0] ?? null;
}

function requireAmount(value: unknown): number {
  if (typeof value !== "number" || !Number.isInteger(value) || value <= 0) {
    throw new Error("amount must be a positive integer");
  }
  return value;
}

async function callRpc<T>(projectUrl: string, serviceKey: string, functionName: string, body: Record<string, unknown>): Promise<T[]> {
  const response = await fetch(`${projectUrl}/rest/v1/rpc/${functionName}`, {
    method: "POST",
    headers: {
      apikey: serviceKey,
      Authorization: `Bearer ${serviceKey}`,
      "Content-Type": "application/json",
      Prefer: "return=representation",
    },
    body: JSON.stringify(body),
  });

  if (!response.ok) {
    const errorText = await response.text();
    throw new Error(`${functionName} rpc failed: ${response.status} ${errorText}`);
  }

  return await response.json() as T[];
}

async function applyCreditChange(projectUrl: string, serviceKey: string, body: Record<string, unknown>): Promise<RpcCreditChangeRow> {
  const rows = await callRpc<RpcCreditChangeRow>(projectUrl, serviceKey, "apply_credit_change", body);
  return rows[0] ?? {};
}

async function recordPaymentEvent(projectUrl: string, serviceKey: string, body: Record<string, unknown>): Promise<RpcPaymentEventRow> {
  const rows = await callRpc<RpcPaymentEventRow>(projectUrl, serviceKey, "record_payment_event", body);
  return rows[0] ?? {};
}

Deno.serve(async (request: Request) => {
  if (request.method === "OPTIONS") {
    return new Response("ok", { headers: corsHeaders });
  }

  if (request.method !== "POST") {
    return jsonResponse(405, { error: "method not allowed" });
  }

  let payload: BalanceRequest;
  try {
    payload = await request.json() as BalanceRequest;
  } catch {
    return jsonResponse(400, { error: "invalid JSON body" });
  }

  const action = payload.action ?? "balance";

  try {
    const serviceKey = getServiceRoleKey();
    const projectUrl = getProjectUrl(request);

    if (action === "grant" || action === "payment") {
      const defaultAdminEmail = Deno.env.get("CREDIT_ADMIN_EMAIL") ?? "service@trainvent.com";
      let isAdmin = false;

      try {
        const user = await getAuthenticatedUser(request).catch(() => null);
        if (user?.email) {
          if (user.email.toLowerCase() === defaultAdminEmail.toLowerCase()) {
            isAdmin = true;
          }

          const meta = (user.user_metadata ?? user.raw_user_meta_data) as Record<string, unknown> | undefined;
          if (!isAdmin && meta && meta["is_admin"] === true) {
            isAdmin = true;
          }
        }
      } catch {
        // fall through to forbidden
      }

      if (!isAdmin) {
        return jsonResponse(403, { error: "forbidden" });
      }

      const userId = payload.user_id?.trim();
      if (!userId) {
        return jsonResponse(400, { error: "user_id is required" });
      }

      const row = await fetchUserRow(projectUrl, serviceKey, userId).catch(() => null);

      if (action === "grant") {
        const amount = requireAmount(payload.amount);
        const email = payload.email?.trim() || row?.email || null;
        const billingEmail = payload.email?.trim() || row?.billing_email || row?.email || null;
        const saved = await applyCreditChange(projectUrl, serviceKey, {
          p_user_id: userId,
          p_delta: amount,
          p_reason: payload.reason ?? "admin grant",
          p_source: "admin",
          p_source_ref: "credit-gateway:grant",
          p_metadata: payload.metadata ?? {},
          p_email: email,
          p_auth_provider: row?.auth_provider ?? "email",
          p_plan: row?.plan ?? "beta",
          p_account_status: row?.account_status ?? "active",
          p_stripe_customer_id: payload.stripe_customer_id ?? row?.stripe_customer_id ?? null,
          p_billing_email: billingEmail,
          p_billing_status: "active",
        });

        return jsonResponse(200, {
          ok: true,
          action,
          user_id: userId,
          credit_balance: saved.credit_balance ?? amount,
          balance_before: saved.balance_before,
          balance_after: saved.balance_after,
          ledger_id: saved.ledger_id,
        });
      }

      const amountCents = requireAmount(payload.amount);
      const creditsGranted = requireAmount(payload.credits_granted ?? amountCents);
      const provider = (payload.provider?.trim() || "stripe").toLowerCase();
      const providerEventId = payload.provider_event_id?.trim();
      if (!providerEventId) {
        return jsonResponse(400, { error: "provider_event_id is required for payment" });
      }

      const saved = await recordPaymentEvent(projectUrl, serviceKey, {
        p_provider: provider,
        p_provider_event_id: providerEventId,
        p_user_id: userId,
        p_amount_cents: amountCents,
        p_currency: (payload.currency?.trim() || "usd").toLowerCase(),
        p_credits_granted: creditsGranted,
        p_status: payload.status?.trim() || "succeeded",
        p_metadata: payload.metadata ?? {},
        p_email: payload.email?.trim() || row?.billing_email || row?.email || null,
        p_stripe_customer_id: payload.stripe_customer_id ?? row?.stripe_customer_id ?? null,
        p_stripe_checkout_session_id: payload.stripe_checkout_session_id ?? null,
        p_stripe_payment_intent_id: payload.stripe_payment_intent_id ?? null,
      });

      return jsonResponse(200, {
        ok: true,
        action,
        payment_event_id: saved.payment_event_id,
        user_id: userId,
        credit_balance: saved.credit_balance,
        balance_before: saved.balance_before,
        balance_after: saved.balance_after,
        ledger_id: saved.ledger_id,
      });
    }

    const user = await getAuthenticatedUser(request);
    const row = await fetchUserRow(projectUrl, serviceKey, user.id).catch(() => null);
    const currentBalance = extractBalance(row ?? undefined, user);

    if (action === "balance") {
      return jsonResponse(200, {
        ok: true,
        action,
        user_id: user.id,
        credit_balance: currentBalance,
        email: pickUserEmail(row ?? undefined, user),
      });
    }

    if (action !== "consume") {
      return jsonResponse(400, { error: `unknown action: ${action}` });
    }

    const amount = requireAmount(payload.amount);
    if (currentBalance < amount) {
      return jsonResponse(409, {
        error: "insufficient credits",
        credit_balance: currentBalance,
        required: amount,
      });
    }

    const email = pickUserEmail(row ?? undefined, user).trim();
    const saved = await applyCreditChange(projectUrl, serviceKey, {
      p_user_id: user.id,
      p_delta: -amount,
      p_reason: payload.reason ?? "credit consume",
      p_source: "consume",
      p_source_ref: "credit-gateway:consume",
      p_metadata: payload.metadata ?? {},
      p_email: email || null,
      p_auth_provider: row?.auth_provider ?? "email",
      p_plan: row?.plan ?? "beta",
      p_account_status: row?.account_status ?? "active",
      p_stripe_customer_id: row?.stripe_customer_id ?? null,
      p_billing_email: row?.billing_email ?? (email || null),
      p_billing_status: row?.billing_status ?? null,
    });

    return jsonResponse(200, {
      ok: true,
      action,
      user_id: user.id,
      credit_balance: saved.credit_balance,
      balance_before: saved.balance_before,
      balance_after: saved.balance_after,
      ledger_id: saved.ledger_id,
    });
  } catch (error) {
    return jsonResponse(500, {
      error: error instanceof Error ? error.message : "unexpected error",
    });
  }
});
