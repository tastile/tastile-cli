#!/usr/bin/env bash
# Spin up a dedicated, isolated Tastile Core stack for CLI end-to-end tests.
#
# The stack is intentionally separate from the shared verify stack and from
# the Muse operational stack:
#
#   * its own PostgreSQL container, database, user and password,
#   * its own API and worker containers,
#   * its own loopback port (never 31400, which is reserved for the Muse
#     operational Tastile API),
#   * `TASTILE_DELIVERY_PROVIDER=fake`, so no real notification sink is
#     ever contacted.
#
# Usage:
#   scripts/e2e-local-stack.sh up      # create and start the stack
#   scripts/e2e-local-stack.sh down    # stop and remove the stack
#
# Environment:
#   TCLI_E2E_API_PORT   (default 31500)
#   TCLI_E2E_DB_PORT    (default 55441)
#   TCLI_E2E_IMAGE     (default tastile-core-verify:rel110-d0c2ce0)
set -euo pipefail
umask 077

API_PORT="${TCLI_E2E_API_PORT:-31500}"
DB_PORT="${TCLI_E2E_DB_PORT:-55441}"
IMAGE="${TCLI_E2E_IMAGE:-tastile-core-verify:rel110-d0c2ce0}"
NETWORK="tcli-e2e-net"
DB_CONTAINER="tcli-e2e-db"
API_CONTAINER="tcli-e2e-api"
WORKER_CONTAINER="tcli-e2e-worker"
DB_NAME="tastile_cli_e2e"
DB_USER="tastile"
STATE_DIR="${TMPDIR:-/tmp}/tcli-e2e"

if [[ "${API_PORT}" == "31400" || "${DB_PORT}" == "31400" ]]; then
  echo "port 31400 belongs to the Muse operational stack; pick another port" >&2
  exit 2
fi

mkdir -p "${STATE_DIR}"
chmod 700 "${STATE_DIR}"

db_url() {
  local password
  password="$(cat "${STATE_DIR}/db-password")"
  echo "postgres://${DB_USER}:${password}@${DB_CONTAINER}:5432/${DB_NAME}"
}

cmd_up() {
  if [[ ! -f "${STATE_DIR}/db-password" ]]; then
    openssl rand -hex 12 >"${STATE_DIR}/db-password"
  fi
  chmod 600 "${STATE_DIR}/db-password"
  docker network inspect "${NETWORK}" >/dev/null 2>&1 || docker network create "${NETWORK}" >/dev/null

  docker rm -f "${API_CONTAINER}" "${WORKER_CONTAINER}" "${DB_CONTAINER}" >/dev/null 2>&1 || true

  docker run -d --name "${DB_CONTAINER}" --network "${NETWORK}" \
    -e POSTGRES_DB="${DB_NAME}" -e POSTGRES_USER="${DB_USER}" \
    -e POSTGRES_PASSWORD="$(cat "${STATE_DIR}/db-password")" \
    -p "127.0.0.1:${DB_PORT}:5432" postgres:16-alpine >/dev/null

  for _ in $(seq 1 30); do
    if docker exec "${DB_CONTAINER}" pg_isready -U "${DB_USER}" >/dev/null 2>&1; then break; fi
    sleep 1
  done

  docker run --rm --network "${NETWORK}" \
    -e TASTILE_DATABASE__MAX_CONNECTIONS=10 \
    -e TASTILE_DATABASE_URL="$(db_url)" \
    "${IMAGE}" /app/migrate_run

  docker run -d --name "${API_CONTAINER}" --network "${NETWORK}" \
    -e TASTILE_API_HOST=0.0.0.0 -e TASTILE_API_PORT="${API_PORT}" \
    -e TASTILE_ENV=development \
    -e TASTILE_DATABASE_URL="$(db_url)" -e DATABASE_URL="$(db_url)" \
    -e TASTILE_DATABASE__MAX_CONNECTIONS=10 \
    -e TASTILE_DELIVERY_PROVIDER=fake \
    -e TASTILE_MAX_REQUEST_BODY_BYTES=2097152 \
    -e TASTILE_REQUEST_TIMEOUT_SECONDS=30 \
    -e TASTILE_ALLOWED_ORIGINS=http://localhost:3000 \
    -e AWS_EC2_METADATA_DISABLED=true \
    -p "127.0.0.1:${API_PORT}:${API_PORT}" "${IMAGE}" >/dev/null

  docker run -d --name "${WORKER_CONTAINER}" --network "${NETWORK}" \
    -e TASTILE_ENV=development \
    -e TASTILE_DATABASE_URL="$(db_url)" -e DATABASE_URL="$(db_url)" \
    -e TASTILE_DATABASE__MAX_CONNECTIONS=10 \
    -e TASTILE_DELIVERY_PROVIDER=fake \
    -e AWS_EC2_METADATA_DISABLED=true \
    "${IMAGE}" /app/worker >/dev/null

  local status="000"
  for _ in $(seq 1 30); do
    status="$(curl -s -o /dev/null -w '%{http_code}' -m 2 \
      "http://127.0.0.1:${API_PORT}/v1/source-tiles" || true)"
    if [[ "${status}" == "200" || "${status}" == "401" ]]; then break; fi
    sleep 1
  done
  if [[ "${status}" != "200" && "${status}" != "401" ]]; then
    echo "Core did not answer the documented GET /v1/source-tiles endpoint" >&2
    exit 2
  fi
  local image_id
  image_id="$(docker image inspect --format '{{.Id}}' "${IMAGE}")"
  echo "API: http://127.0.0.1:${API_PORT}  (database: ${DB_NAME} in ${DB_CONTAINER})"
  echo "Core image id: ${image_id}"
  echo "Documented source-tile read endpoint: HTTP ${status}"
}

cmd_down() {
  docker rm -f "${API_CONTAINER}" "${WORKER_CONTAINER}" "${DB_CONTAINER}" >/dev/null 2>&1 || true
  docker network rm "${NETWORK}" >/dev/null 2>&1 || true
  rm -rf "${STATE_DIR}"
}

case "${1:-up}" in
  up) cmd_up ;;
  down) cmd_down ;;
  *)
    echo "usage: $0 {up|down}" >&2
    exit 2
    ;;
esac
