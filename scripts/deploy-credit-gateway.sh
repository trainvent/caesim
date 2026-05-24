#!/usr/bin/env bash
set -euo pipefail

# deploy-credit-gateway.sh
# Deploys the Supabase Edge Function 'credit-gateway' and sets required secrets.
# Usage:
#   ./scripts/deploy-credit-gateway.sh [--project-ref <ref>] [--env-file .env] [--no-secrets]
#
# Secure-first behavior:
# - Does not require a .env file.
# - Reads secrets from current shell env, optional env-file, or hidden prompt.

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
  PUBLISHABLE_KEY="${PUBLISHABLE_KEY:-}"
  ADMIN_TOKEN="${CREDIT_ADMIN_TOKEN:-}"

  # Prompt securely for missing values in interactive terminals.
  if [[ -t 0 && -z "$SERVICE_KEY" ]]; then
    prompt_hidden SERVICE_KEY "Service role key (SERVICE_ROLE_KEY): "
  fi
  if [[ -t 0 && -z "$PUBLISHABLE_KEY" ]]; then
    prompt_hidden PUBLISHABLE_KEY "Publishable key (PUBLISHABLE_KEY): "
  fi
  if [[ -t 0 && -z "$ADMIN_TOKEN" ]]; then
    prompt_hidden ADMIN_TOKEN "Credit admin token (CREDIT_ADMIN_TOKEN): "
  fi

  if [ -z "$SERVICE_KEY" ] || [ -z "$PUBLISHABLE_KEY" ] || [ -z "$ADMIN_TOKEN" ]; then
    echo "Skipping secrets update: missing one or more secret values."
    echo "Provide values via env vars, --env-file, or interactive prompt; or use --no-secrets."
    exit 1
  fi

  echo "Setting secrets in Supabase service..."
  if [ -n "$PROJECT_REF" ]; then
    supabase secrets set --project-ref "$PROJECT_REF" SERVICE_ROLE_KEY="$SERVICE_KEY" PUBLISHABLE_KEY="$PUBLISHABLE_KEY" CREDIT_ADMIN_TOKEN="$ADMIN_TOKEN"
  else
    supabase secrets set SERVICE_ROLE_KEY="$SERVICE_KEY" PUBLISHABLE_KEY="$PUBLISHABLE_KEY" CREDIT_ADMIN_TOKEN="$ADMIN_TOKEN"
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
