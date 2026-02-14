#!/usr/bin/env bash
set -euo pipefail

APP_DIR="$HOME/MLRunX"
COMPOSE_FILE="infra/docker/docker-compose.prod.yml"
ENV_FILE="infra/docker/.env.prod"
STATE_DIR="$HOME/.mlrunx_deploy"
CURRENT_FILE="$STATE_DIR/current"
PREVIOUS_FILE="$STATE_DIR/previous"

# Derive domain from env file for health checks
MLRUNX_DOMAIN=$(grep -E '^MLRUNX_DOMAIN=' "$APP_DIR/$ENV_FILE" | cut -d= -f2)

HEALTH_URL="https://${MLRUNX_DOMAIN}/health"
HEALTH_RETRIES=12
HEALTH_INTERVAL=5

mkdir -p "$STATE_DIR"

cd "$APP_DIR"

print_usage() {
  echo ""
  echo "Usage:"
  echo "  $0 deploy <branch>"
  echo "  $0 rollback"
  echo "  $0 status"
  echo ""
  exit 1
}

require_clean_worktree() {
  if ! git diff --quiet || ! git diff --cached --quiet; then
    echo "❌  Working tree is not clean. Refusing to deploy."
    exit 1
  fi
}

wait_healthy() {
  echo "🔎  Waiting for health check (${HEALTH_RETRIES} attempts, ${HEALTH_INTERVAL}s apart)..."
  for i in $(seq 1 "$HEALTH_RETRIES"); do
    if curl -fsS "$HEALTH_URL" >/dev/null 2>&1; then
      echo "✅  Health check passed (attempt $i)"
      return 0
    fi
    echo "   attempt $i/$HEALTH_RETRIES — not ready, retrying in ${HEALTH_INTERVAL}s..."
    sleep "$HEALTH_INTERVAL"
  done
  echo "❌  Health check failed after $HEALTH_RETRIES attempts"
  return 1
}

deploy_branch() {
  BRANCH="${1:-main}"
  echo "🚀  Deploying branch: $BRANCH"

  require_clean_worktree

  git fetch --all --prune

  if ! git show-ref --verify --quiet "refs/remotes/origin/$BRANCH"; then
    echo "❌  Remote branch origin/$BRANCH does not exist."
    exit 1
  fi

  # Save previous SHA before switching
  if [ -f "$CURRENT_FILE" ]; then
    cp "$CURRENT_FILE" "$PREVIOUS_FILE"
  fi

  git checkout "$BRANCH" 2>/dev/null || git checkout -t "origin/$BRANCH"

  # Only allow fast-forward pulls
  git pull --ff-only

  NEW_SHA=$(git rev-parse HEAD)
  echo "$NEW_SHA" > "$CURRENT_FILE"

  echo "🔨  Rebuilding and restarting containers..."
  docker compose -f "$COMPOSE_FILE" --env-file "$ENV_FILE" up -d --build

  echo "📦  Deployed commit: $NEW_SHA"
  docker compose -f "$COMPOSE_FILE" --env-file "$ENV_FILE" ps

  if ! wait_healthy; then
    echo "⚠️  Deploy appears unhealthy. Rolling back automatically..."
    rollback
    exit 1
  fi
}

rollback() {
  if [ ! -f "$PREVIOUS_FILE" ]; then
    echo "❌  No previous deployment recorded."
    exit 1
  fi

  PREV_SHA=$(cat "$PREVIOUS_FILE")
  echo "↩️  Rolling back to commit: $PREV_SHA"

  # Determine which branch contains the previous commit so we stay on a branch
  PREV_BRANCH=$(git branch -r --contains "$PREV_SHA" 2>/dev/null | head -1 | sed 's|.*origin/||; s/ //g')

  if [ -n "$PREV_BRANCH" ]; then
    git checkout "$PREV_BRANCH"
    git reset --hard "$PREV_SHA"
  else
    # Fallback: detached HEAD if branch was deleted
    echo "⚠️  Could not find a branch for $PREV_SHA, checking out in detached HEAD."
    git checkout "$PREV_SHA"
  fi

  # Update state: current becomes the rollback target, clear previous
  echo "$PREV_SHA" > "$CURRENT_FILE"
  rm -f "$PREVIOUS_FILE"

  docker compose -f "$COMPOSE_FILE" --env-file "$ENV_FILE" up -d --build

  echo "✅  Rollback complete to $PREV_SHA"
  docker compose -f "$COMPOSE_FILE" --env-file "$ENV_FILE" ps
}

status() {
  echo "📊  Deployment Status"
  echo ""

  CURRENT_SHA="(none)"
  PREVIOUS_SHA="(none)"

  [ -f "$CURRENT_FILE" ] && CURRENT_SHA=$(cat "$CURRENT_FILE")
  [ -f "$PREVIOUS_FILE" ] && PREVIOUS_SHA=$(cat "$PREVIOUS_FILE")

  echo "Current deployed commit : $CURRENT_SHA"
  echo "Previous deployed commit: $PREVIOUS_SHA"
  echo ""
  git --no-pager log --oneline -n 5
  echo ""
  docker compose -f "$COMPOSE_FILE" --env-file "$ENV_FILE" ps
}

case "${1:-}" in
  deploy)
    shift
    deploy_branch "${1:-main}"
    ;;
  rollback)
    rollback
    ;;
  status)
    status
    ;;
  *)
    print_usage
    ;;
esac
