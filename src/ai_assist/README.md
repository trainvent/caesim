ai_assist module
----------------

This module contains a small client that calls Backboard's assistant endpoint.

Configuration (via env):
- `BACKBOARD_API_KEY_CAESIM` or `BACKBOARD_API_KEY` : API key (required at runtime)
- `BACKBOARD_GATEWAY_URL` : optional Supabase proxy URL; when set, CLI sends ai-assist requests via Edge Function and does not require local Backboard API keys
- `BACKBOARD_API_NAME` : assistant name (default: `caesim`)
- `BACKBOARD_API_BASE` : API base URL (default: `https://app.backboard.io/api`)
- `BACKBOARD_ASSISTANT_ID` : optional assistant UUID to pin chats to a saved assistant
- `BACKBOARD_THREAD_ID` : optional thread UUID to continue a prior conversation
- `BACKBOARD_PROMPT` : optional system prompt to guide the assistant

Usage (from code):

```rust
let resp = ai_assist::interact(&api_key, "cut landscape photos into review").await?;
if let Some(cmd) = resp.command { /* ... */ }
```

CLI usage:

```bash
caesim ai-assist
```

This implements a minimal JSON request/response wrapper; consult Backboard docs for the official schema.
