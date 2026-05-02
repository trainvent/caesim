#!/bin/bash

# Caesim Backend Login Flow Test
# Run this script to test the complete auth + assistant request flow

set -e

BASE_URL="http://localhost:3000"
EMAIL="test@example.com"

echo "=== Caesim Backend Login Flow Test ==="
echo ""

# Step 1: Login - request verification code
echo "Step 1: Requesting verification code..."
LOGIN_RESPONSE=$(curl -s -X POST "$BASE_URL/v1/auth/login" \
  -H "Content-Type: application/json" \
  -d "{\"email\": \"$EMAIL\"}")

echo "Login Response:"
echo "$LOGIN_RESPONSE" | jq .

# Extract fields
VERIFICATION_CODE=$(echo "$LOGIN_RESPONSE" | jq -r '.verification_code')
USER_ID=$(echo "$LOGIN_RESPONSE" | jq -r '.user_id')

echo ""
echo "Got verification code: $VERIFICATION_CODE"
echo "User ID: $USER_ID"
echo ""

# Step 2: Verify - exchange code for session token
echo "Step 2: Verifying code and getting session token..."
VERIFY_RESPONSE=$(curl -s -X POST "$BASE_URL/v1/auth/verify" \
  -H "Content-Type: application/json" \
  -d "{\"email\": \"$EMAIL\", \"verification_code\": \"$VERIFICATION_CODE\"}")

echo "Verify Response:"
echo "$VERIFY_RESPONSE" | jq .

# Extract session token
SESSION_TOKEN=$(echo "$VERIFY_RESPONSE" | jq -r '.session_token')

echo ""
echo "Got session token: $SESSION_TOKEN"
echo ""

# Step 3: Use session token to call assistant endpoint
echo "Step 3: Making assistant request with session token..."
ASSISTANT_RESPONSE=$(curl -s -X POST "$BASE_URL/v1/assistant/request" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $SESSION_TOKEN" \
  -d "{\"prompt\": \"find duplicate images\", \"path\": \"/tmp\"}")

echo "Assistant Response:"
echo "$ASSISTANT_RESPONSE" | jq .

echo ""
echo "=== Test Complete ==="
