#!/bin/sh
# Пуска нова версия на сървъра. Вика се от GitHub Actions (.github/workflows/deploy.yml):
#   GHCR_USER=... GHCR_TOKEN=... scripts/deploy.sh ghcr.io/<owner>/qr-manuals:<commit>
#
# Записва image-а в .env (QR_IMAGE, ползва се от docker-compose.yml), рестартира контейнера
# и чака да стане healthy. Ако не стане, връща предишния image и завършва с грешка, за да
# се види в GitHub. Може да се пусне и на ръка за връщане към стара версия:
#   scripts/deploy.sh ghcr.io/<owner>/qr-manuals:<стар-commit>
set -eu

cd "$(dirname "$0")/.."
image=$1
container=qr-manuals

if [ -n "${GHCR_TOKEN:-}" ]; then
    printf '%s' "$GHCR_TOKEN" | docker login ghcr.io -u "$GHCR_USER" --password-stdin >/dev/null
    # Токенът на GitHub Actions е валиден само по време на workflow-а. Не го оставяме на диска.
    trap 'docker logout ghcr.io >/dev/null 2>&1 || true' EXIT
fi

if ! docker pull --quiet "$image"; then
    # Ръчно връщане без login: image-ът вече е на сървъра.
    docker image inspect "$image" >/dev/null 2>&1 || { echo "Няма image $image" >&2; exit 1; }
    echo "Не може да се изтегли, ползвам локалния $image"
fi

previous=$(sed -n 's/^QR_IMAGE=//p' .env 2>/dev/null || true)

set_image() {
    { grep -v '^QR_IMAGE=' .env 2>/dev/null || true; echo "QR_IMAGE=$1"; } > .env.tmp
    mv .env.tmp .env
}

wait_healthy() {
    i=0
    while [ $i -lt 45 ]; do
        status=$(docker inspect -f '{{.State.Health.Status}}' "$container" 2>/dev/null || echo missing)
        [ "$status" = healthy ] && return 0
        sleep 2
        i=$((i + 1))
    done
    echo "Статус: $status" >&2
    return 1
}

echo "Пускане на $image (предишна: ${previous:-няма})"
set_image "$image"
docker compose up -d --no-build --remove-orphans

if wait_healthy; then
    echo "OK: $image работи."
else
    echo "ГРЕШКА: $image не стана healthy. Последни логове:" >&2
    docker compose logs --tail 40 >&2 || true
    if [ -n "$previous" ] && [ "$previous" != "$image" ]; then
        echo "Връщане към $previous" >&2
        set_image "$previous"
        docker compose up -d --no-build --remove-orphans
        wait_healthy && echo "Предишната версия работи отново." >&2
    fi
    exit 1
fi

# Пазим текущия и предишния image (за ръчно връщане), по-старите се изтриват.
repo=${image%:*}
docker images "$repo" --format '{{.Repository}}:{{.Tag}}' \
    | grep -v -x -e "$image" -e "${previous:-$image}" -e "$repo:latest" \
    | xargs -r docker rmi >/dev/null 2>&1 || true
