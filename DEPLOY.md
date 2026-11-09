# Пускане на сървъра

Приложението върви на същия сървър като ORTHOTEH, по същия модел: Docker контейнер,
който слуша само на `127.0.0.1:5200`, и nginx на хоста пред него с HTTPS от Let's Encrypt.

Домейнът е `qr.dron.bg`.

> **Преди първия печат на QR код:** домейнът и `PUBLIC_BASE_URL` трябва да са окончателни.
> Те се съдържат в кода и след отпечатване не могат да се сменят.

## Първоначално пускане

### 1. DNS
A запис за `qr.dron.bg` към IP адреса на сървъра. Провери с `dig +short qr.dron.bg`.

### 2. Кодът
```bash
cd /home/orthoteh-bg.com
git clone <адрес-на-repo> qr-manuals
cd qr-manuals
```

### 3. Настройки
```bash
cp qr-manuals.env.example qr-manuals.env
nano qr-manuals.env         # PUBLIC_BASE_URL=https://qr.dron.bg, TRUST_PROXY=true
```

### 4. Папка за данните
Потребителят в контейнера е с uid 10001 и трябва да може да пише в `data/`:
```bash
mkdir -p data && sudo chown -R 10001:10001 data
```

### 5. Билд и старт
Първият билд компилира Rust и отнема няколко минути (нужни са ~2 GB RAM). Тестовете се
пускат по време на билда и при грешка той спира, без да подмени работещата версия.
```bash
docker compose up -d --build
docker compose ps           # STATUS трябва да стане "healthy"
curl -s http://127.0.0.1:5200/health   # ok
```

### 6. Потребител
```bash
docker compose exec qr-manuals qr-manuals create-admin твоят@email.bg
```
Със същата команда се сменя и забравена парола.

### 7. nginx и HTTPS
```bash
sudo cp deploy/nginx/qr.dron.bg.conf /etc/nginx/sites-available/qr.dron.bg.conf
sudo certbot certonly --nginx -d qr.dron.bg
sudo ln -s /etc/nginx/sites-available/qr.dron.bg.conf /etc/nginx/sites-enabled/
sudo nginx -t && sudo systemctl reload nginx
```

### 8. Проверка
```bash
curl -s -o /dev/null -w "%{http_code}\n" https://qr.dron.bg/health   # 200
curl -s -o /dev/null -w "%{http_code}\n" http://qr.dron.bg/health    # 301
```
После:
1. Влез на `https://qr.dron.bg`, създай пробен продукт и качи снимка и PDF.
2. Изтегли QR кода, **сканирай го с телефон** и провери, че се отваря упътването.
3. Пробвай превключването на езика.
4. Изтрий пробния продукт.

### 9. Нощен backup
```bash
sudo crontab -e
```
```
15 3 * * * BACKUP_REMOTE=backup@друг-сървър:/backups/qr-manuals/ /home/orthoteh-bg.com/qr-manuals/scripts/backup.sh >> /var/log/qr-manuals-backup.log 2>&1
```
- Всяка нощ прави копие на базата в `data/backups/` (пазят се последните 14).
- С `BACKUP_REMOTE` копира `data/` (базата и всички качени файлове) на друг сървър.
  Backup, който е само на същия сървър, не пази от загуба на сървъра.
- Пробвай веднъж ръчно: `./scripts/backup.sh` и провери, че в `data/backups/` има файл.

## Обновяване
```bash
cd /home/orthoteh-bg.com/qr-manuals
git pull
docker compose up -d --build
```
Миграциите на базата се пускат автоматично при старт.

## Възстановяване от backup
```bash
docker compose stop
cp data/backups/app-ГГГГММДД-ЧЧММСС.db data/app.db
rm -f data/app.db-wal data/app.db-shm
sudo chown 10001:10001 data/app.db
docker compose start
```
Качените файлове са в `data/uploads/`. При пълна загуба на сървъра се връщат от `BACKUP_REMOTE`.

## Полезно
```bash
docker compose logs -f --tail 100          # логове
docker compose logs | grep -E 'WARN|ERROR' # само проблемите
du -sh data/uploads                        # колко място заемат файловете
```
