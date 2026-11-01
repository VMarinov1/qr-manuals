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
- Търсенето е по `products.search_text` (име + SKU с малки букви, попълва се от `ProductInput::search_text`), защото `LIKE` в SQLite не прави кирилицата case-insensitive.
- Slug: 8 символа без двусмислени знаци (0/O, 1/l/I), генерира се в `products::new_slug`.
- Формите са обикновен POST + redirect. При грешка страницата се рендира отново с 422 и `FormErrors`. HTMX се ползва само там, където има полза (търсене на живо с `hx-select`).
- Общите класове (`btn-primary`, `card`, `input`...) са `@utility` в `assets/app.css`.
- HTMX е в `static/vendor/htmx-2.0.4.min.js`, без CDN.

## Тестове
```bash
docker run --rm -v "$PWD":/app -v qrm-cargo-target:/app/target -v qrm-cargo-registry:/usr/local/cargo/registry -w /app rust:1-bookworm cargo test
```

## Билд и стартиране
Rust не е инсталиран локално, билдва се през Docker:

```bash
docker compose up -d --build
docker compose exec app qr-manuals create-admin admin@example.com
```

Конфигурация: `.env.example`. Данните (база и файлове) са във volume `/data`.

## Ред на работа
1. ✅ Скелет, база, миграции, Docker, `create-admin`
2. ✅ Вход и сесии
3. ✅ Продукти
4. Упътвания по езици и качване на файлове
5. QR код и публична страница
