# qr-manuals – упътвания на продукти с QR код

Качват се упътвания (снимки и PDF) за продукт, по едно на език. Всеки продукт има постоянен публичен линк `/m/:slug` и QR код, който се отпечатва и слага в опаковката.

## Стек
Rust, Axum 0.8, Askama 0.14 (шаблони в `templates/`), HTMX, Tailwind v4 (standalone CLI, вход `assets/app.css`, изход `static/app.css`), SQLite чрез SQLx 0.8 (runtime заявки, без `query!` макроси), argon2.

## Страници
| Маршрут | Достъп | Бележка |
|---|---|---|
| `/login` | публичен | админ интерфейсът е на български |
| `/products` | с вход | списък, търсене, нов продукт |
| `/products/:id` | с вход | табове по език, качване на файлове, линк и QR |
| `/m/:slug`, `/m/:slug/:lang` | публичен | за телефон, **без текст на интерфейса**, само съдържание и иконки. Езикът се избира от `Accept-Language`, иначе е `default_language` на продукта. Превключвателят показва само кодове (`BG · EN · DE · RO`) |

## Правила
- `public_slug` никога не се сменя, защото вече е отпечатан.
- Приемат се само `image/jpeg`, `image/png`, `image/webp` и `application/pdf`. Типът се проверява по съдържанието, не по разширението. Лимитът е `MAX_UPLOAD_MB`.
- Файловете са в `UPLOAD_DIR` със случайно име (`files.stored_name`).
- Езиците са в таблица `languages`. Нов език се добавя с миграция.
- Нова схема = нова миграция в `migrations/`. Старите не се редактират.
- Защитена страница = handler с аргумент `CurrentUser` (`src/web/auth.rs`). Без вход пренасочва към `/login?next=...`, а при HTMX заявка връща `HX-Redirect`.
- Сесии: случаен токен в бисквитка `qrm_session` (HttpOnly, SameSite=Lax, Secure при https `PUBLIC_BASE_URL`), в базата е само SHA-256 хешът му. Валидност 30 дни.
- CSRF: middleware `check_origin` отхвърля POST/PUT/DELETE с чужд `Origin`.
- Ограничение на входа (`src/web/rate_limit.rs`, в паметта): 10 грешни опита от IP или 30 за email за 15 мин → 429 с `Retry-After`. Блокиран опит не проверява паролата. Успешен вход нулира броячите. Зад reverse proxy трябва `TRUST_PROXY=true`, иначе всички клиенти са с IP-то на proxy-то.
- Търсенето е по `products.search_text` (име + SKU с малки букви, попълва се от `ProductInput::search_text`), защото `LIKE` в SQLite не прави кирилицата case-insensitive.
- Slug: 8 символа без двусмислени знаци (0/O, 1/l/I), генерира се в `products::new_slug`.
- Формите са обикновен POST + redirect. При грешка страницата се рендира отново с 422 и `FormErrors`. HTMX се ползва само там, където има полза (търсене на живо с `hx-select`).
- Общите класове (`btn-primary`, `card`, `input`...) са `@utility` в `assets/app.css`.
- HTMX е в `static/vendor/htmx-2.0.4.min.js`, без CDN.
- Упътване (`manuals`) съществува, докато има файлове: създава се при първото качване и се трие с последния файл (`src/manuals.rs`).
- Качване (`src/web/files.rs`): поточно във временен `.upload-*` файл, лимит на файл `MAX_UPLOAD_MB`, видът по първите байтове (`storage::FileKind::detect`), после rename. Временните файлове се чистят при старт.
- Снимките (`src/images.rs`): при качване се декодират (повредените се отказват) и се прави JPEG копие до 1600 px с приложено EXIF завъртане (`files.display_name`). Оригиналът остава непроменен. Публичната страница и миниатюрите ползват `FileRow::shown_name()`. Обработката е в `spawn_blocking`, най-много 2 едновременно.
- Файловете се сервират публично на `/f/{stored_name}` (с Range и `immutable` кеш). Изтриването на файл или продукт трие и от диска.
- Съобщения след redirect: `web::flash::Flash` (бисквитка за една заявка).
- Публична страница (`src/web/public.rs`, `templates/public/`): езикът се избира от `language::choose` (телефон → по подразбиране → първи наличен). `/m/:slug/:lang` без упътване на този език пренасочва към `/m/:slug`. Отговорът е с `Vary: Accept-Language`, `no-cache`, без бисквитки и с `noindex`.
- QR (`src/qr.rs`): ниво на корекция Q, сочи към `PUBLIC_BASE_URL/m/:slug`. **`PUBLIC_BASE_URL` трябва да е истинският публичен адрес преди първия печат**, иначе отпечатаните кодове ще сочат грешно.
- Product id-тата се преизползват след изтриване (SQLite `INTEGER PRIMARY KEY`), затова навън се ползва само `public_slug`.

## Тестове
```bash
docker run --rm -v "$PWD":/app -v qrm-cargo-target:/app/target -v qrm-cargo-registry:/usr/local/cargo/registry -w /app rust:1-bookworm cargo test
```

## Билд и стартиране
Rust не е инсталиран локално, билдва се през Docker:

```bash
cp qr-manuals.env.example qr-manuals.env   # локално: PUBLIC_BASE_URL=http://localhost:5200, TRUST_PROXY=false
docker compose up -d --build               # билдът пуска и тестовете
docker compose exec qr-manuals qr-manuals create-admin admin@example.com
```

Приложението е на `127.0.0.1:5200`. Данните (база, файлове, `backups/`) са в `./data`.
Пускане на сървъра, nginx, backup и възстановяване: `DEPLOY.md`. Production върви на сървъра на ORTHOTEH зад nginx (`deploy/nginx/qr-manuals.conf`).
Подкоманди на бинарния файл: `serve` (по подразбиране), `create-admin`, `backup [папка] --keep N`, `health`.

## Ред на работа
1. ✅ Скелет, база, миграции, Docker, `create-admin`
2. ✅ Вход и сесии
3. ✅ Продукти
4. ✅ Упътвания по езици и качване на файлове
5. ✅ QR код и публична страница
