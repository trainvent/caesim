#!/usr/bin/env bash
set -euo pipefail

# deploy-credit-gateway.sh
# Deploys the Supabase Edge Function 'credit-gateway' and sets required config.
# Usage:
#   ./scripts/deploy-credit-gateway.sh [--project-ref <ref>] [--env-file .env] [--no-secrets]
#
# Secure-first behavior:
# - Does not require a .env file.
# - Reads config from current shell env, optional env-file, or hidden prompt.

ENV_FILE=""
PROJECT_REF=""
SET_SECRETS=1

prompt_hidden() {
  local var_name="$1"
  local prompt_text="$2"
  local value=""
  read -r -s -p "$prompt_text" value
  echo
  printf -v "$var_name" "%s" "$value"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --env-file) ENV_FILE="$2"; shift 2 ;;
    --project-ref) PROJECT_REF="$2"; shift 2 ;;
    --no-secrets) SET_SECRETS=0; shift ;;
    *) echo "Unknown arg: $1"; exit 1 ;;
  esac
done

if ! command -v supabase >/dev/null 2>&1; then
  echo "Supabase CLI not installed. Install from https://supabase.com/docs/guides/cli"
  exit 1
fi

if [ -n "$ENV_FILE" ]; then
  if [ -f "$ENV_FILE" ]; then
    set -a
    # shellcheck disable=SC1090
    . "$ENV_FILE"
    set +a
  else
    echo "Env file $ENV_FILE not found. Continuing without file-based secrets."
  fi
fi

if [ -n "$PROJECT_REF" ]; then
  echo "Linking project ref $PROJECT_REF"
  supabase link --project-ref "$PROJECT_REF"
fi

# Deploy function
echo "Deploying function credit-gateway..."
if [ -n "$PROJECT_REF" ]; then
  supabase functions deploy credit-gateway --project-ref "$PROJECT_REF" --no-verify-jwt
else
  supabase functions deploy credit-gateway --no-verify-jwt
fi

if [ "$SET_SECRETS" -eq 1 ]; then
  SERVICE_KEY="${SERVICE_ROLE_KEY:-}"
  SUPABASE_URL="${SUPABASE_URL:-${PROJECT_URL:-}}"

  # Prompt securely for missing values in interactive terminals.
  if [[ -t 0 && -z "$SERVICE_KEY" ]]; then
    prompt_hidden SERVICE_KEY "Service role key (SERVICE_ROLE_KEY): "
  fi
  if [[ -t 0 && -z "$SUPABASE_URL" ]]; then
    read -r -p "Supabase URL (SUPABASE_URL): " SUPABASE_URL
  fi

  if [ -z "$SERVICE_KEY" ] || [ -z "$SUPABASE_URL" ]; then
    echo "Skipping config update: missing SERVICE_ROLE_KEY or SUPABASE_URL."
    echo "Provide values via env vars, --env-file, or interactive prompt; or use --no-secrets."
    exit 1
  fi

  echo "Setting gateway config in Supabase service..."
  if [ -n "$PROJECT_REF" ]; then
    supabase secrets set --project-ref "$PROJECT_REF" SERVICE_ROLE_KEY="$SERVICE_KEY" SUPABASE_URL="$SUPABASE_URL"
  else
    supabase secrets set SERVICE_ROLE_KEY="$SERVICE_KEY" SUPABASE_URL="$SUPABASE_URL"
  fi
else
  echo "Skipping secrets update (--no-secrets)."
fi

if [ -n "$PROJECT_REF" ]; then
  echo "Deployment complete. Verify the function at: $(supabase functions list --project-ref "$PROJECT_REF" | grep credit-gateway || true)"
else
  echo "Deployment complete. Verify the function at: $(supabase functions list | grep credit-gateway || true)"
fi

echo "Remember to set CREDIT_GATEWAY_URL in your environment to the function URL."
