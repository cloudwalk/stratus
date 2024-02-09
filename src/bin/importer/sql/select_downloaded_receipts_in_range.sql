select block_number, payload
from external_receipts
where block_number >= $1 and block_number <= $2;