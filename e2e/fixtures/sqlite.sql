-- Demo data for the SQLite stage of the end-to-end test. Loaded into a throwaway file.
CREATE TABLE books (
  id     INTEGER PRIMARY KEY,
  title  TEXT NOT NULL,
  price  DECIMAL(8,2),
  pages  INTEGER NOT NULL DEFAULT 0,
  added  DATE,
  meta   JSON,
  cover  BLOB
);
INSERT INTO books (id, title, price, pages, added, meta, cover) VALUES
  (1, 'ธรรมบท ภาค ๑', 120.5, 248, '2026-01-15', '{"lang": "th", "volumes": 8}', x'89504E47'),
  (2, 'Pali Grammar', 95, 312, '2026-02-01', NULL, NULL),
  (3, 'พจนานุกรมบาลี–ไทย', 9007199254740993, 1024, NULL, '{"lang": "th"}', NULL);
CREATE TABLE tags (book_id INTEGER NOT NULL REFERENCES books (id), tag TEXT NOT NULL, PRIMARY KEY (book_id, tag)) WITHOUT ROWID;
INSERT INTO tags VALUES (1, 'ธรรมะ'), (1, 'บาลี'), (2, 'grammar');
CREATE VIEW long_books AS SELECT id, title, pages FROM books WHERE pages > 300;
