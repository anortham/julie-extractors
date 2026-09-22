CREATE TABLE public.users (
    id bigint NOT NULL,
    email text NOT NULL
);

CREATE TABLE public.orders (
    id bigint NOT NULL,
    user_id bigint NOT NULL,
    created date NOT NULL
);

CREATE TABLE audit.users (
    id bigint NOT NULL
);

CREATE TABLE audit.events (
    id bigint NOT NULL,
    user_id bigint NOT NULL,
    CONSTRAINT events_user_fkey FOREIGN KEY (user_id) REFERENCES identity.users (id)
);

ALTER TABLE ONLY public.orders
    ADD CONSTRAINT orders_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.orders
    ADD CONSTRAINT orders_account_fkey FOREIGN KEY (user_id) REFERENCES billing.accounts(id);

ALTER TABLE public.orders ADD COLUMN status text DEFAULT 'new';

CREATE FUNCTION public.recent_orders(since date) RETURNS SETOF public.orders
LANGUAGE sql STABLE
AS $$
  SELECT *
  FROM public.orders
  WHERE created > since
$$;

CREATE FUNCTION public.order_count() RETURNS bigint
LANGUAGE sql
AS $$ SELECT count(*) FROM public.recent_orders(now()::date) $$;

CREATE FUNCTION public.touch() RETURNS trigger
LANGUAGE plpgsql
AS $$ BEGIN RETURN NEW; END; $$;

CREATE TRIGGER orders_touch BEFORE UPDATE ON public.orders FOR EACH ROW EXECUTE FUNCTION public.touch();

CREATE VIEW public.big_orders AS SELECT id FROM public.orders WHERE id > 100;

CREATE VIEW public.external_invoices AS SELECT * FROM billing.invoices;

CREATE TABLE public.orders_backup AS SELECT * FROM public.orders;
