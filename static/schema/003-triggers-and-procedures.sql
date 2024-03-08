CREATE OR REPLACE FUNCTION sload_cache_notify()
RETURNS trigger AS
$$
BEGIN
    PERFORM pg_notify(
        'sload_cache_channel',
        json_build_object(
            'index', NEW.idx,
            'value', NEW.value,
            'address', NEW.account_address,
            'block', NEW.creation_block,
        )::text
    );
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER sload_cache
	AFTER INSERT OR UPDATE OF cache
	ON sload
	FOR EACH ROW
EXECUTE PROCEDURE sload_cache_notify();
