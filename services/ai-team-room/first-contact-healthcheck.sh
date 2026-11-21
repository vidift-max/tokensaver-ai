#!/usr/bin/env sh
set -eu

: "${TEAM_ROOM_BASE_URL:?TEAM_ROOM_BASE_URL is required}"

printf '%s\n' "Checking live endpoint..."
live_code="$(curl -fsS -o /dev/null -w '%{http_code}' "${TEAM_ROOM_BASE_URL}/health/live")"
[ "$live_code" = "200" ] || {
  printf '%s\n' "FAIL: /health/live returned $live_code" >&2
  exit 1
}

printf '%s\n' "Checking ready endpoint..."
ready_code="$(curl -fsS -o /dev/null -w '%{http_code}' "${TEAM_ROOM_BASE_URL}/health/ready")"
[ "$ready_code" = "200" ] || {
  printf '%s\n' "FAIL: /health/ready returned $ready_code" >&2
  exit 1
}

printf '%s\n' "PASS: public Team Room health endpoints are reachable."
printf '%s\n' "This does not prove authentication, Phone-02 identity, or AI-to-AI communication."
