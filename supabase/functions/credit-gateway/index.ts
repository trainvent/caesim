const corsHeaders = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers": "authorization, x-client-info, apikey, content-type, x-caesim-admin-token",
  "Access-Control-Allow-Methods": "POST, OPTIONS",
};

type Action = "balance" | "consume" | "grant";

type BalanceRequest = {
  action?: Action;
  amount?: number;
  user_id?: string;
  email?: string;
  reason?: string;
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

function getPublishableKey(): string {
  const direct = Deno.env.get("SUPABASE_ANON_KEY") ?? Deno.env.get("SUPABASE_KEY");
  if (direct) return direct;

  const json = Deno.env.get("SUPABASE_PUBLISHABLE_KEYS");
  if (json) {
    try {
      const parsed = JSON.parse(json) as Record<string, string>;
      if (parsed.default) return parsed.default;
    } catch {
      // ignore and fall through
    }
  }

  throw new Error("missing Supabase publishable key: set SUPABASE_ANON_KEY, SUPABASE_KEY, or SUPABASE_PUBLISHABLE_KEYS");
}

function getServiceRoleKey(): string {
  const direct = Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? Deno.env.get("SUPABASE_SECRET_KEY");
  if (direct) return direct;

  const json = Deno.env.get("SUPABASE_SECRET_KEYS");
  if (json) {
    try {
      const parsed = JSON.parse(json) as Record<string, string>;
      if (parsed.default) return parsed.default;
    } catch {
      // ignore and fall through
    }
  }

  throw new Error("missing Supabase service key: set SUPABASE_SERVICE_ROLE_KEY, SUPABASE_SECRET_KEY, or SUPABASE_SECRET_KEYS");
}

function getProjectUrl(): string {
  const url = Deno.env.get("SUPABASE_URL");
  if (!url) {
    throw new Error("missing SUPABASE_URL");
  }
  return url.replace(/\/$/, "");
}

function getBearerToken(request: Request): string {
  const header = request.headers.get("Authorization") ?? "";
  const [scheme, token] = header.split(" ");
  if (scheme?.toLowerCase() !== "bearer" || !token) {
    throw new Error("missing bearer token");
  }
  return token;
}

function getAdminToken(request: Request): string | null {
  return request.headers.get("x-caesim-admin-token");
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
  const projectUrl = getProjectUrl();
  const publishableKey = getPublishableKey();
  const token = getBearerToken(request);

  const response = await fetch(`${projectUrl}/auth/v1/user`, {
    headers: {
      apikey: publishableKey,
      Authorization: `Bearer ${token}`,
    },
  });

  if (!response.ok) {
    const errorText = await response.text();
    throw new Error(`auth lookup failed: ${response.status} ${errorText}`);
  }

  return await response.json() as SupabaseUser;
}

async function fetchUserRow(serviceKey: string, userId: string): Promise<UserRow | null> {
  const projectUrl = getProjectUrl();
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

async function upsertUserRow(serviceKey: string, row: Record<string, unknown>): Promise<UserRow> {
  const projectUrl = getProjectUrl();
  const response = await fetch(`${projectUrl}/rest/v1/users?on_conflict=id`, {
    method: "POST",
    headers: {
      apikey: serviceKey,
      Authorization: `Bearer ${serviceKey}`,
      "Content-Type": "application/json",
      Prefer: "resolution=merge-duplicates,return=representation",
    },
    body: JSON.stringify(row),
  });

  if (!response.ok) {
    const errorText = await response.text();
    throw new Error(`user row upsert failed: ${response.status} ${errorText}`);
  }

  const rows = await response.json() as UserRow[];
  return rows[0] ?? row as UserRow;
}

function requireAmount(value: unknown): number {
  if (typeof value !== "number" || !Number.isInteger(value) || value <= 0) {
    throw new Error("amount must be a positive integer");
  }
  return value;
}

Deno.serve(async (request) => {
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

    if (action === "grant") {
      const adminToken = getAdminToken(request);
      const expectedAdminToken = Deno.env.get("CAESIM_CREDIT_ADMIN_TOKEN");
      if (!expectedAdminToken) {
        throw new Error("missing CAESIM_CREDIT_ADMIN_TOKEN");
      }
      if (adminToken !== expectedAdminToken) {
        return jsonResponse(403, { error: "forbidden" });
      }

      const userId = payload.user_id?.trim();
      if (!userId) {
        return jsonResponse(400, { error: "user_id is required for grant" });
      }

      const amount = requireAmount(payload.amount);
      const row = await fetchUserRow(serviceKey, userId).catch(() => null);
      const currentBalance = typeof row?.credit_balance === "number" ? row.credit_balance : 0;
      const nextBalance = currentBalance + amount;
      const email = (payload.email?.trim() || row?.email || "").toLowerCase();
      if (!email) {
        return jsonResponse(400, { error: "email is required when creating a new user row" });
      }
      const now = Math.floor(Date.now() / 1000);

      const saved = await upsertUserRow(serviceKey, {
        id: userId,
        email,
        auth_provider: row?.auth_provider ?? "email",
        plan: row?.plan ?? "beta",
        created_at: row?.created_at ?? now,
        last_seen_at: now,
        account_status: row?.account_status ?? "active",
        credit_balance: nextBalance,
      });

      return jsonResponse(200, {
        ok: true,
        action,
        user_id: userId,
        credit_balance: saved.credit_balance ?? nextBalance,
      });
    }

    const user = await getAuthenticatedUser(request);
    const row = await fetchUserRow(serviceKey, user.id).catch(() => null);
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

    const nextBalance = currentBalance - amount;
    const email = pickUserEmail(row ?? undefined, user).toLowerCase();
    const now = Math.floor(Date.now() / 1000);

    const saved = await upsertUserRow(serviceKey, {
      id: user.id,
      email,
      auth_provider: row?.auth_provider ?? "email",
      plan: row?.plan ?? "beta",
      created_at: row?.created_at ?? now,
      last_seen_at: now,
      account_status: row?.account_status ?? "active",
      credit_balance: nextBalance,
    });

    return jsonResponse(200, {
      ok: true,
      action,
      user_id: user.id,
      credit_balance: saved.credit_balance ?? nextBalance,
    });
  } catch (error) {
    return jsonResponse(500, {
      error: error instanceof Error ? error.message : "unexpected error",
    });
  }
});
