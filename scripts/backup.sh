#!/bin/sh
# Нощен backup. Пуска се от cron на сървъра (вж. DEPLOY.md):
#   1. съгласувано копие на базата в data/backups/ (пазят се последните KEEP);
#   2. ако е зададен BACKUP_REMOTE, копира data/ на друг сървър с rsync.
#
# Качените файлове не се променят след запис, затова rsync копира само новите.
# Работещата база (app.db*) не се копира директно, защото може да е по средата на запис;
# вместо нея се копират снимките ѝ от data/backups/.
#
#   BACKUP_REMOTE=backup@host:/backups/qr-manuals/ ./scripts/backup.sh
set -eu

cd "$(dirname "$0")/.."

docker compose exec -T qr-manuals qr-manuals backup /data/backups --keep "${KEEP:-14}"

if [ -n "${BACKUP_REMOTE:-}" ]; then
    # Без --delete: изтрит по погрешка файл остава в отдалеченото копие.
    rsync -a --exclude 'app.db' --exclude 'app.db-*' --exclude '.upload-*' data/ "$BACKUP_REMOTE"
fi
