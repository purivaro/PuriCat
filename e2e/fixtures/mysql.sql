-- Disposable demo data for the end-to-end smoke test. The runner recreates `puricat_e2e`.
SET NAMES utf8mb4;
SET SESSION cte_max_recursion_depth = 5000;
CREATE TABLE customers (
  id INT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
  name VARCHAR(120) NOT NULL,
  city VARCHAR(80),
  email VARCHAR(160),
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
INSERT INTO customers (name, city, email) VALUES
  ('สมชาย ใจดี', 'กรุงเทพฯ', 'somchai@example.com'),
  ('Ananya Wong', 'Chiang Mai', 'ananya@example.com'),
  ('ร้านข้าวหอม', 'ขอนแก่น', NULL),
  ('Kenji Tanaka', 'Osaka', 'kenji@example.com'),
  ('มาลี ศรีสุข', 'ภูเก็ต', 'malee@example.com');

CREATE TABLE products (
  sku VARCHAR(20) NOT NULL PRIMARY KEY,
  title VARCHAR(160) NOT NULL,
  price DECIMAL(12,2) NOT NULL,
  in_stock TINYINT(1) NOT NULL DEFAULT 1,
  attributes JSON
);
INSERT INTO products VALUES
  ('TEA-001', 'ชาเขียวมัทฉะ 100 ก.', 320.00, 1, '{"origin": "Uji", "organic": true}'),
  ('RICE-05', 'ข้าวหอมมะลิ 5 กก.', 245.50, 1, '{"origin": "ทุ่งกุลา"}'),
  ('CUP-012', 'Ceramic cup, 350 ml', 189.00, 0, NULL);

CREATE TABLE orders (
  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
  order_no VARCHAR(20) NOT NULL UNIQUE,
  customer_id INT UNSIGNED NOT NULL,
  order_date DATETIME NOT NULL,
  status ENUM('Pending', 'Processing', 'Completed', 'Cancelled') NOT NULL DEFAULT 'Pending',
  total_amount DECIMAL(14,2) NOT NULL DEFAULT 0,
  note TEXT,
  CONSTRAINT fk_orders_customer FOREIGN KEY (customer_id) REFERENCES customers (id)
);
INSERT INTO orders (order_no, customer_id, order_date, status, total_amount, note)
WITH RECURSIVE seq (n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM seq WHERE n < 1250)
SELECT CONCAT('SO-', 10000 + n),
       1 + (n * 7) % 5,
       TIMESTAMPADD(MINUTE, n * 137, '2026-01-10 10:24:31'),
       ELT(1 + (n * 3) % 4, 'Completed', 'Processing', 'Pending', 'Cancelled'),
       ROUND(120 + (n * 7919) % 900000 / 100, 2),
       IF(n % 9 = 0, NULL, IF(n % 5 = 0, 'ส่งด่วน — โทรแจ้งก่อนส่ง', ''))
FROM seq;
-- One note longer than a page shows (1000 characters), for the whole-value dialog.
UPDATE orders SET note = CONCAT(REPEAT('บันทึกยาว ', 250), 'จบ') WHERE id = 3;

CREATE VIEW order_totals AS
  SELECT c.name AS customer, COUNT(*) AS orders, SUM(o.total_amount) AS total
  FROM orders o JOIN customers c ON c.id = o.customer_id GROUP BY c.id, c.name;

CREATE TABLE audit_log (happened_at DATETIME NOT NULL, message VARCHAR(200) NOT NULL);
INSERT INTO audit_log VALUES ('2026-01-10 10:00:00', 'seeded');
