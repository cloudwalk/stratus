CREATE OR REPLACE FUNCTION sload_cache_notify()
RETURNS trigger AS
$$
BEGIN
    PERFORM pg_notify(
        'sload_cache_channel',
        json_build_object(
            'index', encode(NEW.idx, 'hex'),
            'value', encode(NEW.value, 'hex'),
            'address', encode(NEW.account_address, 'hex'),
            'block', NEW.creation_block
        )::text
    );
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER sload_cache
	AFTER INSERT OR UPDATE
	ON account_slots
	FOR EACH ROW
EXECUTE PROCEDURE sload_cache_notify();
