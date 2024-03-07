CREATE OR REPLACE FUNCTION sload_cache_notify()
RETURNS trigger AS
$$
DECLARE
    skip_notification text;
BEGIN
    -- Retrieve the 'skip_notification' session variable to determine if notifications should be skipped.
    -- This is useful during bulk imports or historical data loading where notifying the current node
    -- of its own changes is redundant and unnecessary, reducing unnecessary notification traffic.
    skip_notification := current_setting('skip_notification', true) ON ERROR 'false';

    -- If 'skip_notification' is explicitly set to 'true', the function exits early.
    -- This conditional check serves to bypass the notification mechanism in scenarios where
    -- the current node is already aware of the changes being made, such as during initial data load
    -- or batch updates, thereby improving efficiency by reducing no-op notifications.
    IF skip_notification = 'true' THEN
        RETURN NEW;
    END IF;

    -- For all other cases, proceed with sending a notification about the change.
    -- This ensures that external observers or other components in the system are informed about updates
    -- to the 'account_slots', allowing them to react to new or modified data in a timely manner.
    PERFORM pg_notify(
        'sload_cache_channel',
        json_build_object(
            'index', NEW.idx,
            'value', NEW.value,
            'address', NEW.account_address,
            'block', NEW.creation_block
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
