CREATE SCHEMA app;
CREATE SCHEMA analytics;
CREATE SCHEMA test_named_schema;

CREATE FUNCTION test_user() RETURNS SETOF TEXT LANGUAGE SQL AS $$ SELECT ok(true); $$;
CREATE FUNCTION setup_user() RETURNS SETOF TEXT LANGUAGE SQL AS $$ SELECT pass('setup'); $$;
CREATE FUNCTION teardown_user() RETURNS SETOF TEXT LANGUAGE SQL AS $$ SELECT pass('teardown'); $$;

SELECT * FROM runtests('app', '^test');
SELECT * FROM do_tap('analytics', '^test');
SELECT * FROM runtests('missing_schema', '^test');
