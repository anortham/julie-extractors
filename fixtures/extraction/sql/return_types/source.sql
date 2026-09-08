CREATE TYPE result AS (x int);
CREATE FUNCTION make() RETURNS result LANGUAGE SQL AS $$ SELECT ROW(1)::result $$;
