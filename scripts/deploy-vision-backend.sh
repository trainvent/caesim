#!/usr/bin/env bash
set -euo pipefail

# Deploys the Google Cloud Vision backend as a Gen 2 Cloud Function.
# Usage:
#   ./scripts/deploy-vision-backend.sh \
#     --project caesim-prod \
#     --service-account vision-app-sa@caesim-prod.iam.gserviceaccount.com \
#     --test-invoker you@example.com

PROJECT_ID="${PROJECT_ID:-caesim-prod}"
REGION="${REGION:-europe-west1}"
BUCKET_LOCATION="${BUCKET_LOCATION:-EU}"
PROCESSING_BUCKET="${PROCESSING_BUCKET:-caesim-vision-processing}"
RESULTS_BUCKET="${RESULTS_BUCKET:-caesim-vision-results}"
FUNCTION_NAME="${FUNCTION_NAME:-caesim-vision}"
SERVICE_ACCOUNT="${SERVICE_ACCOUNT:-vision-app-sa@caesim-prod.iam.gserviceaccount.com}"
TEST_INVOKER="${TEST_INVOKER:-}"
SOURCE_BUCKET="${SOURCE_BUCKET:-}"
LIFECYCLE_AGE_DAYS="${LIFECYCLE_AGE_DAYS:-2}"
ALLOW_UNAUTHENTICATED=0

usage() {
  cat <<'EOF'
Usage: scripts/deploy-vision-backend.sh [options]

Options:
  --project <id>              GCP project id (default: caesim-prod)
  --region <region>           Function region (default: europe-west1)
  --bucket-location <loc>     GCS bucket location (default: EU)
  --processing-bucket <name>  Processing bucket name (default: caesim-vision-processing)
  --results-bucket <name>     Results bucket name (default: caesim-vision-results)
  --function-name <name>      Cloud Function name (default: caesim-vision)
  --service-account <email>   Runtime service account email
  --source-bucket <name>      Optional customer upload bucket to grant objectViewer
  --test-invoker <email>      Optional user email to grant roles/run.invoker
  --lifecycle-age-days <n>    Delete bucket objects older than n days (default: 2)
  --allow-unauthenticated     Deploy HTTP endpoint without IAM auth
  -h, --help                  Show this help

Environment variables with matching uppercase names can also be used.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --project) PROJECT_ID="$2"; shift 2 ;;
    --region) REGION="$2"; shift 2 ;;
    --bucket-location) BUCKET_LOCATION="$2"; shift 2 ;;
    --processing-bucket) PROCESSING_BUCKET="$2"; shift 2 ;;
    --results-bucket) RESULTS_BUCKET="$2"; shift 2 ;;
    --function-name) FUNCTION_NAME="$2"; shift 2 ;;
    --service-account) SERVICE_ACCOUNT="$2"; shift 2 ;;
    --source-bucket) SOURCE_BUCKET="$2"; shift 2 ;;
    --test-invoker) TEST_INVOKER="$2"; shift 2 ;;
    --lifecycle-age-days) LIFECYCLE_AGE_DAYS="$2"; shift 2 ;;
    --allow-unauthenticated) ALLOW_UNAUTHENTICATED=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown arg: $1"; usage; exit 1 ;;
  esac
done

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "$1 is required but was not found on PATH."
    exit 1
  fi
}

ensure_bucket() {
  local bucket="$1"
  if gcloud storage buckets describe "gs://${bucket}" --project "$PROJECT_ID" >/dev/null 2>&1; then
    echo "Bucket gs://${bucket} already exists."
  else
    echo "Creating bucket gs://${bucket} in ${BUCKET_LOCATION}..."
    gcloud storage buckets create "gs://${bucket}" \
      --project "$PROJECT_ID" \
      --location "$BUCKET_LOCATION"
  fi
}

bind_bucket_role() {
  local bucket="$1"
  local member="$2"
  local role="$3"
  echo "Granting ${role} on gs://${bucket} to ${member}..."
  gcloud storage buckets add-iam-policy-binding "gs://${bucket}" \
    --project "$PROJECT_ID" \
    --member "$member" \
    --role "$role" \
    --quiet
}

require_command gcloud

LIFECYCLE_FILE="$(mktemp)"
trap 'rm -f "$LIFECYCLE_FILE"' EXIT

cat >"$LIFECYCLE_FILE" <<EOF
{
  "rule": [
    {
      "action": { "type": "Delete" },
      "condition": { "age": ${LIFECYCLE_AGE_DAYS} }
    }
  ]
}
EOF

RUNTIME_MEMBER="serviceAccount:${SERVICE_ACCOUNT}"

echo "Using project ${PROJECT_ID}, region ${REGION}, buckets in ${BUCKET_LOCATION}."

gcloud config set project "$PROJECT_ID" >/dev/null

echo "Enabling required APIs..."
gcloud services enable \
  vision.googleapis.com \
  run.googleapis.com \
  cloudfunctions.googleapis.com \
  cloudbuild.googleapis.com \
  artifactregistry.googleapis.com \
  --project "$PROJECT_ID"

ensure_bucket "$PROCESSING_BUCKET"
ensure_bucket "$RESULTS_BUCKET"

echo "Applying lifecycle cleanup after ${LIFECYCLE_AGE_DAYS} day(s)..."
gcloud storage buckets update "gs://${PROCESSING_BUCKET}" \
  --project "$PROJECT_ID" \
  --lifecycle-file "$LIFECYCLE_FILE"
gcloud storage buckets update "gs://${RESULTS_BUCKET}" \
  --project "$PROJECT_ID" \
  --lifecycle-file "$LIFECYCLE_FILE"

bind_bucket_role "$PROCESSING_BUCKET" "$RUNTIME_MEMBER" "roles/storage.objectAdmin"
bind_bucket_role "$RESULTS_BUCKET" "$RUNTIME_MEMBER" "roles/storage.objectAdmin"

if [[ -n "$SOURCE_BUCKET" ]]; then
  bind_bucket_role "$SOURCE_BUCKET" "$RUNTIME_MEMBER" "roles/storage.objectViewer"
fi

AUTH_FLAG="--no-allow-unauthenticated"
if [[ "$ALLOW_UNAUTHENTICATED" -eq 1 ]]; then
  AUTH_FLAG="--allow-unauthenticated"
fi

echo "Deploying ${FUNCTION_NAME}..."
gcloud functions deploy "$FUNCTION_NAME" \
  --project "$PROJECT_ID" \
  --gen2 \
  --runtime=python312 \
  --region="$REGION" \
  --source=python \
  --entry-point=vision_http \
  --trigger-http \
  --service-account="$SERVICE_ACCOUNT" \
  --set-env-vars="INPUT_BUCKET_NAME=${PROCESSING_BUCKET},OUTPUT_BUCKET_NAME=${RESULTS_BUCKET}" \
  "$AUTH_FLAG"

if [[ -n "$TEST_INVOKER" ]]; then
  echo "Granting Cloud Run invoker to user:${TEST_INVOKER}..."
  gcloud run services add-iam-policy-binding "$FUNCTION_NAME" \
    --project "$PROJECT_ID" \
    --region="$REGION" \
    --member="user:${TEST_INVOKER}" \
    --role="roles/run.invoker" \
    --quiet
fi

FUNCTION_URL="$(gcloud functions describe "$FUNCTION_NAME" \
  --project "$PROJECT_ID" \
  --gen2 \
  --region="$REGION" \
  --format='value(serviceConfig.uri)')"

echo
echo "Deployment complete."
echo "Set this for CLI sync smoke tests:"
echo "  export CAESIM_VISION_URL=\"${FUNCTION_URL}\""
echo "  export CAESIM_VISION_BEARER_TOKEN=\"\$(gcloud auth print-identity-token)\""
