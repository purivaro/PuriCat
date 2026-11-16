-- Disposable demo data for the end-to-end smoke test. The runner recreates `puricat_e2e`.
CREATE SCHEMA shop;
CREATE TYPE shop.order_status AS ENUM ('Pending', 'Processing', 'Completed', 'Cancelled');
CREATE TABLE shop.customers (
  id integer GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  name text NOT NULL,
  city text,
  tags text[] NOT NULL DEFAULT '{}',
  created_at timestamptz NOT NULL DEFAULT now()
);
INSERT INTO shop.customers (name, city, tags) VALUES
  ('สมชาย ใจดี', 'กรุงเทพฯ', '{vip}'),
  ('Ananya Wong', 'Chiang Mai', '{}'),
  ('ร้านข้าวหอม', 'ขอนแก่น', '{wholesale,north-east}'),
  ('Kenji Tanaka', 'Osaka', '{}'),
  ('มาลี ศรีสุข', 'ภูเก็ต', '{vip,south}');

CREATE TABLE shop.orders (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  order_no text NOT NULL UNIQUE,
  customer_id integer NOT NULL REFERENCES shop.customers (id),
  order_date timestamptz NOT NULL,
  status shop.order_status NOT NULL DEFAULT 'Pending',
  total_amount numeric(14,2) NOT NULL DEFAULT 0,
  meta jsonb
);
INSERT INTO shop.orders (order_no, customer_id, order_date, status, total_amount, meta)
SELECT 'SO-' || (10000 + n),
       1 + (n * 7) % 5,
       timestamptz '2026-01-10 10:24:31+07' + n * interval '137 minutes',
       (ARRAY['Completed', 'Processing', 'Pending', 'Cancelled'])[1 + (n * 3) % 4]::shop.order_status,
       round(120 + ((n * 7919) % 900000) / 100.0, 2),
       CASE WHEN n % 9 = 0 THEN NULL ELSE jsonb_build_object('channel', CASE WHEN n % 2 = 0 THEN 'web' ELSE 'line' END) END
FROM generate_series(1, 1250) AS n;

CREATE VIEW shop.order_totals AS
  SELECT c.name AS customer, count(*) AS orders, sum(o.total_amount) AS total
  FROM shop.orders o JOIN shop.customers c ON c.id = o.customer_id GROUP BY c.id, c.name;
ANALYZE;
