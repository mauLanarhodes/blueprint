-- Dense retail ERD example: 30 tables, including self and composite references.
-- Sample schema modeled on the supplied diagram; no SQL is executed by Blueprint.

CREATE TABLE stores (
    store_id BIGINT NOT NULL,
    store_code VARCHAR(20) NOT NULL UNIQUE,
    name VARCHAR(150) NOT NULL,
    phone VARCHAR(30),
    opened_on DATE,
    is_active BOOLEAN NOT NULL DEFAULT true,
    PRIMARY KEY (store_id)
);

CREATE TABLE store_addresses (
    store_id BIGINT NOT NULL,
    street_1 VARCHAR(150) NOT NULL,
    street_2 VARCHAR(150),
    city VARCHAR(100) NOT NULL,
    state_region VARCHAR(100) NOT NULL,
    postal_code VARCHAR(20) NOT NULL,
    country_code CHAR(2) NOT NULL,
    PRIMARY KEY (store_id),
    FOREIGN KEY (store_id) REFERENCES stores (store_id)
);

CREATE TABLE employees (
    employee_id BIGINT NOT NULL,
    store_id BIGINT NOT NULL,
    employee_number VARCHAR(30) NOT NULL UNIQUE,
    first_name VARCHAR(100) NOT NULL,
    last_name VARCHAR(100) NOT NULL,
    email VARCHAR(255) UNIQUE,
    hired_on DATE NOT NULL,
    terminated_on DATE,
    PRIMARY KEY (employee_id),
    FOREIGN KEY (store_id) REFERENCES stores (store_id)
);

CREATE TABLE roles (
    role_id BIGINT NOT NULL,
    role_name VARCHAR(80) NOT NULL UNIQUE,
    PRIMARY KEY (role_id)
);

CREATE TABLE employee_roles (
    employee_id BIGINT NOT NULL,
    role_id BIGINT NOT NULL,
    assigned_on DATE NOT NULL DEFAULT CURRENT_DATE,
    PRIMARY KEY (employee_id, role_id),
    FOREIGN KEY (employee_id) REFERENCES employees (employee_id),
    FOREIGN KEY (role_id) REFERENCES roles (role_id)
);

CREATE TABLE customers (
    customer_id BIGINT NOT NULL,
    first_name VARCHAR(100) NOT NULL,
    last_name VARCHAR(100) NOT NULL,
    email VARCHAR(255) UNIQUE,
    phone VARCHAR(30),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (customer_id)
);

CREATE TABLE customer_addresses (
    address_id BIGINT NOT NULL,
    customer_id BIGINT NOT NULL,
    label VARCHAR(50),
    street_1 VARCHAR(150) NOT NULL,
    street_2 VARCHAR(150),
    city VARCHAR(100) NOT NULL,
    state_region VARCHAR(100) NOT NULL,
    postal_code VARCHAR(20) NOT NULL,
    country_code CHAR(2) NOT NULL,
    PRIMARY KEY (address_id),
    FOREIGN KEY (customer_id) REFERENCES customers (customer_id)
);

CREATE TABLE loyalty_accounts (
    loyalty_account_id BIGINT NOT NULL,
    customer_id BIGINT NOT NULL UNIQUE,
    card_number VARCHAR(40) NOT NULL UNIQUE,
    points_balance INTEGER NOT NULL DEFAULT 0,
    joined_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (loyalty_account_id),
    FOREIGN KEY (customer_id) REFERENCES customers (customer_id)
);

CREATE TABLE suppliers (
    supplier_id BIGINT NOT NULL,
    supplier_code VARCHAR(30) NOT NULL UNIQUE,
    name VARCHAR(150) NOT NULL,
    contact_email VARCHAR(255),
    phone VARCHAR(30),
    is_active BOOLEAN NOT NULL DEFAULT true,
    PRIMARY KEY (supplier_id)
);

CREATE TABLE brands (
    brand_id BIGINT NOT NULL,
    name VARCHAR(120) NOT NULL UNIQUE,
    PRIMARY KEY (brand_id)
);

CREATE TABLE categories (
    category_id BIGINT NOT NULL,
    parent_category_id BIGINT,
    name VARCHAR(120) NOT NULL,
    PRIMARY KEY (category_id),
    FOREIGN KEY (parent_category_id) REFERENCES categories (category_id)
);

CREATE TABLE products (
    product_id BIGINT NOT NULL,
    sku VARCHAR(40) NOT NULL UNIQUE,
    brand_id BIGINT,
    name VARCHAR(180) NOT NULL,
    description TEXT,
    unit_of_measure VARCHAR(20) NOT NULL DEFAULT 'each',
    current_price NUMERIC(12,2) NOT NULL,
    taxable BOOLEAN NOT NULL DEFAULT true,
    is_active BOOLEAN NOT NULL DEFAULT true,
    PRIMARY KEY (product_id),
    FOREIGN KEY (brand_id) REFERENCES brands (brand_id)
);

CREATE TABLE product_categories (
    product_id BIGINT NOT NULL,
    category_id BIGINT NOT NULL,
    PRIMARY KEY (product_id, category_id),
    FOREIGN KEY (product_id) REFERENCES products (product_id),
    FOREIGN KEY (category_id) REFERENCES categories (category_id)
);

CREATE TABLE product_barcodes (
    barcode_id BIGINT NOT NULL,
    product_id BIGINT NOT NULL,
    barcode VARCHAR(64) NOT NULL UNIQUE,
    barcode_type VARCHAR(20) NOT NULL DEFAULT 'EAN',
    PRIMARY KEY (barcode_id),
    FOREIGN KEY (product_id) REFERENCES products (product_id)
);

CREATE TABLE supplier_products (
    supplier_id BIGINT NOT NULL,
    product_id BIGINT NOT NULL,
    supplier_sku VARCHAR(60),
    unit_cost NUMERIC(12,2) NOT NULL,
    lead_time_days INTEGER,
    PRIMARY KEY (supplier_id, product_id),
    FOREIGN KEY (supplier_id) REFERENCES suppliers (supplier_id),
    FOREIGN KEY (product_id) REFERENCES products (product_id)
);

CREATE TABLE store_inventory (
    store_id BIGINT NOT NULL,
    product_id BIGINT NOT NULL,
    quantity_on_hand NUMERIC(12,3) NOT NULL DEFAULT 0,
    reorder_point NUMERIC(12,3) NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (store_id, product_id),
    FOREIGN KEY (store_id) REFERENCES stores (store_id),
    FOREIGN KEY (product_id) REFERENCES products (product_id)
);

CREATE TABLE stock_movements (
    movement_id BIGINT NOT NULL,
    store_id BIGINT NOT NULL,
    product_id BIGINT NOT NULL,
    quantity_change NUMERIC(12,3) NOT NULL,
    reason VARCHAR(40) NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    employee_id BIGINT,
    notes TEXT,
    PRIMARY KEY (movement_id),
    FOREIGN KEY (store_id) REFERENCES stores (store_id),
    FOREIGN KEY (product_id) REFERENCES products (product_id),
    FOREIGN KEY (employee_id) REFERENCES employees (employee_id),
    FOREIGN KEY (store_id, product_id) REFERENCES store_inventory (store_id, product_id)
);

CREATE TABLE purchase_orders (
    purchase_order_id BIGINT NOT NULL,
    store_id BIGINT NOT NULL,
    supplier_id BIGINT NOT NULL,
    ordered_by_employee_id BIGINT,
    status VARCHAR(20) NOT NULL DEFAULT 'draft',
    ordered_at TIMESTAMPTZ,
    expected_on DATE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (purchase_order_id),
    FOREIGN KEY (store_id) REFERENCES stores (store_id),
    FOREIGN KEY (supplier_id) REFERENCES suppliers (supplier_id),
    FOREIGN KEY (ordered_by_employee_id) REFERENCES employees (employee_id)
);

CREATE TABLE purchase_order_lines (
    purchase_order_line_id BIGINT NOT NULL,
    purchase_order_id BIGINT NOT NULL,
    product_id BIGINT NOT NULL,
    quantity_ordered NUMERIC(12,3) NOT NULL,
    unit_cost NUMERIC(12,2) NOT NULL,
    PRIMARY KEY (purchase_order_line_id),
    FOREIGN KEY (purchase_order_id) REFERENCES purchase_orders (purchase_order_id),
    FOREIGN KEY (product_id) REFERENCES products (product_id)
);

CREATE TABLE receipts (
    receipt_id BIGINT NOT NULL,
    purchase_order_id BIGINT NOT NULL,
    received_by_employee_id BIGINT,
    received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    supplier_reference VARCHAR(80),
    PRIMARY KEY (receipt_id),
    FOREIGN KEY (purchase_order_id) REFERENCES purchase_orders (purchase_order_id),
    FOREIGN KEY (received_by_employee_id) REFERENCES employees (employee_id)
);

CREATE TABLE receipt_lines (
    receipt_line_id BIGINT NOT NULL,
    receipt_id BIGINT NOT NULL,
    purchase_order_line_id BIGINT NOT NULL,
    quantity_received NUMERIC(12,3) NOT NULL,
    PRIMARY KEY (receipt_line_id),
    FOREIGN KEY (receipt_id) REFERENCES receipts (receipt_id),
    FOREIGN KEY (purchase_order_line_id) REFERENCES purchase_order_lines (purchase_order_line_id)
);

CREATE TABLE promotions (
    promotion_id BIGINT NOT NULL,
    name VARCHAR(150) NOT NULL,
    discount_type VARCHAR(20) NOT NULL,
    discount_value NUMERIC(12,2) NOT NULL,
    starts_at TIMESTAMPTZ NOT NULL,
    ends_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (promotion_id)
);

CREATE TABLE promotion_products (
    promotion_id BIGINT NOT NULL,
    product_id BIGINT NOT NULL,
    PRIMARY KEY (promotion_id, product_id),
    FOREIGN KEY (promotion_id) REFERENCES promotions (promotion_id),
    FOREIGN KEY (product_id) REFERENCES products (product_id)
);

CREATE TABLE coupons (
    coupon_id BIGINT NOT NULL,
    promotion_id BIGINT NOT NULL,
    code VARCHAR(50) NOT NULL UNIQUE,
    max_uses INTEGER,
    expires_at TIMESTAMPTZ,
    PRIMARY KEY (coupon_id),
    FOREIGN KEY (promotion_id) REFERENCES promotions (promotion_id)
);

CREATE TABLE sales (
    sale_id BIGINT NOT NULL,
    store_id BIGINT NOT NULL,
    customer_id BIGINT,
    cashier_employee_id BIGINT,
    sale_number VARCHAR(40) NOT NULL,
    sold_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    status VARCHAR(20) NOT NULL DEFAULT 'completed',
    subtotal NUMERIC(12,2) NOT NULL,
    tax_total NUMERIC(12,2) NOT NULL DEFAULT 0,
    discount_total NUMERIC(12,2) NOT NULL DEFAULT 0,
    grand_total NUMERIC(12,2) NOT NULL,
    PRIMARY KEY (sale_id),
    FOREIGN KEY (store_id) REFERENCES stores (store_id),
    FOREIGN KEY (customer_id) REFERENCES customers (customer_id),
    FOREIGN KEY (cashier_employee_id) REFERENCES employees (employee_id)
);

CREATE TABLE sale_lines (
    sale_line_id BIGINT NOT NULL,
    sale_id BIGINT NOT NULL,
    product_id BIGINT NOT NULL,
    quantity NUMERIC(12,3) NOT NULL,
    unit_price NUMERIC(12,2) NOT NULL,
    discount_amount NUMERIC(12,2) NOT NULL DEFAULT 0,
    tax_amount NUMERIC(12,2) NOT NULL DEFAULT 0,
    line_total NUMERIC(12,2) NOT NULL,
    PRIMARY KEY (sale_line_id),
    FOREIGN KEY (sale_id) REFERENCES sales (sale_id),
    FOREIGN KEY (product_id) REFERENCES products (product_id)
);

CREATE TABLE sale_coupons (
    sale_id BIGINT NOT NULL,
    coupon_id BIGINT NOT NULL,
    discount_amount NUMERIC(12,2) NOT NULL,
    PRIMARY KEY (sale_id, coupon_id),
    FOREIGN KEY (sale_id) REFERENCES sales (sale_id),
    FOREIGN KEY (coupon_id) REFERENCES coupons (coupon_id)
);

CREATE TABLE payments (
    payment_id BIGINT NOT NULL,
    sale_id BIGINT NOT NULL,
    method VARCHAR(20) NOT NULL,
    amount NUMERIC(12,2) NOT NULL,
    paid_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    transaction_reference VARCHAR(100),
    PRIMARY KEY (payment_id),
    FOREIGN KEY (sale_id) REFERENCES sales (sale_id)
);

CREATE TABLE returns (
    return_id BIGINT NOT NULL,
    sale_id BIGINT NOT NULL,
    store_id BIGINT NOT NULL,
    processed_by_employee_id BIGINT,
    returned_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    reason TEXT,
    refund_total NUMERIC(12,2) NOT NULL,
    PRIMARY KEY (return_id),
    FOREIGN KEY (sale_id) REFERENCES sales (sale_id),
    FOREIGN KEY (store_id) REFERENCES stores (store_id),
    FOREIGN KEY (processed_by_employee_id) REFERENCES employees (employee_id)
);

CREATE TABLE return_lines (
    return_line_id BIGINT NOT NULL,
    return_id BIGINT NOT NULL,
    sale_line_id BIGINT NOT NULL,
    quantity_returned NUMERIC(12,3) NOT NULL,
    refund_amount NUMERIC(12,2) NOT NULL,
    PRIMARY KEY (return_line_id),
    FOREIGN KEY (return_id) REFERENCES returns (return_id),
    FOREIGN KEY (sale_line_id) REFERENCES sale_lines (sale_line_id)
);

