#!/bin/sh
# Генерира nginx настройката, с която nginx вижда реалния адрес на клиента зад Cloudflare.
# Адресите на Cloudflare се сменят рядко. Пусни го отново, ако Cloudflare обяви промяна:
#   ./scripts/cloudflare-real-ip.sh > deploy/nginx/cloudflare-real-ip.conf
set -eu

v4=$(curl -fsS https://www.cloudflare.com/ips-v4)
v6=$(curl -fsS https://www.cloudflare.com/ips-v6)

echo "# Генерирано от scripts/cloudflare-real-ip.sh на $(date -u +%Y-%m-%d). Не се редактира на ръка."
echo "# Само за връзки от тези адреси nginx приема CF-Connecting-IP като адрес на клиента."
echo "# Заявка директно към сървъра (без Cloudflare) не може да подмени адреса си."
for range in $v4 $v6; do
    echo "set_real_ip_from $range;"
done
echo "real_ip_header CF-Connecting-IP;"
