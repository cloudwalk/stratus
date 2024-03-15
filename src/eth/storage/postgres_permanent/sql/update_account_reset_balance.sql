-- Get the block_numbers with the latest balance change for each address
WITH latest_block_numbers AS (
    SELECT address, MAX(block_number) as block_number
    FROM historical_balances
    GROUP BY address
),
-- Get the latest balance change for each address
latest_balances AS (
    SELECT accounts.address, historical_balances.balance
    FROM accounts
    JOIN latest_block_numbers
    ON latest_block_numbers.address = accounts.address
    LEFT JOIN historical_balances on accounts.address = historical_balances.address
        AND latest_block_numbers.block_number = historical_balances.block_number
)
-- Update the accounts with the balances in latest_balances
UPDATE accounts
SET latest_balance = latest_balances.balance
FROM latest_balances
WHERE latest_balances.address = accounts.address
    AND latest_balances.balance IS DISTINCT FROM accounts.latest_balance
