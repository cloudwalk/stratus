WITH delete_blocks AS (
 DELETE FROM blocks WHERE number > $1
),
delete_account_slots AS (
 DELETE FROM account_slots WHERE creation_block > $1
),
delete_accounts AS (
 DELETE FROM accounts WHERE creation_block > $1
),
delete_historical_balances AS (
 DELETE FROM historical_balances WHERE block_number > $1
),
delete_historical_nonces AS (
 DELETE FROM historical_nonces WHERE block_number > $1
),
delete_historical_slots AS (
 DELETE FROM historical_slots WHERE block_number > $1
),
delete_logs AS (
 DELETE FROM logs WHERE block_number > $1
)
DELETE FROM transactions WHERE block_number > $1
