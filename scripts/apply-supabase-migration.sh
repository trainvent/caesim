#!/usr/bin/env bash
set -euo pipefail

# apply-supabase-migration.sh
# Applies the SQL migration in supabase/migrations/001_credit_tables.sql.
# Usage:
#   ./scripts/apply-supabase-migration.sh
# Optional env:
#   SUPABASE_DB_URL=<postgres-connection-string>   # use direct DB URL

MIGRATION_FILE="$(pwd)/supabase/migrations/001_credit_tables.sql"
if [ ! -f "$MIGRATION_FILE" ]; then
  echo "migration file not found: $MIGRATION_FILE"
  exit 1
fi

if command -v supabase >/dev/null 2>&1; then
  echo "Supabase CLI detected. Applying migration file: $MIGRATION_FILE"

  if [ -n "${SUPABASE_DB_URL:-}" ]; then
    echo "Using SUPABASE_DB_URL with 'supabase db query --db-url'."
    supabase db query --db-url "$SUPABASE_DB_URL" -f "$MIGRATION_FILE"
    echo "Migration applied successfully."
    exit 0
  else
    echo "No SUPABASE_DB_URL provided. Using linked project with 'supabase db query --linked'."
    echo "If this fails, run: supabase login && supabase link --project-ref <your-ref>"
    supabase db query --linked -f "$MIGRATION_FILE"
    echo "Migration applied successfully."
    exit 0
  fi
else
  echo "Supabase CLI not found. Please either:
  1) Install Supabase CLI: https://supabase.com/docs/guides/cli
  2) Paste the SQL in $MIGRATION_FILE into the Supabase SQL editor.
"
  exit 1
fi
