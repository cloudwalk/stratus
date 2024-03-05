SELECT *
FROM historical_slots
WHERE NOT EXISTS (
        SELECT 1
        FROM imported_slots
        WHERE historical_slots.account_address = imported_slots.account_address
            AND historical_slots.block_number = imported_slots.block_number
            AND historical_slots.index = imported_slots.index
            AND historical_slots.value = imported_slots.value
    );


SELECT *
FROM historical_slots
EXCEPT
SELECT *
FROM imported_slots; -- I think this is better


-- We can also get both differences

(SELECT *, "not in historical_slots"
FROM imported_slots
EXCEPT
SELECT *
FROM historical_slots)
UNION
(SELECT *, "not in imported_slots"
FROM historical_slots
EXCEPT
SELECT *
FROM imported_slots);
