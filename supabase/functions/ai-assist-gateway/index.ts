declare const Deno: {
  env: {
    get(name: string): string | undefined;
  };
  serve(handler: (request: Request) => Response | Promise<Response>): void;
};

const corsHeaders = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers": "authorization, x-client-info, apikey, content-type",
  "Access-Control-Allow-Methods": "POST, OPTIONS",
};

type AssistRequest = {
  content: string;
  assistant_id?: string;
  thread_id?: string;
  system_prompt?: string;
  json_output?: boolean;
  memory?: string;
};

type SupabaseUser = {
  id: string;
  email?: string;
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

function getBackboardApiKey(): string {
  const names = ["BACKBOARD_API_KEY_CAESIM", "BACKBOARD_API_KEY"];
  for (const name of names) {
    const value = Deno.env.get(name);
    if (value) return value;
  }
  throw new Error("missing BACKBOARD_API_KEY_CAESIM (or BACKBOARD_API_KEY)");
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

Deno.serve(async (request: Request) => {
  if (request.method === "OPTIONS") {
    return new Response("ok", { headers: corsHeaders });
  }

  if (request.method !== "POST") {
    return jsonResponse(405, { error: "method not allowed" });
  }

  let payload: AssistRequest;
  try {
    payload = await request.json() as AssistRequest;
  } catch {
    return jsonResponse(400, { error: "invalid JSON body" });
  }

  if (!payload.content || !payload.content.trim()) {
    return jsonResponse(400, { error: "content is required" });
  }

  try {
    await getAuthenticatedUser(request);

    const backboardApiKey = getBackboardApiKey();
    const backboardApiBase = (Deno.env.get("BACKBOARD_API_BASE") ?? "https://app.backboard.io/api").replace(/\/$/, "");
    const url = `${backboardApiBase}/threads/messages`;

    const response = await fetch(url, {
      method: "POST",
      headers: {
        "X-API-Key": backboardApiKey,
        "Accept": "application/json",
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        content: payload.content,
        assistant_id: payload.assistant_id ?? null,
        thread_id: payload.thread_id ?? null,
        system_prompt: payload.system_prompt ?? null,
        json_output: payload.json_output ?? true,
        memory: payload.memory ?? "Auto",
      }),
    });

    const text = await response.text();
    return new Response(text, {
      status: response.status,
      headers: {
        ...corsHeaders,
        "Content-Type": "application/json; charset=utf-8",
      },
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : "unexpected error";
    if (message.includes("missing bearer token") || message.includes("auth lookup failed")) {
      return jsonResponse(401, { error: message });
    }
    return jsonResponse(500, { error: message });
  }
});
