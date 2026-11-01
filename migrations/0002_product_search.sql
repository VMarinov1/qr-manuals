-- SQLite сравнява без значение от главни/малки букви само латиница, затова
-- за търсенето се пази отделно име + SKU с малки букви, попълвано от приложението.
ALTER TABLE products ADD COLUMN search_text TEXT NOT NULL DEFAULT '';
