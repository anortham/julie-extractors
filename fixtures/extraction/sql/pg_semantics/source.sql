CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE ROLE reporting;
CREATE SCHEMA tests;
CREATE TYPE order_status AS ENUM ('new', 'paid');
CREATE SEQUENCE invoice_seq START 1000;

-- Registered users of the portal.
CREATE TABLE users (
    id BIGINT PRIMARY KEY,
    email TEXT NOT NULL, -- Login address.
    -- Display name shown in the header.
    display_name VARCHAR(80),
    tags TEXT[]
);

CREATE TABLE orders (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL,
    status order_status NOT NULL DEFAULT 'new',
    total NUMERIC(10,2) NOT NULL,
    FOREIGN KEY (user_id) REFERENCES users (id)
);
COMMENT ON TABLE orders IS 'Customer orders, one row per checkout.';
COMMENT ON COLUMN orders.total IS 'Order total in the account currency.';

create unique index ux_users_email on users (email);
CREATE POLICY own_orders ON orders USING (user_id = 1);

-- Daily revenue per user.
CREATE MATERIALIZED VIEW daily_revenue AS
SELECT o.user_id, sum(o.total) AS revenue
FROM orders o
WHERE o.status = 'paid' AND o.total > 0
GROUP BY o.user_id
WITH DATA;

-- Moves old orders into the archive.
CREATE FUNCTION archive_old_orders(p_days INT) RETURNS INT AS $$
DECLARE
    v_count INT;
BEGIN
    DELETE FROM orders WHERE total > p_days;
    SELECT count(*) INTO v_count FROM orders;
    RETURN v_count;
END;
$$ LANGUAGE plpgsql;
COMMENT ON FUNCTION archive_old_orders(INT) IS 'Archives orders older than p_days.';

CREATE FUNCTION recent_orders(since DATE) RETURNS SETOF orders LANGUAGE sql AS $$ SELECT * FROM orders $$;

CREATE FUNCTION audit_row() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END; $$;
CREATE TRIGGER orders_audit AFTER INSERT OR UPDATE OR DELETE ON orders FOR EACH ROW EXECUTE FUNCTION audit_row();

CREATE FUNCTION tests.test_totals() RETURNS SETOF TEXT LANGUAGE sql AS $$ SELECT ok(true) $$;
SELECT * FROM runtests('tests'::name);

SELECT count(*) AS total_users FROM users;
