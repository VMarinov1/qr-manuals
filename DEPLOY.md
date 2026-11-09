# Пускане на сървъра

Приложението върви на същия сървър като ORTHOTEH: Docker контейнер, който слуша само на
`127.0.0.1:5200`, nginx на хоста с HTTPS от Let's Encrypt и Cloudflare (оранжев облак) отпред.

```
телефон → Cloudflare → nginx (443) → 127.0.0.1:5200 → контейнер
```

**Деплоят е автоматичен:** всеки commit в `main` се билдва в GitHub Actions (заедно с
тестовете), качва се в ghcr.io и се пуска на сървъра (`.github/workflows/deploy.yml`).
Ако новата версия не стане healthy, сървърът сам се връща към предишната.
Pull request-ите само се билдват и тестват, без деплой.

> **Преди първия печат на QR код:** домейнът `qr.dron.bg` и `PUBLIC_BASE_URL` трябва да са
> окончателни. Те се съдържат в кода и след отпечатване не могат да се сменят.

## Първоначална настройка (веднъж)

Тези стъпки са ръчни. След тях всичко става с `git push`.

### 1. DNS в Cloudflare
1. A запис `qr` към IP адреса на сървъра, **засега „DNS only“ (сив облак)**. Сертификатът се
   издава по-лесно, докато заявките стигат директно до сървъра. Оранжевият облак се включва в стъпка 4.
2. **SSL/TLS режим** на зоната `dron.bg` трябва да е **„Full (strict)“** (или поне „Full“).
   `orthoteh.dron.bg` работи със същия модел, така че вероятно вече е така, но провери.
   **Не го сменяй на „Flexible“**, защото това ще счупи и другите поддомейни.

Провери с `dig +short qr.dron.bg`: трябва да върне IP адреса на сървъра.

### 2. Папка и настройки на сървъра
Сървърът няма нужда от git. GitHub Actions копира `docker-compose.yml` и скриптовете при всеки деплой.
```bash
mkdir -p /home/orthoteh-bg.com/qr-manuals/data
cd /home/orthoteh-bg.com/qr-manuals
sudo chown -R 10001:10001 data       # потребителят в контейнера (uid 10001) пише тук
nano qr-manuals.env                  # съдържанието е по-долу
```
```
PUBLIC_BASE_URL=https://qr.dron.bg
TRUST_PROXY=true
MAX_UPLOAD_MB=20
MAX_REQUEST_MB=95
RUST_LOG=qr_manuals=info,tower_http=warn
```
SSH потребителят за деплоя (`VPS_USER`) трябва да е собственик на папката и да е в групата
`docker` (`sudo usermod -aG docker <потребител>`).

### 3. GitHub secrets
В repo-то: **Settings → Secrets and variables → Actions → New repository secret**.
Имената са същите като в ORTHOTEH, така че стойностите може да са същите.

| Secret | Стойност |
|---|---|
| `VPS_HOST` | адресът на сървъра |
| `VPS_USER` | SSH потребителят |
| `VPS_SSH_KEY` | частният SSH ключ на потребителя (целият файл) |
| `VPS_SSH_PASSPHRASE` | паролата на ключа, или празно |
| `VPS_DEPLOY_PATH` | `/home/orthoteh-bg.com/qr-manuals` |

Не трябва PAT: сървърът тегли image-а от ghcr.io с временния токен на workflow-а.

### 4. nginx, HTTPS и Cloudflare
Файловете за nginx са в repo-то в `deploy/nginx/` и се копират на сървъра при всеки деплой.
Преди първия деплой ги вземи от GitHub или ги копирай с `scp`.
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
`./scripts/cloudflare-real-ip.sh > deploy/nginx/cloudflare-real-ip.conf`, commit и копиране в `/etc/nginx/snippets/`.

### 5. Първи деплой
`git push` в `main`, или **Actions → Build and deploy → Run workflow**. Първият билд
компилира Rust и отнема около 10 минути, следващите са по-бързи заради кеша.
Workflow-ът завършва с проверка на `https://qr.dron.bg/health`.

### 6. Потребител
```bash
cd /home/orthoteh-bg.com/qr-manuals
docker compose exec qr-manuals qr-manuals create-admin твоят@email.bg
```
Със същата команда се сменя и забравена парола.

### 7. Проверка
```bash
curl -s https://qr.dron.bg/health                                   # ok
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

### 8. Нощен backup
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
Commit в `main`. Ходът се вижда в **Actions** в GitHub. Миграциите на базата се пускат
автоматично при старт. Ако тестовете паднат, нищо не се пуска.

## Връщане към предишна версия
Автоматично, ако новата версия не стане healthy. На ръка, ако тя работи, но има бъг:
- **Най-лесно:** `git revert` на проблемния commit и push. Пуска се като нормален деплой.
- **Веднага, на сървъра:** предишният image се пази.
  ```bash
  cd /home/orthoteh-bg.com/qr-manuals
  docker images ghcr.io/*/qr-manuals         # виж наличните версии (тагът е commit-ът)
  ./scripts/deploy.sh ghcr.io/<owner>/qr-manuals:<commit>
  ```
  Следващият commit в `main` ще пусне отново най-новата версия.

## Възстановяване от backup
```bash
cd /home/orthoteh-bg.com/qr-manuals
docker compose stop
cp data/backups/app-ГГГГММДД-ЧЧММСС.db data/app.db
rm -f data/app.db-wal data/app.db-shm
sudo chown 10001:10001 data/app.db
docker compose start
```
Качените файлове са в `data/uploads/`. При пълна загуба на сървъра се връщат от `BACKUP_REMOTE`.

## Ограничения от Cloudflare (безплатен план)
- Едно качване може да е до 100 MB общо. Приложението го ограничава до 95 MB
  (`MAX_REQUEST_MB`) и предупреждава в браузъра преди изпращане.
- Снимките и PDF файловете се кешират от Cloudflare. Изтрит файл може да остане достъпен в
  кеша му по директния си адрес, но вече не се показва на страницата на продукта.

## Полезно
```bash
docker compose logs -f --tail 100          # логове
docker compose logs | grep -E 'WARN|ERROR' # само проблемите
du -sh data/uploads                        # колко място заемат файловете
```
