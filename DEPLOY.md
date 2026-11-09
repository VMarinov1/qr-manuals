# Пускане на сървъра

Приложението върви на същия сървър като ORTHOTEH, по същия модел: Docker контейнер,
който слуша само на `127.0.0.1:5200`, и nginx на хоста пред него с HTTPS от Let's Encrypt.

Домейнът е `qr.dron.bg`, **зад Cloudflare (оранжев облак)**:

```
телефон → Cloudflare → nginx (443, Let's Encrypt) → 127.0.0.1:5200 → контейнер
```

> **Преди първия печат на QR код:** домейнът и `PUBLIC_BASE_URL` трябва да са окончателни.
> Те се съдържат в кода и след отпечатване не могат да се сменят.

## Първоначално пускане

### 1. DNS в Cloudflare
1. A запис `qr` към IP адреса на сървъра, **засега „DNS only“ (сив облак)**. Сертификатът се
   издава по-лесно, докато заявките стигат директно до сървъра. Оранжевият облак се включва в стъпка 7.
2. **SSL/TLS режим** на зоната `dron.bg` трябва да е **„Full (strict)“** (или поне „Full“).
   `orthoteh.dron.bg` работи със същия модел, така че вероятно вече е така, но провери.
   **Не го сменяй на „Flexible“**, защото това ще счупи и другите поддомейни.

Провери с `dig +short qr.dron.bg`: трябва да върне IP адреса на сървъра.

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

### 7. nginx, HTTPS и Cloudflare
```bash
sudo mkdir -p /etc/nginx/snippets
sudo cp deploy/nginx/cloudflare-real-ip.conf /etc/nginx/snippets/cloudflare-real-ip.conf
sudo cp deploy/nginx/qr.dron.bg.conf /etc/nginx/sites-available/qr.dron.bg.conf
sudo certbot certonly --nginx -d qr.dron.bg          # докато облакът още е сив
sudo ln -s /etc/nginx/sites-available/qr.dron.bg.conf /etc/nginx/sites-enabled/
sudo nginx -t && sudo systemctl reload nginx
```
После в Cloudflare превключи записа `qr` на **„Proxied“ (оранжев облак)** и провери, че
подновяването на сертификата ще работи и през Cloudflare:
```bash
sudo certbot renew --dry-run --cert-name qr.dron.bg
```
Ако `--dry-run` не мине, най-простото е Cloudflare Origin Certificate
(SSL/TLS → Origin Server в Cloudflare). Той е валиден 15 години и не се подновява.
Сложи го на мястото на двата `ssl_certificate` реда.

`cloudflare-real-ip.conf` казва на nginx кои адреси са на Cloudflare, за да вижда реалния
адрес на клиента. Без него ограничението на входа би блокирало всички потребители заедно.
Cloudflare сменя адресите си рядко. При нужда файлът се обновява с
`./scripts/cloudflare-real-ip.sh > deploy/nginx/cloudflare-real-ip.conf`.

### 8. Проверка
```bash
curl -s -o /dev/null -w "%{http_code}\n" https://qr.dron.bg/health   # 200
curl -sI https://qr.dron.bg/health | grep -i '^server'               # server: cloudflare
```
После:
1. Влез на `https://qr.dron.bg`, създай пробен продукт и качи снимка и PDF.
2. Изтегли QR кода, **сканирай го с телефон** и провери, че се отваря упътването.
3. Пробвай превключването на езика.
4. Направи **един грешен опит за вход** и виж в `docker compose logs | grep 'неуспешен вход'`,
   че `ip=` е твоят адрес, а не адрес на Cloudflare (104.x, 172.64-71.x, 162.158.x и т.н.).
   Ако е на Cloudflare, `cloudflare-real-ip.conf` не е включен правилно.
5. Изтрий пробния продукт.

**Ограничения от Cloudflare (безплатен план):** едно качване може да е до 100 MB общо.
Приложението го ограничава до 95 MB (`MAX_REQUEST_MB`) и предупреждава в браузъра преди
изпращане. Снимките и PDF файловете се кешират от Cloudflare. Изтрит файл може да остане
достъпен в кеша му по директния си адрес, но вече не се показва на страницата на продукта.

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
