-- Open this script on an ERD page and review the import preview.
CREATE SCHEMA IF NOT EXISTS shop;

CREATE TABLE shop.customers (
    tenant_id INTEGER NOT NULL,
    id BIGINT NOT NULL,
    email TEXT NOT NULL,
    display_name TEXT,
    CONSTRAINT customers_pk PRIMARY KEY (tenant_id, id),
    CONSTRAINT customers_email_uk UNIQUE (tenant_id, email)
);

CREATE TABLE shop.orders (
    tenant_id INTEGER NOT NULL,
    id BIGINT NOT NULL,
    customer_id BIGINT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT orders_pk PRIMARY KEY (tenant_id, id)
);

-- Relationships may be declared after all tables.
ALTER TABLE shop.orders ADD CONSTRAINT orders_customer_fk
    FOREIGN KEY (tenant_id, customer_id)
    REFERENCES shop.customers (tenant_id, id) ON DELETE RESTRICT;

CREATE INDEX orders_pending_idx ON shop.orders (created_at DESC)
    WHERE status = 'pending';
